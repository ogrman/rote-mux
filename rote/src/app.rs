use std::{io, time::Duration};

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
    panel::{Panel, StreamKind},
    process::{RunningProcess, spawn_process},
    signals::terminate_child,
    ui::UiEvent,
};

pub async fn run() -> io::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<UiEvent>(1024);

    let mut panels = vec![
        Panel::new(vec!["ping".into(), "google.com".into()]),
        Panel::new(vec!["ping".into(), "1.1.1.1".into()]),
    ];

    let mut procs: Vec<Option<RunningProcess>> = panels
        .iter()
        .enumerate()
        .map(|(i, p)| Some(spawn_process(i, &p.cmd, tx.clone())))
        .collect();

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
                procs[active] = Some(spawn_process(active, &panels[active].cmd, tx.clone()));
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
        n += p.stdout.rope.len_lines();
    }
    if p.show_stderr {
        n += p.stderr.rope.len_lines();
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
