use std::collections::{HashMap, HashSet};

use crate::config::{Config, TaskAction};
use crate::error::{Result, RoteError};
use crate::panel::PanelIndex;

/// Manages task lifecycle and dependencies.
pub struct TaskManager {
    /// Tasks waiting to be started (in dependency order).
    pending_tasks: Vec<String>,
    /// Ensure tasks that have completed successfully.
    completed_ensure_tasks: HashSet<String>,
    /// Run tasks with healthchecks that have passed.
    healthy_tasks: HashSet<String>,
    /// Mapping from task name to panel index.
    task_to_panel: HashMap<String, PanelIndex>,
}

impl TaskManager {
    /// Create a new TaskManager with the given list of tasks to start.
    pub fn new(tasks_to_start: Vec<String>, task_to_panel: HashMap<String, PanelIndex>) -> Self {
        Self {
            pending_tasks: tasks_to_start,
            completed_ensure_tasks: HashSet::new(),
            healthy_tasks: HashSet::new(),
            task_to_panel,
        }
    }

    /// Mark an Ensure task as completed (exit code 0).
    pub fn mark_ensure_completed(&mut self, task_name: &str) {
        self.completed_ensure_tasks.insert(task_name.to_string());
    }

    /// Mark a Run task with a healthcheck as healthy.
    pub fn mark_healthy(&mut self, task_name: &str) {
        self.healthy_tasks.insert(task_name.to_string());
    }

    /// Check if a task is marked as healthy.
    pub fn is_healthy(&self, task_name: &str) -> bool {
        self.healthy_tasks.contains(task_name)
    }

    /// Get the panel index for a task.
    pub fn get_panel_index(&self, task_name: &str) -> Option<PanelIndex> {
        self.task_to_panel.get(task_name).copied()
    }

    /// Get tasks that are ready to start (all blocking dependencies satisfied).
    /// Returns the tasks and removes them from the pending list.
    pub fn take_ready_tasks(&mut self, config: &Config) -> Vec<String> {
        let mut ready = Vec::new();
        let mut i = 0;

        while i < self.pending_tasks.len() {
            let task_name = &self.pending_tasks[i];
            if self.are_deps_satisfied(task_name, config) {
                ready.push(self.pending_tasks.remove(i));
            } else {
                i += 1;
            }
        }

        ready
    }

    /// Check if all blocking dependencies for a task have been satisfied.
    /// A dependency blocks if it's an Ensure task (must complete with exit 0)
    /// or a Run task with a healthcheck (must pass healthcheck).
    fn are_deps_satisfied(&self, task_name: &str, config: &Config) -> bool {
        let Some(task_config) = config.tasks.get(task_name) else {
            return true;
        };

        task_config.require.iter().all(|dep| {
            if let Some(dep_config) = config.tasks.get(dep) {
                match &dep_config.action {
                    Some(TaskAction::Ensure { .. }) => {
                        // Ensure tasks must complete successfully
                        self.completed_ensure_tasks.contains(dep)
                    }
                    Some(TaskAction::Run { .. }) => {
                        // Run tasks with healthchecks must become healthy
                        if dep_config.healthcheck.is_some() {
                            self.healthy_tasks.contains(dep)
                        } else {
                            true // Run tasks without healthchecks don't block
                        }
                    }
                    None => true, // No action, assume satisfied
                }
            } else {
                true // Unknown dep, assume satisfied
            }
        })
    }

    /// Check if there are pending tasks.
    pub fn has_pending_tasks(&self) -> bool {
        !self.pending_tasks.is_empty()
    }
}

/// Resolve all dependencies for the target tasks using topological sort.
pub fn resolve_dependencies(config: &Config, targets: &[String]) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut temp_mark = HashSet::new();

    fn visit(
        task: &str,
        config: &Config,
        result: &mut Vec<String>,
        visited: &mut HashSet<String>,
        temp_mark: &mut HashSet<String>,
    ) -> Result<()> {
        if visited.contains(task) {
            return Ok(());
        }

        if temp_mark.contains(task) {
            return Err(RoteError::Dependency(format!(
                "Circular dependency detected involving task '{task}'"
            )));
        }

        temp_mark.insert(task.to_string());

        let task_config = config
            .tasks
            .get(task)
            .ok_or_else(|| RoteError::Dependency(format!("Task '{task}' not found in config")))?;

        // Visit dependencies first
        for dep in &task_config.require {
            visit(dep, config, result, visited, temp_mark)?;
        }

        temp_mark.remove(task);
        visited.insert(task.to_string());
        result.push(task.to_string());

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
    use crate::config::{CommandValue, TaskConfiguration};
    use indexmap::IndexMap;
    use std::borrow::Cow;

    fn make_config_with_tasks(tasks: Vec<(&str, Option<TaskAction>, Vec<&str>)>) -> Config {
        let mut task_map = IndexMap::new();
        for (name, action, require) in tasks {
            task_map.insert(
                name.to_string(),
                TaskConfiguration {
                    action,
                    cwd: None,
                    display: None,
                    require: require.into_iter().map(String::from).collect(),
                    autorestart: false,
                    timestamps: false,
                    healthcheck: None,
                },
            );
        }
        Config {
            default: None,
            tasks: task_map,
        }
    }

    #[test]
    fn test_resolve_dependencies_empty() {
        let config = make_config_with_tasks(vec![]);
        let result = resolve_dependencies(&config, &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_resolve_dependencies_no_deps() {
        let config = make_config_with_tasks(vec![("task1", None, vec![])]);
        let result = resolve_dependencies(&config, &["task1".to_string()]).unwrap();
        assert_eq!(result, vec!["task1"]);
    }

    #[test]
    fn test_resolve_dependencies_with_deps() {
        let config =
            make_config_with_tasks(vec![("task1", None, vec!["dep1"]), ("dep1", None, vec![])]);
        let result = resolve_dependencies(&config, &["task1".to_string()]).unwrap();
        assert_eq!(result, vec!["dep1", "task1"]);
    }

    #[test]
    fn test_resolve_dependencies_circular() {
        let config = make_config_with_tasks(vec![
            ("task1", None, vec!["task2"]),
            ("task2", None, vec!["task1"]),
        ]);
        let result = resolve_dependencies(&config, &["task1".to_string()]);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Circular dependency")
        );
    }

    #[test]
    fn test_task_manager_take_ready_no_deps() {
        let config = make_config_with_tasks(vec![("task1", None, vec![]), ("task2", None, vec![])]);

        let mut tm = TaskManager::new(
            vec!["task1".to_string(), "task2".to_string()],
            HashMap::new(),
        );

        let ready = tm.take_ready_tasks(&config);
        assert_eq!(ready.len(), 2);
        assert!(tm.pending_tasks.is_empty());
    }

    #[test]
    fn test_task_manager_take_ready_with_ensure_dep() {
        let config = make_config_with_tasks(vec![
            (
                "setup",
                Some(TaskAction::Ensure {
                    command: CommandValue::String(Cow::Borrowed("echo setup")),
                }),
                vec![],
            ),
            ("task1", None, vec!["setup"]),
        ]);

        let mut tm = TaskManager::new(
            vec!["setup".to_string(), "task1".to_string()],
            HashMap::new(),
        );

        // Initially only setup should be ready
        let ready = tm.take_ready_tasks(&config);
        assert_eq!(ready, vec!["setup"]);
        assert_eq!(tm.pending_tasks, vec!["task1"]);

        // After marking setup as complete, task1 should be ready
        tm.mark_ensure_completed("setup");
        let ready = tm.take_ready_tasks(&config);
        assert_eq!(ready, vec!["task1"]);
        assert!(tm.pending_tasks.is_empty());
    }

    #[test]
    fn test_task_manager_run_dep_does_not_block() {
        let config = make_config_with_tasks(vec![
            (
                "server",
                Some(TaskAction::Run {
                    command: CommandValue::String(Cow::Borrowed("server")),
                }),
                vec![],
            ),
            ("task1", None, vec!["server"]),
        ]);

        let mut tm = TaskManager::new(
            vec!["server".to_string(), "task1".to_string()],
            HashMap::new(),
        );

        // Both should be ready since Run deps don't block
        let ready = tm.take_ready_tasks(&config);
        assert_eq!(ready.len(), 2);
    }

    #[test]
    fn test_task_manager_run_with_healthcheck_blocks() {
        use crate::config::{Healthcheck, HealthcheckMethod};
        use std::time::Duration;

        let mut task_map = IndexMap::new();
        task_map.insert(
            "server".to_string(),
            TaskConfiguration {
                action: Some(TaskAction::Run {
                    command: CommandValue::String(Cow::Borrowed("./server")),
                }),
                cwd: None,
                display: None,
                require: vec![],
                autorestart: false,
                timestamps: false,
                healthcheck: Some(Healthcheck {
                    method: HealthcheckMethod::Cmd("curl localhost:8080".to_string()),
                    interval: Duration::from_secs(1),
                }),
            },
        );
        task_map.insert(
            "client".to_string(),
            TaskConfiguration {
                action: Some(TaskAction::Run {
                    command: CommandValue::String(Cow::Borrowed("./client")),
                }),
                cwd: None,
                display: None,
                require: vec!["server".to_string()],
                autorestart: false,
                timestamps: false,
                healthcheck: None,
            },
        );

        let config = Config {
            default: None,
            tasks: task_map,
        };

        let mut tm = TaskManager::new(
            vec!["server".to_string(), "client".to_string()],
            HashMap::new(),
        );

        // Only server should be ready - client is blocked by healthcheck
        let ready = tm.take_ready_tasks(&config);
        assert_eq!(ready, vec!["server"]);
        assert_eq!(tm.pending_tasks, vec!["client"]);

        // After marking server as healthy, client should be ready
        tm.mark_healthy("server");
        let ready = tm.take_ready_tasks(&config);
        assert_eq!(ready, vec!["client"]);
        assert!(tm.pending_tasks.is_empty());
    }
}
