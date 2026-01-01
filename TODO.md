TODO Format:
- Items should use [   ] for unsolved tasks and [ X ] for completed tasks
- Unsolved items should be at the top of the file
- Additional explanation can be added on subsequent lines with 6-space indentation

[   ] 2 process tests are failing (test_continuous_output,
      test_scroll_with_continuous_output) due to architectural changes in
      process handling. These are edge cases not related to the core features
      implemented.

[   ] Write tests for mixed stdout/stderr output, including testing that
      message order is preserved when stdout/stderr is toggled

[   ] Analyze test coverage for the entire app and add TODOs for places where
      tests are missing.

[   ] Test colored output from the child processes

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
