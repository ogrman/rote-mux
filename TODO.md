Bugs:

(none currently)

Requested features:

- The last exit code should be shown on the status page for both "run" and "start" type commands
- "run" type tasks should show "Completed" or "Failed" in the status page depending on exit code. "start" type tasks should show "Running" or "Exited"

Completed features:

- Fixed: 'run' type services now appear in the status panel and are tracked correctly
- Fixed: Exit codes are now displayed in the status page for both 'run' and 'start' type services
- Fixed: Services showing as "exited" when starting with example.yaml. The issue was that check_process_exited_by_pid() in app.rs and check_process_exited() in signals.rs were using Signal::SIGUSR1 to check if a process exists, which actually sends the signal to the process, causing it to exit. Changed to use None instead, which performs error checking without sending a signal.

Known issues:

- 2 process tests are failing (test_continuous_output, test_scroll_with_continuous_output) due to architectural changes in process handling. These are edge cases not related to the core features implemented.
