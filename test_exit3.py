#!/usr/bin/env python3
"""
Test that rote exits - only redirect stdin, not stdout
"""

import os
import pty
import subprocess
import sys
import time

# Spawn rote with a pseudo-terminal
config_path = "rote/tests/data/example.yaml"
master_fd, slave_fd = pty.openpty()

# Start rote process - ONLY redirect stdin to PTY, keep stdout/stderr as is
proc = subprocess.Popen(
    ["cargo", "run", "--", "-c", config_path],
    stdin=slave_fd,
    close_fds=True,
    cwd="/home/lars/src/rote",
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE
)

# Close slave end in parent
os.close(slave_fd)

# Wait a bit for app to start
print("Waiting for app to start...")
time.sleep(2)

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
    stdout, stderr = proc.communicate()
    print(f"STDOUT: {stdout[:500] if stdout else 'None'}")
    print(f"STDERR: {stderr[:500] if stderr else 'None'}")
    sys.exit(1)
finally:
    os.close(master_fd)
