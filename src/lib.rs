
pub mod config;
use std::process::{Command, Stdio};
use config::Service;


pub fn execute_service(service: &Service, yaml_dir: &std::path::Path) {
	if let Some(cmd) = &service.command {
		let cwd = service.cwd.as_ref().map(|d| yaml_dir.join(d)).unwrap_or_else(|| yaml_dir.to_path_buf());
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
		println!("No command for service");
	}
}
