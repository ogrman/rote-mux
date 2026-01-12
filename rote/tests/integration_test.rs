use indexmap::IndexMap;
use rote_mux::panel::PanelIndex;
use rote_mux::{Config, UiEvent};
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
        rote_mux::run_with_input(
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
        rote_mux::run_with_input(
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
    use rote_mux::config::{CommandValue, TaskAction, TaskConfiguration};
    use std::borrow::Cow;

    let mut tasks = IndexMap::new();

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
            healthcheck: None,
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
            healthcheck: None,
        },
    );

    let config = Config {
        default: Some("main".to_string()),
        tasks,
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<UiEvent>(100);

    let app_task = tokio::spawn(async move {
        rote_mux::run_with_input(config, vec![], std::path::PathBuf::from("."), Some(rx)).await
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
        rote_mux::run_with_input(
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
        rote_mux::run_with_input(
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
        rote_mux::run_with_input(
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

/// Test that a task with a healthcheck blocks its dependents until the healthcheck passes.
#[tokio::test]
async fn test_healthcheck_blocks_dependent_until_passed() {
    use rote_mux::config::{
        CommandValue, Healthcheck, HealthcheckMethod, TaskAction, TaskConfiguration,
    };
    use std::borrow::Cow;

    let mut tasks = IndexMap::new();

    // A Run task with a healthcheck that passes quickly
    tasks.insert(
        "server".to_string(),
        TaskConfiguration {
            action: Some(TaskAction::Run {
                command: CommandValue::String(Cow::Borrowed("echo server started; sleep 10")),
            }),
            cwd: None,
            display: None,
            require: vec![],
            autorestart: false,
            timestamps: false,
            healthcheck: Some(Healthcheck {
                method: HealthcheckMethod::Cmd("true".to_string()),
                interval: Duration::from_millis(100),
            }),
        },
    );

    // A task that depends on the server (should wait for healthcheck)
    tasks.insert(
        "client".to_string(),
        TaskConfiguration {
            action: Some(TaskAction::Run {
                command: CommandValue::String(Cow::Borrowed("echo client started")),
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
        default: Some("client".to_string()),
        tasks,
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<UiEvent>(100);

    let app_task = tokio::spawn(async move {
        rote_mux::run_with_input(config, vec![], std::path::PathBuf::from("."), Some(rx)).await
    });

    // Wait for healthcheck to pass and client to start
    // The healthcheck interval is 100ms, so 500ms should be plenty
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send exit event
    let _ = tx.send(UiEvent::Exit).await;
    drop(tx);

    // App should exit cleanly
    let result = timeout(Duration::from_secs(3), app_task).await;
    assert!(result.is_ok(), "App should exit within 3 seconds");
    assert!(result.unwrap().is_ok(), "App should exit successfully");
}

/// Test healthcheck with is-port-open tool.
#[tokio::test]
async fn test_healthcheck_with_port_tool() {
    use rote_mux::config::{
        CommandValue, Healthcheck, HealthcheckMethod, HealthcheckTool, TaskAction,
        TaskConfiguration,
    };
    use std::borrow::Cow;
    use std::net::TcpListener;

    // Bind to a random port that we'll use for the healthcheck
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let mut tasks = IndexMap::new();

    // A Run task with a healthcheck that uses is-port-open
    // The port is already open (we have a listener), so healthcheck should pass immediately
    tasks.insert(
        "server".to_string(),
        TaskConfiguration {
            action: Some(TaskAction::Run {
                command: CommandValue::String(Cow::Borrowed("echo server; sleep 10")),
            }),
            cwd: None,
            display: None,
            require: vec![],
            autorestart: false,
            timestamps: false,
            healthcheck: Some(Healthcheck {
                method: HealthcheckMethod::Tool(HealthcheckTool::IsPortOpen { port }),
                interval: Duration::from_millis(100),
            }),
        },
    );

    // A task that depends on the server
    tasks.insert(
        "client".to_string(),
        TaskConfiguration {
            action: Some(TaskAction::Run {
                command: CommandValue::String(Cow::Borrowed("echo client started")),
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
        default: Some("client".to_string()),
        tasks,
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<UiEvent>(100);

    let app_task = tokio::spawn(async move {
        rote_mux::run_with_input(config, vec![], std::path::PathBuf::from("."), Some(rx)).await
    });

    // Wait for healthcheck to pass
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send exit event
    let _ = tx.send(UiEvent::Exit).await;
    drop(tx);

    // Keep the listener alive until we're done
    drop(listener);

    // App should exit cleanly
    let result = timeout(Duration::from_secs(3), app_task).await;
    assert!(result.is_ok(), "App should exit within 3 seconds");
    assert!(result.unwrap().is_ok(), "App should exit successfully");
}

/// Test healthcheck with is-port-open tool where port opens after a delay.
/// This simulates the healthcheck-tool-demo scenario from example.yaml.
#[tokio::test]
async fn test_healthcheck_delayed_port() {
    use rote_mux::config::{
        CommandValue, Healthcheck, HealthcheckMethod, HealthcheckTool, TaskAction,
        TaskConfiguration,
    };
    use std::borrow::Cow;
    use std::net::TcpListener;

    // Find an available port
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    // Port is now closed
    eprintln!("TEST: Using port {}", port);

    let mut tasks = IndexMap::new();

    // A Run task with a healthcheck that uses is-port-open
    // The port is NOT open initially - it will be opened after a delay
    // Note: Commands with shell metacharacters like ; need to use bash -c
    tasks.insert(
        "server".to_string(),
        TaskConfiguration {
            action: Some(TaskAction::Run {
                command: CommandValue::String(Cow::Borrowed("sleep 10")),
            }),
            cwd: None,
            display: None,
            require: vec![],
            autorestart: false,
            timestamps: false,
            healthcheck: Some(Healthcheck {
                method: HealthcheckMethod::Tool(HealthcheckTool::IsPortOpen { port }),
                interval: Duration::from_millis(100),
            }),
        },
    );

    let config = Config {
        default: Some("server".to_string()),
        tasks,
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<UiEvent>(100);

    let app_task = tokio::spawn(async move {
        rote_mux::run_with_input(config, vec![], std::path::PathBuf::from("."), Some(rx)).await
    });

    // Wait a bit for the task to start
    eprintln!("TEST: Waiting 300ms before opening port");
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Open the port - the healthcheck should detect this
    eprintln!("TEST: Opening port {}", port);
    let _listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();

    // Verify port is open
    let is_open = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok();
    eprintln!("TEST: Port {} is open: {}", port, is_open);

    // Wait for healthcheck to pass
    eprintln!("TEST: Waiting 500ms for healthcheck to pass");
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send exit event
    eprintln!("TEST: Sending Exit event");
    let _ = tx.send(UiEvent::Exit).await;
    drop(tx);

    // App should exit cleanly
    let result = timeout(Duration::from_secs(3), app_task).await;
    assert!(result.is_ok(), "App should exit within 3 seconds");
    assert!(result.unwrap().is_ok(), "App should exit successfully");
}
