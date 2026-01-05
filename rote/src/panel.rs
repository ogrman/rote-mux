use ropey::Rope;
use std::collections::HashMap;
use std::ops::Deref;
use unicode_width::UnicodeWidthChar;

pub const MAX_LINES: usize = 5_000;

/// A strongly-typed panel index to prevent accidentally mixing panel indices with other usize values.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PanelIndex(pub usize);

impl PanelIndex {
    pub fn new(index: usize) -> Self {
        Self(index)
    }
}

impl Deref for PanelIndex {
    type Target = usize;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<usize> for PanelIndex {
    fn from(index: usize) -> Self {
        Self(index)
    }
}

impl From<PanelIndex> for usize {
    fn from(index: PanelIndex) -> Self {
        index.0
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum StreamKind {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MessageKind {
    Stdout,
    Stderr,
    Status,
}

pub struct MessageBuf {
    pub rope: Rope,
}

impl Default for MessageBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBuf {
    pub fn new() -> Self {
        Self { rope: Rope::new() }
    }

    pub fn push(&mut self, kind: MessageKind, line: &str, timestamp: Option<&str>) {
        let kind_byte = match kind {
            MessageKind::Stdout => b'o',
            MessageKind::Stderr => b'e',
            MessageKind::Status => b's',
        };
        let content = match timestamp {
            Some(ts) => format!("{ts} {line}"),
            None => line.to_string(),
        };
        let encoded = format!("\x1E{}\x1F{}", kind_byte as char, content);
        self.rope.insert(self.rope.len_chars(), &encoded);
        self.rope.insert(self.rope.len_chars(), "\n");

        let excess = self.rope.len_lines().saturating_sub(MAX_LINES);
        if excess > 0 {
            let cut = self.rope.line_to_char(excess);
            self.rope.remove(0..cut);
        }
    }

    pub fn lines_filtered(
        &self,
        show_stdout: bool,
        show_stderr: bool,
        show_status: bool,
    ) -> Vec<(MessageKind, String)> {
        let mut result = Vec::new();
        for line in self.rope.lines() {
            let line_str = line.to_string();
            if line_str.starts_with('\x1E') {
                if let Some(rest) = line_str.strip_prefix('\x1E') {
                    if let Some(kind_char) = rest.chars().next() {
                        if let Some(content) = rest
                            .strip_prefix(kind_char)
                            .and_then(|s| s.strip_prefix('\x1F'))
                        {
                            let kind = match kind_char {
                                'o' => MessageKind::Stdout,
                                'e' => MessageKind::Stderr,
                                's' => MessageKind::Status,
                                _ => continue,
                            };
                            let should_include = match kind {
                                MessageKind::Stdout => show_stdout,
                                MessageKind::Stderr => show_stderr,
                                MessageKind::Status => show_status,
                            };
                            if should_include {
                                result.push((kind, content.trim_end_matches('\n').to_string()));
                            }
                        }
                    }
                }
            }
        }
        result
    }
}

/// Wrap indicator shown at the start of continuation lines.
pub const WRAP_INDICATOR: &str = "↪ ";
/// Display width of the wrap indicator.
pub const WRAP_INDICATOR_WIDTH: usize = 2;

/// Wrap a line to fit within the given width, returning visual line segments.
/// The first segment uses full width, continuation segments are prefixed with WRAP_INDICATOR.
/// Returns (is_continuation, content) pairs.
pub fn wrap_line(line: &str, width: usize) -> Vec<(bool, String)> {
    if width == 0 {
        return vec![(false, line.to_string())];
    }

    let mut result = Vec::new();
    let chars = line.chars();
    let mut current_width = 0;
    let mut current_segment = String::new();
    let mut is_first = true;

    for c in chars {
        let char_width = c.width().unwrap_or(0);
        let effective_width = if is_first {
            width
        } else {
            width.saturating_sub(WRAP_INDICATOR_WIDTH)
        };

        if current_width + char_width > effective_width && !current_segment.is_empty() {
            // Current segment is full, push it and start a new one
            result.push((!is_first, current_segment));
            current_segment = String::new();
            current_width = 0;
            is_first = false;
        }

        current_segment.push(c);
        current_width += char_width;
    }

    // Push the last segment if non-empty, or if the line was empty push one empty segment
    if !current_segment.is_empty() || result.is_empty() {
        result.push((!is_first, current_segment));
    }

    result
}

pub struct Panel {
    pub title: String,
    pub task_name: String,
    pub cmd: Vec<String>,
    pub cwd: Option<String>,
    pub messages: MessageBuf,
    pub scroll: usize,
    pub follow: bool,
    pub show_stdout: bool,
    pub show_stderr: bool,
    pub show_status: bool,
    pub timestamps: bool,
    pub process_status: Option<crate::ui::ProcessStatus>,
}

impl Panel {
    pub fn new(
        task_name: String,
        cmd: Vec<String>,
        cwd: Option<String>,
        show_stdout: bool,
        show_stderr: bool,
        timestamps: bool,
    ) -> Self {
        Self {
            title: task_name.clone(),
            task_name,
            cmd,
            cwd,
            messages: MessageBuf::new(),
            scroll: 0,
            follow: true,
            show_stdout,
            show_stderr,
            show_status: true,
            timestamps,
            process_status: None,
        }
    }

    pub fn with_timestamps(mut self, timestamps: bool) -> Self {
        self.timestamps = timestamps;
        self
    }

    pub fn visible_len(&self) -> usize {
        self.messages
            .lines_filtered(self.show_stdout, self.show_stderr, self.show_status)
            .len()
    }

    /// Compute total visual lines when wrapped to the given width.
    pub fn total_visual_lines(&self, width: usize) -> usize {
        self.messages
            .lines_filtered(self.show_stdout, self.show_stderr, self.show_status)
            .iter()
            .map(|(_, line)| wrap_line(line, width).len())
            .sum()
    }
}

#[derive(Default)]
pub struct StatusPanel {
    pub entries: Vec<StatusEntry>,
    pub scroll: usize,
    pub entry_indices: HashMap<String, usize>,
}

#[derive(Clone)]
pub struct StatusEntry {
    pub task_name: String,
    pub status: crate::ui::ProcessStatus,
    pub exit_code: Option<i32>,
    pub action_type: Option<crate::config::TaskAction>,
    pub dependencies: Vec<String>,
}

impl StatusPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a mutable reference to an entry, creating it if it doesn't exist.
    fn get_or_create_entry(&mut self, task_name: &str) -> &mut StatusEntry {
        let pos = self.entries.iter().position(|e| e.task_name == task_name);
        match pos {
            Some(idx) => &mut self.entries[idx],
            None => {
                self.entries.push(StatusEntry {
                    task_name: task_name.to_string(),
                    status: crate::ui::ProcessStatus::Exited,
                    exit_code: None,
                    action_type: None,
                    dependencies: Vec::new(),
                });
                self.entries.last_mut().unwrap()
            }
        }
    }

    pub fn update_entry(&mut self, task_name: String, status: crate::ui::ProcessStatus) {
        self.get_or_create_entry(&task_name).status = status;
    }

    pub fn update_exit_code(&mut self, task_name: String, exit_code: Option<i32>) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.task_name == task_name) {
            entry.exit_code = exit_code;
        }
    }

    pub fn update_entry_with_action(
        &mut self,
        task_name: String,
        status: crate::ui::ProcessStatus,
        action_type: crate::config::TaskAction,
    ) {
        let entry = self.get_or_create_entry(&task_name);
        entry.status = status;
        entry.action_type = Some(action_type);
    }

    pub fn update_dependencies(&mut self, task_name: String, dependencies: Vec<String>) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.task_name == task_name) {
            entry.dependencies = dependencies;
        }
    }

    pub fn get_health_status(&self) -> (usize, usize, bool) {
        let mut total = 0;
        let mut healthy = 0;

        for entry in &self.entries {
            // Skip tasks that haven't started yet
            if entry.status == crate::ui::ProcessStatus::NotStarted {
                continue;
            }

            if entry.action_type.is_some() {
                total += 1;

                let is_healthy = match (&entry.action_type, entry.status) {
                    (
                        Some(crate::config::TaskAction::Run { .. }),
                        crate::ui::ProcessStatus::Exited,
                    ) => entry.exit_code == Some(0),
                    (
                        Some(crate::config::TaskAction::Start { .. }),
                        crate::ui::ProcessStatus::Running,
                    ) => true,
                    _ => false,
                };

                if is_healthy {
                    healthy += 1;
                }
            }
        }

        let has_issues = total > 0 && healthy < total;
        (healthy, total, has_issues)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TaskAction;

    #[test]
    fn test_message_buf_new() {
        let buf = MessageBuf::new();
        assert_eq!(buf.rope.len_lines(), 1);
        assert_eq!(buf.rope.len_chars(), 0);
    }

    #[test]
    fn test_message_buf_push_single_line() {
        let mut buf = MessageBuf::new();
        buf.push(MessageKind::Stdout, "test line", None);
        assert_eq!(buf.rope.len_lines(), 2);
        assert!(buf.rope.to_string().contains("test line"));
    }

    #[test]
    fn test_message_buf_push_multiple_lines() {
        let mut buf = MessageBuf::new();
        buf.push(MessageKind::Stdout, "line 1", None);
        buf.push(MessageKind::Stderr, "line 2", None);
        buf.push(MessageKind::Status, "line 3", None);
        assert_eq!(buf.rope.len_lines(), 4);
        let text = buf.rope.to_string();
        assert!(text.contains("line 1"));
        assert!(text.contains("line 2"));
        assert!(text.contains("line 3"));
    }

    #[test]
    fn test_message_buf_truncation() {
        let mut buf = MessageBuf::new();
        for i in 0..MAX_LINES + 100 {
            buf.push(MessageKind::Stdout, &format!("line {i}"), None);
        }
        assert_eq!(buf.rope.len_lines(), MAX_LINES);
    }

    #[test]
    fn test_message_buf_lines_filtered() {
        let mut buf = MessageBuf::new();
        buf.push(MessageKind::Stdout, "stdout line", None);
        buf.push(MessageKind::Stderr, "stderr line", None);
        buf.push(MessageKind::Status, "status line", None);

        let lines = buf.lines_filtered(true, true, true);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].0, MessageKind::Stdout);
        assert_eq!(lines[0].1, "stdout line");
        assert_eq!(lines[1].0, MessageKind::Stderr);
        assert_eq!(lines[1].1, "stderr line");
        assert_eq!(lines[2].0, MessageKind::Status);
        assert_eq!(lines[2].1, "status line");
    }

    #[test]
    fn test_message_buf_lines_filtered_stdout_only() {
        let mut buf = MessageBuf::new();
        buf.push(MessageKind::Stdout, "stdout line", None);
        buf.push(MessageKind::Stderr, "stderr line", None);
        buf.push(MessageKind::Status, "status line", None);

        let lines = buf.lines_filtered(true, false, false);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, MessageKind::Stdout);
        assert_eq!(lines[0].1, "stdout line");
    }

    #[test]
    fn test_message_buf_lines_filtered_stderr_only() {
        let mut buf = MessageBuf::new();
        buf.push(MessageKind::Stdout, "stdout line", None);
        buf.push(MessageKind::Stderr, "stderr line", None);
        buf.push(MessageKind::Status, "status line", None);

        let lines = buf.lines_filtered(false, true, false);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, MessageKind::Stderr);
        assert_eq!(lines[0].1, "stderr line");
    }

    #[test]
    fn test_message_buf_lines_filtered_status_only() {
        let mut buf = MessageBuf::new();
        buf.push(MessageKind::Stdout, "stdout line", None);
        buf.push(MessageKind::Stderr, "stderr line", None);
        buf.push(MessageKind::Status, "status line", None);

        let lines = buf.lines_filtered(false, false, true);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, MessageKind::Status);
        assert_eq!(lines[0].1, "status line");
    }

    #[test]
    fn test_panel_new() {
        let panel = Panel::new(
            "test-task".to_string(),
            vec!["echo".to_string(), "hello".to_string()],
            Some("/tmp".to_string()),
            true,
            true,
            false,
        );

        assert_eq!(panel.title, "test-task");
        assert_eq!(panel.task_name, "test-task");
        assert_eq!(panel.cmd, vec!["echo".to_string(), "hello".to_string()]);
        assert_eq!(panel.cwd, Some("/tmp".to_string()));
        assert_eq!(panel.scroll, 0);
        assert!(panel.follow);
        assert!(panel.show_stdout);
        assert!(panel.show_stderr);
        assert!(!panel.timestamps);
        assert_eq!(panel.process_status, None);
    }

    #[test]
    fn test_panel_new_with_defaults() {
        let panel = Panel::new(
            "task".to_string(),
            vec!["command".to_string()],
            None,
            false,
            false,
            false,
        );

        assert_eq!(panel.title, "task");
        assert_eq!(panel.cwd, None);
        assert!(!panel.show_stdout);
        assert!(!panel.show_stderr);
        assert!(!panel.timestamps);
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
    fn test_status_panel_new() {
        let panel = StatusPanel::new();
        assert!(panel.entries.is_empty());
        assert_eq!(panel.scroll, 0);
        assert!(panel.entry_indices.is_empty());
    }

    #[test]
    fn test_status_panel_update_entry_new() {
        let mut panel = StatusPanel::new();
        panel.update_entry("task1".to_string(), crate::ui::ProcessStatus::Running);

        assert_eq!(panel.entries.len(), 1);
        assert_eq!(panel.entries[0].task_name, "task1");
        assert_eq!(panel.entries[0].status, crate::ui::ProcessStatus::Running);
        assert_eq!(panel.entries[0].exit_code, None);
        assert_eq!(panel.entries[0].action_type, None);
    }

    #[test]
    fn test_status_panel_update_entry_existing() {
        let mut panel = StatusPanel::new();
        panel.update_entry("task1".to_string(), crate::ui::ProcessStatus::Running);
        panel.update_entry("task1".to_string(), crate::ui::ProcessStatus::Exited);

        assert_eq!(panel.entries.len(), 1);
        assert_eq!(panel.entries[0].status, crate::ui::ProcessStatus::Exited);
    }

    #[test]
    fn test_status_panel_update_exit_code() {
        let mut panel = StatusPanel::new();
        panel.update_entry("task1".to_string(), crate::ui::ProcessStatus::Running);
        panel.update_exit_code("task1".to_string(), Some(0));

        assert_eq!(panel.entries[0].exit_code, Some(0));

        panel.update_exit_code("task1".to_string(), Some(1));
        assert_eq!(panel.entries[0].exit_code, Some(1));
    }

    #[test]
    fn test_status_panel_update_exit_code_nonexistent() {
        let mut panel = StatusPanel::new();
        panel.update_exit_code("nonexistent".to_string(), Some(0));
        assert!(panel.entries.is_empty());
    }

    #[test]
    fn test_status_panel_update_entry_with_action_new() {
        let mut panel = StatusPanel::new();
        panel.update_entry_with_action(
            "task1".to_string(),
            crate::ui::ProcessStatus::Running,
            TaskAction::Start {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo test",
                )),
            },
        );

        assert_eq!(panel.entries.len(), 1);
        assert_eq!(panel.entries[0].task_name, "task1");
        assert_eq!(panel.entries[0].status, crate::ui::ProcessStatus::Running);
        assert!(matches!(
            panel.entries[0].action_type,
            Some(TaskAction::Start { .. })
        ));
    }

    #[test]
    fn test_status_panel_update_entry_with_action_existing() {
        let mut panel = StatusPanel::new();
        panel.update_entry_with_action(
            "task1".to_string(),
            crate::ui::ProcessStatus::Running,
            TaskAction::Start {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo test",
                )),
            },
        );
        panel.update_entry_with_action(
            "task1".to_string(),
            crate::ui::ProcessStatus::Exited,
            TaskAction::Run {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo test2",
                )),
            },
        );

        assert_eq!(panel.entries.len(), 1);
        assert_eq!(panel.entries[0].status, crate::ui::ProcessStatus::Exited);
        assert!(matches!(
            panel.entries[0].action_type,
            Some(TaskAction::Run { .. })
        ));
    }

    #[test]
    fn test_stream_kind_variants() {
        let stdout = StreamKind::Stdout;
        let stderr = StreamKind::Stderr;

        assert_eq!(stdout, StreamKind::Stdout);
        assert_eq!(stderr, StreamKind::Stderr);
        assert_ne!(stdout, stderr);
    }

    #[test]
    fn test_status_entry_clone() {
        let entry = StatusEntry {
            task_name: "test".to_string(),
            status: crate::ui::ProcessStatus::Running,
            exit_code: None,
            action_type: None,
            dependencies: Vec::new(),
        };
        let cloned = entry.clone();
        assert_eq!(entry.task_name, cloned.task_name);
        assert_eq!(entry.status, cloned.status);
    }

    #[test]
    fn test_status_panel_multiple_entries() {
        let mut panel = StatusPanel::new();
        panel.update_entry("task1".to_string(), crate::ui::ProcessStatus::Running);
        panel.update_entry("task2".to_string(), crate::ui::ProcessStatus::Exited);
        panel.update_exit_code("task2".to_string(), Some(1));

        assert_eq!(panel.entries.len(), 2);
        assert!(panel.entries.iter().any(|e| e.task_name == "task1"));
        assert!(panel.entries.iter().any(|e| e.task_name == "task2"));
    }

    #[test]
    fn test_status_panel_update_dependencies() {
        let mut panel = StatusPanel::new();
        panel.update_entry("task1".to_string(), crate::ui::ProcessStatus::Running);
        panel.update_dependencies(
            "task1".to_string(),
            vec!["dep1".to_string(), "dep2".to_string()],
        );

        let entry = panel
            .entries
            .iter()
            .find(|e| e.task_name == "task1")
            .unwrap();
        assert_eq!(entry.dependencies, vec!["dep1", "dep2"]);
    }

    #[test]
    fn test_status_panel_update_dependencies_empty() {
        let mut panel = StatusPanel::new();
        panel.update_entry("task1".to_string(), crate::ui::ProcessStatus::Running);
        panel.update_dependencies("task1".to_string(), vec![]);

        let entry = panel
            .entries
            .iter()
            .find(|e| e.task_name == "task1")
            .unwrap();
        assert!(entry.dependencies.is_empty());
    }

    #[test]
    fn test_get_health_status_empty() {
        let panel = StatusPanel::new();
        let (healthy, total, has_issues) = panel.get_health_status();
        assert_eq!(healthy, 0);
        assert_eq!(total, 0);
        assert!(!has_issues);
    }

    #[test]
    fn test_get_health_status_all_healthy_run() {
        let mut panel = StatusPanel::new();
        panel.update_entry_with_action(
            "task1".to_string(),
            crate::ui::ProcessStatus::Exited,
            TaskAction::Run {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo test",
                )),
            },
        );
        panel.update_exit_code("task1".to_string(), Some(0));

        let (healthy, total, has_issues) = panel.get_health_status();
        assert_eq!(healthy, 1);
        assert_eq!(total, 1);
        assert!(!has_issues);
    }

    #[test]
    fn test_get_health_status_unhealthy_run_nonzero_exit() {
        let mut panel = StatusPanel::new();
        panel.update_entry_with_action(
            "task1".to_string(),
            crate::ui::ProcessStatus::Exited,
            TaskAction::Run {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo test",
                )),
            },
        );
        panel.update_exit_code("task1".to_string(), Some(1));

        let (healthy, total, has_issues) = panel.get_health_status();
        assert_eq!(healthy, 0);
        assert_eq!(total, 1);
        assert!(has_issues);
    }

    #[test]
    fn test_get_health_status_healthy_start_running() {
        let mut panel = StatusPanel::new();
        panel.update_entry_with_action(
            "task1".to_string(),
            crate::ui::ProcessStatus::Running,
            TaskAction::Start {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo test",
                )),
            },
        );

        let (healthy, total, has_issues) = panel.get_health_status();
        assert_eq!(healthy, 1);
        assert_eq!(total, 1);
        assert!(!has_issues);
    }

    #[test]
    fn test_get_health_status_unhealthy_start_exited() {
        let mut panel = StatusPanel::new();
        panel.update_entry_with_action(
            "task1".to_string(),
            crate::ui::ProcessStatus::Exited,
            TaskAction::Start {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo test",
                )),
            },
        );

        let (healthy, total, has_issues) = panel.get_health_status();
        assert_eq!(healthy, 0);
        assert_eq!(total, 1);
        assert!(has_issues);
    }

    #[test]
    fn test_get_health_status_mixed() {
        let mut panel = StatusPanel::new();

        panel.update_entry_with_action(
            "run_success".to_string(),
            crate::ui::ProcessStatus::Exited,
            TaskAction::Run {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo success",
                )),
            },
        );
        panel.update_exit_code("run_success".to_string(), Some(0));

        panel.update_entry_with_action(
            "run_failure".to_string(),
            crate::ui::ProcessStatus::Exited,
            TaskAction::Run {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo failure",
                )),
            },
        );
        panel.update_exit_code("run_failure".to_string(), Some(1));

        panel.update_entry_with_action(
            "start_running".to_string(),
            crate::ui::ProcessStatus::Running,
            TaskAction::Start {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo start",
                )),
            },
        );

        panel.update_entry_with_action(
            "start_exited".to_string(),
            crate::ui::ProcessStatus::Exited,
            TaskAction::Start {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo start",
                )),
            },
        );

        panel.update_entry("no_action".to_string(), crate::ui::ProcessStatus::Running);

        let (healthy, total, has_issues) = panel.get_health_status();
        assert_eq!(healthy, 2);
        assert_eq!(total, 4);
        assert!(has_issues);
    }

    #[test]
    fn test_get_health_status_ignores_no_action() {
        let mut panel = StatusPanel::new();
        panel.update_entry("task1".to_string(), crate::ui::ProcessStatus::Running);
        panel.update_entry("task2".to_string(), crate::ui::ProcessStatus::Exited);

        let (healthy, total, has_issues) = panel.get_health_status();
        assert_eq!(healthy, 0);
        assert_eq!(total, 0);
        assert!(!has_issues);
    }

    #[test]
    fn test_get_health_status_multiple_healthy() {
        let mut panel = StatusPanel::new();

        panel.update_entry_with_action(
            "run1".to_string(),
            crate::ui::ProcessStatus::Exited,
            TaskAction::Run {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed("echo 1")),
            },
        );
        panel.update_exit_code("run1".to_string(), Some(0));

        panel.update_entry_with_action(
            "run2".to_string(),
            crate::ui::ProcessStatus::Exited,
            TaskAction::Run {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed("echo 2")),
            },
        );
        panel.update_exit_code("run2".to_string(), Some(0));

        panel.update_entry_with_action(
            "start1".to_string(),
            crate::ui::ProcessStatus::Running,
            TaskAction::Start {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo start",
                )),
            },
        );

        let (healthy, total, has_issues) = panel.get_health_status();
        assert_eq!(healthy, 3);
        assert_eq!(total, 3);
        assert!(!has_issues);
    }

    #[test]
    fn test_get_health_status_excludes_not_started() {
        let mut panel = StatusPanel::new();

        // A task that has started and is running - should be counted
        panel.update_entry_with_action(
            "started".to_string(),
            crate::ui::ProcessStatus::Running,
            TaskAction::Start {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo started",
                )),
            },
        );

        // A task that hasn't started yet - should NOT be counted
        panel.update_entry_with_action(
            "pending".to_string(),
            crate::ui::ProcessStatus::NotStarted,
            TaskAction::Start {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo pending",
                )),
            },
        );

        let (healthy, total, has_issues) = panel.get_health_status();
        assert_eq!(healthy, 1);
        assert_eq!(total, 1);
        assert!(!has_issues);
    }

    #[test]
    fn test_panel_index_new() {
        let idx = PanelIndex::new(5);
        assert_eq!(*idx, 5);
        assert_eq!(idx.0, 5);
    }

    #[test]
    fn test_panel_index_deref() {
        let idx = PanelIndex::new(3);
        // Deref should give us the inner usize
        let val: usize = *idx;
        assert_eq!(val, 3);
    }

    #[test]
    fn test_panel_index_from_usize() {
        let idx: PanelIndex = 7.into();
        assert_eq!(*idx, 7);
    }

    #[test]
    fn test_panel_index_into_usize() {
        let idx = PanelIndex::new(4);
        let val: usize = idx.into();
        assert_eq!(val, 4);
    }

    #[test]
    fn test_panel_index_equality() {
        let a = PanelIndex::new(2);
        let b = PanelIndex::new(2);
        let c = PanelIndex::new(3);

        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_panel_index_copy_clone() {
        let a = PanelIndex::new(1);
        let b = a; // Copy
        let c = a.clone(); // Clone

        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn test_wrap_line_short_line() {
        let result = wrap_line("short", 20);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], (false, "short".to_string()));
    }

    #[test]
    fn test_wrap_line_exact_fit() {
        let result = wrap_line("12345", 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], (false, "12345".to_string()));
    }

    #[test]
    fn test_wrap_line_needs_wrap() {
        let result = wrap_line("hello world", 5);
        // "hello" (5), " worl" (5 - 2 for indicator = 3), "d" (remaining)
        assert!(result.len() >= 2);
        assert!(!result[0].0); // First segment is not continuation
        assert!(result[1].0); // Second segment is continuation
    }

    #[test]
    fn test_wrap_line_empty() {
        let result = wrap_line("", 20);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], (false, "".to_string()));
    }

    #[test]
    fn test_wrap_line_zero_width() {
        let result = wrap_line("test", 0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], (false, "test".to_string()));
    }

    #[test]
    fn test_wrap_line_unicode() {
        // Test with CJK characters (2-column width each)
        let result = wrap_line("你好世界", 4);
        // Each CJK char is 2 columns wide, so "你好" (4 cols) fits in first segment
        // "世界" would need continuation
        assert!(result.len() >= 2);
    }

    #[test]
    fn test_wrap_line_continuation_marker() {
        let result = wrap_line("abcdefghij", 5);
        // First segment: "abcde" (5 chars)
        // Second segment is continuation, gets WRAP_INDICATOR prefix
        // So effective width is 5 - 2 = 3 chars: "fgh"
        // Third segment: "ij"
        assert_eq!(result[0], (false, "abcde".to_string()));
        assert!(result[1].0); // is_continuation = true
        assert!(result.len() >= 2);
    }

    #[test]
    fn test_total_visual_lines() {
        let mut panel = Panel::new(
            "test".to_string(),
            vec!["echo".to_string()],
            None,
            true,
            false,
            false,
        );
        // Add a short line and a long line
        panel.messages.push(MessageKind::Stdout, "short", None);
        panel.messages.push(
            MessageKind::Stdout,
            "this is a very long line that should wrap",
            None,
        );

        // With width 10, the long line should wrap
        let visual = panel.total_visual_lines(10);
        assert!(visual > 2); // More visual lines than logical lines
    }
}
