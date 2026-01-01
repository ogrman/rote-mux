use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::mpsc,
    task::JoinHandle,
};

use crate::panel::StreamKind;
use crate::ui::UiEvent;

pub struct RunningProcess {
    pub child: Child,
    pub _stdout_task: JoinHandle<()>,
    pub _stderr_task: JoinHandle<()>,
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

    RunningProcess {
        child,
        _stdout_task: stdout_task,
        _stderr_task: stderr_task,
    }
}
