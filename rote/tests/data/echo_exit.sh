#!/usr/bin/env bash
# Simple script that echoes to stdout and stderr, then exits
echo "stdout message"
echo "stderr message" >&2
exit 0
