#!/usr/bin/env bash
# Script that ignores SIGINT but respects SIGTERM
echo "started $$"

cleanup() {
    echo "received SIGTERM, exiting"
    exit 0
}

trap '' INT
trap cleanup TERM

# Use a loop with short sleeps so signals are handled promptly
for i in {1..100}; do
    sleep 0.1
done
echo "finished normally"
