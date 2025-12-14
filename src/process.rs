use std::{
    collections::VecDeque,
    process::{Child, Command, ExitCode, Stdio},
};

use super::{ServiceAction, ServiceConfiguration};

pub struct Process {
    config: ServiceConfiguration,
    log: VecDeque<LogLine>,
    state: ProcessState,
}

pub enum ProcessState {
    Unstarted,
    Started { child: Child },
    Finished { exit_code: ExitCode },
}

pub enum LogStream {
    Stdout,
    Stderr,
}

pub struct LogLine {
    stream: LogStream,
    timestamp: std::time::SystemTime,
    content: String,
}

impl Process {
    pub fn new(config: ServiceConfiguration) -> Self {
        Process {
            config,
            log: VecDeque::new(),
            state: ProcessState::Unstarted,
        }
    }

    pub fn healthy(&self) -> bool {
        match &self.state {
            ProcessState::Started { .. } => true,
            ProcessState::Finished { exit_code } => match &self.config.action {
                Some(super::ServiceAction::Run { .. }) => *exit_code == ExitCode::from(0),
                Some(super::ServiceAction::Start { .. }) => false,
                None => false,
            },
            _ => false,
        }
    }

    pub fn start(&mut self, root: &std::path::Path) -> anyhow::Result<()> {
        let action = match &self.config.action {
            None => {
                // When no action, just mark as finished successfully. This will mean
                // that other services that depend on this one can start.
                self.state = ProcessState::Finished {
                    exit_code: ExitCode::SUCCESS,
                };
                return Ok(());
            }
            Some(action) => action,
        };

        let child = Command::new("sh")
            .arg("-c")
            .arg(&match action {
                ServiceAction::Run { command } => command.as_ref(),
                ServiceAction::Start { command } => command.as_ref(),
            })
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        self.state = ProcessState::Started { child };

        Ok(())
    }

    pub fn shut_down(&mut self) -> anyhow::Result<()> {
        match &mut self.state {
            ProcessState::Started { child } => {
                child.kill()?;
                let exit_status = child.wait()?;
                self.state = ProcessState::Finished {
                    exit_code: ExitCode::from(exit_status.code().unwrap_or(1) as u8),
                };
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
            ProcessState::Finished { .. } => {}
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
            ProcessState::Finished { exit_code } => {
                assert_eq!(exit_code, std::process::ExitCode::SUCCESS)
            }
            _ => panic!("Process should be Finished after start with no action"),
        }
    }
}
