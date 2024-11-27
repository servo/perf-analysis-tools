#!/usr/bin/env zsh
# Usage: benchmark-servo.sh <path/to/servo> <url> <run count> [path/to/results] [extra servo arguments ...]
set -euo pipefail
script_dir=${0:a:h}
servo=$1; shift
url=$1; shift
run_count=$1; shift
results=${1-$(mktemp -d)}
if [ $# -gt 0 ]; then
    shift
fi
browser_open_time=${SERVO_PERF_BROWSER_OPEN_TIME-10}

mkdir -p "$results"
if [ -e "$results/done" ]; then
    echo ">>> $results is done; skipping"
    exit
fi
rm -f "$results/*"

export SERVO_TRACING='info'
for i in {01..$run_count}; do
    echo ">>> $i"

    # Write a manifest that pairs the HTML and Perfetto traces of each run,
    # both as paths relative to the directory containing the manifest file.
    html_trace=trace$i.html
    perfetto_trace=servo$i.pftrace
    jq -en \
        --arg html "$html_trace" \
        --arg perfetto "$perfetto_trace" \
        '{$html, $perfetto}' > "$results/manifest$i.json"

    "$servo" \
        --profiler-trace-path="$results/$html_trace" --print-pwm \
        --ignore-certificate-errors \
        "$@" \
        "$url" &
    pid=$!

    sleep "$browser_open_time"
    printf 'Closing window'
    while kill -0 $pid 2> /dev/null; do
        kill $pid
        printf .
        sleep 1
    done
    echo
    mv servo.pftrace "$results/$perfetto_trace"
    echo
    echo
done

touch "$results/done"
echo "Results: $results"
