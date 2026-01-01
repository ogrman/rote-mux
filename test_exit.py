#!/usr/bin/env python3
"""
Test that rote exits properly when 'q' is pressed.
Uses pseudo-terminal to simulate interactive terminal.
"""

import os
import pty
import subprocess
import sys
import time

# Spawn rote with a pseudo-terminal
config_path = "rote/tests/data/example.yaml"
master_fd, slave_fd = pty.openpty()

# Start rote process
proc = subprocess.Popen(
    ["cargo", "run", "--", "-c", config_path],
    stdin=slave_fd,
    stdout=slave_fd,
    stderr=slave_fd,
    close_fds=True,
    cwd="/home/lars/src/rote"
)

# Close slave end in parent
os.close(slave_fd)

# Wait a bit for the app to start
time.sleep(2)

# Send 'q' to quit
os.write(master_fd, b'q')
time.sleep(0.5)

# Check if process is still running
try:
    proc.wait(timeout=3)
    print("SUCCESS: App exited cleanly after pressing 'q'")
    sys.exit(0)
except subprocess.TimeoutExpired:
    print("FAILURE: App did not exit after pressing 'q'")
    proc.kill()
    proc.wait()
    sys.exit(1)
finally:
    os.close(master_fd)
