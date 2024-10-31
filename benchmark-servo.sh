#!/usr/bin/env zsh
# Usage: benchmark-servo.sh <path/to/servo> <url> <run count> [path/to/results]
set -euo pipefail
servo=$1; shift
url=$1; shift
run_count=$1; shift
script_dir=${0:a:h}

results=${1-$(mktemp -d)}
mkdir -p "$results"

for i in {01..$run_count}; do
    echo ">>> $i"

    "$servo" \
        --profiler-trace-path="$results/trace$i.html" --print-pwm \
        --ignore-certificate-errors \
        "$url" &
    pid=$!

    "$script_dir/custom-servo-window-commands.sh" $pid

    sleep 5
    printf 'Closing window'
    while kill -0 $pid 2> /dev/null; do
        kill $pid
        printf .
        sleep 1
    done
    echo
    echo
    echo
done

echo "Results: $results"
