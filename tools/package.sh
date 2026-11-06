#!/bin/sh
# Compatibility entry point for macOS, Linux, and Git Bash.
# Windows PowerShell: py -3 tools/package.py
set -eu
exec python3 "$(dirname "$0")/package.py" "$@"
