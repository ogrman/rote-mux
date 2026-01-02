use rote::{Config, UiEvent};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_example_yaml_quit() {
    // Load example.yaml
    let example_path = format!("{}/tests/data/example.yaml", env!("CARGO_MANIFEST_DIR"));
    let yaml_str = std::fs::read_to_string(&example_path).expect("Failed to read example.yaml");
    let config: Config = serde_yaml::from_str(&yaml_str).expect("Failed to parse example.yaml");

    let (tx, rx) = tokio::sync::mpsc::channel::<UiEvent>(100);

    eprintln!("Test: Starting app with external rx");

    // Spawn app with external event receiver
    let app_task = tokio::spawn(async move {
        rote::run_with_input(
            config,
            vec![],
            std::path::PathBuf::from("/home/lars/src/rote/rote/tests/data"),
            Some(rx),
        )
        .await
    });

    eprintln!("Test: Waiting 500ms for processes to start");
    // Wait a bit for processes to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    eprintln!("Test: Sending Exit event");
    // Send quit event
    let send_result = tx.send(UiEvent::Exit).await;
    eprintln!("Test: Exit event send result: {:?}", send_result);
    drop(tx);

    eprintln!("Test: Waiting for app to exit (max 5s)");
    // Wait for app to exit
    let result = timeout(Duration::from_secs(5), app_task).await;

    eprintln!("Test: App task completed: {:?}", result);

    // The app should exit cleanly, not timeout
    assert!(result.is_ok(), "App should exit within 5 seconds");
    assert!(result.unwrap().is_ok(), "App should exit successfully");
}

#[tokio::test]
async fn test_can_switch_to_run_task_panel() {
    // Load example.yaml
    let example_path = format!("{}/tests/data/example.yaml", env!("CARGO_MANIFEST_DIR"));
    let yaml_str = std::fs::read_to_string(&example_path).expect("Failed to read example.yaml");
    let config: Config = serde_yaml::from_str(&yaml_str).expect("Failed to parse example.yaml");

    let (tx, rx) = tokio::sync::mpsc::channel::<UiEvent>(100);

    eprintln!("Test: Starting app with external rx");

    // Spawn app with external event receiver
    let app_task = tokio::spawn(async move {
        rote::run_with_input(
            config,
            vec![],
            std::path::PathBuf::from("/home/lars/src/rote/rote/tests/data"),
            Some(rx),
        )
        .await
    });

    eprintln!("Test: Waiting 500ms for processes to start");
    // Wait a bit for processes to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    eprintln!("Test: Switching to panel 3 (setup-task)");
    // Send event to switch to panel 3 (setup-task should be at index 2, which is panel 3)
    let _ = tx.send(UiEvent::SwitchPanel(2)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    eprintln!("Test: Sending Exit event");
    // Send quit event
    let _ = tx.send(UiEvent::Exit).await;
    drop(tx);

    eprintln!("Test: Waiting for app to exit (max 5s)");
    // Wait for app to exit
    let result = timeout(Duration::from_secs(5), app_task).await;

    eprintln!("Test: App task completed: {:?}", result);

    // The app should exit cleanly, not timeout
    assert!(result.is_ok(), "App should exit within 5 seconds");
    assert!(result.unwrap().is_ok(), "App should exit successfully");
}
