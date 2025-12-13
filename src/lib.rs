pub mod config;
use config::Service;
use std::process::{Command, Stdio};

pub fn execute_service(service: &Service, yaml_dir: &std::path::Path) {
    let cmd = service.run.as_ref().or(service.start.as_ref());
    if let Some(cmd) = cmd {
        let cwd = service
            .cwd
            .as_ref()
            .map(|d| yaml_dir.join(d))
            .unwrap_or_else(|| yaml_dir.to_path_buf());
        println!("Running: {} in {:?}", cmd, cwd);
        let mut child = Command::new("bash")
            .arg("-c")
            .arg(cmd)
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
