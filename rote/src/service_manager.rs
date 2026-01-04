use std::collections::{HashMap, HashSet};

use crate::config::{Config, ServiceAction};
use crate::error::{Result, RoteError};
use crate::panel::PanelIndex;

/// Manages service lifecycle and dependencies.
pub struct ServiceManager {
    /// Services waiting to be started (in dependency order).
    pending_services: Vec<String>,
    /// Run services that have completed successfully.
    completed_run_services: HashSet<String>,
    /// Mapping from service name to panel index.
    service_to_panel: HashMap<String, PanelIndex>,
}

impl ServiceManager {
    /// Create a new ServiceManager with the given list of services to start.
    pub fn new(
        services_to_start: Vec<String>,
        service_to_panel: HashMap<String, PanelIndex>,
    ) -> Self {
        Self {
            pending_services: services_to_start,
            completed_run_services: HashSet::new(),
            service_to_panel,
        }
    }

    /// Mark a Run service as completed (exit code 0).
    pub fn mark_run_completed(&mut self, service_name: &str) {
        self.completed_run_services.insert(service_name.to_string());
    }

    /// Get the panel index for a service.
    pub fn get_panel_index(&self, service_name: &str) -> Option<PanelIndex> {
        self.service_to_panel.get(service_name).copied()
    }

    /// Get services that are ready to start (all Run dependencies satisfied).
    /// Returns the services and removes them from the pending list.
    pub fn take_ready_services(&mut self, config: &Config) -> Vec<String> {
        let mut ready = Vec::new();
        let mut i = 0;

        while i < self.pending_services.len() {
            let service_name = &self.pending_services[i];
            if self.are_run_deps_satisfied(service_name, config) {
                ready.push(self.pending_services.remove(i));
            } else {
                i += 1;
            }
        }

        ready
    }

    /// Check if all Run dependencies for a service have completed successfully.
    fn are_run_deps_satisfied(&self, service_name: &str, config: &Config) -> bool {
        let Some(service_config) = config.services.get(service_name) else {
            return true;
        };

        service_config.require.iter().all(|dep| {
            if let Some(dep_config) = config.services.get(dep) {
                if matches!(dep_config.action, Some(ServiceAction::Run { .. })) {
                    self.completed_run_services.contains(dep)
                } else {
                    true // Start dependencies don't block
                }
            } else {
                true // Unknown dep, assume satisfied
            }
        })
    }

    /// Check if there are pending services.
    pub fn has_pending_services(&self) -> bool {
        !self.pending_services.is_empty()
    }
}

/// Resolve all dependencies for the target services using topological sort.
pub fn resolve_dependencies(config: &Config, targets: &[String]) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut temp_mark = HashSet::new();

    fn visit(
        service: &str,
        config: &Config,
        result: &mut Vec<String>,
        visited: &mut HashSet<String>,
        temp_mark: &mut HashSet<String>,
    ) -> Result<()> {
        if visited.contains(service) {
            return Ok(());
        }

        if temp_mark.contains(service) {
            return Err(RoteError::Dependency(format!(
                "Circular dependency detected involving service '{service}'"
            )));
        }

        temp_mark.insert(service.to_string());

        let service_config = config.services.get(service).ok_or_else(|| {
            RoteError::Dependency(format!("Service '{service}' not found in config"))
        })?;

        // Visit dependencies first
        for dep in &service_config.require {
            visit(dep, config, result, visited, temp_mark)?;
        }

        temp_mark.remove(service);
        visited.insert(service.to_string());
        result.push(service.to_string());

        Ok(())
    }

    for target in targets {
        visit(target, config, &mut result, &mut visited, &mut temp_mark)?;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CommandValue, ServiceConfiguration};
    use std::borrow::Cow;

    fn make_config_with_services(
        services: Vec<(&str, Option<ServiceAction>, Vec<&str>)>,
    ) -> Config {
        let mut svc_map = HashMap::new();
        for (name, action, require) in services {
            svc_map.insert(
                name.to_string(),
                ServiceConfiguration {
                    action,
                    cwd: None,
                    display: None,
                    require: require.into_iter().map(String::from).collect(),
                },
            );
        }
        Config {
            default: None,
            services: svc_map,
            timestamps: false,
        }
    }

    #[test]
    fn test_resolve_dependencies_empty() {
        let config = make_config_with_services(vec![]);
        let result = resolve_dependencies(&config, &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_resolve_dependencies_no_deps() {
        let config = make_config_with_services(vec![("service1", None, vec![])]);
        let result = resolve_dependencies(&config, &["service1".to_string()]).unwrap();
        assert_eq!(result, vec!["service1"]);
    }

    #[test]
    fn test_resolve_dependencies_with_deps() {
        let config = make_config_with_services(vec![
            ("service1", None, vec!["dep1"]),
            ("dep1", None, vec![]),
        ]);
        let result = resolve_dependencies(&config, &["service1".to_string()]).unwrap();
        assert_eq!(result, vec!["dep1", "service1"]);
    }

    #[test]
    fn test_resolve_dependencies_circular() {
        let config = make_config_with_services(vec![
            ("service1", None, vec!["service2"]),
            ("service2", None, vec!["service1"]),
        ]);
        let result = resolve_dependencies(&config, &["service1".to_string()]);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Circular dependency")
        );
    }

    #[test]
    fn test_service_manager_take_ready_no_deps() {
        let config =
            make_config_with_services(vec![("service1", None, vec![]), ("service2", None, vec![])]);

        let mut sm = ServiceManager::new(
            vec!["service1".to_string(), "service2".to_string()],
            HashMap::new(),
        );

        let ready = sm.take_ready_services(&config);
        assert_eq!(ready.len(), 2);
        assert!(sm.pending_services.is_empty());
    }

    #[test]
    fn test_service_manager_take_ready_with_run_dep() {
        let config = make_config_with_services(vec![
            (
                "setup",
                Some(ServiceAction::Run {
                    command: CommandValue::String(Cow::Borrowed("echo setup")),
                }),
                vec![],
            ),
            ("service1", None, vec!["setup"]),
        ]);

        let mut sm = ServiceManager::new(
            vec!["setup".to_string(), "service1".to_string()],
            HashMap::new(),
        );

        // Initially only setup should be ready
        let ready = sm.take_ready_services(&config);
        assert_eq!(ready, vec!["setup"]);
        assert_eq!(sm.pending_services, vec!["service1"]);

        // After marking setup as complete, service1 should be ready
        sm.mark_run_completed("setup");
        let ready = sm.take_ready_services(&config);
        assert_eq!(ready, vec!["service1"]);
        assert!(sm.pending_services.is_empty());
    }

    #[test]
    fn test_service_manager_start_dep_does_not_block() {
        let config = make_config_with_services(vec![
            (
                "server",
                Some(ServiceAction::Start {
                    command: CommandValue::String(Cow::Borrowed("server")),
                }),
                vec![],
            ),
            ("service1", None, vec!["server"]),
        ]);

        let mut sm = ServiceManager::new(
            vec!["server".to_string(), "service1".to_string()],
            HashMap::new(),
        );

        // Both should be ready since Start deps don't block
        let ready = sm.take_ready_services(&config);
        assert_eq!(ready.len(), 2);
    }
}
