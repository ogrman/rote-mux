#!/usr/bin/env python3
import signal
import sys
import time
import os

def sigint_handler(signum, frame):
    print("received SIGINT, exiting", flush=True)
    sys.exit(0)

signal.signal(signal.SIGINT, sigint_handler)
print(f"started {os.getpid()}", flush=True)

# Sleep in small increments to handle signals promptly
for _ in range(100):
    time.sleep(0.1)

print("finished normally", flush=True)
