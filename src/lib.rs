pub mod config;
use config::Service;
use std::process::{Command, Stdio};

use crate::config::ServiceAction;

pub fn execute_service(service: &Service, yaml_dir: &std::path::Path) {
    let cmd = service.action.as_ref();
    if let Some(cmd) = cmd {
        let cwd = service
            .cwd
            .as_ref()
            .map(|d| yaml_dir.join(d))
            .unwrap_or_else(|| yaml_dir.to_path_buf());
        println!("Running: {:?} in {:?}", cmd, cwd);
        let mut child = Command::new("bash")
            .arg("-c")
            .arg(&match cmd {
                ServiceAction::Run { command } => command.as_ref(),
                ServiceAction::Start { command } => command.as_ref(),
            })
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Failed to start service");
        let _ = child.wait();
    } else {
        println!("No run/start command for service");
    }
}
