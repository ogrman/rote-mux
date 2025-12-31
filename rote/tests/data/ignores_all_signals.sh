#!/usr/bin/env bash
# Script that ignores both SIGINT and SIGTERM (requires SIGKILL)
echo "started $$"

trap '' INT TERM

# Use a loop with short sleeps so the script keeps running
for i in {1..100}; do
    sleep 0.1
done
echo "finished normally"
