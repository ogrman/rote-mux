use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

use rote::panel::{Panel, StreamKind};
use rote::process::spawn_process;
use rote::signals::terminate_child;
use rote::ui::UiEvent;

/// Helper to simulate visible_len calculation from app.rs
fn visible_len(panel: &Panel) -> usize {
    let mut n = 0;
    if panel.show_stdout {
        let lines = panel.stdout.rope.len_lines();
        n += if lines > 0 {
            lines.saturating_sub(1)
        } else {
            0
        };
    }
    if panel.show_stderr {
        let lines = panel.stderr.rope.len_lines();
        n += if lines > 0 {
            lines.saturating_sub(1)
        } else {
            0
        };
    }
    n
}

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

#[tokio::test]
async fn test_multiline_stdout() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);

    // Use a command that outputs multiple lines
    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "echo line1; echo line2; echo line3".to_string(),
    ];

    let mut proc = spawn_process(0, &cmd, None, tx);

    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    let (stdout_lines, _) = collect_output_events(rx, 500).await;
    assert_eq!(stdout_lines.len(), 3);
    assert_eq!(stdout_lines[0], "line1");
    assert_eq!(stdout_lines[1], "line2");
    assert_eq!(stdout_lines[2], "line3");
}

#[tokio::test]
async fn test_rapid_output() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);

    // Generate many lines quickly
    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "for i in {1..50}; do echo \"line $i\"; done".to_string(),
    ];

    let mut proc = spawn_process(0, &cmd, None, tx);

    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    let (stdout_lines, _) = collect_output_events(rx, 1000).await;
    assert_eq!(stdout_lines.len(), 50, "Should capture all 50 lines");
    assert_eq!(stdout_lines[0], "line 1");
    assert_eq!(stdout_lines[49], "line 50");
}

#[tokio::test]
async fn test_interleaved_stdout_stderr() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);

    // Output to both stdout and stderr in sequence
    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "echo out1; echo err1 >&2; echo out2; echo err2 >&2; echo out3".to_string(),
    ];

    let mut proc = spawn_process(0, &cmd, None, tx);

    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    let (stdout_lines, stderr_lines) = collect_output_events(rx, 500).await;

    assert_eq!(stdout_lines.len(), 3);
    assert_eq!(stdout_lines[0], "out1");
    assert_eq!(stdout_lines[1], "out2");
    assert_eq!(stdout_lines[2], "out3");

    assert_eq!(stderr_lines.len(), 2);
    assert_eq!(stderr_lines[0], "err1");
    assert_eq!(stderr_lines[1], "err2");
}

#[tokio::test]
async fn test_long_lines() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);

    // Create a very long line
    let long_str = "x".repeat(1000);
    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        format!("echo {}", long_str),
    ];

    let mut proc = spawn_process(0, &cmd, None, tx);

    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    let (stdout_lines, _) = collect_output_events(rx, 500).await;
    assert_eq!(stdout_lines.len(), 1);
    assert_eq!(stdout_lines[0], long_str);
}

#[tokio::test]
async fn test_empty_lines() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);

    // Output with empty lines
    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "echo line1; echo; echo line3; echo".to_string(),
    ];

    let mut proc = spawn_process(0, &cmd, None, tx);

    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    let (stdout_lines, _) = collect_output_events(rx, 500).await;
    assert_eq!(stdout_lines.len(), 4);
    assert_eq!(stdout_lines[0], "line1");
    assert_eq!(stdout_lines[1], "");
    assert_eq!(stdout_lines[2], "line3");
    assert_eq!(stdout_lines[3], "");
}

#[tokio::test]
async fn test_panel_stdout_buffer() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "echo line1; echo line2; echo line3".to_string(),
    ];

    let mut panel = Panel::new(
        "test".to_string(),
        cmd.clone(),
        None,
        true, // show_stdout
        true, // show_stderr
    );

    let mut proc = spawn_process(0, &cmd, None, tx);

    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    // Collect events and add them to the panel
    let deadline = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                match event {
                    UiEvent::Line { stream, text, .. } => {
                        match stream {
                            StreamKind::Stdout => panel.stdout.push(&text),
                            StreamKind::Stderr => panel.stderr.push(&text),
                        }
                    }
                    _ => {}
                }
            }
            _ = &mut deadline => break,
        }
    }

    // Verify the panel buffer contains the lines
    assert_eq!(panel.stdout.rope.len_lines(), 4); // 3 lines + final newline = 4 lines in rope

    let lines: Vec<String> = panel
        .stdout
        .rope
        .lines()
        .map(|line| line.to_string())
        .collect();

    // Each line from rope.lines() includes its terminating newline
    assert_eq!(lines[0], "line1\n");
    assert_eq!(lines[1], "line2\n");
    assert_eq!(lines[2], "line3\n");
}

#[tokio::test]
async fn test_panel_stderr_buffer() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "echo err1 >&2; echo err2 >&2".to_string(),
    ];

    let mut panel = Panel::new(
        "test".to_string(),
        cmd.clone(),
        None,
        true, // show_stdout
        true, // show_stderr
    );

    let mut proc = spawn_process(0, &cmd, None, tx);

    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    // Collect events and add them to the panel
    let deadline = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                match event {
                    UiEvent::Line { stream, text, .. } => {
                        match stream {
                            StreamKind::Stdout => panel.stdout.push(&text),
                            StreamKind::Stderr => panel.stderr.push(&text),
                        }
                    }
                    _ => {}
                }
            }
            _ = &mut deadline => break,
        }
    }

    // Verify the panel buffer contains the lines
    assert_eq!(panel.stderr.rope.len_lines(), 3); // 2 lines + final newline = 3 lines in rope

    let lines: Vec<String> = panel
        .stderr
        .rope
        .lines()
        .map(|line| line.to_string())
        .collect();

    assert_eq!(lines[0], "err1\n");
    assert_eq!(lines[1], "err2\n");
}

#[tokio::test]
async fn test_continuous_output() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);

    // Simulate a continuously outputting process like ping
    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "for i in 1 2 3 4 5; do echo \"output $i\"; sleep 0.05; done".to_string(),
    ];

    let mut proc = spawn_process(0, &cmd, None, tx);

    // Don't wait for process to complete, but collect output as it comes
    let mut stdout_lines = Vec::new();
    let mut process_done = false;

    let deadline = tokio::time::sleep(Duration::from_millis(1000));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                match event {
                    UiEvent::Line { stream: StreamKind::Stdout, text, .. } => {
                        stdout_lines.push(text);
                    }
                    _ => {}
                }
            }
            result = proc.child.wait(), if !process_done => {
                process_done = true;
                assert!(result.is_ok());
            }
            _ = &mut deadline => break,
        }
    }

    // Should have received all 5 lines
    assert_eq!(stdout_lines.len(), 5);
    assert_eq!(stdout_lines[0], "output 1");
    assert_eq!(stdout_lines[1], "output 2");
    assert_eq!(stdout_lines[2], "output 3");
    assert_eq!(stdout_lines[3], "output 4");
    assert_eq!(stdout_lines[4], "output 5");
}

#[tokio::test]
async fn test_visible_len_calculation() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "echo line1; echo line2; echo line3".to_string(),
    ];

    let mut panel = Panel::new(
        "test".to_string(),
        cmd.clone(),
        None,
        true,  // show_stdout
        false, // show_stderr
    );

    let mut proc = spawn_process(0, &cmd, None, tx);

    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    // Collect events and add them to the panel
    let deadline = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                match event {
                    UiEvent::Line { stream, text, .. } => {
                        match stream {
                            StreamKind::Stdout => panel.stdout.push(&text),
                            StreamKind::Stderr => panel.stderr.push(&text),
                        }
                    }
                    _ => {}
                }
            }
            _ = &mut deadline => break,
        }
    }

    // After adding 3 lines, rope.len_lines() = 4 (3 lines + empty line after final \n)
    assert_eq!(panel.stdout.rope.len_lines(), 4);

    // But visible_len should return 3 (the actual number of text lines)
    assert_eq!(visible_len(&panel), 3);

    // When following, scroll should be set to visible_len - 1 = 2
    // This means we start rendering from line index 2, which shows line3
    let scroll = visible_len(&panel).saturating_sub(1);
    assert_eq!(scroll, 2);

    // Verify we can access all lines
    let all_lines: Vec<String> = panel.stdout.rope.lines().map(|l| l.to_string()).collect();
    assert_eq!(all_lines.len(), 4); // includes empty line
    assert_eq!(all_lines[0], "line1\n");
    assert_eq!(all_lines[1], "line2\n");
    assert_eq!(all_lines[2], "line3\n");
    assert_eq!(all_lines[3], ""); // empty line after final newline
}

#[tokio::test]
async fn test_scroll_with_continuous_output() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "for i in 1 2 3 4 5; do echo \"line $i\"; done".to_string(),
    ];

    let mut panel = Panel::new(
        "test".to_string(),
        cmd.clone(),
        None,
        true, // show_stdout
        true, // show_stderr
    );

    let mut proc = spawn_process(0, &cmd, None, tx);

    // Simulate the scroll update logic from app.rs
    let mut scroll = 0;
    let follow = true;

    let deadline = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                match event {
                    UiEvent::Line { stream, text, .. } => {
                        let at_bottom = follow;

                        match stream {
                            StreamKind::Stdout => panel.stdout.push(&text),
                            StreamKind::Stderr => panel.stderr.push(&text),
                        }

                        if at_bottom {
                            scroll = visible_len(&panel).saturating_sub(1);
                        }
                    }
                    _ => {}
                }
            }
            result = proc.child.wait() => {
                assert!(result.is_ok());
                // Continue collecting any remaining events
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            _ = &mut deadline => break,
        }
    }

    // After 5 lines, visible_len should be 5
    assert_eq!(visible_len(&panel), 5);

    // Scroll should be at the last line (4)
    assert_eq!(scroll, 4);

    // Verify we can render from scroll position
    let all_lines: Vec<String> = panel.stdout.rope.lines().map(|l| l.to_string()).collect();
    assert!(
        scroll < all_lines.len(),
        "Scroll position should be within bounds"
    );

    // The line at scroll position should be the last text line
    assert_eq!(all_lines[scroll], "line 5\n");
}

#[tokio::test]
async fn test_draw_logic_with_few_lines() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "echo line1; echo line2; echo line3".to_string(),
    ];

    let mut panel = Panel::new("test".to_string(), cmd.clone(), None, true, false);

    let mut proc = spawn_process(0, &cmd, None, tx);

    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    // Collect events and update panel
    let deadline = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(deadline);

    let mut scroll = 0;
    let follow = true;

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                if let UiEvent::Line { stream: StreamKind::Stdout, text, .. } = event {
                    let at_bottom = follow;
                    panel.stdout.push(&text);
                    if at_bottom {
                        scroll = visible_len(&panel).saturating_sub(1);
                    }
                }
            }
            _ = &mut deadline => break,
        }
    }

    // Simulate draw function with large terminal (height > number of lines)
    let height: usize = 10;
    let mut lines: Vec<String> = panel.stdout.rope.lines().map(|l| l.to_string()).collect();

    // Skip trailing empty line
    if let Some(last) = lines.last() {
        if last.is_empty() {
            lines.pop();
        }
    }

    let start = scroll
        .saturating_sub(height.saturating_sub(1))
        .min(lines.len());
    let end = (scroll + 1).min(lines.len());

    // With 3 lines, scroll=2, height=10:
    // start = 2.saturating_sub(9) = 0
    // end = min(3, 3) = 3
    // Should show all 3 lines
    assert_eq!(start, 0, "Start should be 0 when content fits");
    assert_eq!(end, 3, "End should be 3 (all lines)");
    assert_eq!(
        lines[start..end].len(),
        3,
        "Should show all 3 lines when terminal is large"
    );
    assert_eq!(lines[0], "line1\n");
    assert_eq!(lines[1], "line2\n");
    assert_eq!(lines[2], "line3\n");
}

#[tokio::test]
async fn test_draw_logic_with_many_lines() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "for i in {1..10}; do echo \"line $i\"; done".to_string(),
    ];

    let mut panel = Panel::new("test".to_string(), cmd.clone(), None, true, false);

    let mut proc = spawn_process(0, &cmd, None, tx);

    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    // Collect events and update panel
    let deadline = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(deadline);

    let mut scroll = 0;
    let follow = true;

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                if let UiEvent::Line { stream: StreamKind::Stdout, text, .. } = event {
                    let at_bottom = follow;
                    panel.stdout.push(&text);
                    if at_bottom {
                        scroll = visible_len(&panel).saturating_sub(1);
                    }
                }
            }
            _ = &mut deadline => break,
        }
    }

    // Simulate draw function with small terminal (height < number of lines)
    let height: usize = 3;
    let mut lines: Vec<String> = panel.stdout.rope.lines().map(|l| l.to_string()).collect();

    // Skip trailing empty line
    if let Some(last) = lines.last() {
        if last.is_empty() {
            lines.pop();
        }
    }

    let start = scroll
        .saturating_sub(height.saturating_sub(1))
        .min(lines.len());
    let end = (scroll + 1).min(lines.len());

    // With 10 lines, scroll=9, height=3:
    // start = 9.saturating_sub(2) = 7
    // end = min(10, 10) = 10
    // Should show last 3 lines: lines[7..10] = [line8, line9, line10]
    assert_eq!(start, 7, "Start should show last 3 lines");
    assert_eq!(end, 10, "End should include all lines");
    assert_eq!(
        lines[start..end].len(),
        3,
        "Should show exactly 3 lines (terminal height)"
    );
    assert_eq!(lines[7], "line 8\n");
    assert_eq!(lines[8], "line 9\n");
    assert_eq!(lines[9], "line 10\n");
}

#[tokio::test]
async fn test_toggle_stream_visibility() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "echo stdout1; echo stderr1 >&2; echo stdout2; echo stderr2 >&2".to_string(),
    ];

    let mut panel = Panel::new("test".to_string(), cmd.clone(), None, true, false);

    let mut proc = spawn_process(0, &cmd, None, tx);

    let status = timeout(Duration::from_secs(2), proc.child.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    // Collect events
    let deadline = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                if let UiEvent::Line { stream, text, .. } = event {
                    match stream {
                        StreamKind::Stdout => panel.stdout.push(&text),
                        StreamKind::Stderr => panel.stderr.push(&text),
                    }
                }
            }
            _ = &mut deadline => break,
        }
    }

    // Verify initial state: only stdout shown
    assert_eq!(visible_len(&panel), 2);

    // Toggle stderr on
    panel.show_stderr = true;
    let max = visible_len(&panel).saturating_sub(1);
    panel.scroll = max;
    panel.follow = true;

    // Now should show 4 lines (2 stdout + 2 stderr)
    assert_eq!(visible_len(&panel), 4);
    assert_eq!(panel.scroll, 3, "Scroll should be at bottom");
    assert_eq!(panel.follow, true, "Should be following");

    // Toggle stderr off
    panel.show_stderr = false;
    let max = visible_len(&panel).saturating_sub(1);
    panel.scroll = panel.scroll.min(max);
    panel.follow = panel.scroll == max;

    // Back to 2 lines
    assert_eq!(visible_len(&panel), 2);
    assert_eq!(panel.scroll, 1, "Scroll should be clamped to bottom");
    assert_eq!(panel.follow, true, "Should still be following");

    // Toggle stdout off
    panel.show_stdout = false;
    let max = visible_len(&panel).saturating_sub(1);
    panel.scroll = panel.scroll.min(max);
    panel.follow = panel.scroll == max;

    // No lines shown
    assert_eq!(visible_len(&panel), 0);
    assert_eq!(panel.scroll, 0, "Scroll should be 0");

    // Toggle stdout back on
    panel.show_stdout = true;
    let max = visible_len(&panel).saturating_sub(1);
    panel.scroll = max;
    panel.follow = true;

    // Back to 2 lines, should be at bottom
    assert_eq!(visible_len(&panel), 2);
    assert_eq!(panel.scroll, 1, "Scroll should be at bottom");
    assert_eq!(panel.follow, true, "Should be following");
}

#[tokio::test]
async fn test_terminate_multiple_processes() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);

    // Spawn multiple long-running processes
    let cmd1 = vec!["sleep".to_string(), "0.1".to_string()];
    let cmd2 = vec!["sleep".to_string(), "0.1".to_string()];
    let cmd3 = vec!["sleep".to_string(), "0.1".to_string()];

    let mut proc1 = spawn_process(0, &cmd1, None, tx.clone());
    let mut proc2 = spawn_process(1, &cmd2, None, tx.clone());
    let mut proc3 = spawn_process(2, &cmd3, None, tx.clone());

    // Give processes time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify all processes are running
    assert!(proc1.child.try_wait().unwrap().is_none());
    assert!(proc2.child.try_wait().unwrap().is_none());
    assert!(proc3.child.try_wait().unwrap().is_none());

    // Terminate all processes (simulating Exit event handler)
    terminate_child(&mut proc1.child).await;
    terminate_child(&mut proc2.child).await;
    terminate_child(&mut proc3.child).await;

    // Verify all processes are terminated
    let status1 = proc1.child.try_wait().unwrap();
    let status2 = proc2.child.try_wait().unwrap();
    let status3 = proc3.child.try_wait().unwrap();

    assert!(status1.is_some(), "Process 1 should be terminated");
    assert!(status2.is_some(), "Process 2 should be terminated");
    assert!(status3.is_some(), "Process 3 should be terminated");

    // Clean up receiver
    drop(rx);
}
