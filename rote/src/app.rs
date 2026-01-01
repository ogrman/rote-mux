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
use ratatui::{
    Terminal,
    layout::{Alignment, Constraint},
    prelude::CrosstermBackend,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};

use crate::{
    config::{Config, ServiceAction},
    panel::{Panel, StatusPanel, StreamKind},
    process::{RunningProcess, spawn_process},
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
                        draw_shutdown(terminal, status_panel)?;
                    }
                } else {
                    all_exited = false;
                }
            }
        }

        if all_exited {
            return Ok(true);
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
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

fn draw_shutdown(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    status_panel: &StatusPanel,
) -> io::Result<()> {
    terminal.draw(|f| {
        let area = f.size();

        let mut lines = vec![String::from("Shutting down...")];
        lines.push(String::new());

        for entry in &status_panel.entries {
            let status_str = match entry.status {
                ProcessStatus::Running => "●",
                ProcessStatus::Exited => "✓",
            };
            lines.push(format!("  {} {}", status_str, entry.service_name));
        }

        lines.push(String::new());
        lines.push(String::from("Waiting for all processes to exit..."));

        let text = lines.join("\n");
        let widget = Paragraph::new(text).block(
            Block::default()
                .title("Shutdown Progress")
                .borders(Borders::ALL),
        );

        f.render_widget(widget, area);
    })?;
    Ok(())
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

    let (internal_tx, mut internal_rx) = tokio::sync::mpsc::channel::<UiEvent>(1024);

    // The sender to use for spawning processes - always internal_tx
    let tx = internal_tx.clone();
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(16);

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
            let cmd = shell_words::split(&command).map_err(|e| {
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

    // Start processes according to dependencies
    let mut status_panel = StatusPanel::new();
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
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        loop {
            interval.tick().await;
            let _ = status_check_tx.send(UiEvent::CheckStatus).await;
        }
    });

    // keyboard - spawn if we created internal_tx (i.e., no external input)
    let keyboard_task = if external_rx.is_none() {
        let tx_kb = tx.clone();
        Some(tokio::spawn(async move {
            loop {
                if event::poll(Duration::from_millis(250)).unwrap() {
                    if let Event::Key(k) = event::read().unwrap() {
                        let ev = match k.code {
                            KeyCode::Char('q') => UiEvent::Exit,
                            KeyCode::Char('R') => UiEvent::Restart,
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
                        let _ = tx_kb.send(ev).await;
                    }
                }
            }
        }))
    } else {
        None
    };

    // Initialize status panel with all services that have actions
    for service_name in &services_list {
        let service_config = config.services.get(service_name).unwrap();

        match &service_config.action {
            Some(ServiceAction::Run { .. }) => {
                status_panel.update_entry(service_name.clone(), ProcessStatus::Running);
                status_panel
                    .entry_indices
                    .insert(service_name.clone(), usize::MAX);
            }
            Some(ServiceAction::Start { .. }) => {
                if let Some(&panel_idx) = service_to_panel.get(service_name) {
                    status_panel.update_entry(service_name.clone(), ProcessStatus::Running);
                    status_panel
                        .entry_indices
                        .insert(service_name.clone(), panel_idx);
                }
            }
            None => {}
        }
    }

    if showing_status {
        draw_status(&mut terminal, &panels, &status_panel)?;
    } else {
        draw(&mut terminal, &panels[active])?;
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
                    p.scroll = visible_len(p).saturating_sub(1);
                }

                if panel == active {
                    redraw = true;
                }
            }

            UiEvent::Exited {
                panel,
                status,
                exit_code,
                title: _,
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
                let max = visible_len(p).saturating_sub(1);
                let new = (p.scroll as i32 + delta).clamp(0, max as i32) as usize;
                p.follow = new == max;
                p.scroll = new;
                redraw = true;
            }

            UiEvent::ToggleStdout => {
                let p = &mut panels[active];
                p.show_stdout = !p.show_stdout;
                if p.show_stdout {
                    let max = visible_len(p).saturating_sub(1);
                    p.scroll = max;
                    p.follow = true;
                } else {
                    let max = visible_len(p).saturating_sub(1);
                    p.scroll = p.scroll.min(max);
                    p.follow = p.scroll == max;
                }
                redraw = true;
            }

            UiEvent::ToggleStderr => {
                let p = &mut panels[active];
                p.show_stderr = !p.show_stderr;
                if p.show_stderr {
                    let max = visible_len(p).saturating_sub(1);
                    p.scroll = max;
                    p.follow = true;
                } else {
                    let max = visible_len(p).saturating_sub(1);
                    p.scroll = p.scroll.min(max);
                    p.follow = p.scroll == max;
                }
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
                    draw_shutdown(&mut terminal, &status_panel)?;
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
                draw_status(&mut terminal, &panels, &status_panel)?;
            } else {
                draw(&mut terminal, &panels[active])?;
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
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
    Ok(())
}

fn visible_len(p: &Panel) -> usize {
    let mut n = 0;
    if p.show_stdout {
        // rope.len_lines() includes an extra empty line after the final newline,
        // so we subtract 1 if there's any content, otherwise keep it at 0
        let lines = p.stdout.rope.len_lines();
        n += if lines > 0 {
            lines.saturating_sub(1)
        } else {
            0
        };
    }
    if p.show_stderr {
        let lines = p.stderr.rope.len_lines();
        n += if lines > 0 {
            lines.saturating_sub(1)
        } else {
            0
        };
    }
    n
}

fn draw_status(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    _panels: &[Panel],
    status_panel: &StatusPanel,
) -> io::Result<()> {
    terminal.draw(|f| {
        let area = f.size();

        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    Constraint::Length(status_panel.entries.len() as u16 + 3),
                    Constraint::Length(4),
                ]
                .as_ref(),
            )
            .split(area);

        let table_area = chunks[0];
        let help_area = chunks[1];

        let header_style = Style::default()
            .fg(Color::Reset)
            .add_modifier(Modifier::BOLD);

        let header = Row::new(vec![
            Cell::from("#"),
            Cell::from("Service").style(header_style),
            Cell::from("Status").style(header_style),
            Cell::from("Exit Code").style(header_style),
        ])
        .style(Style::default().bg(Color::Reset));

        let rows: Vec<Row> = status_panel
            .entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let (status_text, status_color) = match entry.status {
                    ProcessStatus::Running => ("● Running", Color::Green),
                    ProcessStatus::Exited => ("✓ Exited", Color::Gray),
                };

                let exit_code_text = match entry.status {
                    ProcessStatus::Running => String::from("-"),
                    ProcessStatus::Exited => entry
                        .exit_code
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| String::from("unknown")),
                };

                Row::new(vec![
                    Cell::from((i + 1).to_string()),
                    Cell::from(entry.service_name.clone()),
                    Cell::from(status_text).style(Style::default().fg(status_color)),
                    Cell::from(exit_code_text),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(3),
                Constraint::Min(30),
                Constraint::Min(10),
                Constraint::Min(10),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .title("Process Status")
                .borders(Borders::ALL),
        );

        f.render_widget(table, table_area);

        let help_text = vec![
            String::from("Press a number (1-9) to view a process"),
            String::from("Press 's' to refresh this status screen"),
            String::from("Press 'q' to quit"),
        ]
        .join("\n");

        let help_widget = Paragraph::new(help_text)
            .alignment(Alignment::Left)
            .block(Block::default());

        f.render_widget(help_widget, help_area);
    })?;
    Ok(())
}

fn draw(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, panel: &Panel) -> io::Result<()> {
    terminal.draw(|f| {
        let area = f.size();
        let height = area.height.saturating_sub(2) as usize;

        let mut lines = Vec::new();

        if panel.show_stdout {
            lines.extend(panel.stdout.rope.lines());
        }
        if panel.show_stderr {
            lines.extend(panel.stderr.rope.lines());
        }

        // Skip trailing empty line if present
        if let Some(last) = lines.last() {
            if last.len_chars() == 0 {
                lines.pop();
            }
        }

        let start = panel
            .scroll
            .saturating_sub(height.saturating_sub(1))
            .min(lines.len());
        let end = (panel.scroll + 1).min(lines.len());
        let text = lines[start..end]
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<String>>()
            .join("");

        let title = format!(
            "{}  [o:{} e:{}]",
            panel.title,
            if panel.show_stdout { "on" } else { "off" },
            if panel.show_stderr { "on" } else { "off" },
        );

        let widget =
            Paragraph::new(text).block(Block::default().title(title).borders(Borders::ALL));

        f.render_widget(widget, area);
    })?;
    Ok(())
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
                let cmd = shell_words::split(&command).map_err(|e| {
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
