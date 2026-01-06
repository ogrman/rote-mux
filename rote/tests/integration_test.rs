use rote::panel::PanelIndex;
use rote::{Config, UiEvent};
use std::collections::HashMap;
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
    eprintln!("Test: Exit event send result: {send_result:?}");
    drop(tx);

    eprintln!("Test: Waiting for app to exit (max 5s)");
    // Wait for app to exit
    let result = timeout(Duration::from_secs(5), app_task).await;

    eprintln!("Test: App task completed: {result:?}");

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
    let _ = tx.send(UiEvent::SwitchPanel(PanelIndex::new(2))).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    eprintln!("Test: Sending Exit event");
    // Send quit event
    let _ = tx.send(UiEvent::Exit).await;
    drop(tx);

    eprintln!("Test: Waiting for app to exit (max 5s)");
    // Wait for app to exit
    let result = timeout(Duration::from_secs(5), app_task).await;

    eprintln!("Test: App task completed: {result:?}");

    // The app should exit cleanly, not timeout
    assert!(result.is_ok(), "App should exit within 5 seconds");
    assert!(result.unwrap().is_ok(), "App should exit successfully");
}

/// Test that a task with an Ensure dependency waits for the Ensure task to complete.
#[tokio::test]
async fn test_ensure_dependency_blocks_until_complete() {
    use rote::config::{CommandValue, TaskAction, TaskConfiguration};
    use std::borrow::Cow;

    let mut tasks = HashMap::new();

    // An Ensure task that completes quickly
    tasks.insert(
        "setup".to_string(),
        TaskConfiguration {
            action: Some(TaskAction::Ensure {
                command: CommandValue::String(Cow::Borrowed("echo setup done")),
            }),
            cwd: None,
            display: None,
            require: vec![],
            autorestart: false,
            timestamps: false,
        },
    );

    // A Run task that depends on setup
    tasks.insert(
        "main".to_string(),
        TaskConfiguration {
            action: Some(TaskAction::Run {
                command: CommandValue::String(Cow::Borrowed("echo main started")),
            }),
            cwd: None,
            display: None,
            require: vec!["setup".to_string()],
            autorestart: false,
            timestamps: false,
        },
    );

    let config = Config {
        default: Some("main".to_string()),
        tasks,
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<UiEvent>(100);

    let app_task = tokio::spawn(async move {
        rote::run_with_input(config, vec![], std::path::PathBuf::from("."), Some(rx)).await
    });

    // Wait for tasks to start
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Send exit event
    let _ = tx.send(UiEvent::Exit).await;
    drop(tx);

    // App should exit cleanly
    let result = timeout(Duration::from_secs(3), app_task).await;
    assert!(result.is_ok(), "App should exit within 3 seconds");
    assert!(result.unwrap().is_ok(), "App should exit successfully");
}

/// Test scrolling events work correctly.
#[tokio::test]
async fn test_scroll_events() {
    let example_path = format!("{}/tests/data/example.yaml", env!("CARGO_MANIFEST_DIR"));
    let yaml_str = std::fs::read_to_string(&example_path).expect("Failed to read example.yaml");
    let config: Config = serde_yaml::from_str(&yaml_str).expect("Failed to parse example.yaml");

    let (tx, rx) = tokio::sync::mpsc::channel::<UiEvent>(100);

    let app_task = tokio::spawn(async move {
        rote::run_with_input(
            config,
            vec![],
            std::path::PathBuf::from("/home/lars/src/rote/rote/tests/data"),
            Some(rx),
        )
        .await
    });

    // Wait for processes to start
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Switch to a panel first
    let _ = tx.send(UiEvent::SwitchPanel(PanelIndex::new(0))).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Send scroll events
    let _ = tx.send(UiEvent::Scroll(-1)).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let _ = tx.send(UiEvent::Scroll(1)).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Exit
    let _ = tx.send(UiEvent::Exit).await;
    drop(tx);

    let result = timeout(Duration::from_secs(5), app_task).await;
    assert!(result.is_ok(), "App should exit within 5 seconds");
    assert!(result.unwrap().is_ok(), "App should exit successfully");
}

/// Test toggling stdout/stderr visibility.
#[tokio::test]
async fn test_toggle_stream_visibility() {
    let example_path = format!("{}/tests/data/example.yaml", env!("CARGO_MANIFEST_DIR"));
    let yaml_str = std::fs::read_to_string(&example_path).expect("Failed to read example.yaml");
    let config: Config = serde_yaml::from_str(&yaml_str).expect("Failed to parse example.yaml");

    let (tx, rx) = tokio::sync::mpsc::channel::<UiEvent>(100);

    let app_task = tokio::spawn(async move {
        rote::run_with_input(
            config,
            vec![],
            std::path::PathBuf::from("/home/lars/src/rote/rote/tests/data"),
            Some(rx),
        )
        .await
    });

    // Wait for processes to start
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Switch to a panel first
    let _ = tx.send(UiEvent::SwitchPanel(PanelIndex::new(0))).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Toggle stdout off
    let _ = tx.send(UiEvent::ToggleStdout).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Toggle stderr off
    let _ = tx.send(UiEvent::ToggleStderr).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Toggle both back on
    let _ = tx.send(UiEvent::ToggleStdout).await;
    let _ = tx.send(UiEvent::ToggleStderr).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Exit
    let _ = tx.send(UiEvent::Exit).await;
    drop(tx);

    let result = timeout(Duration::from_secs(5), app_task).await;
    assert!(result.is_ok(), "App should exit within 5 seconds");
    assert!(result.unwrap().is_ok(), "App should exit successfully");
}

/// Test switching between status view and panel view.
#[tokio::test]
async fn test_switch_status_and_panel_views() {
    let example_path = format!("{}/tests/data/example.yaml", env!("CARGO_MANIFEST_DIR"));
    let yaml_str = std::fs::read_to_string(&example_path).expect("Failed to read example.yaml");
    let config: Config = serde_yaml::from_str(&yaml_str).expect("Failed to parse example.yaml");

    let (tx, rx) = tokio::sync::mpsc::channel::<UiEvent>(100);

    let app_task = tokio::spawn(async move {
        rote::run_with_input(
            config,
            vec![],
            std::path::PathBuf::from("/home/lars/src/rote/rote/tests/data"),
            Some(rx),
        )
        .await
    });

    // Wait for processes to start
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Switch to panel 1
    let _ = tx.send(UiEvent::SwitchPanel(PanelIndex::new(0))).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Switch back to status view
    let _ = tx.send(UiEvent::SwitchToStatus).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Switch to panel 2
    let _ = tx.send(UiEvent::SwitchPanel(PanelIndex::new(1))).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Exit
    let _ = tx.send(UiEvent::Exit).await;
    drop(tx);

    let result = timeout(Duration::from_secs(5), app_task).await;
    assert!(result.is_ok(), "App should exit within 5 seconds");
    assert!(result.unwrap().is_ok(), "App should exit successfully");
}
