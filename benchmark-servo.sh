#!/usr/bin/env zsh
# Usage: benchmark-servo.sh <path/to/servo> <url> <run count> [path/to/results]
set -euo pipefail

servo=$1; shift
url=$1; shift
run_count=$1; shift

results=${1-$(mktemp -d)}
mkdir -p "$results"

for i in {01..$run_count}; do
    echo ">>> $i"

    "$servo" \
        --profiler-trace-path="$results/trace$i.html" --print-pwm \
        --ignore-certificate-errors \
        "$url" &
    pid=$!

    sleep 5
    kill $pid
    echo
    echo
done

echo "Results: $results"
