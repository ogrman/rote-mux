TODO Format:
- Items should use [   ] for unsolved tasks and [ X ] for completed tasks
- Unsolved items should be at the top of the file
- Additional explanation can be added on subsequent lines with 6-space indentation

[ X ] Replace all python code with bash scripts if they are still needed. If
      they are not needed, remove them. If test_* scripts are still needed
      they should reside in "tests/". Make sure all script names match the test
      that they are used in. Removed test_exit.py, test_exit2.py, test_exit3.py,
      test_exit.sh, and test_yaml_parse.rs as they were not used in automated
      tests.

[ X ] 2 process tests are failing (test_continuous_output,
      test_scroll_with_continuous_output) due to architectural changes in
      process handling. Fixed by replacing oneshot channel with Arc<Mutex<Option>>>
      to handle futures being dropped in tokio::select! loops.

[ X ] Write tests for mixed stdout/stderr output, including testing that
      message order is preserved when stdout/stderr is toggled

[ X ] Analyze test coverage for the entire app and add TODOs for places where
      tests are missing.

[ X ] Test colored output from the child processes

[ X ] Add unit tests for ui.rs:
      - ProcessStatus enum variants and behavior
      - UiEvent enum variants

[ X ] Add unit tests for panel.rs:
      - StreamBuf::push method and line truncation at MAX_LINES
      - Panel::new constructor
      - StatusPanel::new constructor
      - StatusPanel::update_entry method
      - StatusPanel::update_exit_code method
      - StatusPanel::update_entry_with_action method

[ X ] Add unit tests for signals.rs:
      - terminate_child function with various signal scenarios
      - wait_for_child_exit function

[ X ] Add unit tests for app.rs:
      - visible_len function for panel display calculation
      - resolve_dependencies function for dependency resolution

[ X ] 'run' type services now appear in the status panel and are tracked
      correctly

[ X ] Exit codes are now displayed in the status page for both 'run' and
      'start' type services

[ X ] Services showing as "exited" when starting with example.yaml

      The issue was that check_process_exited_by_pid() in app.rs and
      check_process_exited() in signals.rs were using Signal::SIGUSR1 to check
      if a process exists, which actually sends the signal to the process,
      causing it to exit. Changed to use None instead, which performs error
      checking without sending a signal.

[ X ] The last exit code should be shown on the status page for both "run" and
      "start" type commands

[ X ] "run" type tasks should show "Completed" or "Failed" in the status page
      depending on exit code. "start" type tasks should show "Running" or
      "Exited"

[ X ] Add message showing why we switch to status panel when it automatically
      switches (e.g., when a process exits)

[ X ] Keep panels for exited "run" type tasks around so their output can be
      viewed

      Panels are now created for both 'start' and 'run' type services. Run type
      services spawn processes and wait for completion, capturing output to the
      panel. The output remains viewable even after the task completes.

[   ] Add option to show timestamps for messages in config format. When it
      is enabled all logs should have the current time prepended to the line.

      Config option added and MessageBuf::push signature updated to accept
      optional timestamp, but full implementation requires updating all message
      push callsites throughout codebase.

[   ] ServiceInstance should not know its panel location. Refactor that away,
      and do the mapping in a better way if possible. Skip doing this work and
      update this TODO entry if this would lead to things being a lot worse.

[   ] Even services that have not been started should have a panel. Change the
      semantics of the restart command so that it can be used to start a
      service that was never started. There should be a 1:1 mapping for service
      to panel.

[   ] Services should be started asynchronously -- calculate which ones should
      be started automatically when the application starts and then go straight
      to the status screen.
