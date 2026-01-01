use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::time::Duration;

pub async fn terminate_child(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };
    let pid = Pid::from_raw(pid as i32);

    let _ = kill(pid, Signal::SIGINT);
    tokio::time::sleep(Duration::from_millis(300)).await;
    if check_process_exited(pid) {
        return;
    }

    let _ = kill(pid, Signal::SIGTERM);
    tokio::time::sleep(Duration::from_millis(300)).await;
    if check_process_exited(pid) {
        return;
    }

    let _ = kill(pid, Signal::SIGKILL);
}

fn check_process_exited(pid: Pid) -> bool {
    use nix::sys::signal::kill;
    match kill(pid, None) {
        Err(nix::Error::ESRCH) => true, // Process does not exist
        Ok(_) => false,                 // Process still exists
        Err(_) => false,
    }
}

pub async fn wait_for_child_exit(pid: Option<u32>) -> bool {
    let Some(pid) = pid else {
        return false;
    };
    let pid = Pid::from_raw(pid as i32);

    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if check_process_exited(pid) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use tokio::time;

    #[tokio::test]
    async fn test_terminate_child_none_pid() {
        terminate_child(None).await;
    }

    #[tokio::test]
    async fn test_terminate_child_sigint() {
        let mut child = Command::new("sleep")
            .arg("10")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        let pid = child.id();
        assert_ne!(pid, 0);

        let _ = terminate_child(Some(pid)).await;

        time::sleep(Duration::from_millis(100)).await;
        let status = child.try_wait();
        assert!(
            status.is_ok() && status.unwrap().is_some(),
            "Process should have exited"
        );
    }

    #[tokio::test]
    async fn test_wait_for_child_exit_none_pid() {
        let result = wait_for_child_exit(None).await;
        assert!(!result);
    }

    #[tokio::test]
    async fn test_wait_for_child_exit_nonexistent_process() {
        let result = wait_for_child_exit(Some(999999)).await;
        assert!(result, "Nonexistent process should be considered exited");
    }

    #[tokio::test]
    async fn test_wait_for_child_exit_timeout() {
        let mut child = Command::new("sleep")
            .arg("10")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        let pid = child.id();

        let start = time::Instant::now();
        let result = wait_for_child_exit(Some(pid)).await;
        let elapsed = start.elapsed();

        assert!(!result, "Wait should timeout");
        assert!(
            elapsed >= Duration::from_secs(4),
            "Should have waited at least 4 seconds"
        );
        assert!(
            elapsed < Duration::from_secs(6),
            "Should not have waited more than 5 seconds"
        );

        child.kill().expect("Failed to kill process");
        child.wait().expect("Failed to wait for process");
    }

    #[test]
    fn test_check_process_exited_nonexistent() {
        let pid = Pid::from_raw(999999);
        assert!(
            check_process_exited(pid),
            "Nonexistent process should be considered exited"
        );
    }

    #[tokio::test]
    async fn test_terminate_child_ignores_sigint() {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("trap '' SIGINT; sleep 10")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sh process");

        let pid = child.id();

        let _ = terminate_child(Some(pid)).await;

        time::sleep(Duration::from_millis(100)).await;
        let status = child.try_wait();
        assert!(
            status.is_ok() && status.unwrap().is_some(),
            "Process should have exited after SIGTERM or SIGKILL"
        );
    }
}
