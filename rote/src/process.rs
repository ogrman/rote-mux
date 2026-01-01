use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::{Notify, mpsc, oneshot},
    task::JoinHandle,
};

use crate::panel::StreamKind;
use crate::ui::UiEvent;

pub struct RunningProcess {
    pub pid: Option<u32>,
    pub _stdout_task: JoinHandle<()>,
    pub _stderr_task: JoinHandle<()>,
    pub _wait_task: JoinHandle<()>,
    pub _exit_rx: oneshot::Receiver<std::io::Result<std::process::ExitStatus>>,
    pub _exit_notify: Arc<Notify>,
}

impl RunningProcess {
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        let rx = std::mem::replace(&mut self._exit_rx, oneshot::channel().1);
        rx.await.map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Process handle was dropped")
        })?
    }

    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        match &self._exit_rx {
            _ => match self._exit_rx.try_recv() {
                Ok(result) => Ok(Some(result?)),
                Err(oneshot::error::TryRecvError::Empty) => Ok(None),
                Err(oneshot::error::TryRecvError::Closed) => Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "Process handle was dropped",
                )),
            },
        }
    }
}

pub fn spawn_process(
    panel: usize,
    cmd: &[String],
    cwd: Option<&str>,
    tx: mpsc::Sender<UiEvent>,
    shutdown_rx: tokio::sync::broadcast::Receiver<()>,
) -> RunningProcess {
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

    let (exit_tx_internal, exit_rx) = oneshot::channel();

    let exit_notify = Arc::new(Notify::new());

    let exit_tx_ui = tx.clone();
    let panel_idx = panel;
    let exit_notify_clone = exit_notify.clone();
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
            exit_notify_clone.notify_one();

            let _ = exit_tx_internal.send(result);

            if is_ok {
                let _ = exit_tx_ui
                    .send(UiEvent::Exited {
                        panel: panel_idx,
                        status: status.map(|s| s),
                        exit_code,
                        title: String::new(),
                    })
                    .await;
            } else {
                let _ = exit_tx_ui
                    .send(UiEvent::Exited {
                        panel: panel_idx,
                        status: None,
                        exit_code: None,
                        title: String::new(),
                    })
                    .await;
            }
        }
    });

    RunningProcess {
        pid,
        _stdout_task: stdout_task,
        _stderr_task: stderr_task,
        _wait_task: wait_task,
        _exit_rx: exit_rx,
        _exit_notify: exit_notify,
    }
}
