use ropey::Rope;

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
    pub cmd: Vec<String>,
    pub stdout: StreamBuf,
    pub stderr: StreamBuf,
    pub scroll: usize,
    pub follow: bool,
    pub show_stdout: bool,
    pub show_stderr: bool,
}

impl Panel {
    pub fn new(cmd: Vec<String>) -> Self {
        Self {
            title: cmd.join(" "),
            cmd,
            stdout: StreamBuf::new(),
            stderr: StreamBuf::new(),
            scroll: 0,
            follow: true,
            show_stdout: true,
            show_stderr: true,
        }
    }
}
