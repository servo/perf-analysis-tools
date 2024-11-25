#!/usr/bin/env zsh
# Usage: convert-pftrace-to-json.sh <path/to/traceconv> <path/to/chrome01.pftrace [path/to/chrome02.pftrace]>
set -euo pipefail -o bsdecho
script_dir=${0:a:h}
traceconv_path=$1

for i; do
    shift
    set -- "$@" "$i" "${i%.pftrace}.json"
done
printf \%s\\n "$@" | tr \\n \\0 | xargs -0n2 -P16 steam-run "$traceconv_path" json
