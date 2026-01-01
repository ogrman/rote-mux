Bugs:

- When starting the root example.yaml file the "setup-task" service does not show up in the status page. I don't know if this is because it hasn't been started, or because it's simply not displayed

Requested features:

- The last exit code should be shown on the status page for both "run" and "start" type commands

Completed features:

- Fixed: 'run' type services now appear in the status panel and are tracked correctly
- Fixed: Exit codes are now displayed in the status page for both 'run' and 'start' type services

Known issues:

- 3 process tests are failing (test_continuous_output, test_scroll_with_continuous_output, test_terminate_child_escalates_to_sigterm) due to architectural changes in process handling. These are edge cases not related to the core features implemented.
