use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::time::Duration;
use tokio::process::Child;

pub async fn terminate_child(child: &mut Child) {
    let Some(pid) = child.id() else { return };
    let pid = Pid::from_raw(pid as i32);

    let _ = kill(pid, Signal::SIGINT);
    tokio::time::sleep(Duration::from_millis(300)).await;
    if child.try_wait().ok().flatten().is_some() {
        return;
    }

    let _ = kill(pid, Signal::SIGTERM);
    tokio::time::sleep(Duration::from_millis(300)).await;
    if child.try_wait().ok().flatten().is_some() {
        return;
    }

    let _ = kill(pid, Signal::SIGKILL);
}

pub async fn wait_for_child_exit(child: &mut Child) -> bool {
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if child.try_wait().ok().flatten().is_some() {
            return true;
        }
    }
    false
}
