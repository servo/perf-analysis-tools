#!/usr/bin/env zsh
set -euo pipefail -o bsdecho
sample_dir=$1

# CONFIG: list each site you want to run benchmarks for here
./run-benchmarks-for-site.sh servo.org https://servo.org/ "$sample_dir"
# ./run-benchmarks-for-site.sh zh.wikipedia.org https://zh.wikipedia.org/wiki/Servo "$sample_dir"
