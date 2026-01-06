use std::{collections::HashMap, io, io::Write, path::PathBuf, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, prelude::CrosstermBackend};

const UI_EVENT_CHANNEL_SIZE: usize = 1024;
const SHUTDOWN_CHANNEL_SIZE: usize = 16;
const STATUS_CHECK_INTERVAL_MS: u64 = 250;
const KEYBOARD_POLL_INTERVAL_MS: u64 = 250;

use crate::{
    config::{Config, TaskAction},
    panel::{MessageKind, Panel, PanelIndex, StatusPanel, StreamKind},
    process::TaskInstance,
    render,
    signals::is_process_exited_by_pid,
    task_manager::{TaskManager, resolve_dependencies},
    ui::{ProcessStatus, UiEvent},
};
fn format_timestamp(timestamps: bool) -> Option<String> {
    if timestamps {
        Some(
            chrono::Local::now()
                .format("[%Y-%m-%d %H:%M:%S]")
                .to_string(),
        )
    } else {
        None
    }
}

pub async fn run(config: Config, tasks_to_run: Vec<String>, config_dir: PathBuf) -> io::Result<()> {
    run_with_input(config, tasks_to_run, config_dir, None).await
}

pub async fn run_with_input(
    config: Config,
    tasks_to_run: Vec<String>,
    config_dir: PathBuf,
    mut external_rx: Option<tokio::sync::mpsc::Receiver<UiEvent>>,
) -> io::Result<()> {
    let enable_terminal = external_rx.is_none();
    if enable_terminal {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
    }

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let (internal_tx, mut internal_rx) =
        tokio::sync::mpsc::channel::<UiEvent>(UI_EVENT_CHANNEL_SIZE);

    // The sender to use for spawning processes - always internal_tx
    let tx = internal_tx.clone();
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(SHUTDOWN_CHANNEL_SIZE);

    // Resolve which tasks to run
    let target_tasks = if tasks_to_run.is_empty() {
        if let Some(default) = &config.default {
            vec![default.clone()]
        } else {
            vec![]
        }
    } else {
        tasks_to_run
    };

    // Resolve all dependencies to get the full list of tasks to start
    let tasks_list = resolve_dependencies(&config, &target_tasks)?;

    // Create panels for ALL tasks with actions (not just those being started)
    // Panels are ordered according to their order in the YAML config file
    let mut panels = Vec::new();
    let mut task_to_panel: HashMap<String, PanelIndex> = HashMap::new();

    // Collect task names, preserving YAML file order (IndexMap preserves insertion order)
    let task_names: Vec<_> = config
        .tasks
        .iter()
        .filter(|(_, cfg)| {
            matches!(
                cfg.action,
                Some(TaskAction::Run { .. }) | Some(TaskAction::Ensure { .. })
            )
        })
        .map(|(name, _)| name.clone())
        .collect();

    for task_name in &task_names {
        let task_config = config.tasks.get(task_name).unwrap();
        // Create panels for tasks with "run" or "ensure" actions
        if let Some(TaskAction::Run { command }) | Some(TaskAction::Ensure { command }) =
            &task_config.action
        {
            let cmd = shell_words::split(&command.as_command()).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Failed to parse command: {e}"),
                )
            })?;

            let cwd = task_config.cwd.as_ref().map(|c| {
                let path = config_dir.join(c);
                path.to_string_lossy().to_string()
            });

            // Determine which streams to show
            let (show_stdout, show_stderr) = match &task_config.display {
                None => (true, true), // Show both by default
                Some(streams) => {
                    if streams.is_empty() {
                        (false, false) // Empty list means show nothing
                    } else {
                        let show_stdout = streams.iter().any(|s| s == "stdout");
                        let show_stderr = streams.iter().any(|s| s == "stderr");
                        (show_stdout, show_stderr)
                    }
                }
            };

            task_to_panel.insert(task_name.clone(), PanelIndex::new(panels.len()));
            panels.push(Panel::new(
                task_name.clone(),
                cmd,
                cwd,
                show_stdout,
                show_stderr,
                task_config.timestamps,
            ));
        }
    }

    if panels.is_empty() {
        disable_raw_mode()?;
        eprintln!("No tasks with 'run' or 'ensure' action to display");
        return Ok(());
    }

    // Initialize status panel with all tasks that have actions (YAML file order)
    let mut status_panel = StatusPanel::new();
    for task_name in &task_names {
        let task_config = config.tasks.get(task_name).unwrap();
        if let Some(action) = &task_config.action {
            // Tasks in tasks_list are being started, others show as "Not started"
            let initial_status = if tasks_list.contains(task_name) {
                ProcessStatus::Running
            } else {
                ProcessStatus::NotStarted
            };
            status_panel.update_entry_with_action(
                task_name.clone(),
                initial_status,
                action.clone(),
            );
            status_panel
                .entry_indices
                .insert(task_name.clone(), usize::MAX);
            status_panel.update_dependencies(task_name.clone(), task_config.require.clone());
        }
    }

    // Initialize process slots
    let mut procs: Vec<Option<TaskInstance>> = (0..panels.len()).map(|_| None).collect();

    // Task manager tracks pending tasks and completed Run tasks
    let mut task_manager = TaskManager::new(tasks_list.clone(), task_to_panel.clone());

    let mut active = PanelIndex::new(0);
    let mut showing_status = true;
    let mut prev_statuses_storage: Option<Vec<ProcessStatus>> = None;

    // Trigger initial task startup
    let _ = tx.send(UiEvent::StartNextTask).await;

    // Periodic status check task
    let status_check_tx = internal_tx.clone();
    let status_check_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(STATUS_CHECK_INTERVAL_MS));
        loop {
            interval.tick().await;
            // Ignore send errors - if the channel is closed, we're shutting down
            let _ = status_check_tx.send(UiEvent::CheckStatus).await;
        }
    });

    // keyboard - spawn if we created internal_tx (i.e., no external input)
    let keyboard_shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let keyboard_task = if external_rx.is_none() {
        let tx_kb = tx.clone();
        let shutdown_flag = keyboard_shutdown.clone();
        Some(tokio::spawn(async move {
            loop {
                // Check shutdown flag before polling
                if shutdown_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let poll_result = event::poll(Duration::from_millis(KEYBOARD_POLL_INTERVAL_MS));
                match poll_result {
                    Ok(true) => {
                        if let Ok(Event::Key(k)) = event::read() {
                            let ev = match k.code {
                                KeyCode::Char('q') => UiEvent::Exit,
                                KeyCode::Char('r') => UiEvent::Restart,
                                KeyCode::Char('t') => UiEvent::Stop,
                                KeyCode::Char('o') => UiEvent::ToggleStdout,
                                KeyCode::Char('e') => UiEvent::ToggleStderr,
                                KeyCode::Char('s') => UiEvent::SwitchToStatus,
                                KeyCode::Char(c @ '1'..='9') => {
                                    UiEvent::SwitchPanel(PanelIndex::new((c as u8 - b'1') as usize))
                                }
                                KeyCode::Up => UiEvent::Scroll(-1),
                                KeyCode::Down => UiEvent::Scroll(1),
                                KeyCode::PageUp => UiEvent::Scroll(-20),
                                KeyCode::PageDown => UiEvent::Scroll(20),
                                KeyCode::Left => UiEvent::PrevPanel,
                                KeyCode::Right => UiEvent::NextPanel,
                                _ => continue,
                            };
                            // Ignore send errors - if channel is closed, we're shutting down
                            let _ = tx_kb.send(ev).await;
                        }
                    }
                    Ok(false) => {}  // No event available
                    Err(_) => break, // Terminal error, exit keyboard loop
                }
            }
        }))
    } else {
        None
    };

    if showing_status {
        render::draw_status(&mut terminal, &panels, &status_panel)?;
    } else {
        render::draw(&mut terminal, &panels[*active], &status_panel)?;
    }

    loop {
        let ev = if let Some(ref mut external) = external_rx {
            tokio::select! {
                Some(ev) = internal_rx.recv() => Some(ev),
                Some(ev) = external.recv() => Some(ev),
            }
        } else {
            internal_rx.recv().await
        };

        let Some(ev) = ev else {
            break;
        };
        let mut redraw = false;

        match ev {
            UiEvent::Line {
                panel,
                stream,
                text,
            } => {
                let p = &mut panels[*panel];
                let at_bottom = p.follow;

                let kind = match stream {
                    StreamKind::Stdout => MessageKind::Stdout,
                    StreamKind::Stderr => MessageKind::Stderr,
                };
                let timestamp = format_timestamp(p.timestamps);
                p.messages.push(kind, &text, timestamp.as_deref());

                if at_bottom {
                    p.scroll = p.visible_len().saturating_sub(1);
                }

                if panel == active {
                    redraw = true;
                }
            }

            UiEvent::Exited {
                panel,
                status,
                exit_code,
            } => {
                // Skip if a new process is already running (restart handler already added exit message)
                let new_process_running = procs[*panel]
                    .as_ref()
                    .map(|p| !is_process_exited_by_pid(p.pid))
                    .unwrap_or(false);

                if !new_process_running {
                    let p = &mut panels[*panel];
                    let was_following = p.follow;

                    let msg = format!(
                        "[exited: {}]",
                        status.map(|s| s.to_string()).unwrap_or("unknown".into())
                    );
                    let timestamp = format_timestamp(p.timestamps);
                    p.messages
                        .push(MessageKind::Status, &msg, timestamp.as_deref());

                    // Update scroll to show the exit message if following
                    if was_following {
                        let max_len = p.visible_len();
                        if max_len > 0 {
                            p.scroll = max_len - 1;
                        }
                    }
                }

                status_panel.update_exit_code(panels[*panel].task_name.clone(), exit_code);

                // If this was an Ensure task, mark it as completed and try to start more tasks
                let task_name = panels[*panel].task_name.clone();
                if let Some(task_config) = config.tasks.get(&task_name) {
                    if matches!(task_config.action, Some(TaskAction::Ensure { .. })) {
                        // Only mark as completed if it succeeded
                        if exit_code == Some(0) {
                            task_manager.mark_ensure_completed(&task_name);
                            // Try to start more tasks
                            let _ = tx.send(UiEvent::StartNextTask).await;
                        }
                    }

                    // Auto-restart if configured (only for Run tasks, not Ensure tasks)
                    // Skip if a new process is already running (e.g., manual restart already happened)
                    let should_auto_restart = task_config.autorestart
                        && matches!(task_config.action, Some(TaskAction::Run { .. }))
                        && procs[*panel]
                            .as_ref()
                            .map(|p| is_process_exited_by_pid(p.pid))
                            .unwrap_or(true);

                    if should_auto_restart {
                        // Wait for the old process to fully clean up
                        if let Some(proc) = procs[*panel].take() {
                            let _ = proc.wait_task.await;
                            let _ = proc.stdout_task.await;
                            let _ = proc.stderr_task.await;
                        }

                        let p = &mut panels[*panel];
                        let was_following = p.follow;
                        let timestamp = format_timestamp(p.timestamps);
                        p.messages.push(
                            MessageKind::Status,
                            "[auto-restarting]",
                            timestamp.as_deref(),
                        );
                        let max_len = p.visible_len();
                        if max_len > 0 && was_following {
                            p.scroll = max_len - 1;
                        }
                        p.follow = was_following;

                        let cwd = panels[*panel].cwd.as_deref();
                        match TaskInstance::spawn(
                            panel,
                            &panels[*panel].cmd,
                            cwd,
                            tx.clone(),
                            shutdown_tx.subscribe(),
                        ) {
                            Ok(proc) => {
                                procs[*panel] = Some(proc);
                                status_panel
                                    .update_entry(task_name.clone(), ProcessStatus::Running);
                            }
                            Err(e) => {
                                let timestamp = format_timestamp(panels[*panel].timestamps);
                                panels[*panel].messages.push(
                                    MessageKind::Status,
                                    &format!("[auto-restart failed: {e}]"),
                                    timestamp.as_deref(),
                                );
                            }
                        }
                    }
                }

                redraw = true;
            }

            UiEvent::Scroll(delta) => {
                let p = &mut panels[*active];
                let visible_len = p.visible_len();
                let max = visible_len.saturating_sub(1);
                // Scroll operates on logical lines. Allow scrolling from 0 to max.
                // With line wrapping, even a small number of logical lines may need scrolling.
                let new = (p.scroll as i32 + delta).clamp(0, max as i32) as usize;
                p.follow = new == max;
                p.scroll = new;
                redraw = true;
            }

            UiEvent::ToggleStdout => {
                let p = &mut panels[*active];
                p.show_stdout = !p.show_stdout;
                toggle_stream_visibility(p, p.show_stdout);
                redraw = true;
            }

            UiEvent::ToggleStderr => {
                let p = &mut panels[*active];
                p.show_stderr = !p.show_stderr;
                toggle_stream_visibility(p, p.show_stderr);
                redraw = true;
            }

            UiEvent::SwitchPanel(i) if *i < panels.len() => {
                active = i;
                showing_status = false;
                redraw = true;
            }

            UiEvent::SwitchToStatus => {
                showing_status = true;
                redraw = true;
            }

            UiEvent::PrevPanel => {
                if showing_status {
                    // Wrap from status to last panel
                    if !panels.is_empty() {
                        active = PanelIndex::new(panels.len() - 1);
                        showing_status = false;
                    }
                } else if *active == 0 {
                    // Go from first panel to status
                    showing_status = true;
                } else {
                    // Go to previous panel
                    active = PanelIndex::new(*active - 1);
                }
                redraw = true;
            }

            UiEvent::NextPanel => {
                if showing_status {
                    // Go from status to first panel
                    if !panels.is_empty() {
                        active = PanelIndex::new(0);
                        showing_status = false;
                    }
                } else if *active >= panels.len() - 1 {
                    // Go from last panel to status
                    showing_status = true;
                } else {
                    // Go to next panel
                    active = PanelIndex::new(*active + 1);
                }
                redraw = true;
            }

            UiEvent::CheckStatus => {
                let mut prev_statuses = prev_statuses_storage.take().unwrap_or_default();
                if prev_statuses.is_empty() && !procs.is_empty() {
                    // Initialize with correct status based on whether task is in tasks_list
                    prev_statuses = panels
                        .iter()
                        .map(|p| {
                            if tasks_list.contains(&p.task_name) {
                                ProcessStatus::Running
                            } else {
                                ProcessStatus::NotStarted
                            }
                        })
                        .collect();
                }

                let mut any_change = false;

                for (i, proc) in procs.iter_mut().enumerate() {
                    let current_status = if let Some(p) = proc {
                        if p.pid.is_none() || is_process_exited_by_pid(p.pid) {
                            ProcessStatus::Exited
                        } else {
                            ProcessStatus::Running
                        }
                    } else {
                        // Preserve NotStarted status for tasks that were never started
                        match prev_statuses.get(i) {
                            Some(ProcessStatus::NotStarted) => ProcessStatus::NotStarted,
                            _ => ProcessStatus::Exited,
                        }
                    };

                    if prev_statuses.get(i) != Some(&current_status) {
                        any_change = true;
                        status_panel.update_entry(panels[i].task_name.clone(), current_status);
                    }

                    if i >= prev_statuses.len() {
                        prev_statuses.push(current_status);
                    } else {
                        prev_statuses[i] = current_status;
                    }
                }

                if any_change {
                    redraw = true;
                }

                prev_statuses_storage = Some(prev_statuses);
            }

            UiEvent::Restart => {
                // Check if task was NotStarted before we potentially terminate it
                let was_not_started = status_panel
                    .entries
                    .iter()
                    .find(|e| e.task_name == panels[*active].task_name)
                    .map(|e| e.status == ProcessStatus::NotStarted)
                    .unwrap_or(false);

                if let Some(proc) = procs[*active].take() {
                    // Get exit status Arc before awaiting (which partially moves proc)
                    let exit_status_arc = proc.exit_status_arc();

                    proc.terminate().await;
                    // Wait for the process to fully exit and all I/O to drain
                    let _ = proc.wait_task.await;
                    let _ = proc.stdout_task.await;
                    let _ = proc.stderr_task.await;

                    // Add exit message before restart message
                    let exit_status = exit_status_arc.lock().unwrap();
                    if let Some(Ok(status)) = exit_status.as_ref() {
                        use std::os::unix::process::ExitStatusExt;
                        let exit_code = status.code().or_else(|| status.signal().map(|s| 128 + s));
                        let msg = format!(
                            "[exited: {}]",
                            exit_code
                                .map(|c| c.to_string())
                                .unwrap_or_else(|| "unknown".into())
                        );
                        let timestamp = format_timestamp(panels[*active].timestamps);
                        panels[*active].messages.push(
                            MessageKind::Status,
                            &msg,
                            timestamp.as_deref(),
                        );
                    }
                }

                let was_following = panels[*active].follow;
                let timestamp = format_timestamp(panels[*active].timestamps);
                let status_msg = if was_not_started {
                    "[starting]"
                } else {
                    "[restarting]"
                };
                panels[*active].messages.push(
                    MessageKind::Status,
                    status_msg,
                    timestamp.as_deref(),
                );
                let max_len = panels[*active].visible_len();
                if max_len > 0 && was_following {
                    panels[*active].scroll = max_len - 1;
                }
                panels[*active].follow = was_following;

                let cwd = panels[*active].cwd.as_deref();
                match TaskInstance::spawn(
                    active,
                    &panels[*active].cmd,
                    cwd,
                    tx.clone(),
                    shutdown_tx.subscribe(),
                ) {
                    Ok(proc) => {
                        procs[*active] = Some(proc);
                    }
                    Err(e) => {
                        let timestamp = format_timestamp(panels[*active].timestamps);
                        panels[*active].messages.push(
                            MessageKind::Status,
                            &format!("[spawn failed: {e}]"),
                            timestamp.as_deref(),
                        );
                    }
                }
                redraw = true;
            }

            UiEvent::Stop => {
                if let Some(proc) = procs[*active].take() {
                    // Get exit status Arc before awaiting (which partially moves proc)
                    let exit_status_arc = proc.exit_status_arc();

                    proc.terminate().await;
                    // Wait for the process to fully exit and all I/O to drain
                    let _ = proc.wait_task.await;
                    let _ = proc.stdout_task.await;
                    let _ = proc.stderr_task.await;

                    // Add exit message
                    let exit_status = exit_status_arc.lock().unwrap();
                    if let Some(Ok(status)) = exit_status.as_ref() {
                        use std::os::unix::process::ExitStatusExt;
                        let exit_code = status.code().or_else(|| status.signal().map(|s| 128 + s));
                        let msg = format!(
                            "[stopped: {}]",
                            exit_code
                                .map(|c| c.to_string())
                                .unwrap_or_else(|| "unknown".into())
                        );
                        let timestamp = format_timestamp(panels[*active].timestamps);
                        panels[*active].messages.push(
                            MessageKind::Status,
                            &msg,
                            timestamp.as_deref(),
                        );
                    }

                    // Update scroll if following
                    let was_following = panels[*active].follow;
                    let max_len = panels[*active].visible_len();
                    if max_len > 0 && was_following {
                        panels[*active].scroll = max_len - 1;
                    }
                    panels[*active].follow = was_following;

                    redraw = true;
                }
            }

            UiEvent::Exit => {
                // Signal keyboard task to stop and abort status check task
                keyboard_shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
                status_check_task.abort();

                // Exit alternate screen immediately so user sees shutdown progress
                if enable_terminal {
                    let _ = execute!(io::stdout(), LeaveAlternateScreen);
                    let _ = disable_raw_mode();
                }

                // Ignore send errors - if all receivers are gone, shutdown proceeds anyway
                let _ = shutdown_tx.send(());

                // Collect running tasks
                let mut running_tasks: Vec<String> = procs
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| {
                        if p.is_some() {
                            Some(panels[i].task_name.clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                // Helper to print current status (uses ANSI escape to clear to end of line)
                let print_status = |tasks: &[String]| {
                    if tasks.is_empty() {
                        print!("\rWaiting for tasks to shut down: (none)\x1b[K");
                    } else {
                        print!(
                            "\rWaiting for tasks to shut down: [{}]\x1b[K",
                            tasks.join(", ")
                        );
                    }
                    let _ = io::stdout().flush();
                };

                if enable_terminal && !running_tasks.is_empty() {
                    print_status(&running_tasks);
                }

                // Terminate and wait for each process synchronously
                for (i, proc) in procs.iter_mut().enumerate() {
                    if let Some(p) = proc.take() {
                        p.terminate().await;
                        let _ = p.wait_task.await;
                        let _ = p.stdout_task.await;
                        let _ = p.stderr_task.await;

                        // Remove from running list and update display
                        if enable_terminal {
                            running_tasks.retain(|s| s != &panels[i].task_name);
                            print_status(&running_tasks);
                        }
                    }
                }

                // Abort keyboard task if it's still running
                if let Some(ref task) = keyboard_task {
                    task.abort();
                }

                if enable_terminal {
                    println!();
                }

                break;
            }

            UiEvent::StartNextTask => {
                // Try to start the next task(s) whose dependencies are satisfied
                let ready_tasks = task_manager.take_ready_tasks(&config);
                let mut started_any = false;

                for task_name in ready_tasks {
                    if let Some(panel_idx) = task_manager.get_panel_index(&task_name) {
                        let panel = &panels[*panel_idx];
                        let cwd = panel.cwd.as_deref();
                        match TaskInstance::spawn(
                            panel_idx,
                            &panel.cmd,
                            cwd,
                            tx.clone(),
                            shutdown_tx.subscribe(),
                        ) {
                            Ok(proc) => {
                                procs[*panel_idx] = Some(proc);
                                status_panel
                                    .update_entry(task_name.clone(), ProcessStatus::Running);
                                started_any = true;
                            }
                            Err(e) => {
                                let timestamp = format_timestamp(panels[*panel_idx].timestamps);
                                panels[*panel_idx].messages.push(
                                    MessageKind::Status,
                                    &format!("[spawn failed: {e}]"),
                                    timestamp.as_deref(),
                                );
                                status_panel.update_entry(task_name.clone(), ProcessStatus::Exited);
                            }
                        }
                    }
                }

                if started_any {
                    redraw = true;
                }
            }

            _ => {}
        }

        if redraw {
            if showing_status {
                render::draw_status(&mut terminal, &panels, &status_panel)?;
            } else {
                render::draw(&mut terminal, &panels[*active], &status_panel)?;
            }
        }
    }

    for proc in procs.iter().flatten() {
        proc.stdout_task.abort();
        proc.stderr_task.abort();
        proc.wait_task.abort();
    }

    if enable_terminal {
        // Ignore terminal cleanup errors - state may already be restored
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
    Ok(())
}

fn toggle_stream_visibility(panel: &mut Panel, show: bool) {
    if show {
        let max = panel.visible_len().saturating_sub(1);
        panel.scroll = max;
        panel.follow = true;
    } else {
        let max = panel.visible_len().saturating_sub(1);
        panel.scroll = panel.scroll.min(max);
        panel.follow = panel.scroll == max;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    #[test]
    fn test_visible_len_empty_panel() {
        let panel = Panel::new(
            "test".to_string(),
            vec!["echo".to_string()],
            None,
            false,
            false,
            false,
        );
        assert_eq!(panel.visible_len(), 0);
    }

    #[test]
    fn test_visible_len_only_stdout() {
        let mut panel = Panel::new(
            "test".to_string(),
            vec!["echo".to_string()],
            None,
            true,
            false,
            false,
        );
        panel.messages.push(MessageKind::Stdout, "line 1", None);
        panel.messages.push(MessageKind::Stdout, "line 2", None);
        assert_eq!(panel.visible_len(), 2);
    }

    #[test]
    fn test_visible_len_only_stderr() {
        let mut panel = Panel::new(
            "test".to_string(),
            vec!["echo".to_string()],
            None,
            false,
            true,
            false,
        );
        panel.messages.push(MessageKind::Stderr, "error 1", None);
        panel.messages.push(MessageKind::Stderr, "error 2", None);
        panel.messages.push(MessageKind::Stderr, "error 3", None);
        assert_eq!(panel.visible_len(), 3);
    }

    #[test]
    fn test_visible_len_both_streams() {
        let mut panel = Panel::new(
            "test".to_string(),
            vec!["echo".to_string()],
            None,
            true,
            true,
            false,
        );
        panel.messages.push(MessageKind::Stdout, "line 1", None);
        panel.messages.push(MessageKind::Stderr, "error 1", None);
        panel.messages.push(MessageKind::Stdout, "line 2", None);
        assert_eq!(panel.visible_len(), 3);
    }

    #[test]
    fn test_resolve_dependencies_empty() {
        let config = Config {
            default: None,
            tasks: IndexMap::new(),
        };
        let result = resolve_dependencies(&config, &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_resolve_dependencies_no_deps() {
        let mut tasks = IndexMap::new();
        tasks.insert(
            "task1".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
                autorestart: false,
                timestamps: false,
            },
        );

        let config = Config {
            default: None,
            tasks,
        };

        let result = resolve_dependencies(&config, &["task1".to_string()]).unwrap();
        assert_eq!(result, vec!["task1"]);
    }

    #[test]
    fn test_resolve_dependencies_with_deps() {
        let mut tasks = IndexMap::new();
        tasks.insert(
            "task1".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep1".to_string()],
                autorestart: false,
                timestamps: false,
            },
        );
        tasks.insert(
            "dep1".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
                autorestart: false,
                timestamps: false,
            },
        );

        let config = Config {
            default: None,
            tasks,
        };

        let result = resolve_dependencies(&config, &["task1".to_string()]).unwrap();
        assert_eq!(result, vec!["dep1", "task1"]);
    }

    #[test]
    fn test_resolve_dependencies_multiple_deps() {
        let mut tasks = IndexMap::new();
        tasks.insert(
            "task1".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep1".to_string(), "dep2".to_string()],
                autorestart: false,
                timestamps: false,
            },
        );
        tasks.insert(
            "dep1".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
                autorestart: false,
                timestamps: false,
            },
        );
        tasks.insert(
            "dep2".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
                autorestart: false,
                timestamps: false,
            },
        );

        let config = Config {
            default: None,
            tasks,
        };

        let result = resolve_dependencies(&config, &["task1".to_string()]).unwrap();
        assert!(result.contains(&"dep1".to_string()));
        assert!(result.contains(&"dep2".to_string()));
        assert!(result.contains(&"task1".to_string()));
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_resolve_dependencies_nested_deps() {
        let mut tasks = IndexMap::new();
        tasks.insert(
            "task1".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep1".to_string()],
                autorestart: false,
                timestamps: false,
            },
        );
        tasks.insert(
            "dep1".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep2".to_string()],
                autorestart: false,
                timestamps: false,
            },
        );
        tasks.insert(
            "dep2".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
                autorestart: false,
                timestamps: false,
            },
        );

        let config = Config {
            default: None,
            tasks,
        };

        let result = resolve_dependencies(&config, &["task1".to_string()]).unwrap();
        assert_eq!(result, vec!["dep2", "dep1", "task1"]);
    }

    #[test]
    fn test_resolve_dependencies_circular() {
        let mut tasks = IndexMap::new();
        tasks.insert(
            "task1".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["task2".to_string()],
                autorestart: false,
                timestamps: false,
            },
        );
        tasks.insert(
            "task2".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["task1".to_string()],
                autorestart: false,
                timestamps: false,
            },
        );

        let config = Config {
            default: None,
            tasks,
        };

        let result = resolve_dependencies(&config, &["task1".to_string()]);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Circular dependency")
        );
    }

    #[test]
    fn test_resolve_dependencies_task_not_found() {
        let config = Config {
            default: None,
            tasks: IndexMap::new(),
        };

        let result = resolve_dependencies(&config, &["nonexistent".to_string()]);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not found in config")
        );
    }

    #[test]
    fn test_resolve_dependencies_dep_not_found() {
        let mut tasks = IndexMap::new();
        tasks.insert(
            "task1".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["nonexistent".to_string()],
                autorestart: false,
                timestamps: false,
            },
        );

        let config = Config {
            default: None,
            tasks,
        };

        let result = resolve_dependencies(&config, &["task1".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_dependencies_multiple_targets() {
        let mut tasks = IndexMap::new();
        tasks.insert(
            "task1".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep1".to_string()],
                autorestart: false,
                timestamps: false,
            },
        );
        tasks.insert(
            "task2".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep1".to_string()],
                autorestart: false,
                timestamps: false,
            },
        );
        tasks.insert(
            "dep1".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
                autorestart: false,
                timestamps: false,
            },
        );

        let config = Config {
            default: None,
            tasks,
        };

        let result =
            resolve_dependencies(&config, &["task1".to_string(), "task2".to_string()]).unwrap();
        assert!(result.contains(&"dep1".to_string()));
        assert!(result.contains(&"task1".to_string()));
        assert!(result.contains(&"task2".to_string()));
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_resolve_dependencies_diamond_graph() {
        let mut tasks = IndexMap::new();
        tasks.insert(
            "task1".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep1".to_string(), "dep2".to_string()],
                autorestart: false,
                timestamps: false,
            },
        );
        tasks.insert(
            "dep1".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["base".to_string()],
                autorestart: false,
                timestamps: false,
            },
        );
        tasks.insert(
            "dep2".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["base".to_string()],
                autorestart: false,
                timestamps: false,
            },
        );
        tasks.insert(
            "base".to_string(),
            crate::config::TaskConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
                autorestart: false,
                timestamps: false,
            },
        );

        let config = Config {
            default: None,
            tasks,
        };

        let result = resolve_dependencies(&config, &["task1".to_string()]).unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], "base");
        assert!(result.contains(&"dep1".to_string()));
        assert!(result.contains(&"dep2".to_string()));
        assert_eq!(result[3], "task1");
    }
}
