#!/usr/bin/env zsh
# Usage: benchmark-servo.sh <path/to/servo> <url> <run count>
set -euo pipefail

servo=$1; shift
url=$1; shift
run_count=$1; shift

results=$(mktemp -d)

for i in {01..$run_count}; do
    echo ">>> $i"

    "$servo" --profiler-trace-path="$results/trace$i.html" --print-pwm "$url" &
    pid=$!

    sleep 5
    kill $pid
    echo
    echo
done

echo "Results: $results"
