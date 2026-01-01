use ratatui::{
    Terminal,
    layout::{Alignment, Constraint, Direction, Layout},
    prelude::CrosstermBackend,
    style::{Color, Modifier, Style},
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
                (Some(ServiceAction::Run { .. }), ProcessStatus::Exited) => {
                    if entry.exit_code.map_or(false, |c| c == 0) {
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
            .direction(Direction::Vertical)
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
                let (status_text, status_color) = match (&entry.action_type, entry.status) {
                    (Some(ServiceAction::Run { .. }), ProcessStatus::Exited) => {
                        if entry.exit_code.map_or(false, |c| c == 0) {
                            ("✓ Completed", Color::Green)
                        } else {
                            ("✗ Failed", Color::Red)
                        }
                    }
                    (Some(ServiceAction::Start { .. }), ProcessStatus::Running) => {
                        ("● Running", Color::Green)
                    }
                    (Some(ServiceAction::Start { .. }), ProcessStatus::Exited) => {
                        ("✓ Exited", Color::Gray)
                    }
                    (_, ProcessStatus::Running) => ("● Running", Color::Green),
                    (_, ProcessStatus::Exited) => ("✓ Exited", Color::Gray),
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

pub fn draw(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    panel: &Panel,
) -> io::Result<()> {
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
