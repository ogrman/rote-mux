use indexmap::IndexMap;
use serde::Deserialize;
use std::borrow::Cow;
use std::time::Duration;

/// Represents a healthcheck method - either a shell command or a built-in tool.
#[derive(Debug, Clone, PartialEq)]
pub enum HealthcheckMethod {
    /// A shell command to run (via sh -c)
    Cmd(String),
    /// A built-in tool to call directly (without spawning a process)
    Tool(HealthcheckTool),
}

/// Built-in healthcheck tools that can be called directly without spawning a process.
#[derive(Debug, Clone, PartialEq)]
pub enum HealthcheckTool {
    /// Check if a port is open on localhost
    IsPortOpen { port: u16 },
}

/// Healthcheck configuration for a task.
/// When specified, a task with `run` action is not considered healthy
/// until the healthcheck command exits with code 0.
#[derive(Debug, Clone, PartialEq)]
pub struct Healthcheck {
    /// The method to use for the healthcheck (either cmd or tool).
    pub method: HealthcheckMethod,
    /// How often to run the healthcheck (in seconds).
    pub interval: Duration,
}

impl<'de> serde::Deserialize<'de> for Healthcheck {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawHealthcheck {
            cmd: Option<String>,
            tool: Option<String>,
            #[serde(deserialize_with = "deserialize_duration_secs")]
            interval: Duration,
        }

        let raw = RawHealthcheck::deserialize(deserializer)?;

        let method = match (raw.cmd, raw.tool) {
            (Some(cmd), None) => HealthcheckMethod::Cmd(cmd),
            (None, Some(tool_str)) => {
                let tool = parse_tool(&tool_str).map_err(serde::de::Error::custom)?;
                HealthcheckMethod::Tool(tool)
            }
            (Some(_), Some(_)) => {
                return Err(serde::de::Error::custom(
                    "healthcheck cannot have both 'cmd' and 'tool' specified",
                ));
            }
            (None, None) => {
                return Err(serde::de::Error::custom(
                    "healthcheck must have either 'cmd' or 'tool' specified",
                ));
            }
        };

        Ok(Healthcheck {
            method,
            interval: raw.interval,
        })
    }
}

/// Parse a tool string like "is-port-open 5432" into a HealthcheckTool.
fn parse_tool(s: &str) -> Result<HealthcheckTool, String> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.is_empty() {
        return Err("empty tool specification".to_string());
    }

    match parts[0] {
        "is-port-open" => {
            if parts.len() != 2 {
                return Err("is-port-open requires exactly one argument: port".to_string());
            }
            let port: u16 = parts[1]
                .parse()
                .map_err(|_| format!("invalid port number: {}", parts[1]))?;
            Ok(HealthcheckTool::IsPortOpen { port })
        }
        _ => Err(format!("unknown tool: {}", parts[0])),
    }
}

fn deserialize_duration_secs<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let secs: f64 = Deserialize::deserialize(deserializer)?;
    Ok(Duration::from_secs_f64(secs))
}

#[derive(Debug, Deserialize)]
pub struct Config {
    /// The default task to run when none is specified.
    pub default: Option<String>,
    /// A mapping of task names to their configurations (preserves YAML order).
    pub tasks: IndexMap<String, TaskConfiguration>,
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
    /// Optional healthcheck configuration. When specified, dependents will
    /// wait for this task's healthcheck to pass before starting.
    #[serde(default)]
    pub healthcheck: Option<Healthcheck>,
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
    fn test_task_order_preserved_from_yaml() {
        let yaml = r#"
default: main
tasks:
  first:
    run: echo first
  second:
    run: echo second
  third:
    run: echo third
  fourth:
    ensure: true
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let task_names: Vec<_> = config.tasks.keys().collect();
        assert_eq!(task_names, vec!["first", "second", "third", "fourth"]);
    }

    #[test]
    fn test_example_yaml_task_order() {
        let config = load_config();
        let task_names: Vec<_> = config.tasks.keys().collect();
        // Tasks should be in the order they appear in example.yaml
        assert_eq!(
            task_names,
            vec![
                "google-ping",
                "cloudflare-ping",
                "github-ping",
                "short-lived",
                "auto-restarting",
                "setup-task",
                "ping-demo"
            ]
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

    #[test]
    fn test_healthcheck_parsing_cmd() {
        let yaml = r#"
default: task
tasks:
  task:
    run: ./server
    healthcheck:
      cmd: "rote tool is-port-open 8080"
      interval: 1
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let task = &config.tasks["task"];
        assert!(task.healthcheck.is_some());
        let hc = task.healthcheck.as_ref().unwrap();
        assert_eq!(
            hc.method,
            HealthcheckMethod::Cmd("rote tool is-port-open 8080".to_string())
        );
        assert_eq!(hc.interval, std::time::Duration::from_secs(1));
    }

    #[test]
    fn test_healthcheck_parsing_tool() {
        let yaml = r#"
default: task
tasks:
  task:
    run: ./server
    healthcheck:
      tool: is-port-open 8080
      interval: 1
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let task = &config.tasks["task"];
        assert!(task.healthcheck.is_some());
        let hc = task.healthcheck.as_ref().unwrap();
        assert_eq!(
            hc.method,
            HealthcheckMethod::Tool(HealthcheckTool::IsPortOpen { port: 8080 })
        );
        assert_eq!(hc.interval, std::time::Duration::from_secs(1));
    }

    #[test]
    fn test_healthcheck_parsing_fractional_interval() {
        let yaml = r#"
default: task
tasks:
  task:
    run: ./server
    healthcheck:
      cmd: curl http://localhost:8080/health
      interval: 0.5
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let task = &config.tasks["task"];
        let hc = task.healthcheck.as_ref().unwrap();
        assert_eq!(hc.interval, std::time::Duration::from_millis(500));
    }

    #[test]
    fn test_healthcheck_optional() {
        let yaml = r#"
default: task
tasks:
  task:
    run: ./server
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let task = &config.tasks["task"];
        assert!(task.healthcheck.is_none());
    }

    #[test]
    fn test_healthcheck_both_cmd_and_tool_error() {
        let yaml = r#"
default: task
tasks:
  task:
    run: ./server
    healthcheck:
      cmd: "true"
      tool: is-port-open 8080
      interval: 1
"#;
        let result: Result<Config, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("both"));
    }

    #[test]
    fn test_healthcheck_neither_cmd_nor_tool_error() {
        let yaml = r#"
default: task
tasks:
  task:
    run: ./server
    healthcheck:
      interval: 1
"#;
        let result: Result<Config, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("either"));
    }

    #[test]
    fn test_healthcheck_invalid_tool() {
        let yaml = r#"
default: task
tasks:
  task:
    run: ./server
    healthcheck:
      tool: unknown-tool 123
      interval: 1
"#;
        let result: Result<Config, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown tool"));
    }

    #[test]
    fn test_healthcheck_tool_invalid_port() {
        let yaml = r#"
default: task
tasks:
  task:
    run: ./server
    healthcheck:
      tool: is-port-open not-a-number
      interval: 1
"#;
        let result: Result<Config, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid port"));
    }
}
