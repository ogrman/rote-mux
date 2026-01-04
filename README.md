# Rote

A terminal multiplexer for monitoring and managing multiple processes together.

## Features

- Process Management: Start and monitor multiple processes simultaneously
- TUI Interface: Clean terminal UI with separate panels for each process
- Real-time Output: View stdout and stderr from all processes in real-time
- Smart Signal Handling: Graceful shutdown with signal escalation (SIGINT → SIGTERM → SIGKILL)
- YAML Configuration: Define services and dependencies in a simple config file
- Process Restart: Restart individual processes on the fly
- Scrollable Output: Navigate through process output with keyboard controls
- Stream Filtering: Toggle stdout/stderr visibility per panel
- Status Panel: View the status of all services at a glance
- Service Dependencies: Services can require other services to start first
- Automatic Line Limits: Maximum 5,000 lines per stream to prevent memory issues

## Installation

```bash
cargo install --path rote
```

Or build from source:

```bash
cargo build --release
```

## Quick Start

Create a `rote.yaml` file:

```yaml
default: ping-demo
services:
  google-ping:
    start: ping google.com
  cloudflare-ping:
    start: ping 1.1.1.1
  ping-demo:
    require: [google-ping, cloudflare-ping]
```

Then run:

```bash
rote
```

## Configuration

### Top-Level Fields

- `default` (optional): The default service to run when none is specified
- `services`: A mapping of service names to their configurations

### Service Definition

Each service can have the following properties:

- `start`: Command to start a long-running service
- `run`: Command to run to completion (blocks dependent services until complete)
- `cwd` (optional): Working directory for the command (relative to the config file)
- `display` (optional): List of streams to display (["stdout"], ["stderr"], or both by default)
- `require` (optional): List of services that must be started before this one
- `autorestart` (optional): If true, automatically restart the service when it exits (default: false)

### Actions: `start` vs `run`

- `start`: For long-running processes (servers, daemons). These are spawned in the background and their output is displayed in a panel.
- `run`: For one-time setup tasks (migrations, installations). These run to completion before dependent services start. They do not create a panel.

These are mutually exclusive - a service can only have one or the other.

### Example: Full-Stack Application

```yaml
default: dev
services:
  # One-time setup
  install:
    run: npm install

  # Database
  postgres:
    start: docker run --rm -p 5432:5432 -e POSTGRES_PASSWORD=dev postgres
    display: [stderr]

  # Run migrations after DB is ready
  migrate:
    run: alembic upgrade head
    require: [postgres, install]

  # Backend server
  api:
    cwd: backend
    start: npm run start
    require: [migrate]

  # Frontend dev server
  web:
    cwd: frontend
    start: npm run dev
    require: [install]

  # Development target
  dev:
    require: [api, web]
```

### Display Streams

The `display` field controls which streams are shown for a service:

- Omit or `null`: Show both stdout and stderr (default)
- `["stdout"]`: Show only stdout
- `["stderr"]`: Show only stderr
- `[]`: Hide all output
- `["stdout", "stderr"]`: Show both streams (same as default)

### Dependency Resolution

Services are started in topological order based on their dependencies. Circular dependencies are detected and will cause an error. Services with a `run` action must complete successfully before dependent services start.

## Key Bindings

When running, the following keyboard shortcuts are available:

- `q`: Quit and terminate all processes
- `r`: Restart the currently active process
- `o`: Toggle stdout visibility for the active panel
- `e`: Toggle stderr visibility for the active panel
- `s`: Switch to status panel showing all services
- `1-9`: Switch to panel 1-9
- `↑/↓`: Scroll up/down one line
- `PgUp/PgDn`: Scroll up/down 20 lines

## Process Termination

Rote handles process shutdown gracefully with signal escalation:

1. SIGINT is sent first (Ctrl+C equivalent)
   - Wait 300ms for graceful shutdown
2. SIGTERM is sent if process doesn't exit
   - Wait another 300ms
3. SIGKILL is sent as a last resort
   - Force terminates the process

This ensures processes have an opportunity to clean up resources before being forcefully killed.

## Architecture

Rote is built with Rust and uses:

- Tokio: Async runtime for process management
- Ratatui: Terminal UI framework
- Ropey: Efficient text rope for storing process output
- Crossterm: Cross-platform terminal manipulation

The architecture features:

- Async process spawning with stdout/stderr capture
- Event-driven UI updates via channels
- Efficient text buffer management with automatic line limits (5,000 lines per stream)
- Panel-based organization for multi-process views

## Testing

Rote includes comprehensive tests for process management and signal handling:

```bash
# Run all tests
cargo test

# Run process-specific tests
cargo test --test process_tests

# Run with output
cargo test -- --nocapture
```

Test coverage includes:

- Basic process spawning and output capture
- Multi-panel management
- Signal escalation (SIGINT → SIGTERM → SIGKILL)
- Process exit status handling
- Long-running processes

## Development

### Project Structure

```
rote/
├── src/
│   ├── app.rs       # Main TUI application loop
│   ├── config.rs    # YAML configuration parsing
│   ├── panel.rs     # Panel and output buffer management
│   ├── process.rs   # Process spawning and management
│   ├── signals.rs   # Signal handling and escalation
│   ├── ui.rs        # UI event definitions
│   ├── render.rs    # UI rendering
│   └── bin/
│       └── rote.rs  # CLI entry point
└── tests/
    ├── process_tests.rs  # Process management tests
    ├── integration_test.rs  # Integration tests
    └── data/             # Test scripts and fixtures
```

### Building

```bash
# Development build
cargo build

# Release build (optimized)
cargo build --release

# Run tests
cargo test

# Format code
cargo fmt

# Lint
cargo clippy
```

## Use Cases

- Development Environments: Manage multiple services during local development
- Service Orchestration: Manage microservices during development
- System Administration: Monitor multiple long-running services
- Testing: Run integration tests with multiple service dependencies
