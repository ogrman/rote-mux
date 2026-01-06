use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::{Notify, mpsc},
    task::JoinHandle,
};

use crate::panel::{PanelIndex, StreamKind};
use crate::signals::is_process_exited;
use crate::ui::UiEvent;

pub struct TaskInstance {
    pub pid: Option<u32>,
    pub stdout_task: JoinHandle<()>,
    pub stderr_task: JoinHandle<()>,
    pub wait_task: JoinHandle<()>,
    exit_status: Arc<Mutex<Option<std::io::Result<std::process::ExitStatus>>>>,
    exit_done: Arc<tokio::sync::Notify>,
}

impl TaskInstance {
    pub fn spawn(
        panel: PanelIndex,
        cmd: &[String],
        cwd: Option<&str>,
        tx: mpsc::Sender<UiEvent>,
        shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) -> std::io::Result<Self> {
        spawn_process(panel, cmd, cwd, tx, shutdown_rx)
    }

    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.exit_done.notified().await;
        let result = self
            .exit_status
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| std::io::Error::other("Exit status not set"))??;
        Ok(result)
    }

    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        let status = self.exit_status.lock().unwrap();
        match status.as_ref() {
            None => Ok(None),
            Some(Ok(s)) => Ok(Some(*s)),
            Some(Err(e)) => Err(std::io::Error::other(e.to_string())),
        }
    }

    /// Get the exit status Arc for use after partial moves
    pub fn exit_status_arc(&self) -> Arc<Mutex<Option<std::io::Result<std::process::ExitStatus>>>> {
        self.exit_status.clone()
    }

    pub async fn terminate(&self) {
        let Some(pid) = self.pid else {
            return;
        };
        let pid = Pid::from_raw(pid as i32);

        let _ = kill(pid, Signal::SIGINT);
        tokio::time::sleep(Duration::from_millis(300)).await;
        if is_process_exited(pid) {
            return;
        }

        let _ = kill(pid, Signal::SIGTERM);
        tokio::time::sleep(Duration::from_millis(300)).await;
        if is_process_exited(pid) {
            return;
        }

        let _ = kill(pid, Signal::SIGKILL);
    }

    fn send_exit_event(
        tx: &mpsc::Sender<UiEvent>,
        panel: PanelIndex,
        result: &std::io::Result<std::process::ExitStatus>,
    ) {
        use std::os::unix::process::ExitStatusExt;

        let exit_code = result.as_ref().ok().and_then(|s| {
            // First try to get the exit code directly
            if let Some(code) = s.code() {
                Some(code)
            } else {
                s.signal().map(|s| {
                    // Process was killed by signal - use standard 128+signal convention
                    128 + s
                })
            }
        });
        let is_ok = result.is_ok();
        let status = result.as_ref().ok().copied();

        if is_ok {
            let _ = tx.try_send(UiEvent::Exited {
                panel,
                status,
                exit_code,
            });
        } else {
            let _ = tx.try_send(UiEvent::Exited {
                panel,
                status: None,
                exit_code: None,
            });
        }
    }
}

/// Spawn a task that reads lines from a stream and sends them as events.
fn spawn_stream_reader(
    panel: PanelIndex,
    stream: StreamKind,
    lines: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    tx: mpsc::Sender<UiEvent>,
    mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut lines = lines;
        loop {
            tokio::select! {
                result = lines.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            let _ = tx.send(UiEvent::Line { panel, stream, text: line }).await;
                        }
                        _ => break,
                    }
                }
                _ = shutdown_rx.recv() => break,
            }
        }
    })
}

/// Spawn a task that reads lines from stderr and sends them as events.
fn spawn_stderr_reader(
    panel: PanelIndex,
    lines: tokio::io::Lines<BufReader<tokio::process::ChildStderr>>,
    tx: mpsc::Sender<UiEvent>,
    mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut lines = lines;
        loop {
            tokio::select! {
                result = lines.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            let _ = tx.send(UiEvent::Line { panel, stream: StreamKind::Stderr, text: line }).await;
                        }
                        _ => break,
                    }
                }
                _ = shutdown_rx.recv() => break,
            }
        }
    })
}

/// Spawn a task that waits for the child process to exit and sends an exit event.
fn spawn_exit_waiter(
    panel: PanelIndex,
    mut child: tokio::process::Child,
    tx: mpsc::Sender<UiEvent>,
    exit_status: Arc<Mutex<Option<std::io::Result<std::process::ExitStatus>>>>,
    exit_done: Arc<Notify>,
    mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let result = tokio::select! {
            _ = shutdown_rx.recv() => {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "Process was terminated",
                ))
            }
            result = child.wait() => result,
        };

        *exit_status.lock().unwrap() = Some(result);
        exit_done.notify_one();

        TaskInstance::send_exit_event(&tx, panel, exit_status.lock().unwrap().as_ref().unwrap());
    })
}

fn spawn_process(
    panel: PanelIndex,
    cmd: &[String],
    cwd: Option<&str>,
    tx: mpsc::Sender<UiEvent>,
    shutdown_rx: tokio::sync::broadcast::Receiver<()>,
) -> std::io::Result<TaskInstance> {
    // Configure command
    let mut command = Command::new(&cmd[0]);
    command
        .args(&cmd[1..])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    // Spawn process
    let mut child = command.spawn()?;
    let pid = child.id();

    // Take stdout/stderr handles
    let stdout = BufReader::new(child.stdout.take().expect("stdout should be piped")).lines();
    let stderr = BufReader::new(child.stderr.take().expect("stderr should be piped")).lines();

    // Spawn stream reader tasks
    let stdout_task = spawn_stream_reader(
        panel,
        StreamKind::Stdout,
        stdout,
        tx.clone(),
        shutdown_rx.resubscribe(),
    );
    let stderr_task = spawn_stderr_reader(panel, stderr, tx.clone(), shutdown_rx.resubscribe());

    // Spawn exit waiter task
    let exit_status: Arc<Mutex<Option<std::io::Result<std::process::ExitStatus>>>> =
        Arc::new(Mutex::new(None));
    let exit_done = Arc::new(Notify::new());
    let wait_task = spawn_exit_waiter(
        panel,
        child,
        tx,
        exit_status.clone(),
        exit_done.clone(),
        shutdown_rx.resubscribe(),
    );

    Ok(TaskInstance {
        pid,
        stdout_task,
        stderr_task,
        wait_task,
        exit_status,
        exit_done,
    })
}
