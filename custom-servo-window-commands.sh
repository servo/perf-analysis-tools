#!/usr/bin/env zsh
# Usage: custom-servo-window-commands.sh <pid>
set -euo pipefail

xdotool search --sync --onlyvisible --pid $1 --class servo windowmove $((2560+2)) $((0+28))
