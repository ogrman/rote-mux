use crate::panel::StreamKind;
use std::process::ExitStatus;

pub enum UiEvent {
    Line {
        panel: usize,
        stream: StreamKind,
        text: String,
    },
    Exited {
        panel: usize,
        status: Option<ExitStatus>,
        title: String,
    },
    SwitchPanel(usize),
    Scroll(i32),
    ToggleStdout,
    ToggleStderr,
    Restart,
    Exit,
}
