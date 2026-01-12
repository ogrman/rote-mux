# Rote

A terminal multiplexer for monitoring and managing multiple processes together.

## Features

- Process Management: Start and monitor multiple processes simultaneously
- TUI Interface: Clean terminal UI with separate panels for each process
- Real-time Output: View stdout and stderr from all processes in real-time
- Smart Signal Handling: Graceful shutdown with signal escalation (SIGINT → SIGTERM → SIGKILL)
- YAML Configuration: Define tasks and dependencies in a simple config file
- Process Restart: Restart individual processes on the fly
- Scrollable Output: Navigate through process output with keyboard controls
- Stream Filtering: Toggle stdout/stderr visibility per panel
- Status Panel: View the status of all tasks at a glance
- Task Dependencies: Tasks can require other tasks to start first
- Automatic Line Limits: Maximum 5,000 lines per stream to prevent memory issues

## Installation

### From crates.io

```bash
cargo install rote-mux
```

### From GitHub releases

Download a pre-built binary from the [releases page](https://github.com/ogrman/rote-mux/releases).

### From source

```bash
git clone https://github.com/ogrman/rote-mux.git
cd rote-mux
cargo build --release
```

The binary will be at `target/release/rote`.

## Quick Start

Generate an example configuration file and run it:

```bash
rote --generate-example > rote.yaml
rote
```

Or create your own `rote.yaml` file:

```yaml
default: ping-demo
tasks:
  google-ping:
    run: ping google.com
  cloudflare-ping:
    run: ping 1.1.1.1
  ping-demo:
    require: [google-ping, cloudflare-ping]
```

Then run:

```bash
rote
```

## Configuration

### Top-Level Fields

- `default` (optional): The default task to run when none is specified
- `tasks`: A mapping of task names to their configurations

### Task Definition

Each task can have the following properties:

- `run`: Command to start a long-running task
- `ensure`: Command to run to completion (blocks dependent tasks until complete)
- `cwd` (optional): Working directory for the command (relative to the config file)
- `display` (optional): List of streams to display (["stdout"], ["stderr"], or both by default)
- `require` (optional): List of tasks that must be started before this one
- `autorestart` (optional): If true, automatically restart the task when it exits (default: false)
- `timestamps` (optional): If true, show timestamps for log messages (default: false)
- `healthcheck` (optional): Healthcheck configuration for the task (see below)

### Actions: `run` vs `ensure`

- `run`: For long-running processes (servers, daemons). These are spawned in the background and their output is displayed in a panel.
- `ensure`: For one-time setup tasks (migrations, installations). These run to completion before dependent tasks start. They do not create a panel.

These are mutually exclusive - a task can only have one or the other.

### Healthchecks

Tasks with a `run` action can optionally specify a healthcheck. When a healthcheck is configured, dependent tasks will wait for the healthcheck to pass before starting (similar to how `ensure` tasks block dependents until complete).

```yaml
tasks:
  postgres:
    run: docker run --rm -p 5432:5432 postgres
    healthcheck:
      tool: is-port-open 5432
      interval: 1

  api:
    run: ./server
    require: [postgres]  # Won't start until postgres healthcheck passes
```

Healthcheck fields:
- `cmd`: A shell command to run. Healthcheck passes when it exits with code 0.
- `tool`: A built-in tool to run directly (without spawning a process). See below for available tools.
- `interval`: How often to run the healthcheck, in seconds (supports decimals like `0.5`).

You must specify either `cmd` or `tool`, but not both.

#### Built-in Healthcheck Tools

- `is-port-open <port>`: Check if a TCP port is open on localhost.
- `http-get <port or URL>`: Perform an HTTP GET request. If given a port number, hits `http://127.0.0.1:{port}/`. If given a full URL (starting with `http://` or `https://`), uses that URL directly. Passes if the server responds (any status code).
- `http-get-ok <port or URL>`: Same as `http-get`, but only passes if the server returns a 2xx status code.

Using `tool` is equivalent to `cmd: "rote tool ..."` but more efficient since it doesn't spawn a new process for each healthcheck.

### Example: Full-Stack Application

```yaml
default: dev
tasks:
  # One-time setup
  init-config:
    cwd: backend
    ensure: bash -c '[ -f .env ] || cp env_template .env'

  # Install dependencies
  frontend-install:
    cwd: frontend
    ensure: npm install

  # Database with healthcheck - migrations wait until postgres is accepting connections
  postgres:
    run: docker run --rm -p 5432:5432 -e POSTGRES_PASSWORD=dev postgres
    display: [stderr]
    healthcheck:
      tool: is-port-open 5432
      interval: 0.5

  # Run migrations after DB is ready (healthcheck must pass first)
  migrate:
    ensure: ./run-migrations.sh
    require: [postgres]

  # Backend server with healthcheck - frontend waits until API is responding
  api:
    cwd: backend
    run: cargo run --bin api
    require: [migrate, init-config]
    healthcheck:
      tool: http-get 8080
      interval: 1

  # Frontend dev server - starts after API is healthy
  web:
    cwd: frontend
    run: npm run dev
    require: [frontend-install, api]

  # Development target
  dev:
    require: [web]
```

### Display Streams

The `display` field controls which streams are shown for a task:

- Omit or `null`: Show both stdout and stderr (default)
- `["stdout"]`: Show only stdout
- `["stderr"]`: Show only stderr
- `[]`: Hide all output
- `["stdout", "stderr"]`: Show both streams (same as default)

### Dependency Resolution

Tasks are started in topological order based on their dependencies. Circular dependencies are detected and will cause an error. Tasks with an `ensure` action must complete successfully before dependent tasks start.

## Key Bindings

When running, the following keyboard shortcuts are available:

- `q`: Quit and terminate all processes
- `r`: Restart the currently active process
- `o`: Toggle stdout visibility for the active panel
- `e`: Toggle stderr visibility for the active panel
- `s`: Switch to status panel showing all tasks
- `1-9`: Switch to panel 1-9
- `←/→`: Navigate to previous/next panel
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
.
├── Cargo.toml           # Workspace manifest
├── example.yaml         # Example configuration
├── scripts/             # CI/build scripts
├── rote/
│   ├── Cargo.toml       # Package manifest
│   ├── src/
│   │   ├── lib.rs           # Library root
│   │   ├── app.rs           # Main TUI application loop
│   │   ├── config.rs        # YAML configuration parsing
│   │   ├── error.rs         # Error types
│   │   ├── panel.rs         # Panel and output buffer management
│   │   ├── process.rs       # Process spawning and management
│   │   ├── render.rs        # UI rendering
│   │   ├── signals.rs       # Signal handling utilities
│   │   ├── task_manager.rs  # Task lifecycle and dependency resolution
│   │   ├── ui.rs            # UI event definitions
│   │   └── bin/
│   │       └── rote.rs      # CLI entry point
│   └── tests/
│       ├── integration_test.rs  # Integration tests
│       ├── process_tests.rs     # Process management tests
│       └── data/                # Test configs and scripts
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
