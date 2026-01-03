use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio::time::timeout;

use rote::panel::{MessageKind, Panel, StreamKind};
use rote::process::RunningProcess;
use rote::ui::UiEvent;

#[tokio::test]
async fn test_panel_stderr_buffer() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);
    let (shutdown_tx, _) = broadcast::channel::<()>(16);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "echo err1 >&2; echo err2 >&2".to_string(),
    ];

    let mut panel = Panel::new(
        "test".to_string(),
        cmd.clone(),
        None,
        true,  // show_stdout
        true,  // show_stderr
        false, // timestamps
    );

    let mut proc = RunningProcess::spawn(0, &cmd, None, tx, shutdown_tx.subscribe());

    let status = timeout(Duration::from_secs(2), proc.wait())
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
                        let kind = match stream {
                            StreamKind::Stdout => MessageKind::Stdout,
                            StreamKind::Stderr => MessageKind::Stderr,
                        };
                        panel.messages.push(kind, &text, None);
                    }
                    _ => {}
                }
            }
            _ = &mut deadline => break,
        }
    }

    // Verify the panel buffer contains lines
    assert_eq!(panel.messages.rope.len_lines(), 3); // 2 lines + final newline = 3 lines in rope

    let lines: Vec<String> = panel
        .messages
        .rope
        .lines()
        .map(|line| line.to_string())
        .collect();

    assert_eq!(lines[0], "\x1Ee\x1Ferr1\n");
    assert_eq!(lines[1], "\x1Ee\x1Ferr2\n");
}

#[tokio::test]
async fn test_continuous_output() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);
    let (shutdown_tx, _) = broadcast::channel::<()>(16);

    // Simulate a continuously outputting process like ping
    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "for i in 1 2 3 4 5; do echo \"output $i\"; sleep 0.05; done".to_string(),
    ];

    let mut proc = RunningProcess::spawn(0, &cmd, None, tx, shutdown_tx.subscribe());

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
            result = proc.wait(), if !process_done => {
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
    let (shutdown_tx, _) = broadcast::channel::<()>(16);

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
        true,  // show_stderr
        false, // timestamps
    );

    let mut proc = RunningProcess::spawn(0, &cmd, None, tx, shutdown_tx.subscribe());

    let status = timeout(Duration::from_secs(2), proc.wait())
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
                        let kind = match stream {
                            StreamKind::Stdout => MessageKind::Stdout,
                            StreamKind::Stderr => MessageKind::Stderr,
                        };
                        panel.messages.push(kind, &text, None);
                    }
                    _ => {}
                }
            }
            _ = &mut deadline => break,
        }
    }

    // After adding 3 lines, rope.len_lines() = 4 (3 lines + empty line after final \n)
    assert_eq!(panel.messages.rope.len_lines(), 4);

    // But visible_len should return 3 (the actual number of text lines)
    assert_eq!(panel.visible_len(), 3);

    // When following, scroll should be set to visible_len - 1 = 2
    let scroll = panel.visible_len().saturating_sub(1);
    assert_eq!(scroll, 2);

    // Verify we can access all lines
    let all_lines: Vec<String> = panel.messages.rope.lines().map(|l| l.to_string()).collect();
    assert_eq!(all_lines.len(), 4); // includes empty line
    assert_eq!(all_lines[0], "\x1Eo\x1Fline1\n");
    assert_eq!(all_lines[1], "\x1Eo\x1Fline2\n");
    assert_eq!(all_lines[2], "\x1Eo\x1Fline3\n");
    assert_eq!(all_lines[3], ""); // empty line after final newline
}

#[tokio::test]
async fn test_scroll_with_continuous_output() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);
    let (shutdown_tx, _) = broadcast::channel::<()>(16);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "for i in 1 2 3 4 5; do echo \"line $i\"; done".to_string(),
    ];

    let mut panel = Panel::new(
        "test".to_string(),
        cmd.clone(),
        None,
        true,  // show_stdout
        true,  // show_stderr
        false, // timestamps
    );

    let mut proc = RunningProcess::spawn(0, &cmd, None, tx, shutdown_tx.subscribe());

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

                        let kind = match stream {
                            StreamKind::Stdout => MessageKind::Stdout,
                            StreamKind::Stderr => MessageKind::Stderr,
                        };
                        panel.messages.push(kind, &text, None);

                        if at_bottom {
                            scroll = panel.visible_len().saturating_sub(1);
                        }
                    }
                    _ => {}
                }
            }
            result = proc.wait() => {
                assert!(result.is_ok());
                // Continue collecting any remaining events
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            _ = &mut deadline => break,
        }
    }

    // After 5 lines, visible_len should be 5
    assert_eq!(panel.visible_len(), 5);

    // Scroll should be at the last line (4)
    assert_eq!(scroll, 4);

    // Verify we can render from scroll position
    let all_lines: Vec<String> = panel.messages.rope.lines().map(|l| l.to_string()).collect();
    assert!(
        scroll < all_lines.len(),
        "Scroll position should be within bounds"
    );

    // The line at scroll position should be last text line
    assert_eq!(all_lines[scroll], "\x1Eo\x1Fline 5\n");
}

#[tokio::test]
async fn test_draw_logic_with_few_lines() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);
    let (shutdown_tx, _) = broadcast::channel::<()>(16);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "echo line1; echo line2; echo line3".to_string(),
    ];

    let mut panel = Panel::new("test".to_string(), cmd.clone(), None, true, false, false);

    let mut proc = RunningProcess::spawn(0, &cmd, None, tx, shutdown_tx.subscribe());

    let status = timeout(Duration::from_secs(2), proc.wait())
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
                    panel.messages.push(MessageKind::Stdout, &text, None);
                    if at_bottom {
                        scroll = panel.visible_len().saturating_sub(1);
                    }
                }
            }
            _ = &mut deadline => break
        }
    }

    // Simulate draw function with large terminal (height > number of lines)
    let height: usize = 10;
    let filtered_lines =
        panel
            .messages
            .lines_filtered(panel.show_stdout, panel.show_stderr, panel.show_status);

    let start = scroll
        .saturating_sub(height.saturating_sub(1))
        .min(filtered_lines.len());
    let end = (scroll + 1).min(filtered_lines.len());

    // With 3 lines, scroll=2, height=10:
    // start = 2.saturating_sub(9) = 0
    // end = min(3, 3) = 3
    // Should show all 3 lines
    assert_eq!(start, 0, "Start should be 0 when content fits");
    assert_eq!(end, 3, "End should be 3 (all lines)");
    assert_eq!(
        filtered_lines[start..end].len(),
        3,
        "Should show all 3 lines when terminal is large"
    );
    assert_eq!(filtered_lines[0].1, "line1");
    assert_eq!(filtered_lines[1].1, "line2");
    assert_eq!(filtered_lines[2].1, "line3");
}

#[tokio::test]
async fn test_draw_logic_with_scrolling() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);
    let (shutdown_tx, _) = broadcast::channel::<()>(16);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "for i in 1 2 3 4 5 6 7 8 9 10; do echo \"line $i\"; done".to_string(),
    ];

    let mut panel = Panel::new("test".to_string(), cmd.clone(), None, true, false, false);

    let mut proc = RunningProcess::spawn(0, &cmd, None, tx, shutdown_tx.subscribe());

    let status = timeout(Duration::from_secs(2), proc.wait())
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
                    panel.messages.push(MessageKind::Stdout, &text, None);
                    if at_bottom {
                        scroll = panel.visible_len().saturating_sub(1);
                    }
                }
            }
            _ = &mut deadline => break,
        }
    }

    // Simulate draw function with small terminal (height < number of lines)
    let height: usize = 3;
    let filtered_lines =
        panel
            .messages
            .lines_filtered(panel.show_stdout, panel.show_stderr, panel.show_status);

    let start = scroll
        .saturating_sub(height.saturating_sub(1))
        .min(filtered_lines.len());
    let end = (scroll + 1).min(filtered_lines.len());

    // With 10 lines, scroll=9, height=3:
    // start = 9.saturating_sub(2) = 7
    // end = min(10, 10) = 10
    // Should show last 3 lines: lines[7..10] = [line8, line9, line10]
    assert_eq!(start, 7, "Start should show last 3 lines");
    assert_eq!(end, 10, "End should include all lines");
    assert_eq!(
        filtered_lines[start..end].len(),
        3,
        "Should show exactly 3 lines (terminal height)"
    );
    assert_eq!(filtered_lines[7].1, "line 8");
    assert_eq!(filtered_lines[8].1, "line 9");
    assert_eq!(filtered_lines[9].1, "line 10");
}

#[tokio::test]
async fn test_mixed_output_order_preservation() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);
    let (shutdown_tx, _) = broadcast::channel::<()>(16);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "echo 'stdout1'; echo 'stderr1' >&2; echo 'stdout2'; echo 'stderr2' >&2; echo 'stdout3'; echo 'stderr3' >&2".to_string(),
    ];

    let mut panel = Panel::new("test".to_string(), cmd.clone(), None, true, true, false);

    let mut proc = RunningProcess::spawn(0, &cmd, None, tx, shutdown_tx.subscribe());

    let status = timeout(Duration::from_secs(2), proc.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    let mut all_events = Vec::new();

    let deadline = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                if let UiEvent::Line { stream, text, .. } = event {
                    all_events.push((stream, text.clone()));
                    let kind = match stream {
                        StreamKind::Stdout => MessageKind::Stdout,
                        StreamKind::Stderr => MessageKind::Stderr,
                    };
                    panel.messages.push(kind, &text, None);
                }
            }
            _ = &mut deadline => break,
        }
    }

    // Verify all events were received
    assert_eq!(all_events.len(), 6);

    // Collect stdout and stderr events separately and verify their order
    let stdout_events: Vec<_> = all_events
        .iter()
        .filter(|(stream, _)| *stream == StreamKind::Stdout)
        .map(|(_, text)| text.clone())
        .collect();

    let stderr_events: Vec<_> = all_events
        .iter()
        .filter(|(stream, _)| *stream == StreamKind::Stderr)
        .map(|(_, text)| text.clone())
        .collect();

    assert_eq!(stdout_events, vec!["stdout1", "stdout2", "stdout3"]);
    assert_eq!(stderr_events, vec!["stderr1", "stderr2", "stderr3"]);

    // With both streams visible, total should be 6
    assert_eq!(panel.visible_len(), 6);

    // Verify order using lines_filtered
    let filtered_lines =
        panel
            .messages
            .lines_filtered(panel.show_stdout, panel.show_stderr, panel.show_status);

    // Note: Due to buffering, stderr messages may come before stdout messages
    // The important thing is that chronological order is preserved
    assert_eq!(filtered_lines.len(), 6);

    // Just verify that all expected messages are present
    let messages: Vec<_> = filtered_lines
        .iter()
        .map(|(_, text)| text.as_str())
        .collect();
    assert!(messages.contains(&"stdout1"));
    assert!(messages.contains(&"stdout2"));
    assert!(messages.contains(&"stdout3"));
    assert!(messages.contains(&"stderr1"));
    assert!(messages.contains(&"stderr2"));
    assert!(messages.contains(&"stderr3"));

    // Toggle stderr off - order of stdout should be preserved
    panel.show_stderr = false;
    assert_eq!(panel.visible_len(), 3);

    let stdout_only =
        panel
            .messages
            .lines_filtered(panel.show_stdout, panel.show_stderr, panel.show_status);
    assert_eq!(stdout_only.len(), 3);
    assert_eq!(stdout_only[0].0, MessageKind::Stdout);
    assert_eq!(stdout_only[0].1, "stdout1");
    assert_eq!(stdout_only[1].0, MessageKind::Stdout);
    assert_eq!(stdout_only[1].1, "stdout2");
    assert_eq!(stdout_only[2].0, MessageKind::Stdout);
    assert_eq!(stdout_only[2].1, "stdout3");

    // Toggle stderr back on - both should still be present
    panel.show_stderr = true;
    assert_eq!(panel.visible_len(), 6);
    let both =
        panel
            .messages
            .lines_filtered(panel.show_stdout, panel.show_stderr, panel.show_status);
    assert_eq!(both.len(), 6);

    // Assert that stdout1 comes before stdout2 and stdout3, and similarly for stderr
    {
        let both_texts: Vec<_> = both
            .iter()
            .map(|(kind, text)| (kind, text.as_str()))
            .collect();

        // Find indices for each stdout and stderr message
        let idx_stdout1 = both_texts
            .iter()
            .position(|(k, t)| **k == MessageKind::Stdout && *t == "stdout1")
            .expect("stdout1 not found");
        let idx_stdout2 = both_texts
            .iter()
            .position(|(k, t)| **k == MessageKind::Stdout && *t == "stdout2")
            .expect("stdout2 not found");
        let idx_stdout3 = both_texts
            .iter()
            .position(|(k, t)| **k == MessageKind::Stdout && *t == "stdout3")
            .expect("stdout3 not found");
        let idx_stderr1 = both_texts
            .iter()
            .position(|(k, t)| **k == MessageKind::Stderr && *t == "stderr1")
            .expect("stderr1 not found");
        let idx_stderr2 = both_texts
            .iter()
            .position(|(k, t)| **k == MessageKind::Stderr && *t == "stderr2")
            .expect("stderr2 not found");
        let idx_stderr3 = both_texts
            .iter()
            .position(|(k, t)| **k == MessageKind::Stderr && *t == "stderr3")
            .expect("stderr3 not found");

        // Assert order for stdout
        assert!(
            idx_stdout1 < idx_stdout2,
            "stdout1 should come before stdout2"
        );
        assert!(
            idx_stdout2 < idx_stdout3,
            "stdout2 should come before stdout3"
        );

        // Assert order for stderr
        assert!(
            idx_stderr1 < idx_stderr2,
            "stderr1 should come before stderr2"
        );
        assert!(
            idx_stderr2 < idx_stderr3,
            "stderr2 should come before stderr3"
        );
    }
}

#[tokio::test]
async fn test_colored_output() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);
    let (shutdown_tx, _) = broadcast::channel::<()>(16);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "echo -e '\\033[31mRed text\\033[0m'; echo -e '\\033[32mGreen text\\033[0m'; echo -e '\\033[34mBlue text\\033[0m'".to_string(),
    ];

    let mut panel = Panel::new("test".to_string(), cmd.clone(), None, true, false, false);

    let mut proc = RunningProcess::spawn(0, &cmd, None, tx, shutdown_tx.subscribe());

    let status = timeout(Duration::from_secs(2), proc.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    let deadline = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                if let UiEvent::Line { stream, text, .. } = event {
                    let kind = match stream {
                        StreamKind::Stdout => MessageKind::Stdout,
                        StreamKind::Stderr => MessageKind::Stderr,
                    };
                    panel.messages.push(kind, &text, None);
                }
            }
            _ = &mut deadline => break,
        }
    }

    let stdout_lines: Vec<String> = panel.messages.rope.lines().map(|l| l.to_string()).collect();
    assert_eq!(stdout_lines.len(), 4);

    assert!(
        stdout_lines[0].contains("\x1b[31m"),
        "Should contain red color code"
    );
    assert!(stdout_lines[0].contains("Red text"), "Should contain text");
    assert!(
        stdout_lines[0].contains("\x1b[0m"),
        "Should contain reset code"
    );

    assert!(
        stdout_lines[1].contains("\x1b[32m"),
        "Should contain green color code"
    );
    assert!(
        stdout_lines[1].contains("Green text"),
        "Should contain text"
    );
    assert!(
        stdout_lines[1].contains("\x1b[0m"),
        "Should contain reset code"
    );

    assert!(
        stdout_lines[2].contains("\x1b[34m"),
        "Should contain blue color code"
    );
    assert!(stdout_lines[2].contains("Blue text"), "Should contain text");
    assert!(
        stdout_lines[2].contains("\x1b[0m"),
        "Should contain reset code"
    );
}

#[tokio::test]
async fn test_visible_len_with_stream_toggles() {
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);
    let (shutdown_tx, _) = broadcast::channel::<()>(16);

    let cmd = vec![
        "bash".to_string(),
        "-c".to_string(),
        "echo stdout1; echo stdout2; echo stderr1 >&2; echo stderr2 >&2".to_string(),
    ];

    let mut panel = Panel::new("test".to_string(), cmd.clone(), None, true, false, false);

    let mut proc = RunningProcess::spawn(0, &cmd, None, tx, shutdown_tx.subscribe());

    let status = timeout(Duration::from_secs(2), proc.wait())
        .await
        .expect("Process timed out")
        .expect("Failed to wait for process");

    assert!(status.success());

    // Collect events
    let deadline = tokio::time::sleep(Duration::from_millis(1000));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                if let UiEvent::Line { stream, text, .. } = event {
                    let kind = match stream {
                        StreamKind::Stdout => MessageKind::Stdout,
                        StreamKind::Stderr => MessageKind::Stderr,
                    };
                    panel.messages.push(kind, &text, None);
                }
            }
            _ = &mut deadline => break,
        }
    }

    // Verify initial state: only stdout shown
    assert_eq!(panel.visible_len(), 2);

    // Toggle stderr on
    panel.show_stderr = true;
    let max = panel.visible_len().saturating_sub(1);
    panel.scroll = max;
    panel.follow = true;

    // Now should show 4 lines (2 stdout + 2 stderr)
    assert_eq!(panel.visible_len(), 4);
    assert_eq!(panel.scroll, 3, "Scroll should be at bottom");
    assert_eq!(panel.follow, true, "Should be following");

    // Toggle stderr off
    panel.show_stderr = false;
    let max = panel.visible_len().saturating_sub(1);
    panel.scroll = panel.scroll.min(max);
    panel.follow = panel.scroll == max;

    // Back to 2 stdout lines
    assert_eq!(panel.visible_len(), 2);
    assert_eq!(panel.scroll, 1, "Scroll should be clamped to bottom");
    assert_eq!(panel.follow, true, "Should still be following");

    // Toggle stdout off
    panel.show_stdout = false;
    let max = panel.visible_len().saturating_sub(1);
    panel.scroll = panel.scroll.min(max);
    panel.follow = panel.scroll == max;

    // No lines shown
    assert_eq!(panel.visible_len(), 0);
    assert_eq!(panel.scroll, 0, "Scroll should be 0");

    // Toggle stdout back on
    panel.show_stdout = true;
    let max = panel.visible_len().saturating_sub(1);
    panel.scroll = max;
    panel.follow = true;

    // Back to 2 lines, should be at bottom
    assert_eq!(panel.visible_len(), 2);
    assert_eq!(panel.scroll, 1, "Scroll should be at bottom");
    assert_eq!(panel.follow, true, "Should be following");
}

#[tokio::test]
async fn test_terminate_multiple_processes() {
    let (tx, rx) = mpsc::channel::<UiEvent>(100);
    let (shutdown_tx, _) = broadcast::channel::<()>(16);

    // Spawn multiple long-running processes
    let cmd1 = vec!["sleep".to_string(), "0.1".to_string()];
    let cmd2 = vec!["sleep".to_string(), "0.1".to_string()];
    let cmd3 = vec!["sleep".to_string(), "0.1".to_string()];

    let mut proc1 = RunningProcess::spawn(0, &cmd1, None, tx.clone(), shutdown_tx.subscribe());
    let mut proc2 = RunningProcess::spawn(
        1,
        &cmd2,
        None,
        tx.clone(),
        shutdown_tx.subscribe().resubscribe(),
    );
    let mut proc3 = RunningProcess::spawn(
        2,
        &cmd3,
        None,
        tx.clone(),
        shutdown_tx.subscribe().resubscribe(),
    );

    // Give processes time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify all processes are running
    assert!(proc1.try_wait().unwrap().is_none());
    assert!(proc2.try_wait().unwrap().is_none());
    assert!(proc3.try_wait().unwrap().is_none());

    // Terminate all processes (simulating Exit event handler)
    proc1.terminate().await;
    proc2.terminate().await;
    proc3.terminate().await;

    // Verify all processes are terminated
    let status1 = proc1.try_wait().unwrap();
    let status2 = proc2.try_wait().unwrap();
    let status3 = proc3.try_wait().unwrap();

    assert!(status1.is_some(), "Process 1 should be terminated");
    assert!(status2.is_some(), "Process 2 should be terminated");
    assert!(status3.is_some(), "Process 3 should be terminated");

    // Clean up receiver
    drop(rx);
}
