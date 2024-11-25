#!/usr/bin/env zsh
set -euo pipefail -o bsdecho

# CONFIG: list each cpu config you want to run benchmarks for here
sudo ./isolate-cpu-for-shell.sh $$ 14 15
./run-benchmarks-for-cpu.sh ./2cpu  # results dir for cpu config

# sudo ./isolate-cpu-for-shell.sh $$ {12..15}
# ./run-benchmarks-for-cpu.sh ./4cpu  # results dir for cpu config

# sudo ./isolate-cpu-for-shell.sh $$ {10..15}
# ./run-benchmarks-for-cpu.sh ./6cpu  # results dir for cpu config

# sudo ./isolate-cpu-for-shell.sh $$ {8..15}
# ./run-benchmarks-for-cpu.sh ./8cpu  # results dir for cpu config
