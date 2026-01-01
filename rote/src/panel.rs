use ropey::Rope;
use std::collections::HashMap;

pub const MAX_LINES: usize = 5_000;

#[derive(Copy, Clone, Debug)]
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
            });
        }
    }
}
