#!/usr/bin/env zsh
# Usage: custom-chromium-window-commands.sh <pid>
set -euo pipefail

xdotool search --sync --onlyvisible --pid $1 --class google-chrome windowmove $((2560+2)) $((0+28))
i3-msg 'workspace back_and_forth'
