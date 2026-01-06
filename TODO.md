# TODO

## 1. ~~Rename "service" to "task"~~ ✓

Update terminology throughout the codebase:
- [x] Update config format to use "tasks" instead of "services"
- [x] Update CLAUDE.md documentation
- [x] Update README.md documentation
- [x] Update example.yaml
- [x] Rename code types and variables (e.g., `ServiceInstance` → `TaskInstance`, `ServiceManager` → `TaskManager`)
- [x] Update error messages and UI text

## 2. ~~Change action type semantics~~ ✓

Rename action types in the config format:
- [x] Change "run" to "ensure" (one-time commands that block dependents until complete)
- [x] Change "start" to "run" (long-running processes)
- [x] Update config parsing in config.rs
- [x] Update documentation (CLAUDE.md, README.md, example.yaml)
- [x] Update any code references to these action types

## 3. Add new "start"/"stop" service type

Add a new task type for managed services:
- [ ] Add "start" field - command that starts a service
- [ ] Add optional "stop" field - command to stop the service on shutdown
- [ ] Implement stop command execution during shutdown sequence
- [ ] Update config parsing
- [ ] Update documentation
- [ ] Add tests for the new service type
