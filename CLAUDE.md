# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Test Commands

```bash
cargo build                    # Development build
cargo build --release          # Release build
cargo test                     # Run all tests
cargo test --lib               # Run only unit tests
cargo test test_name           # Run specific test
cargo test -- --nocapture      # Run with output visible
cargo fmt                      # Format code
cargo clippy                   # Lint
```

## Development Workflow

Always run `cargo test` after making changes. Fix any failing tests.
Always run `cargo fmt` after all tests pass.
Whenever the config format changes, update `example.yaml` and `README.md`.
Integration test scripts should have names starting with the test name.

## Project Overview

Rote is a terminal multiplexer for monitoring and managing multiple processes. Users define services in YAML with dependencies, and Rote starts them in topological order with a TUI for monitoring output.

## Architecture

### Core Modules (in `rote/src/`)

- **app.rs**: Main event loop, dependency resolution (topological sort with cycle detection), service lifecycle management. Entry point is `run_with_input()`.
- **config.rs**: YAML parsing. `Config` struct defines the schema. Two action types: `start` (long-running) and `run` (one-time, blocks dependents).
- **process.rs**: `ServiceInstance` wraps spawned processes. Signal escalation: SIGINT→SIGTERM→SIGKILL with 300ms between each.
- **panel.rs**: `Panel` holds output buffer per service using Ropey rope. `StatusPanel` tracks all services. MAX_LINES=5000 per stream.
- **render.rs**: Ratatui rendering for panels and status view.
- **ui.rs**: `UiEvent` enum for keyboard, process, and UI events.
- **signals.rs**: Process existence checking and signal utilities.

### Event Flow

1. `run_with_input()` spawns keyboard task and status check task (250ms interval)
2. Events flow through mpsc channel: keyboard input, process output, exit notifications
3. Services start in dependency order; `run` services block dependents until complete
4. Output captured via tokio tasks piping stdout/stderr to panels

### Key Types

- `ServiceInstance`: Spawned process with PID, stdout/stderr tasks, exit status
- `Panel`: Service output buffer with filtering (stdout/stderr/status visibility)
- `StatusPanel`: Overview of all services with health status and exit codes
- `MessageBuf`: Uses Ropey for efficient text storage with auto-truncation

## Test Structure

- Unit tests embedded in source files via `#[cfg(test)]` modules
- Integration tests in `tests/`: `integration_test.rs`, `process_tests.rs`
- Test fixtures in `tests/data/`: YAML configs and shell scripts for signal handling tests

## Outstanding Work

See `TODO.md` for current tasks. Key incomplete items:
- Timestamp support for log messages
- Panels for services that haven't started yet
- Async service startup with immediate status screen
