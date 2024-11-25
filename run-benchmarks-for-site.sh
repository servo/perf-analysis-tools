#!/usr/bin/env zsh
set -euo pipefail -o bsdecho
key=$1
url=$2
sample_dir=$3

# CONFIG: list each engine you want to run benchmarks for here
./benchmark-servo.sh ~/code/servo/target/production-stripped/servo "$url" 30 "$sample_dir/$key.servo"
# ./benchmark-servo.sh ~/code/servo/servo.1/servo "$url" 30 "$sample_dir/$key.servo"
# ./benchmark-servo.sh ~/code/servo/servo.2/servo "$url" 30 "$sample_dir/$key.servo"
# ./benchmark-servo.sh ~/code/servo/servo.3/servo "$url" 30 "$sample_dir/$key.servo"
# ./benchmark-chromium.sh google-chrome-stable "$url" 30 "$sample_dir/$key.chromium"
