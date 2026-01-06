use serde::Deserialize;
use std::{borrow::Cow, collections::HashMap};

#[derive(Debug, Deserialize)]
pub struct Config {
    /// The default task to run when none is specified.
    pub default: Option<String>,
    /// A mapping of task names to their configurations.
    pub tasks: HashMap<String, TaskConfiguration>,
}

#[derive(Debug, Deserialize)]
pub struct TaskConfiguration {
    /// The action to be performed for the task (either `run` or `start`).
    #[serde(default, flatten)]
    pub action: Option<TaskAction>,
    /// The working directory for the task command, relative to the
    /// directory containing the YAML file.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Specifies which output streams to display. If omitted, all streams
    /// are displayed. An empty list means no output is displayed.
    #[serde(default)]
    pub display: Option<Vec<String>>,
    /// A list of other tasks that must be started before this task.
    #[serde(default)]
    pub require: Vec<String>,
    /// Whether to automatically restart the task when it exits.
    #[serde(default)]
    pub autorestart: bool,
    /// Whether to show timestamps for log messages.
    #[serde(default)]
    pub timestamps: bool,
}

/// Represents the action to be performed for a task.
///
/// This can either be an `ensure` action or a `run` action, each containing
/// a command to be executed. `ensure` is used for something that should run
/// to completion before the task is considered ready, while `run` is used
/// for long-running tasks. These are mutually exclusive.
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(untagged)]
pub enum TaskAction {
    Ensure {
        #[serde(rename = "ensure")]
        command: CommandValue,
    },
    Run {
        #[serde(rename = "run")]
        command: CommandValue,
    },
}

/// Represents a command value that can be either a string or a boolean.
#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum CommandValue {
    String(Cow<'static, str>),
    Bool(bool),
}

impl CommandValue {
    pub fn as_command(&self) -> Cow<'static, str> {
        match self {
            CommandValue::String(s) => s.clone(),
            CommandValue::Bool(true) => Cow::Borrowed("true"),
            CommandValue::Bool(false) => Cow::Borrowed("false"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn load_config() -> Config {
        let path = Path::new("tests/data/example.yaml");
        let yaml_str = fs::read_to_string(path).expect("Failed to read example.yaml");
        serde_yaml::from_str(&yaml_str).expect("Failed to deserialize YAML")
    }

    #[test]
    fn test_deserialize_action() {
        {
            let yaml_ensure = r#"
                ensure: echo 'Hello, World!'
                "#;
            let action: TaskAction = serde_yaml::from_str(yaml_ensure).unwrap();
            assert_eq!(
                action,
                TaskAction::Ensure {
                    command: CommandValue::String(Cow::Borrowed("echo 'Hello, World!'")),
                },
            );
        }

        {
            let yaml_run = r#"
                run: ./start_task.sh
                "#;
            let action: TaskAction = serde_yaml::from_str(yaml_run).unwrap();
            assert_eq!(
                action,
                TaskAction::Run {
                    command: CommandValue::String(Cow::Borrowed("./start_task.sh")),
                }
            );
        }
    }

    #[test]
    fn test_deserialize_example_yaml() {
        let config = load_config();
        assert_eq!(config.default.as_deref(), Some("ping-demo"));
        let map = &config.tasks;
        assert_eq!(
            map["google-ping"].action,
            Some(TaskAction::Run {
                command: CommandValue::String(Cow::Borrowed("ping google.com")),
            })
        );
        assert_eq!(map["google-ping"].display, None);

        assert_eq!(
            map["cloudflare-ping"].action,
            Some(TaskAction::Run {
                command: CommandValue::String(Cow::Borrowed("ping 1.1.1.1")),
            }),
        );
        assert_eq!(map["cloudflare-ping"].display, None);

        assert_eq!(
            map["ping-demo"].require,
            vec![
                "google-ping".to_string(),
                "cloudflare-ping".to_string(),
                "short-lived".to_string(),
                "auto-restarting".to_string()
            ]
        );
        assert!(map["ping-demo"].action.is_none());

        // Check setup-task task
        assert_eq!(
            map["setup-task"].action,
            Some(TaskAction::Ensure {
                command: CommandValue::Bool(true),
            })
        );
    }

    #[test]
    fn test_missing_optional_fields() {
        let yaml = r#"
    default: task
    tasks:
      task:
        ensure: echo 'hi'
    "#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let task = &config.tasks["task"];
        assert_eq!(
            task.action,
            Some(TaskAction::Ensure {
                command: CommandValue::String(Cow::Borrowed("echo 'hi'")),
            })
        );
        assert_eq!(task.cwd, None);
        assert_eq!(task.display, None);
        assert_eq!(task.require, Vec::<String>::new());
    }

    #[test]
    fn test_default_field_optional() {
        let yaml = r#"
            tasks: {}
            "#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.default, None);
    }

    #[test]
    fn test_invalid_yaml() {
        let yaml = "not: valid: yaml";
        let result: Result<Config, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_extra_fields_are_ignored() {
        let yaml = r#"
            default: task
            tasks:
                task:
                    ensure: echo 'hi'
                    extra: value
            "#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let task = &config.tasks["task"];
        assert_eq!(
            task.action,
            Some(TaskAction::Ensure {
                command: CommandValue::String(Cow::Borrowed("echo 'hi'")),
            })
        );
    }

    #[test]
    fn test_display_and_require_empty() {
        let yaml = r#"
    default: task
    tasks:
      task:
        ensure: echo 'hi'
        display: []
        require: []
    "#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let task = &config.tasks["task"];
        assert_eq!(
            task.action,
            Some(TaskAction::Ensure {
                command: CommandValue::String(Cow::Borrowed("echo 'hi'")),
            })
        );
        assert_eq!(task.display, Some(vec![]));
        assert_eq!(task.require, Vec::<String>::new());
    }

    #[test]
    fn test_ensure_with_boolean_true() {
        let yaml = r#"
    default: task
    tasks:
      task:
        ensure: true
    "#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let task = &config.tasks["task"];
        assert_eq!(
            task.action,
            Some(TaskAction::Ensure {
                command: CommandValue::Bool(true),
            })
        );
        if let Some(TaskAction::Ensure { command }) = &task.action {
            assert_eq!(command.as_command(), Cow::Borrowed("true"));
        }
    }

    #[test]
    fn test_ensure_with_boolean_false() {
        let yaml = r#"
    default: task
    tasks:
      task:
        ensure: false
    "#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let task = &config.tasks["task"];
        assert_eq!(
            task.action,
            Some(TaskAction::Ensure {
                command: CommandValue::Bool(false),
            })
        );
        if let Some(TaskAction::Ensure { command }) = &task.action {
            assert_eq!(command.as_command(), Cow::Borrowed("false"));
        }
    }
}
