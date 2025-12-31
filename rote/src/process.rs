use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::mpsc,
};

use crate::panel::StreamKind;
use crate::ui::UiEvent;

pub struct RunningProcess {
    pub child: Child,
}

pub fn spawn_process(
    panel: usize,
    cmd: &[String],
    cwd: Option<&str>,
    tx: mpsc::Sender<UiEvent>,
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

    tokio::spawn(async move {
        let mut lines = stdout;
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx_out
                .send(UiEvent::Line {
                    panel,
                    stream: StreamKind::Stdout,
                    text: line,
                })
                .await;
        }
    });

    tokio::spawn(async move {
        let mut lines = stderr;
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx_err
                .send(UiEvent::Line {
                    panel,
                    stream: StreamKind::Stderr,
                    text: line,
                })
                .await;
        }
    });

    RunningProcess { child }
}
