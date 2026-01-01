#!/bin/bash
# Test script to verify the application exits properly

set -x

cd rote

# Build first
cargo build --release

# Start the app in the background
timeout 5 cargo run --release -- -c tests/data/example.yaml < /dev/null &
APP_PID=$!

# Wait a bit for it to start
sleep 1

# Try to send a SIGTERM to trigger shutdown
kill -TERM $APP_PID 2>/dev/null

# Wait for process to exit
TIMEOUT=5
for i in $(seq 1 $TIMEOUT); do
    if ! kill -0 $APP_PID 2>/dev/null; then
        echo "App exited cleanly after SIGTERM"
        wait $APP_PID
        EXIT_CODE=$?
        echo "Exit code: $EXIT_CODE"
        exit $EXIT_CODE
    fi
    sleep 0.5
done

# If we get here, the app is still running
echo "ERROR: App did not exit after $TIMEOUT seconds"
kill -9 $APP_PID 2>/dev/null
exit 1
