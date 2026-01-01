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
    use nix::sys::signal::{Signal, kill};
    match kill(pid, Signal::SIGUSR1) {
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
