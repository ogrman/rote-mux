use ratatui::{
    Terminal,
    layout::{Alignment, Constraint, Direction, Layout},
    prelude::CrosstermBackend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};
use std::io;

use crate::{
    config::ServiceAction,
    panel::{Panel, StatusPanel},
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
        let help_area = chunks[1];

        let (healthy, total, has_issues) = status_panel.get_health_status();

        let status_summary = if total > 0 {
            let status_icon = if has_issues { "⚠" } else { "✓" };
            let status_style = if has_issues {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            };
            Line::from(vec![
                Span::styled(
                    format!(" {} {}/{}", status_icon, healthy, total),
                    status_style,
                ),
                Span::raw(" services healthy"),
            ])
        } else {
            Line::from(" No services")
        };

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
        .block(Block::default().title(status_summary).borders(Borders::ALL));

        f.render_widget(table, main_area);

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
    _status_panel: &StatusPanel,
) -> io::Result<()> {
    terminal.draw(|f| {
        let area = f.size();

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .margin(0)
            .constraints([Constraint::Min(0), Constraint::Length(22)].as_ref())
            .split(area);

        let content_area = chunks[0];
        let help_area = chunks[1];

        let height = content_area.height.saturating_sub(2) as usize;

        let filtered_lines =
            panel
                .messages
                .lines_filtered(panel.show_stdout, panel.show_stderr, panel.show_status);

        let start = panel
            .scroll
            .saturating_sub(height.saturating_sub(1))
            .min(filtered_lines.len());
        let end = (panel.scroll + 1).min(filtered_lines.len());
        let text = filtered_lines[start..end]
            .iter()
            .map(|(_, line)| format!("{line}\n"))
            .collect::<Vec<String>>()
            .join("");

        let title = format!(
            "{} [stdout: {}, stderr: {}]",
            panel.title,
            if panel.show_stdout { "on" } else { "off" },
            if panel.show_stderr { "on" } else { "off" },
        );

        let widget =
            Paragraph::new(text).block(Block::default().title(title).borders(Borders::ALL));

        f.render_widget(widget, content_area);

        let help_text = [
            "1-9  view process",
            "←/→  navigate",
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
