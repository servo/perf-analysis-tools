#!/usr/bin/env zsh
# Usage: generate-report-tables.sh
set -euo pipefail -o bsdecho
script_dir=${0:a:h}
cd -- "$script_dir"

get_summary() {
    local cpu=$1
    local site=$2
    local engine=$3
    local metric=$4
    local metric_kind=${metric%:*}
    local metric_name=${metric#*:}
    case "$metric_kind" in
    (real) < "$cpu/$site.$engine/summary.txt" head -1 | jq -e --arg name "$metric_name" '.real_events[] | select(.name == $name)' ;;
    (synthetic) < "$cpu/$site.$engine/summary.txt" head -1 | jq -e --arg name "$metric_name" '.synthetic_and_interpreted_events[] | select(.name == $name)' ;;
    esac
}
get_summary_text() {
    get_summary "$@" | jq -er '.representative'
}
get_summary_tooltip() {
    get_summary "$@" | jq -er '.full'
}
for metric in \
    synthetic:FP synthetic:FCP \
    real:Compositing real:LayoutPerform real:ScriptEvaluate real:ScriptParseHTML \
    real:EvaluateScript real:FunctionCall real:Layerize real:Layout real:Paint real:ParseHTML real:PrePaint real:TimerFire real:UpdateLayoutTree \
    synthetic:Parse synthetic:Script synthetic:Layout synthetic:Rasterise \
    synthetic:Renderer \
; do
    local metric_kind=${metric%:*}
    local metric_name=${metric#*:}
    echo "### $metric_name ($metric_kind)"
    echo
    # CONFIG: list each site key, to match run-benchmarks-for-cpu.sh
    for site in servo.org $(: zh.wikipedia.org ); do
        echo "#### $site"
        echo
        echo "<table>"
        echo "<tr>"
        echo "<th>"
        # CONFIG: list each cpu results dir, to match run-all-benchmarks.sh
        for cpu in 2cpu; do
            echo "<th>$cpu"
        done
        # CONFIG: list each engine key, to match run-benchmarks-for-site.sh
        for engine in servo chromium; do
            # CONFIG: pick one of your cpu results dirs and name it here instead of “2cpu”
            if [ -n "$(get_summary_text 2cpu $site $engine "$metric")" ]; then
                echo "<tr>"
                echo "<th>$engine"
                # CONFIG: list each cpu results dir, to match run-all-benchmarks.sh
                for cpu in 2cpu; do
                    printf \%s "<td title='$(get_summary_tooltip $cpu $site $engine "$metric")'>"
                    printf \%s\\n "$(get_summary_text $cpu $site $engine "$metric")"
                done
            fi
        done
        echo "</table>"
        echo
    done
done
