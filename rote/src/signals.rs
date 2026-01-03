use nix::sys::signal::kill;
use nix::unistd::Pid;
use std::time::Duration;

fn check_process_exited(pid: Pid) -> bool {
    match kill(pid, None) {
        Err(nix::Error::ESRCH) => true,
        Ok(_) => false,
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
}
