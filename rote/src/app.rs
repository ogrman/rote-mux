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
    prelude::CrosstermBackend,
    widgets::{Block, Borders, Paragraph},
};

use crate::{
    config::{Config, ServiceAction},
    panel::{Panel, StreamKind},
    process::{RunningProcess, spawn_process},
    signals::terminate_child,
    ui::UiEvent,
};

pub async fn run(
    config: Config,
    services_to_run: Vec<String>,
    config_dir: PathBuf,
) -> io::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<UiEvent>(1024);

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
    let mut procs: Vec<Option<RunningProcess>> = (0..panels.len()).map(|_| None).collect();
    start_services(
        &config,
        &services_list,
        &service_to_panel,
        &panels,
        &mut procs,
        tx.clone(),
    )
    .await?;

    let mut active = 0;

    // keyboard
    {
        let tx = tx.clone();
        tokio::spawn(async move {
            loop {
                if event::poll(Duration::from_millis(250)).unwrap() {
                    if let Event::Key(k) = event::read().unwrap() {
                        let ev = match k.code {
                            KeyCode::Char('q') => UiEvent::Exit,
                            KeyCode::Char('R') => UiEvent::Restart,
                            KeyCode::Char('o') => UiEvent::ToggleStdout,
                            KeyCode::Char('e') => UiEvent::ToggleStderr,
                            KeyCode::Char(c @ '1'..='9') => {
                                UiEvent::SwitchPanel((c as u8 - b'1') as usize)
                            }
                            KeyCode::Up => UiEvent::Scroll(-1),
                            KeyCode::Down => UiEvent::Scroll(1),
                            KeyCode::PageUp => UiEvent::Scroll(-20),
                            KeyCode::PageDown => UiEvent::Scroll(20),
                            _ => continue,
                        };
                        let _ = tx.send(ev).await;
                    }
                }
            }
        });
    }

    draw(&mut terminal, &panels[active])?;

    while let Some(ev) = rx.recv().await {
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
                title: _,
            } => {
                let msg = format!(
                    "[exited: {}]",
                    status.map(|s| s.to_string()).unwrap_or("unknown".into())
                );
                panels[panel].stdout.push(&msg);
                panels[panel].stderr.push(&msg);
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
                panels[active].show_stdout = !panels[active].show_stdout;
                redraw = true;
            }

            UiEvent::ToggleStderr => {
                panels[active].show_stderr = !panels[active].show_stderr;
                redraw = true;
            }

            UiEvent::SwitchPanel(i) if i < panels.len() => {
                active = i;
                redraw = true;
            }

            UiEvent::Restart => {
                if let Some(mut proc) = procs[active].take() {
                    terminate_child(&mut proc.child).await;
                }
                panels[active].stdout.push("[restarting]");
                panels[active].stderr.push("[restarting]");
                let cwd = panels[active].cwd.as_deref();
                procs[active] = Some(spawn_process(active, &panels[active].cmd, cwd, tx.clone()));
                redraw = true;
            }

            UiEvent::Exit => {
                for p in procs.iter_mut().flatten() {
                    terminate_child(&mut p.child).await;
                }
                break;
            }

            _ => {}
        }

        if redraw {
            draw(&mut terminal, &panels[active])?;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
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

fn draw(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, panel: &Panel) -> io::Result<()> {
    terminal.draw(|f| {
        let area = f.size();
        let height = area.height.saturating_sub(2) as usize;

        let mut lines = Vec::with_capacity(height);

        if panel.show_stdout {
            lines.extend(panel.stdout.rope.lines());
        }
        if panel.show_stderr {
            lines.extend(panel.stderr.rope.lines());
        }

        let start = panel.scroll.min(lines.len());
        let end = (start + height).min(lines.len());
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

                if !status.success() {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!(
                            "Service '{}' failed with exit code: {:?}",
                            service_name,
                            status.code()
                        ),
                    ));
                }
            }
            Some(ServiceAction::Start { .. }) => {
                // Start long-running service
                if let Some(&panel_idx) = service_to_panel.get(service_name) {
                    let panel = &panels[panel_idx];
                    let cwd = panel.cwd.as_deref();
                    procs[panel_idx] = Some(spawn_process(panel_idx, &panel.cmd, cwd, tx.clone()));
                }
            }
            None => {
                // No action - just a dependency aggregator
            }
        }
    }

    Ok(())
}
