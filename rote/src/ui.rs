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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_status_variants() {
        let running = ProcessStatus::Running;
        let exited = ProcessStatus::Exited;

        assert_eq!(running, ProcessStatus::Running);
        assert_eq!(exited, ProcessStatus::Exited);
        assert_ne!(running, exited);
    }

    #[test]
    fn test_process_status_clone() {
        let status = ProcessStatus::Running;
        let cloned = status.clone();
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_process_status_copy() {
        let status1 = ProcessStatus::Exited;
        let status2 = status1;
        assert_eq!(status1, ProcessStatus::Exited);
        assert_eq!(status2, ProcessStatus::Exited);
    }

    #[test]
    fn test_process_status_debug() {
        let running = ProcessStatus::Running;
        let exited = ProcessStatus::Exited;

        assert_eq!(format!("{:?}", running), "Running");
        assert_eq!(format!("{:?}", exited), "Exited");
    }

    #[test]
    fn test_ui_event_line() {
        let event = UiEvent::Line {
            panel: 1,
            stream: StreamKind::Stdout,
            text: String::from("test output"),
        };

        match event {
            UiEvent::Line {
                panel,
                stream,
                text,
            } => {
                assert_eq!(panel, 1);
                assert_eq!(stream, StreamKind::Stdout);
                assert_eq!(text, "test output");
            }
            _ => panic!("Expected UiEvent::Line"),
        }
    }

    #[test]
    fn test_ui_event_exited() {
        let event = UiEvent::Exited {
            panel: 2,
            status: None,
            exit_code: Some(0),
            title: String::from("test-service"),
        };

        match event {
            UiEvent::Exited {
                panel,
                status,
                exit_code,
                title,
            } => {
                assert_eq!(panel, 2);
                assert_eq!(status, None);
                assert_eq!(exit_code, Some(0));
                assert_eq!(title, "test-service");
            }
            _ => panic!("Expected UiEvent::Exited"),
        }
    }

    #[test]
    fn test_ui_event_process_status() {
        let event = UiEvent::ProcessStatus {
            panel: 0,
            status: ProcessStatus::Running,
        };

        match event {
            UiEvent::ProcessStatus { panel, status } => {
                assert_eq!(panel, 0);
                assert_eq!(status, ProcessStatus::Running);
            }
            _ => panic!("Expected UiEvent::ProcessStatus"),
        }
    }

    #[test]
    fn test_ui_event_simple_variants() {
        let switch_panel = UiEvent::SwitchPanel(3);
        let switch_to_status = UiEvent::SwitchToStatus;
        let check_status = UiEvent::CheckStatus;
        let scroll = UiEvent::Scroll(-10);
        let toggle_stdout = UiEvent::ToggleStdout;
        let toggle_stderr = UiEvent::ToggleStderr;
        let restart = UiEvent::Restart;
        let exit = UiEvent::Exit;

        match switch_panel {
            UiEvent::SwitchPanel(panel) => assert_eq!(panel, 3),
            _ => panic!("Expected UiEvent::SwitchPanel"),
        }

        assert!(matches!(switch_to_status, UiEvent::SwitchToStatus));
        assert!(matches!(check_status, UiEvent::CheckStatus));
        assert!(matches!(toggle_stdout, UiEvent::ToggleStdout));
        assert!(matches!(toggle_stderr, UiEvent::ToggleStderr));
        assert!(matches!(restart, UiEvent::Restart));
        assert!(matches!(exit, UiEvent::Exit));

        match scroll {
            UiEvent::Scroll(amount) => assert_eq!(amount, -10),
            _ => panic!("Expected UiEvent::Scroll"),
        }
    }

    #[test]
    fn test_ui_event_clone() {
        let event = UiEvent::Line {
            panel: 1,
            stream: StreamKind::Stderr,
            text: String::from("error message"),
        };
        let cloned = event.clone();
        assert!(matches!(cloned, UiEvent::Line { .. }));
    }
}
