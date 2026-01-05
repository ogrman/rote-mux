use ratatui::{
    Terminal,
    layout::{Alignment, Constraint, Direction, Layout},
    prelude::CrosstermBackend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Table,
    },
};
use std::io;

use crate::{
    config::ServiceAction,
    panel::{Panel, StatusPanel, WRAP_INDICATOR, wrap_line},
    ui::ProcessStatus,
};

pub fn draw_shutdown(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    status_panel: &StatusPanel,
) -> io::Result<()> {
    terminal.draw(|f| {
        let area = f.size();

        let mut lines = vec![String::from("Shutting down...")];
        lines.push(String::new());

        for entry in &status_panel.entries {
            let status_str = match (&entry.action_type, entry.status) {
                (_, ProcessStatus::NotStarted) => "○",
                (Some(ServiceAction::Run { .. }), ProcessStatus::Exited) => {
                    if entry.exit_code == Some(0) {
                        "✓"
                    } else {
                        "✗"
                    }
                }
                (_, ProcessStatus::Running) => "●",
                (_, ProcessStatus::Exited) => "✓",
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

fn render_service_status(status_panel: &StatusPanel) -> Paragraph<'static> {
    let (healthy, total, has_issues) = status_panel.get_health_status();

    let status_text = if total > 0 {
        let status_icon = if has_issues { "⚠" } else { "✓" };
        let status_style = if has_issues {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };
        Line::from(vec![
            Span::styled(format!("{status_icon} {healthy}/{total}"), status_style),
            Span::raw(" healthy"),
        ])
    } else {
        Line::from("No services")
    };

    Paragraph::new(status_text)
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .title("Service status")
                .borders(Borders::ALL),
        )
}

pub fn draw_status(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    _panels: &[Panel],
    status_panel: &StatusPanel,
) -> io::Result<()> {
    terminal.draw(|f| {
        let area = f.size();

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .margin(0)
            .constraints([Constraint::Min(0), Constraint::Length(22)].as_ref())
            .split(area);

        let main_area = chunks[0];
        let sidebar_area = chunks[1];

        // Split sidebar into status and help sections
        let sidebar_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
            .split(sidebar_area);

        let status_area = sidebar_chunks[0];
        let help_area = sidebar_chunks[1];

        let header_style = Style::default()
            .fg(Color::Reset)
            .add_modifier(Modifier::BOLD);

        let header = Row::new(vec![
            Cell::from("#"),
            Cell::from("Service").style(header_style),
            Cell::from("Status").style(header_style),
            Cell::from("Previous exit code").style(header_style),
            Cell::from("Dependencies").style(header_style),
        ])
        .style(Style::default().bg(Color::Reset));

        let rows: Vec<Row> = status_panel
            .entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let (status_text, status_color) = match (&entry.action_type, entry.status) {
                    (_, ProcessStatus::NotStarted) => ("○ Not started", Color::Gray),
                    (Some(ServiceAction::Run { .. }), ProcessStatus::Exited) => {
                        if entry.exit_code == Some(0) {
                            ("✓ Completed", Color::Green)
                        } else {
                            ("✗ Failed", Color::Red)
                        }
                    }
                    (Some(ServiceAction::Start { .. }), ProcessStatus::Running) => {
                        ("● Running", Color::Green)
                    }
                    (Some(ServiceAction::Start { .. }), ProcessStatus::Exited) => {
                        ("✗ Exited", Color::Red)
                    }
                    (_, ProcessStatus::Running) => ("● Running", Color::Green),
                    (_, ProcessStatus::Exited) => ("✓ Exited", Color::Gray),
                };

                let (exit_code_text, exit_code_color) = match entry.exit_code {
                    Some(code) => {
                        let color = if code == 0 { Color::Reset } else { Color::Red };
                        (code.to_string(), color)
                    }
                    None => (String::from("-"), Color::Reset),
                };

                let dependencies_cell = if entry.dependencies.is_empty() {
                    Cell::from(String::new())
                } else {
                    let mut spans = Vec::new();
                    for (j, dep) in entry.dependencies.iter().enumerate() {
                        if j > 0 {
                            spans.push(Span::from(", "));
                        }
                        let dep_status =
                            status_panel.entries.iter().find(|e| e.service_name == *dep);
                        let is_down_or_failed = match dep_status {
                            Some(dep_entry) => match (&dep_entry.action_type, dep_entry.status) {
                                (_, ProcessStatus::NotStarted) => false,
                                (Some(ServiceAction::Run { .. }), ProcessStatus::Exited) => {
                                    dep_entry.exit_code != Some(0)
                                }
                                (Some(ServiceAction::Start { .. }), ProcessStatus::Exited) => true,
                                (_, ProcessStatus::Exited) => true,
                                _ => false,
                            },
                            None => true,
                        };
                        spans.push(Span::styled(
                            dep.clone(),
                            if is_down_or_failed {
                                Style::default().fg(Color::Red)
                            } else {
                                Style::default()
                            },
                        ));
                    }
                    Cell::from(Line::from(spans))
                };

                Row::new(vec![
                    Cell::from((i + 1).to_string()),
                    Cell::from(entry.service_name.clone()),
                    Cell::from(status_text).style(Style::default().fg(status_color)),
                    Cell::from(exit_code_text).style(Style::default().fg(exit_code_color)),
                    dependencies_cell,
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
                Constraint::Min(20),
            ],
        )
        .header(header)
        .block(Block::default().title("Services").borders(Borders::ALL));

        f.render_widget(table, main_area);

        // Render service status
        let status_widget = render_service_status(status_panel);
        f.render_widget(status_widget, status_area);

        let help_text = [
            "1-9  view process",
            "←/→  navigate",
            "s    status",
            "q    quit",
        ]
        .join("\n");

        let help_widget = Paragraph::new(help_text)
            .alignment(Alignment::Left)
            .block(Block::default().title("Keys").borders(Borders::ALL));

        f.render_widget(help_widget, help_area);
    })?;
    Ok(())
}

pub fn draw(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    panel: &Panel,
    status_panel: &StatusPanel,
) -> io::Result<()> {
    terminal.draw(|f| {
        let area = f.size();

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .margin(0)
            .constraints([Constraint::Min(0), Constraint::Length(22)].as_ref())
            .split(area);

        let content_area = chunks[0];
        let sidebar_area = chunks[1];

        // Split sidebar into status and help sections
        let sidebar_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
            .split(sidebar_area);

        let status_area = sidebar_chunks[0];
        let help_area = sidebar_chunks[1];

        let height = content_area.height.saturating_sub(2) as usize;
        // Inner width for text (subtract 2 for borders)
        let inner_width = content_area.width.saturating_sub(2) as usize;

        let filtered_lines =
            panel
                .messages
                .lines_filtered(panel.show_stdout, panel.show_stderr, panel.show_status);

        let total_lines = filtered_lines.len();

        // Build visual lines by wrapping logical lines, working backwards from scroll position
        // panel.scroll is the index of the bottom logical line to show
        let mut visual_lines: Vec<String> = Vec::new();

        if total_lines > 0 {
            // Clamp scroll to valid range
            let effective_scroll = if total_lines <= height {
                // If all logical lines fit when not wrapped, show from the end
                total_lines.saturating_sub(1)
            } else {
                panel
                    .scroll
                    .clamp(height.saturating_sub(1), total_lines.saturating_sub(1))
            };

            // Work backwards from the scroll position, collecting wrapped lines
            let mut logical_idx = effective_scroll as i32;
            while logical_idx >= 0 && visual_lines.len() < height {
                let (_, line) = &filtered_lines[logical_idx as usize];
                let wrapped = wrap_line(line, inner_width);

                // Add wrapped segments in reverse order (we're building bottom-up)
                for (is_continuation, segment) in wrapped.into_iter().rev() {
                    if visual_lines.len() >= height {
                        break;
                    }
                    let display_line = if is_continuation {
                        format!("{WRAP_INDICATOR}{segment}")
                    } else {
                        segment
                    };
                    visual_lines.push(display_line);
                }
                logical_idx -= 1;
            }

            // Reverse to get top-to-bottom order
            visual_lines.reverse();
        }

        // Count total visual lines for scrollbar
        let total_visual_lines: usize = filtered_lines
            .iter()
            .map(|(_, line)| wrap_line(line, inner_width).len())
            .sum();

        // Calculate visual scroll position for scrollbar (approximate)
        let visual_scroll_pos = if total_lines > 0 {
            let effective_scroll = panel.scroll.min(total_lines.saturating_sub(1));
            // Sum visual lines up to scroll position
            filtered_lines[..=effective_scroll]
                .iter()
                .map(|(_, line)| wrap_line(line, inner_width).len())
                .sum::<usize>()
                .saturating_sub(1)
        } else {
            0
        };

        let text = visual_lines.join("\n");

        let title = format!(
            "{} [stdout: {}, stderr: {}]",
            panel.title,
            if panel.show_stdout { "on" } else { "off" },
            if panel.show_stderr { "on" } else { "off" },
        );

        let widget =
            Paragraph::new(text).block(Block::default().title(title).borders(Borders::ALL));

        f.render_widget(widget, content_area);

        // Render scrollbar if there are more visual lines than can fit on screen
        if total_visual_lines > height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None);
            let scrollbar_range = total_visual_lines.saturating_sub(height);
            let scrollbar_pos = visual_scroll_pos
                .saturating_sub(height.saturating_sub(1))
                .min(scrollbar_range);
            let mut scrollbar_state = ScrollbarState::new(scrollbar_range).position(scrollbar_pos);
            // Render inside the border (inset by 1 on top and bottom)
            let scrollbar_area = ratatui::layout::Rect {
                x: content_area.x + content_area.width.saturating_sub(1),
                y: content_area.y + 1,
                width: 1,
                height: content_area.height.saturating_sub(2),
            };
            f.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
        }

        // Render service status
        let status_widget = render_service_status(status_panel);
        f.render_widget(status_widget, status_area);

        let help_text = [
            "1-9  view process",
            "←/→  navigate",
            "↑/↓  scroll",
            "PgUp/PgDn scroll fast",
            "s    status",
            "q    quit",
            "r    restart",
            "o    toggle stdout",
            "e    toggle stderr",
        ]
        .join("\n");

        let help_widget = Paragraph::new(help_text)
            .alignment(Alignment::Left)
            .block(Block::default().title("Keys").borders(Borders::ALL));

        f.render_widget(help_widget, help_area);
    })?;
    Ok(())
}
