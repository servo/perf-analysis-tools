#!/usr/bin/env zsh
# Usage: analyse-cpu-results.sh <cpu config dir>
set -euo pipefail -o bsdecho
script_dir=${0:a:h}
cpu_config_dir=$1
analyse=$script_dir/target/release/analyse

cargo build -r
cd -- "$cpu_config_dir"

# CONFIG: list each site as <key> <url> <key> <url> ..., to match run-benchmarks-for-cpu.sh
printf $'%s\t%s\n' \
    servo.org https://servo.org/ \
    $(: zh.wikipedia.org https://zh.wikipedia.org/wiki/Servo) \
| while read -r key url; do
    echo ">>> Analysing: $key"
    (
        set -x
        # CONFIG: one for each engine, to match run-benchmarks-for-site.sh
        "$analyse" servo "$url" $key.servo/manifest*.json > $key.servo/summary.txt
        # "$analyse" chromium "$url" $key.chromium/*.json > $key.chromium/summary.txt
    )
done
