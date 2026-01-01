use serde::Deserialize;
use std::{borrow::Cow, collections::HashMap};

#[derive(Debug, Deserialize)]
pub struct Config {
    /// The default service to run when none is specified.
    pub default: Option<String>,
    /// A mapping of service names to their configurations.
    pub services: HashMap<String, ServiceConfiguration>,
}

#[derive(Debug, Deserialize)]
pub struct ServiceConfiguration {
    /// The action to be performed for the service (either `run` or `start`).
    #[serde(default, flatten)]
    pub action: Option<ServiceAction>,
    /// The working directory for the service command, relative to the
    /// directory containing the YAML file.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Specifies which output streams to display. If omitted, all streams
    /// are displayed. An empty list means no output is displayed.
    #[serde(default)]
    pub display: Option<Vec<String>>,
    /// A list of other services that must be started before this service.
    #[serde(default)]
    pub require: Vec<String>,
}

/// Represents the action to be performed for a service.
///
/// This can either be a `run` action or a `start` action, each containing
/// a command to be executed. `run` is used for something that should run
/// to completion before the service is considered ready, while `start` is used
/// for long-running services. These are mutually exclusive.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ServiceAction {
    Run {
        #[serde(rename = "run")]
        command: Cow<'static, str>,
    },
    Start {
        #[serde(rename = "start")]
        command: Cow<'static, str>,
    },
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
            let yaml_run = r#"
                run: echo 'Hello, World!'
                "#;
            let action: ServiceAction = serde_yaml::from_str(yaml_run).unwrap();
            assert_eq!(
                action,
                ServiceAction::Run {
                    command: Cow::Borrowed("echo 'Hello, World!'"),
                },
            );
        }

        {
            let yaml_start = r#"
                start: ./start_service.sh
                "#;
            let action: ServiceAction = serde_yaml::from_str(yaml_start).unwrap();
            assert_eq!(
                action,
                ServiceAction::Start {
                    command: Cow::Borrowed("./start_service.sh"),
                }
            );
        }
    }

    #[test]
    fn test_deserialize_example_yaml() {
        let config = load_config();
        assert_eq!(config.default.as_deref(), Some("ping-demo"));
        let map = &config.services;
        assert_eq!(
            map["google-ping"].action,
            Some(ServiceAction::Start {
                command: Cow::Borrowed("ping google.com"),
            })
        );
        assert_eq!(map["google-ping"].display, None);

        assert_eq!(
            map["cloudflare-ping"].action,
            Some(ServiceAction::Start {
                command: Cow::Borrowed("ping 1.1.1.1"),
            }),
        );
        assert_eq!(map["cloudflare-ping"].display, None);

        assert_eq!(
            map["ping-demo"].require,
            vec!["google-ping".to_string(), "cloudflare-ping".to_string()]
        );
        assert!(map["ping-demo"].action.is_none());
    }

    #[test]
    fn test_missing_optional_fields() {
        let yaml = r#"
    default: service
    services:
      service:
        run: echo 'hi'
    "#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let service = &config.services["service"];
        assert_eq!(
            service.action,
            Some(ServiceAction::Run {
                command: Cow::Borrowed("echo 'hi'"),
            })
        );
        assert_eq!(service.cwd, None);
        assert_eq!(service.display, None);
        assert_eq!(service.require, Vec::<String>::new());
    }

    #[test]
    fn test_default_field_optional() {
        let yaml = r#"
            services: {}
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
            default: service
            services:
                service:
                    run: echo 'hi'
                    extra: value
            "#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let service = &config.services["service"];
        assert_eq!(
            service.action,
            Some(ServiceAction::Run {
                command: Cow::Borrowed("echo 'hi'"),
            })
        );
    }

    #[test]
    fn test_display_and_require_empty() {
        let yaml = r#"
    default: service
    services:
      service:
        run: echo 'hi'
        display: []
        require: []
    "#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let service = &config.services["service"];
        assert_eq!(
            service.action,
            Some(ServiceAction::Run {
                command: Cow::Borrowed("echo 'hi'"),
            })
        );
        assert_eq!(service.display, Some(vec![]));
        assert_eq!(service.require, Vec::<String>::new());
    }
}
