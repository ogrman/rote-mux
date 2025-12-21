#!/usr/bin/env bash
echo "started $$"
trap 'echo "got INT"' INT
trap 'echo "got TERM"' TERM
trap '' INT TERM
sleep 2.1
