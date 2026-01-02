use ropey::Rope;
use std::collections::HashMap;

pub const MAX_LINES: usize = 5_000;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum StreamKind {
    Stdout,
    Stderr,
}

pub struct StreamBuf {
    pub rope: Rope,
}

impl StreamBuf {
    pub fn new() -> Self {
        Self { rope: Rope::new() }
    }

    pub fn push(&mut self, line: &str) {
        self.rope.insert(self.rope.len_chars(), line);
        self.rope.insert(self.rope.len_chars(), "\n");

        let excess = self.rope.len_lines().saturating_sub(MAX_LINES);
        if excess > 0 {
            let cut = self.rope.line_to_char(excess);
            self.rope.remove(0..cut);
        }
    }
}

pub struct Panel {
    pub title: String,
    pub service_name: String,
    pub cmd: Vec<String>,
    pub cwd: Option<String>,
    pub stdout: StreamBuf,
    pub stderr: StreamBuf,
    pub scroll: usize,
    pub follow: bool,
    pub show_stdout: bool,
    pub show_stderr: bool,
    pub process_status: Option<crate::ui::ProcessStatus>,
}

impl Panel {
    pub fn new(
        service_name: String,
        cmd: Vec<String>,
        cwd: Option<String>,
        show_stdout: bool,
        show_stderr: bool,
    ) -> Self {
        Self {
            title: service_name.clone(),
            service_name,
            cmd,
            cwd,
            stdout: StreamBuf::new(),
            stderr: StreamBuf::new(),
            scroll: 0,
            follow: true,
            show_stdout,
            show_stderr,
            process_status: None,
        }
    }

    pub fn visible_len(&self) -> usize {
        let mut n = 0;
        if self.show_stdout {
            let lines = self.stdout.rope.len_lines();
            n += if lines > 0 {
                lines.saturating_sub(1)
            } else {
                0
            };
        }
        if self.show_stderr {
            let lines = self.stderr.rope.len_lines();
            n += if lines > 0 {
                lines.saturating_sub(1)
            } else {
                0
            };
        }
        n
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
    pub service_name: String,
    pub status: crate::ui::ProcessStatus,
    pub exit_code: Option<i32>,
    pub action_type: Option<crate::config::ServiceAction>,
    pub dependencies: Vec<String>,
}

impl StatusPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_entry(&mut self, service_name: String, status: crate::ui::ProcessStatus) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.service_name == service_name)
        {
            entry.status = status;
        } else {
            self.entries.push(StatusEntry {
                service_name,
                status,
                exit_code: None,
                action_type: None,
                dependencies: Vec::new(),
            });
        }
    }

    pub fn update_exit_code(&mut self, service_name: String, exit_code: Option<i32>) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.service_name == service_name)
        {
            entry.exit_code = exit_code;
        }
    }

    pub fn update_entry_with_action(
        &mut self,
        service_name: String,
        status: crate::ui::ProcessStatus,
        action_type: crate::config::ServiceAction,
    ) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.service_name == service_name)
        {
            entry.status = status;
            entry.action_type = Some(action_type);
        } else {
            self.entries.push(StatusEntry {
                service_name,
                status,
                exit_code: None,
                action_type: Some(action_type),
                dependencies: Vec::new(),
            });
        }
    }

    pub fn update_dependencies(&mut self, service_name: String, dependencies: Vec<String>) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.service_name == service_name)
        {
            entry.dependencies = dependencies;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ServiceAction;

    #[test]
    fn test_stream_buf_new() {
        let buf = StreamBuf::new();
        assert_eq!(buf.rope.len_lines(), 1);
        assert_eq!(buf.rope.len_chars(), 0);
    }

    #[test]
    fn test_stream_buf_push_single_line() {
        let mut buf = StreamBuf::new();
        buf.push("test line");
        assert_eq!(buf.rope.len_lines(), 2);
        assert!(buf.rope.to_string().contains("test line"));
    }

    #[test]
    fn test_stream_buf_push_multiple_lines() {
        let mut buf = StreamBuf::new();
        buf.push("line 1");
        buf.push("line 2");
        buf.push("line 3");
        assert_eq!(buf.rope.len_lines(), 4);
        let text = buf.rope.to_string();
        assert!(text.contains("line 1"));
        assert!(text.contains("line 2"));
        assert!(text.contains("line 3"));
    }

    #[test]
    fn test_stream_buf_truncation() {
        let mut buf = StreamBuf::new();
        for i in 0..MAX_LINES + 100 {
            buf.push(&format!("line {}", i));
        }
        assert_eq!(buf.rope.len_lines(), MAX_LINES);
    }

    #[test]
    fn test_panel_new() {
        let panel = Panel::new(
            "test-service".to_string(),
            vec!["echo".to_string(), "hello".to_string()],
            Some("/tmp".to_string()),
            true,
            true,
        );

        assert_eq!(panel.title, "test-service");
        assert_eq!(panel.service_name, "test-service");
        assert_eq!(panel.cmd, vec!["echo".to_string(), "hello".to_string()]);
        assert_eq!(panel.cwd, Some("/tmp".to_string()));
        assert_eq!(panel.scroll, 0);
        assert!(panel.follow);
        assert!(panel.show_stdout);
        assert!(panel.show_stderr);
        assert_eq!(panel.process_status, None);
    }

    #[test]
    fn test_panel_new_with_defaults() {
        let panel = Panel::new(
            "service".to_string(),
            vec!["command".to_string()],
            None,
            false,
            false,
        );

        assert_eq!(panel.title, "service");
        assert_eq!(panel.cwd, None);
        assert!(!panel.show_stdout);
        assert!(!panel.show_stderr);
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
        panel.update_entry("service1".to_string(), crate::ui::ProcessStatus::Running);

        assert_eq!(panel.entries.len(), 1);
        assert_eq!(panel.entries[0].service_name, "service1");
        assert_eq!(panel.entries[0].status, crate::ui::ProcessStatus::Running);
        assert_eq!(panel.entries[0].exit_code, None);
        assert_eq!(panel.entries[0].action_type, None);
    }

    #[test]
    fn test_status_panel_update_entry_existing() {
        let mut panel = StatusPanel::new();
        panel.update_entry("service1".to_string(), crate::ui::ProcessStatus::Running);
        panel.update_entry("service1".to_string(), crate::ui::ProcessStatus::Exited);

        assert_eq!(panel.entries.len(), 1);
        assert_eq!(panel.entries[0].status, crate::ui::ProcessStatus::Exited);
    }

    #[test]
    fn test_status_panel_update_exit_code() {
        let mut panel = StatusPanel::new();
        panel.update_entry("service1".to_string(), crate::ui::ProcessStatus::Running);
        panel.update_exit_code("service1".to_string(), Some(0));

        assert_eq!(panel.entries[0].exit_code, Some(0));

        panel.update_exit_code("service1".to_string(), Some(1));
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
            "service1".to_string(),
            crate::ui::ProcessStatus::Running,
            ServiceAction::Start {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo test",
                )),
            },
        );

        assert_eq!(panel.entries.len(), 1);
        assert_eq!(panel.entries[0].service_name, "service1");
        assert_eq!(panel.entries[0].status, crate::ui::ProcessStatus::Running);
        assert!(matches!(
            panel.entries[0].action_type,
            Some(ServiceAction::Start { .. })
        ));
    }

    #[test]
    fn test_status_panel_update_entry_with_action_existing() {
        let mut panel = StatusPanel::new();
        panel.update_entry_with_action(
            "service1".to_string(),
            crate::ui::ProcessStatus::Running,
            ServiceAction::Start {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo test",
                )),
            },
        );
        panel.update_entry_with_action(
            "service1".to_string(),
            crate::ui::ProcessStatus::Exited,
            ServiceAction::Run {
                command: crate::config::CommandValue::String(std::borrow::Cow::Borrowed(
                    "echo test2",
                )),
            },
        );

        assert_eq!(panel.entries.len(), 1);
        assert_eq!(panel.entries[0].status, crate::ui::ProcessStatus::Exited);
        assert!(matches!(
            panel.entries[0].action_type,
            Some(ServiceAction::Run { .. })
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
            service_name: "test".to_string(),
            status: crate::ui::ProcessStatus::Running,
            exit_code: None,
            action_type: None,
            dependencies: Vec::new(),
        };
        let cloned = entry.clone();
        assert_eq!(entry.service_name, cloned.service_name);
        assert_eq!(entry.status, cloned.status);
    }

    #[test]
    fn test_status_panel_multiple_entries() {
        let mut panel = StatusPanel::new();
        panel.update_entry("service1".to_string(), crate::ui::ProcessStatus::Running);
        panel.update_entry("service2".to_string(), crate::ui::ProcessStatus::Exited);
        panel.update_exit_code("service2".to_string(), Some(1));

        assert_eq!(panel.entries.len(), 2);
        assert!(panel.entries.iter().any(|e| e.service_name == "service1"));
        assert!(panel.entries.iter().any(|e| e.service_name == "service2"));
    }

    #[test]
    fn test_status_panel_update_dependencies() {
        let mut panel = StatusPanel::new();
        panel.update_entry("service1".to_string(), crate::ui::ProcessStatus::Running);
        panel.update_dependencies(
            "service1".to_string(),
            vec!["dep1".to_string(), "dep2".to_string()],
        );

        let entry = panel
            .entries
            .iter()
            .find(|e| e.service_name == "service1")
            .unwrap();
        assert_eq!(entry.dependencies, vec!["dep1", "dep2"]);
    }

    #[test]
    fn test_status_panel_update_dependencies_empty() {
        let mut panel = StatusPanel::new();
        panel.update_entry("service1".to_string(), crate::ui::ProcessStatus::Running);
        panel.update_dependencies("service1".to_string(), vec![]);

        let entry = panel
            .entries
            .iter()
            .find(|e| e.service_name == "service1")
            .unwrap();
        assert!(entry.dependencies.is_empty());
    }
}
