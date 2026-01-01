TODO Format:
- Items should use [   ] for unsolved tasks and [ X ] for completed tasks
- Unsolved items should be at the top of the file
- Additional explanation can be added on subsequent lines with 6-space indentation

 [ X ] Replace all python code with bash scripts if they are still needed. If
      they are not needed, remove them. If the test_* scripts are still needed
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
