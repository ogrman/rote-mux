use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub default: String,
    pub services: HashMap<String, Service>,
}

#[derive(Debug, Deserialize)]
pub struct Service {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub display: Option<Vec<String>>,
    #[serde(default)]
    pub require: Option<Vec<String>>,
    #[serde(default)]
    pub default: bool,
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
    fn test_deserialize_example_yaml() {
        let config = load_config();
        assert_eq!(config.default, "run");
        let map = &config.services;
        assert_eq!(map["database"].command.as_deref(), Some("docker-compose up database --detach --wait"));
        assert_eq!(map["database"].display, Some(vec![]));

        assert_eq!(map["frontend"].cwd.as_deref(), Some("frontend"));
        assert_eq!(map["frontend"].command.as_deref(), Some("scripts/frontend.sh"));
        assert_eq!(map["frontend"].display, Some(vec!["stderr".to_string()]));

        assert_eq!(map["backend"].cwd.as_deref(), Some("backend"));
        assert_eq!(map["backend"].command.as_deref(), Some("scripts/backend.sh"));
        assert_eq!(map["backend"].require, Some(vec!["database".to_string(), "setup".to_string()]));
        assert_eq!(map["backend"].display, None);

        assert_eq!(map["setup"].command.as_deref(), Some("scripts/setup.sh"));

        assert!(map["run"].command.is_none());
        assert_eq!(map["run"].require, Some(vec!["backend".to_string(), "frontend".to_string()]));
    }

    #[test]
    fn test_missing_optional_fields() {
        let yaml = r#"
default: service
services:
  service:
    command: echo 'hi'
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let service = &config.services["service"];
        assert_eq!(service.command.as_deref(), Some("echo 'hi'"));
        assert_eq!(service.cwd, None);
        assert_eq!(service.display, None);
        assert_eq!(service.require, None);
    }

    #[test]
    fn test_default_field_true() {
        let yaml = r#"
default: service
services:
  service:
    command: echo 'hi'
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.default, "service");
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
    command: echo 'hi'
    extra: value
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let service = &config.services["service"];
        assert_eq!(service.command.as_deref(), Some("echo 'hi'"));
    }

    #[test]
    fn test_display_and_require_empty() {
        let yaml = r#"
default: service
services:
  service:
    command: echo 'hi'
    display: []
    require: []
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let service = &config.services["service"];
        assert_eq!(service.display, Some(vec![]));
        assert_eq!(service.require, Some(vec![]));
    }
}
