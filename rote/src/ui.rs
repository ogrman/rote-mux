use crate::panel::StreamKind;
use std::process::ExitStatus;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcessStatus {
    Running,
    Exited,
}

#[derive(Clone)]
pub enum UiEvent {
    Line {
        panel: usize,
        stream: StreamKind,
        text: String,
    },
    Exited {
        panel: usize,
        status: Option<ExitStatus>,
        exit_code: Option<i32>,
        title: String,
    },
    ProcessStatus {
        panel: usize,
        status: ProcessStatus,
    },
    SwitchPanel(usize),
    SwitchToStatus,
    CheckStatus,
    Scroll(i32),
    ToggleStdout,
    ToggleStderr,
    Restart,
    Exit,
}
