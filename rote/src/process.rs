use std::{
    collections::VecDeque, io::{BufRead, BufReader}, process::{Child, ChildStderr, ChildStdout, Command, ExitCode, Stdio}
};

use nix::sys::signal::{kill as nix_kill};

use super::{ServiceAction, ServiceConfiguration};

pub struct Process {
    config: ServiceConfiguration,
    log: ProcessLog,
    state: ProcessState,
}

pub enum ProcessState {
    Unstarted,
    Started {
        child: Child,
        stdout: Option<BufReader<ChildStdout>>,
        stderr: Option<BufReader<ChildStderr>>,
    },
    Finished {
        exit_code: ExitCode,
        cause: ExitCause,
    },
}

pub struct ProcessLog {
    max_lines: usize,
    lines: VecDeque<LogLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitCause {
    Exited,
    SigInt,
    SigTerm,
    SigKill,
    FailedToStart,
}

impl std::fmt::Display for ExitCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExitCause::Exited => write!(f, "Exited normally"),
            ExitCause::SigInt => write!(f, "Terminated by SIGINT"),
            ExitCause::SigTerm => write!(f, "Terminated by SIGTERM"),
            ExitCause::SigKill => write!(f, "Terminated by SIGKILL"),
            ExitCause::FailedToStart => write!(f, "Failed to start"),
        }
    }
}

impl ProcessLog {
    pub fn new(max_lines: usize) -> Self {
        ProcessLog {
            max_lines,
            lines: VecDeque::new(),
        }
    }

    pub fn add_line(&mut self, line: LogLine) {
        if self.lines.len() >= self.max_lines {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    pub fn tail(&self) -> impl Iterator<Item = &LogLine> {
        self.lines.iter().rev()
    }
}

#[derive(Clone, Copy)]
pub enum LogStream {
    Stdout,
    Stderr,
}

pub struct LogLine {
    pub stream: LogStream,
    pub timestamp: std::time::SystemTime,
    pub content: String,
}

impl Process {
    pub fn new(config: ServiceConfiguration) -> Self {
        Process {
            config,
            log: ProcessLog::new(1000),
            state: ProcessState::Unstarted,
        }
    }

    pub fn healthy(&self) -> bool {
        match &self.state {
            ProcessState::Started { .. } => true,
            ProcessState::Finished { exit_code, .. } => match &self.config.action {
                Some(super::ServiceAction::Run { .. }) => *exit_code == ExitCode::from(0),
                Some(super::ServiceAction::Start { .. }) => false,
                None => false,
            },
            _ => false,
        }
    }

    pub fn poll(&mut self) -> anyhow::Result<()> {
        let new_state = match &mut self.state {
            ProcessState::Started { child, stdout, stderr } => {
                if let Some(stdout) = stdout {
                    poll_stream(stdout, &mut self.log, LogStream::Stdout)?;
                }
                if let Some(stderr) = stderr {
                    poll_stream(stderr, &mut self.log, LogStream::Stderr)?;
                }

                if let Some(status) = child.try_wait()? {
                    Some(ProcessState::Finished {
                        exit_code: ExitCode::from(status.code().unwrap_or(1) as u8),
                        cause: ExitCause::Exited,
                    })
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(new_state) = new_state {
            self.state = new_state;
        }
        Ok(())
    }

    pub fn start(&mut self, root: &std::path::Path) -> anyhow::Result<()> {
        let action = match &self.config.action {
            None => {
                // When no action, just mark as finished successfully. This will mean
                // that other services that depend on this one can start.
                self.state = ProcessState::Finished {
                    exit_code: ExitCode::SUCCESS,
                    cause: ExitCause::FailedToStart,
                };
                return Ok(());
            }
            Some(action) => action,
        };

        // Extract command and arguments
        let (program, args) = match action {
            ServiceAction::Run { command } | ServiceAction::Start { command } => {
                let mut parts = shell_words::split(command.as_ref())?;
                let program = parts.remove(0);
                (program, parts)
            }
        };

        let mut child = Command::new(program)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        self.state = ProcessState::Started {
            stdout: child.stdout.take().map(BufReader::new),
            stderr: child.stderr.take().map(BufReader::new),
            child,
        };

        Ok(())
    }

    pub fn shut_down(&mut self) -> anyhow::Result<()> {
        self.poll()?;

        if matches!(self.state, ProcessState::Finished { .. }) {
            return Ok(());
        }

        match &mut self.state {
            ProcessState::Started { child, stdout: _, stderr: _ } => {
                if self.config.action.is_none() {
                    return Ok(());
                }
                // If already exited, just collect exit code
                if let Some(status) = child.try_wait()? {
                    self.state = ProcessState::Finished {
                        exit_code: ExitCode::from(status.code().unwrap_or(1) as u8),
                        cause: ExitCause::Exited,
                    };
                    return Ok(());
                }
                let (exit_code, cause) = shut_down(child, true);
                self.state = ProcessState::Finished { exit_code, cause };
            }
            _ => {}
        }
        Ok(())
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.shut_down();
    }
}

/// Attempt to gracefully shut down a child process: SIGINT, then SIGTERM, then SIGKILL.
/// If wait_for_shutdown is true, waits for process to exit after each signal.
/// Returns the final exit code.
pub fn shut_down(child: &mut Child, wait_for_shutdown: bool) -> (ExitCode, ExitCause) {
    use nix::sys::signal::Signal;
    use nix::unistd::Pid;
    use std::process::ExitCode;
    let pid = child.id() as i32;
    let pid = Pid::from_raw(pid);

    // Helper to wait for process exit
    let wait = |child: &mut Child, rounds: u32| -> Option<i32> {
        for _ in 0..rounds {
            if let Ok(Some(status)) = child.try_wait() {
                return status.code();
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        None
    };

    // 1. SIGINT (Ctrl-C)
    let _ = nix_kill(pid, Signal::SIGINT);
    if wait_for_shutdown {
        if let Some(code) = wait(child, 10) {
            return (ExitCode::from(code as u8), ExitCause::SigInt);
        }
    }

    // 2. SIGTERM
    let _ = nix_kill(pid, Signal::SIGTERM);
    if wait_for_shutdown {
        if let Some(code) = wait(child, 10) {
            return (ExitCode::from(code as u8), ExitCause::SigTerm);
        }
    }

    // 3. SIGKILL
    let _ = nix_kill(pid, Signal::SIGKILL);
    // Always wait for process to exit after SIGKILL

    let status = child.wait().ok();

    (
        ExitCode::from(status.and_then(|s| s.code()).unwrap_or(1) as u8),
        ExitCause::SigKill,
    )
}

fn poll_stream<T: std::io::Read>(
    stream: &mut BufReader<T>,
    log: &mut ProcessLog,
    log_stream: LogStream,
) -> anyhow::Result<()> {
    let mut buffer = String::new();
    loop {
        buffer.clear();
        let bytes_read = stream.read_line(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        let line = LogLine {
            stream: log_stream,
            timestamp: std::time::SystemTime::now(),
            content: buffer.trim_end().to_string(),
        };
        log.add_line(line);
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ServiceAction;
    use std::borrow::Cow;
    use std::path::PathBuf;

    fn dummy_config_run() -> ServiceConfiguration {
        ServiceConfiguration {
            action: Some(ServiceAction::Run {
                command: Cow::Borrowed("echo 'hi'"),
            }),
            cwd: None,
            display: None,
            require: vec![],
        }
    }

    fn dummy_config_start() -> ServiceConfiguration {
        ServiceConfiguration {
            action: Some(ServiceAction::Start {
                command: Cow::Borrowed("sleep 1"),
            }),
            cwd: None,
            display: None,
            require: vec![],
        }
    }

    #[test]
    fn test_shutdown_escalates_to_sigkill() {
        // Use a script that ignores SIGINT and sleeps
        let config = ServiceConfiguration {
            action: Some(ServiceAction::Start {
                command: Cow::Borrowed("./tests/ignore_sigint_sleep.sh"),
            }),
            cwd: None,
            display: None,
            require: vec![],
        };
        let mut proc = Process::new(config);
        let root = PathBuf::from(".");
        let result = proc.start(&root);
        assert!(result.is_ok(), "Process::start should succeed for start");
        match proc.state {
            ProcessState::Started { .. } => {}
            _ => panic!("Process should be Started after start"),
        }
        // Shut down should escalate to SIGKILL and finish the process
        let result = proc.shut_down();
        assert!(result.is_ok(), "Process::shut_down should succeed");
        match proc.state {
            ProcessState::Finished { cause, .. } => {
                assert_eq!(cause, ExitCause::SigKill);
            }
            _ => panic!("Process should be Finished after shut_down"),
        }
    }

    #[test]
    fn test_start_and_shutdown_run() {
        let config = dummy_config_run();
        let mut proc = Process::new(config);
        let root = PathBuf::from(".");
        let result = proc.start(&root);
        assert!(result.is_ok(), "Process::start should succeed for run");
        match proc.state {
            ProcessState::Started { .. } => {}
            _ => panic!("Process should be Started after start"),
        }
        // Shut down should finish the process
        let result = proc.shut_down();
        assert!(result.is_ok(), "Process::shut_down should succeed");
        match proc.state {
            ProcessState::Finished { exit_code, cause } => {
                // Accept both 0 (success) and 1 (killed) as valid
                assert!(
                    exit_code == ExitCode::SUCCESS,
                    "Expected exit code 0, got {exit_code:?}",
                );
                assert_eq!(cause, ExitCause::Exited);
            }
            _ => panic!("Process should be Finished after shut_down"),
        }
    }

    #[test]
    fn test_start_and_shutdown_start() {
        let config = dummy_config_start();
        let mut proc = Process::new(config);
        let root = PathBuf::from(".");
        let result = proc.start(&root);
        assert!(result.is_ok(), "Process::start should succeed for start");
        match proc.state {
            ProcessState::Started { .. } => {}
            _ => panic!("Process should be Started after start"),
        }
        // Shut down should finish the process
        let result = proc.shut_down();
        assert!(result.is_ok(), "Process::shut_down should succeed");
        match proc.state {
            ProcessState::Finished { .. } => {}
            _ => panic!("Process should be Finished after shut_down"),
        }
    }

    #[test]
    fn test_new_sets_unstarted() {
        let config = dummy_config_run();
        let proc = Process::new(config);
        match proc.state {
            ProcessState::Unstarted => {}
            _ => panic!("Process should be Unstarted on creation"),
        }
    }

    #[test]
    fn test_healthy_unstarted() {
        let config = dummy_config_run();
        let proc = Process::new(config);
        assert!(!proc.healthy(), "Unstarted process should not be healthy");
    }

    #[test]
    fn test_healthy_run_finished_success() {
        let config = dummy_config_run();
        let mut proc = Process::new(config);
        proc.state = ProcessState::Finished {
            exit_code: std::process::ExitCode::from(0),
            cause: ExitCause::Exited,
        };
        assert!(
            proc.healthy(),
            "Run process with exit code 0 should be healthy"
        );
    }

    #[test]
    fn test_healthy_run_finished_failure() {
        let config = dummy_config_run();
        let mut proc = Process::new(config);
        proc.state = ProcessState::Finished {
            exit_code: std::process::ExitCode::from(1),
            cause: ExitCause::Exited,
        };
        assert!(
            !proc.healthy(),
            "Run process with exit code 1 should not be healthy"
        );
    }

    #[test]
    fn test_healthy_start_finished() {
        let config = dummy_config_start();
        let mut proc = Process::new(config);
        proc.state = ProcessState::Finished {
            exit_code: std::process::ExitCode::from(0),
            cause: ExitCause::Exited,
        };
        assert!(
            !proc.healthy(),
            "Start process should never be healthy when finished"
        );
    }

    #[test]
    fn test_start_and_shutdown_no_action() {
        let mut config = dummy_config_run();
        config.action = None;
        let mut proc = Process::new(config);
        let root = PathBuf::from(".");
        let result = proc.start(&root);
        assert!(result.is_ok());
        match proc.state {
            ProcessState::Finished { exit_code, cause } => {
                assert_eq!(exit_code, std::process::ExitCode::SUCCESS);
                assert_eq!(cause, ExitCause::FailedToStart);
            }
            _ => panic!("Process should be Finished after start with no action"),
        }
    }
}
