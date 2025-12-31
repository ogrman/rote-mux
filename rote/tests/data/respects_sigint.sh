#!/usr/bin/env bash
# Script that respects SIGINT and exits gracefully
echo "started $$"

cleanup() {
    echo "received SIGINT, exiting"
    exit 0
}

trap cleanup INT

# Use a loop with short sleeps so signals are handled promptly
for i in {1..100}; do
    sleep 0.1
done
echo "finished normally"
