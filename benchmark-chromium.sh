#!/usr/bin/env zsh
# Usage: benchmark-chromium.sh <path/to/chrome> <url> <run count> [path/to/results]
set -euo pipefail

chromium=$1; shift
url=$1; shift
run_count=$1; shift

results=${1-$(mktemp -d)}
mkdir -p "$results"

for i in {01..$run_count}; do
    echo ">>> $i"

    # Fresh --user-data-dir to avoid interfering with user’s default profile
    # and avoid disk caching. Disk caching helps control for network performance
    # but it may unfairly punish Servo.
    # <https://peter.sh/experiments/chromium-command-line-switches/#user-data-dir>
    # <https://peter.sh/experiments/chromium-command-line-switches/#no-first-run>
    profile=$(mktemp -d)
    "$chromium" \
        --user-data-dir="$profile" --no-first-run \
        --trace-startup --trace-startup-file="$results/chrome$i.pftrace" \
        --ignore-certificate-errors \
        "$url" &
    pid=$!

    # Resize the visible Chromium window with our pid to the same size as default servoshell.
    # TODO: can we have both Servo and Chromium windows at the same size before loading a page?
    xdotool search --sync --onlyvisible --pid $pid --class google-chrome windowsize 1024 740

    sleep 5
    # Close that window gracefully. Chromium does not write a trace file if sent a SIGTERM.
    printf 'Closing window'
    while kill -0 $pid 2> /dev/null; do
        # No --sync here, because the window may be gone by now.
        xdotool search --onlyvisible --pid $pid --class google-chrome windowquit || :
        printf .
        sleep 1
    done
    echo
    while ! rm -R "$profile"; do
        >&2 echo 'Failed to delete Chromium profile; will retry'
        sleep 1
    done
    echo
    echo
done

echo "Results: $results"
