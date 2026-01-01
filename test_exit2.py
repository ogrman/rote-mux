#!/usr/bin/env python3
"""
Test that rote runs and try different input methods.
"""

import os
import pty
import subprocess
import sys
import time
import select

# Spawn rote with a pseudo-terminal
config_path = "rote/tests/data/example.yaml"
master_fd, slave_fd = pty.openpty()

print(f"Master fd: {master_fd}, Slave fd: {slave_fd}")

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

# Wait a bit for app to start
print("Waiting for app to start...")
time.sleep(2)

# Try to read from master to see what's happening
if select.select([master_fd], [], [], 0.1)[0]:
    output = os.read(master_fd, 1024)
    print(f"Got output: {output[:100]}")

# Send 'q' to quit
print("Sending 'q'")
os.write(master_fd, b'q')
time.sleep(1)

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
