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

[ X ] Add option to show timestamps for messages in config format. When it
      is enabled all logs should have the current time prepended to the line.

      Implementation complete. The `timestamps: true` config option enables
      HH:MM:SS timestamps prepended to all log messages. format_timestamp()
      in app.rs generates timestamps, and all message push callsites pass
      timestamps to MessageBuf::push().

[ X ] Even services that have not been started should have a panel. Change the
      semantics of the restart command so that it can be used to start a
      service that was never started. There should be a 1:1 mapping for service
      to panel.

      Panels are now created for all services with actions in config.services,
      not just those in the resolved dependency list. The restart command can
      start services that weren't initially started. Status panel shows all
      services with their initial status based on whether they're being started.

[ X ] Services should be started asynchronously -- calculate which ones should
      be started automatically when the application starts and then go straight
      to the status screen.

      Replaced blocking start_services() with event-driven startup. Services
      start via StartNextService events in the main event loop. Run services
      track completion in completed_run_services set, and dependent services
      start once their Run dependencies complete. Status screen shown immediately.

[ X ] Wait for the process to exit before starting it again when restarting

      Now waits for wait_task, stdout_task, and stderr_task to complete before
      spawning the new process. This ensures all I/O is drained and the process
      is fully terminated.

[ X ] The program does not exit correctly when pressing 'q'. The program exits
      alternative screen, but does not actually quit. This is true, at least,
      when running through `cargo run -- -c example.yaml`. This needs to be
      fixed. I would prefer if we, when the user presses 'q', we exited the
      alternative screen immediately and showed shutdown progress instead of
      having the program freeze. None of the keybindings should be active
      during the shutdown process.

      Fixed: Now exits alternate screen immediately on 'q', shows shutdown
      progress in normal terminal, properly signals keyboard task to stop,
      and waits for all processes and their I/O tasks to complete before
      exiting.

[ X ] Make the left and right arrows scroll through the panels (with status
      being before the first process panel)

      Added Left/Right arrow key navigation. Left goes to previous panel (or
      status if at first panel), Right goes to next panel (or status if at
      last panel). Updated help text to show the new keybindings.

[ X ] Sort the process list and panels by the service name so that ordering
      is always consistent.

      Panels and status panel entries are now sorted alphabetically by service
      name. This ensures consistent ordering regardless of HashMap iteration
      order.

[ X ] Fix scrolling. It's now possible to scroll up beyond the start of the
      output buffer, leaving parts of the screen empty. Limit the amount of
      scrolling so that we can't scroll beyond the start of the buffer.

      Fixed by clamping scroll position based on viewport height in both the
      scroll event handler (app.rs) and the render function (render.rs).

[ X ] Add scrollbars to the panels. Use line count, ignore lines that are
      longer than the output buffer for the calculation.

      Added vertical scrollbar using ratatui's Scrollbar widget. The scrollbar
      appears when content exceeds the viewport height and accurately reflects
      the scroll position.

[ X ] Add the keybindings for scrolling to the key bindings panel.

      Added ↑/↓ (scroll) and PgUp/PgDn (scroll fast) to the help panel.
