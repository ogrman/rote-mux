use std::{
    collections::{HashMap, HashSet},
    io,
    path::PathBuf,
    time::Duration,
};

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
const SHUTDOWN_CHECK_INTERVAL_MS: u64 = 100;

use crate::{
    config::{Config, ServiceAction},
    panel::{Panel, StatusPanel, StreamKind},
    process::{RunningProcess, spawn_process},
    render,
    signals::terminate_child,
    ui::{ProcessStatus, UiEvent},
};

async fn wait_for_shutdown(
    procs: &mut [Option<RunningProcess>],
    panels: &[Panel],
    status_panel: &mut StatusPanel,
    enable_terminal: bool,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> io::Result<bool> {
    loop {
        // Check process status
        let mut all_exited = true;
        for (i, proc) in procs.iter_mut().enumerate() {
            if let Some(p) = proc {
                if p.pid.is_none() || check_process_exited_by_pid(p.pid) {
                    // Process has exited
                    status_panel
                        .update_entry(panels[i].service_name.clone(), ProcessStatus::Exited);
                    if enable_terminal {
                        render::draw_shutdown(terminal, status_panel)?;
                    }
                } else {
                    all_exited = false;
                }
            }
        }

        if all_exited {
            return Ok(true);
        }

        tokio::time::sleep(Duration::from_millis(SHUTDOWN_CHECK_INTERVAL_MS)).await;
    }
}

fn check_process_exited_by_pid(pid: Option<u32>) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    let Some(pid) = pid else {
        return true;
    };
    let pid = Pid::from_raw(pid as i32);
    match kill(pid, None) {
        Err(nix::Error::ESRCH) => true, // Process does not exist
        Ok(_) => false,                 // Process still exists
        Err(_) => false,
    }
}

pub async fn run(
    config: Config,
    services_to_run: Vec<String>,
    config_dir: PathBuf,
) -> io::Result<()> {
    run_with_input(config, services_to_run, config_dir, None).await
}

pub async fn run_with_input(
    config: Config,
    services_to_run: Vec<String>,
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

    // Resolve which services to run
    let target_services = if services_to_run.is_empty() {
        if let Some(default) = &config.default {
            vec![default.clone()]
        } else {
            vec![]
        }
    } else {
        services_to_run
    };

    // Resolve all dependencies to get the full list of services to start
    let services_list = resolve_dependencies(&config, &target_services)?;

    // Create panels only for services with a "start" action
    let mut panels = Vec::new();
    let mut service_to_panel: HashMap<String, usize> = HashMap::new();

    for service_name in &services_list {
        let service_config = config.services.get(service_name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("Service '{}' not found", service_name),
            )
        })?;

        // Only create panels for services with a "start" action
        if let Some(ServiceAction::Start { command }) = &service_config.action {
            let cmd = shell_words::split(&command.as_command()).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Failed to parse command: {}", e),
                )
            })?;

            let cwd = service_config.cwd.as_ref().map(|c| {
                let path = config_dir.join(c);
                path.to_string_lossy().to_string()
            });

            // Determine which streams to show
            let (show_stdout, show_stderr) = match &service_config.display {
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

            service_to_panel.insert(service_name.clone(), panels.len());
            panels.push(Panel::new(
                service_name.clone(),
                cmd,
                cwd,
                show_stdout,
                show_stderr,
            ));
        }
    }

    if panels.is_empty() {
        disable_raw_mode()?;
        eprintln!("No services with 'start' action to display");
        return Ok(());
    }

    // Initialize status panel with all services that have actions
    let mut status_panel = StatusPanel::new();
    for service_name in &services_list {
        let service_config = config.services.get(service_name).unwrap();

        match &service_config.action {
            Some(action) => {
                status_panel.update_entry_with_action(
                    service_name.clone(),
                    ProcessStatus::Running,
                    action.clone(),
                );
                status_panel
                    .entry_indices
                    .insert(service_name.clone(), usize::MAX);
                status_panel
                    .update_dependencies(service_name.clone(), service_config.require.clone());
            }
            None => {}
        }
    }

    // Start processes according to dependencies
    let mut procs: Vec<Option<RunningProcess>> = (0..panels.len()).map(|_| None).collect();
    start_services(
        &config,
        &services_list,
        &service_to_panel,
        &panels,
        &mut procs,
        tx.clone(),
        &shutdown_tx,
        &mut status_panel,
    )
    .await?;

    let mut active = 0;
    let mut showing_status = true;
    let mut prev_statuses_storage: Option<Vec<ProcessStatus>> = None;

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
    let keyboard_task = if external_rx.is_none() {
        let tx_kb = tx.clone();
        Some(tokio::spawn(async move {
            loop {
                if event::poll(Duration::from_millis(KEYBOARD_POLL_INTERVAL_MS)).unwrap() {
                    if let Event::Key(k) = event::read().unwrap() {
                        let ev = match k.code {
                            KeyCode::Char('q') => UiEvent::Exit,
                            KeyCode::Char('r') => UiEvent::Restart,
                            KeyCode::Char('o') => UiEvent::ToggleStdout,
                            KeyCode::Char('e') => UiEvent::ToggleStderr,
                            KeyCode::Char('s') => UiEvent::SwitchToStatus,
                            KeyCode::Char(c @ '1'..='9') => {
                                UiEvent::SwitchPanel((c as u8 - b'1') as usize)
                            }
                            KeyCode::Up => UiEvent::Scroll(-1),
                            KeyCode::Down => UiEvent::Scroll(1),
                            KeyCode::PageUp => UiEvent::Scroll(-20),
                            KeyCode::PageDown => UiEvent::Scroll(20),
                            _ => continue,
                        };
                        // Ignore send errors - if channel is closed, we're shutting down
                        let _ = tx_kb.send(ev).await;
                    }
                }
            }
        }))
    } else {
        None
    };

    if showing_status {
        render::draw_status(&mut terminal, &panels, &status_panel)?;
    } else {
        render::draw(&mut terminal, &panels[active])?;
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
                let p = &mut panels[panel];
                let at_bottom = p.follow;

                match stream {
                    StreamKind::Stdout => p.stdout.push(&text),
                    StreamKind::Stderr => p.stderr.push(&text),
                }

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
                let msg = format!(
                    "[exited: {}]",
                    status.map(|s| s.to_string()).unwrap_or("unknown".into())
                );
                panels[panel].stdout.push(&msg);
                panels[panel].stderr.push(&msg);
                status_panel.update_exit_code(panels[panel].service_name.clone(), exit_code);
                redraw = panel == active;
            }

            UiEvent::Scroll(delta) => {
                let p = &mut panels[active];
                let max = p.visible_len().saturating_sub(1);
                let new = (p.scroll as i32 + delta).clamp(0, max as i32) as usize;
                p.follow = new == max;
                p.scroll = new;
                redraw = true;
            }

            UiEvent::ToggleStdout => {
                let p = &mut panels[active];
                p.show_stdout = !p.show_stdout;
                toggle_stream_visibility(p, p.show_stdout);
                redraw = true;
            }

            UiEvent::ToggleStderr => {
                let p = &mut panels[active];
                p.show_stderr = !p.show_stderr;
                toggle_stream_visibility(p, p.show_stderr);
                redraw = true;
            }

            UiEvent::SwitchPanel(i) if i < panels.len() => {
                active = i;
                showing_status = false;
                redraw = true;
            }

            UiEvent::SwitchToStatus => {
                showing_status = true;
                redraw = true;
            }

            UiEvent::CheckStatus => {
                let mut prev_statuses = prev_statuses_storage.take().unwrap_or_default();
                if prev_statuses.is_empty() && !procs.is_empty() {
                    prev_statuses = vec![ProcessStatus::Running; procs.len()];
                }

                let mut any_change = false;
                let mut any_exited = false;

                for (i, proc) in procs.iter_mut().enumerate() {
                    let current_status = if let Some(p) = proc {
                        if p.pid.is_none() || check_process_exited_by_pid(p.pid) {
                            ProcessStatus::Exited
                        } else {
                            ProcessStatus::Running
                        }
                    } else {
                        ProcessStatus::Exited
                    };

                    if prev_statuses.get(i) != Some(&current_status) {
                        any_change = true;
                        status_panel.update_entry(panels[i].service_name.clone(), current_status);
                    }

                    if current_status == ProcessStatus::Exited {
                        any_exited = true;
                    }

                    if i >= prev_statuses.len() {
                        prev_statuses.push(current_status);
                    } else {
                        prev_statuses[i] = current_status;
                    }
                }

                if any_change {
                    if showing_status {
                        redraw = true;
                    } else if any_exited {
                        showing_status = true;
                        redraw = true;
                    }
                }

                prev_statuses_storage = Some(prev_statuses);
            }

            UiEvent::Restart => {
                if let Some(proc) = procs[active].take() {
                    terminate_child(proc.pid).await;
                }
                panels[active].stdout.push("[restarting]");
                panels[active].stderr.push("[restarting]");
                let cwd = panels[active].cwd.as_deref();
                procs[active] = Some(spawn_process(
                    active,
                    &panels[active].cmd,
                    cwd,
                    tx.clone(),
                    shutdown_tx.subscribe(),
                ));
                redraw = true;
            }

            UiEvent::Exit => {
                // Ignore send errors - if all receivers are gone, shutdown proceeds anyway
                let _ = shutdown_tx.send(());

                // Initialize status panel with all running services
                for (i, panel) in panels.iter().enumerate() {
                    if procs[i].is_some() {
                        status_panel
                            .update_entry(panel.service_name.clone(), ProcessStatus::Running);
                    }
                }

                // Terminate all processes
                for p in procs.iter_mut() {
                    if let Some(proc) = p {
                        terminate_child(proc.pid).await;
                    }
                }

                // Switch to showing shutdown progress
                if enable_terminal {
                    render::draw_shutdown(&mut terminal, &status_panel)?;
                }

                // Wait for all processes to exit
                let shutdown_complete = wait_for_shutdown(
                    &mut procs,
                    &panels,
                    &mut status_panel,
                    enable_terminal,
                    &mut terminal,
                )
                .await?;

                if let Some(ref task) = keyboard_task {
                    task.abort();
                }

                status_check_task.abort();

                if shutdown_complete {
                    break;
                }
            }

            _ => {}
        }

        if redraw {
            if showing_status {
                render::draw_status(&mut terminal, &panels, &status_panel)?;
            } else {
                render::draw(&mut terminal, &panels[active])?;
            }
        }
    }

    for p in procs.iter() {
        if let Some(proc) = p {
            proc._stdout_task.abort();
            proc._stderr_task.abort();
            proc._wait_task.abort();
        }
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

fn resolve_dependencies(config: &Config, targets: &[String]) -> io::Result<Vec<String>> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut temp_mark = HashSet::new();

    fn visit(
        service: &str,
        config: &Config,
        result: &mut Vec<String>,
        visited: &mut HashSet<String>,
        temp_mark: &mut HashSet<String>,
    ) -> io::Result<()> {
        if visited.contains(service) {
            return Ok(());
        }

        if temp_mark.contains(service) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Circular dependency detected involving service '{}'",
                    service
                ),
            ));
        }

        temp_mark.insert(service.to_string());

        let service_config = config.services.get(service).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("Service '{}' not found in config", service),
            )
        })?;

        // Visit dependencies first
        for dep in &service_config.require {
            visit(dep, config, result, visited, temp_mark)?;
        }

        temp_mark.remove(service);
        visited.insert(service.to_string());
        result.push(service.to_string());

        Ok(())
    }

    for target in targets {
        visit(target, config, &mut result, &mut visited, &mut temp_mark)?;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_process_exited_by_pid_none() {
        assert!(check_process_exited_by_pid(None));
    }

    #[test]
    fn test_visible_len_empty_panel() {
        let panel = Panel::new(
            "test".to_string(),
            vec!["echo".to_string()],
            None,
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
        );
        panel.stdout.push("line 1");
        panel.stdout.push("line 2");
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
        );
        panel.stderr.push("error 1");
        panel.stderr.push("error 2");
        panel.stderr.push("error 3");
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
        );
        panel.stdout.push("line 1");
        panel.stderr.push("error 1");
        panel.stdout.push("line 2");
        assert_eq!(panel.visible_len(), 3);
    }

    #[test]
    fn test_resolve_dependencies_empty() {
        let config = Config {
            default: None,
            services: HashMap::new(),
        };
        let result = resolve_dependencies(&config, &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_resolve_dependencies_no_deps() {
        let mut services = HashMap::new();
        services.insert(
            "service1".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
            },
        );

        let config = Config {
            default: None,
            services,
        };

        let result = resolve_dependencies(&config, &["service1".to_string()]).unwrap();
        assert_eq!(result, vec!["service1"]);
    }

    #[test]
    fn test_resolve_dependencies_with_deps() {
        let mut services = HashMap::new();
        services.insert(
            "service1".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep1".to_string()],
            },
        );
        services.insert(
            "dep1".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
            },
        );

        let config = Config {
            default: None,
            services,
        };

        let result = resolve_dependencies(&config, &["service1".to_string()]).unwrap();
        assert_eq!(result, vec!["dep1", "service1"]);
    }

    #[test]
    fn test_resolve_dependencies_multiple_deps() {
        let mut services = HashMap::new();
        services.insert(
            "service1".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep1".to_string(), "dep2".to_string()],
            },
        );
        services.insert(
            "dep1".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
            },
        );
        services.insert(
            "dep2".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
            },
        );

        let config = Config {
            default: None,
            services,
        };

        let result = resolve_dependencies(&config, &["service1".to_string()]).unwrap();
        assert!(result.contains(&"dep1".to_string()));
        assert!(result.contains(&"dep2".to_string()));
        assert!(result.contains(&"service1".to_string()));
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_resolve_dependencies_nested_deps() {
        let mut services = HashMap::new();
        services.insert(
            "service1".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep1".to_string()],
            },
        );
        services.insert(
            "dep1".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep2".to_string()],
            },
        );
        services.insert(
            "dep2".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
            },
        );

        let config = Config {
            default: None,
            services,
        };

        let result = resolve_dependencies(&config, &["service1".to_string()]).unwrap();
        assert_eq!(result, vec!["dep2", "dep1", "service1"]);
    }

    #[test]
    fn test_resolve_dependencies_circular() {
        let mut services = HashMap::new();
        services.insert(
            "service1".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["service2".to_string()],
            },
        );
        services.insert(
            "service2".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["service1".to_string()],
            },
        );

        let config = Config {
            default: None,
            services,
        };

        let result = resolve_dependencies(&config, &["service1".to_string()]);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Circular dependency")
        );
    }

    #[test]
    fn test_resolve_dependencies_service_not_found() {
        let config = Config {
            default: None,
            services: HashMap::new(),
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
        let mut services = HashMap::new();
        services.insert(
            "service1".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["nonexistent".to_string()],
            },
        );

        let config = Config {
            default: None,
            services,
        };

        let result = resolve_dependencies(&config, &["service1".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_dependencies_multiple_targets() {
        let mut services = HashMap::new();
        services.insert(
            "service1".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep1".to_string()],
            },
        );
        services.insert(
            "service2".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep1".to_string()],
            },
        );
        services.insert(
            "dep1".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
            },
        );

        let config = Config {
            default: None,
            services,
        };

        let result =
            resolve_dependencies(&config, &["service1".to_string(), "service2".to_string()])
                .unwrap();
        assert!(result.contains(&"dep1".to_string()));
        assert!(result.contains(&"service1".to_string()));
        assert!(result.contains(&"service2".to_string()));
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_resolve_dependencies_diamond_graph() {
        let mut services = HashMap::new();
        services.insert(
            "service1".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["dep1".to_string(), "dep2".to_string()],
            },
        );
        services.insert(
            "dep1".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["base".to_string()],
            },
        );
        services.insert(
            "dep2".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec!["base".to_string()],
            },
        );
        services.insert(
            "base".to_string(),
            crate::config::ServiceConfiguration {
                action: None,
                cwd: None,
                display: None,
                require: vec![],
            },
        );

        let config = Config {
            default: None,
            services,
        };

        let result = resolve_dependencies(&config, &["service1".to_string()]).unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], "base");
        assert!(result.contains(&"dep1".to_string()));
        assert!(result.contains(&"dep2".to_string()));
        assert_eq!(result[3], "service1");
    }
}

async fn start_services(
    config: &Config,
    services_list: &[String],
    service_to_panel: &HashMap<String, usize>,
    panels: &[Panel],
    procs: &mut Vec<Option<RunningProcess>>,
    tx: tokio::sync::mpsc::Sender<UiEvent>,
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
    status_panel: &mut StatusPanel,
) -> io::Result<()> {
    for service_name in services_list {
        let service_config = config.services.get(service_name).unwrap();

        match &service_config.action {
            Some(ServiceAction::Run { command }) => {
                // Run to completion
                let cmd = shell_words::split(&command.as_command()).map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("Failed to parse command: {}", e),
                    )
                })?;

                let mut command = tokio::process::Command::new(&cmd[0]);
                command.args(&cmd[1..]);

                if let Some(cwd) = &service_config.cwd {
                    command.current_dir(cwd);
                }

                let status = command.status().await?;

                let exit_code = status.code();
                status_panel.update_entry(service_name.clone(), ProcessStatus::Exited);
                status_panel.update_exit_code(service_name.clone(), exit_code);

                if !status.success() {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!(
                            "Service '{}' failed with exit code: {:?}",
                            service_name, exit_code
                        ),
                    ));
                }
            }
            Some(ServiceAction::Start { .. }) => {
                // Start long-running service
                if let Some(&panel_idx) = service_to_panel.get(service_name) {
                    let panel = &panels[panel_idx];
                    let cwd = panel.cwd.as_deref();
                    procs[panel_idx] = Some(spawn_process(
                        panel_idx,
                        &panel.cmd,
                        cwd,
                        tx.clone(),
                        shutdown_tx.subscribe(),
                    ));
                }
            }
            None => {
                // No action - just a dependency aggregator
            }
        }
    }

    Ok(())
}
