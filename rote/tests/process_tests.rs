use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

use rote::panel::StreamKind;
use rote::process::spawn_process;
use rote::signals::terminate_child;
use rote::ui::UiEvent;

/// Helper function to collect output events from a process
async fn collect_output_events(
    mut rx: mpsc::Receiver<UiEvent>,
    timeout_ms: u64,
) -> (Vec<String>, Vec<String>) {
    let mut stdout_lines = Vec::new();
    let mut stderr_lines = Vec::new();

    let deadline = tokio::time::sleep(Duration::from_millis(timeout_ms));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                match event {
                    UiEvent::Line { stream, text, .. } => {
                        match stream {
                            StreamKind::Stdout => stdout_lines.push(text),
                            StreamKind::Stderr => stderr_lines.push(text),
                        }
                    }
                    _ => {}
                }
            }
            _ = &mut deadline => break,
        }
    }

    (stdout_lines, stderr_lines)
}

#[tokio::test]
async fn test_spawn_simple_process() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);
    let cmd = vec!["echo".to_string(), "hello world".to_string()];

    let mut proc = spawn_process(0, &cmd, None, tx);

    // Wait for process to complete
    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    // Collect output
    let (stdout_lines, _) = collect_output_events(rx, 500).await;
    assert_eq!(stdout_lines.len(), 1);
    assert_eq!(stdout_lines[0], "hello world");
}

#[tokio::test]
async fn test_capture_stdout_and_stderr() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);

    // Use our test script that outputs to both stdout and stderr
    let script_path = format!("{}/tests/data/echo_exit.sh", env!("CARGO_MANIFEST_DIR"));
    let cmd = vec![script_path];

    let mut proc = spawn_process(0, &cmd, None, tx);

    // Wait for process to complete
    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    // Collect output
    let (stdout_lines, stderr_lines) = collect_output_events(rx, 500).await;

    assert_eq!(stdout_lines.len(), 1);
    assert_eq!(stdout_lines[0], "stdout message");

    assert_eq!(stderr_lines.len(), 1);
    assert_eq!(stderr_lines[0], "stderr message");
}

#[tokio::test]
async fn test_multiple_panels() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);

    // Spawn two processes for different panels
    let cmd1 = vec!["echo".to_string(), "panel0".to_string()];
    let cmd2 = vec!["echo".to_string(), "panel1".to_string()];

    let mut proc1 = spawn_process(0, &cmd1, None, tx.clone());
    let mut proc2 = spawn_process(1, &cmd2, None, tx.clone());

    // Wait for both processes
    let _ = timeout(Duration::from_secs(2), proc1.child.wait()).await;
    let _ = timeout(Duration::from_secs(2), proc2.child.wait()).await;

    // Collect and verify events have correct panel IDs
    let mut panel0_lines = Vec::new();
    let mut panel1_lines = Vec::new();

    let deadline = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                if let UiEvent::Line { panel, text, .. } = event {
                    match panel {
                        0 => panel0_lines.push(text),
                        1 => panel1_lines.push(text),
                        _ => panic!("Unexpected panel ID"),
                    }
                }
            }
            _ = &mut deadline => break,
        }
    }

    assert_eq!(panel0_lines.len(), 1);
    assert_eq!(panel0_lines[0], "panel0");

    assert_eq!(panel1_lines.len(), 1);
    assert_eq!(panel1_lines[0], "panel1");
}

#[tokio::test]
async fn test_terminate_child_respects_sigint() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);

    let script_path = format!(
        "{}/tests/data/respects_sigint.sh",
        env!("CARGO_MANIFEST_DIR")
    );
    let cmd = vec![script_path];

    let mut proc = spawn_process(0, &cmd, None, tx);

    // Give process time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Terminate the process
    let start = std::time::Instant::now();
    terminate_child(&mut proc.child).await;
    let elapsed = start.elapsed();

    // Should terminate quickly with SIGINT (within first 300ms window)
    assert!(
        elapsed < Duration::from_millis(500),
        "Process should respect SIGINT and exit quickly, took {:?}",
        elapsed
    );

    // Verify process is actually terminated
    let status = proc.child.try_wait().expect("Failed to check status");
    assert!(status.is_some(), "Process should be terminated");

    // Collect output to verify the signal was handled
    let (stdout_lines, _) = collect_output_events(rx, 100).await;
    assert!(
        stdout_lines.iter().any(|line| line.contains("started")),
        "Process should have started"
    );
    assert!(
        stdout_lines
            .iter()
            .any(|line| line.contains("received SIGINT")),
        "Process should have received SIGINT"
    );
}

#[tokio::test]
async fn test_terminate_child_escalates_to_sigterm() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);

    let script_path = format!(
        "{}/tests/data/respects_sigterm.sh",
        env!("CARGO_MANIFEST_DIR")
    );
    let cmd = vec![script_path];

    let mut proc = spawn_process(0, &cmd, None, tx);

    // Give process time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Terminate the process
    let start = std::time::Instant::now();
    terminate_child(&mut proc.child).await;
    let elapsed = start.elapsed();

    // Should take at least 300ms (SIGINT wait) but less than 1000ms
    assert!(
        elapsed >= Duration::from_millis(300),
        "Should wait for SIGINT timeout"
    );
    assert!(
        elapsed < Duration::from_millis(1000),
        "Should terminate with SIGTERM before SIGKILL, took {:?}",
        elapsed
    );

    // Verify process is actually terminated
    let status = proc.child.try_wait().expect("Failed to check status");
    assert!(status.is_some(), "Process should be terminated");

    // Collect output
    let (stdout_lines, _) = collect_output_events(rx, 100).await;
    assert!(
        stdout_lines.iter().any(|line| line.contains("started")),
        "Process should have started"
    );
    assert!(
        stdout_lines
            .iter()
            .any(|line| line.contains("received SIGTERM")),
        "Process should have received SIGTERM"
    );
}

#[tokio::test]
async fn test_terminate_child_escalates_to_sigkill() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);

    let script_path = format!(
        "{}/tests/data/ignores_all_signals.sh",
        env!("CARGO_MANIFEST_DIR")
    );
    let cmd = vec![script_path];

    let mut proc = spawn_process(0, &cmd, None, tx);

    // Give process time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Terminate the process
    let start = std::time::Instant::now();
    terminate_child(&mut proc.child).await;
    let elapsed = start.elapsed();

    // Should take at least 600ms (SIGINT + SIGTERM waits) but not too long
    assert!(
        elapsed >= Duration::from_millis(600),
        "Should wait for SIGINT and SIGTERM timeouts"
    );
    assert!(
        elapsed < Duration::from_millis(1500),
        "Should complete within reasonable time after SIGKILL, took {:?}",
        elapsed
    );

    // Give SIGKILL time to take effect
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify process is actually terminated
    let status = proc.child.try_wait().expect("Failed to check status");
    assert!(status.is_some(), "Process should be terminated by SIGKILL");

    // Collect output
    let (stdout_lines, _) = collect_output_events(rx, 100).await;
    assert!(
        stdout_lines.iter().any(|line| line.contains("started")),
        "Process should have started"
    );
    // Process should NOT have finished normally since it was killed
    assert!(
        !stdout_lines
            .iter()
            .any(|line| line.contains("finished normally")),
        "Process should not have finished normally"
    );
}

#[tokio::test]
async fn test_process_exit_status() {
    let (tx, _rx) = mpsc::channel::<UiEvent>(100);

    // Test successful exit
    let cmd = vec!["true".to_string()];
    let mut proc = spawn_process(0, &cmd, None, tx.clone());
    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");
    assert!(status.success());

    // Test failed exit
    let cmd = vec!["false".to_string()];
    let mut proc = spawn_process(0, &cmd, None, tx.clone());
    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");
    assert!(!status.success());
}

#[tokio::test]
async fn test_long_running_process() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);

    // Start a process that sleeps for a while
    let cmd = vec!["sleep".to_string(), "0.5".to_string()];
    let mut proc = spawn_process(0, &cmd, None, tx);

    // Verify process is still running
    tokio::time::sleep(Duration::from_millis(100)).await;
    let status = proc.child.try_wait().expect("Failed to check status");
    assert!(status.is_none(), "Process should still be running");

    // Wait for it to complete
    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");
    assert!(status.success());

    // Clean up receiver
    drop(rx);
}

#[tokio::test]
async fn test_process_with_args() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);

    let cmd = vec![
        "printf".to_string(),
        "%s %s %s".to_string(),
        "arg1".to_string(),
        "arg2".to_string(),
        "arg3".to_string(),
    ];

    let mut proc = spawn_process(0, &cmd, None, tx);

    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    let (stdout_lines, _) = collect_output_events(rx, 500).await;
    assert_eq!(stdout_lines.len(), 1);
    assert_eq!(stdout_lines[0], "arg1 arg2 arg3");
}
