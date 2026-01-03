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

use crate::panel::StreamKind;
use crate::ui::UiEvent;

pub struct ServiceInstance {
    pub pid: Option<u32>,
    pub stdout_task: JoinHandle<()>,
    pub stderr_task: JoinHandle<()>,
    pub wait_task: JoinHandle<()>,
    exit_status: Arc<Mutex<Option<std::io::Result<std::process::ExitStatus>>>>,
    exit_done: Arc<tokio::sync::Notify>,
}

impl ServiceInstance {
    pub fn spawn(
        panel: usize,
        cmd: &[String],
        cwd: Option<&str>,
        tx: mpsc::Sender<UiEvent>,
        shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) -> Self {
        spawn_process(panel, cmd, cwd, tx, shutdown_rx)
    }

    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.exit_done.notified().await;
        let result = self.exit_status.lock().unwrap().take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "Exit status not set")
        })??;
        Ok(result)
    }

    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        let status = self.exit_status.lock().unwrap();
        match status.as_ref() {
            None => Ok(None),
            Some(Ok(s)) => Ok(Some(*s)),
            Some(Err(e)) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )),
        }
    }

    pub async fn terminate(&self) {
        let Some(pid) = self.pid else {
            return;
        };
        let pid = Pid::from_raw(pid as i32);

        let _ = kill(pid, Signal::SIGINT);
        tokio::time::sleep(Duration::from_millis(300)).await;
        if Self::check_process_exited(pid) {
            return;
        }

        let _ = kill(pid, Signal::SIGTERM);
        tokio::time::sleep(Duration::from_millis(300)).await;
        if Self::check_process_exited(pid) {
            return;
        }

        let _ = kill(pid, Signal::SIGKILL);
    }

    fn check_process_exited(pid: Pid) -> bool {
        use nix::sys::signal::kill;
        match kill(pid, None) {
            Err(nix::Error::ESRCH) => true,
            Ok(_) => false,
            Err(_) => false,
        }
    }
}

fn spawn_process(
    panel: usize,
    cmd: &[String],
    cwd: Option<&str>,
    tx: mpsc::Sender<UiEvent>,
    shutdown_rx: tokio::sync::broadcast::Receiver<()>,
) -> ServiceInstance {
    let mut command = Command::new(&cmd[0]);
    command
        .args(&cmd[1..])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    let mut child = command.spawn().expect("spawn failed");

    let stdout = BufReader::new(child.stdout.take().unwrap()).lines();
    let stderr = BufReader::new(child.stderr.take().unwrap()).lines();

    let tx_out = tx.clone();
    let tx_err = tx.clone();

    let stdout_task = tokio::spawn({
        let mut rx = shutdown_rx.resubscribe();
        async move {
            let mut lines = stdout;
            loop {
                tokio::select! {
                    result = lines.next_line() => {
                            match result {
                                Ok(Some(line)) => {
                                    // Ignore send errors - if channel is closed, we're shutting down
                                    let _ = tx_out
                                        .send(UiEvent::Line {
                                            panel,
                                            stream: StreamKind::Stdout,
                                            text: line,
                                        })
                                        .await;
                                }
                                _ => break,
                            }
                    }
                    _ = rx.recv() => {
                        break;
                    }
                }
            }
        }
    });

    let stderr_task = tokio::spawn({
        let mut rx = shutdown_rx.resubscribe();
        async move {
            let mut lines = stderr;
            loop {
                tokio::select! {
                    result = lines.next_line() => {
                        match result {
                            Ok(Some(line)) => {
                                // Ignore send errors - if channel is closed, we're shutting down
                                let _ = tx_err
                                    .send(UiEvent::Line {
                                        panel,
                                        stream: StreamKind::Stderr,
                                        text: line,
                                    })
                                    .await;
                            }
                            _ => break,
                        }
                    }
                    _ = rx.recv() => {
                        break;
                    }
                }
            }
        }
    });

    let pid = child.id();

    let exit_status: Arc<Mutex<Option<std::io::Result<std::process::ExitStatus>>>> =
        Arc::new(Mutex::new(None));
    let exit_done = Arc::new(Notify::new());

    let exit_tx_ui = tx.clone();
    let panel_idx = panel;
    let exit_status_clone = exit_status.clone();
    let exit_done_clone = exit_done.clone();
    let wait_task = tokio::spawn({
        let mut rx = shutdown_rx.resubscribe();
        async move {
            let result = tokio::select! {
                _ = rx.recv() => {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "Process was terminated",
                    ))
                }
                result = child.wait() => result,
            };

            let exit_code = result.as_ref().ok().and_then(|s| s.code());
            let is_ok = result.is_ok();
            let status = result.as_ref().ok().copied();

            *exit_status_clone.lock().unwrap() = Some(result);
            exit_done_clone.notify_one();

            if is_ok {
                // Ignore send errors - if channel is closed, we're shutting down
                let _ = exit_tx_ui
                    .send(UiEvent::Exited {
                        panel: panel_idx,
                        status: status.map(|s| s),
                        exit_code,
                    })
                    .await;
            } else {
                // Ignore send errors - if channel is closed, we're shutting down
                let _ = exit_tx_ui
                    .send(UiEvent::Exited {
                        panel: panel_idx,
                        status: None,
                        exit_code: None,
                    })
                    .await;
            }
        }
    });

    ServiceInstance {
        pid,
        stdout_task,
        stderr_task,
        wait_task,
        exit_status,
        exit_done,
    }
}
