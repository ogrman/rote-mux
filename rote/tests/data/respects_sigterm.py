#!/usr/bin/env python3
import signal
import sys
import time
import os

def sigterm_handler(signum, frame):
    print("received SIGTERM, exiting", flush=True)
    sys.exit(0)

# Ignore SIGINT
signal.signal(signal.SIGINT, signal.SIG_IGN)
# Handle SIGTERM
signal.signal(signal.SIGTERM, sigterm_handler)

print(f"started {os.getpid()}", flush=True)

# Sleep in small increments to handle signals promptly
for _ in range(100):
    time.sleep(0.1)

print("finished normally", flush=True)
