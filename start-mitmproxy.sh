#!/usr/bin/env zsh
# Usage: start-mitmproxy.sh <record|replay> <path/to/dump>
set -euo pipefail
mode=$1; shift
dump_path=$1; shift
proxied_group=mitmproxy
proxied_gid=$(< /etc/group tr : \\t | cut -f 1,3 | rg $'^mitmproxy\t' | cut -f 2)

echo Configuring sysctl and iptables
# <https://docs.mitmproxy.org/stable/howto-transparent/#1-enable-ip-forwarding>
sysctl -w net.ipv4.ip_forward=1
sysctl -w net.ipv6.conf.all.forwarding=1

# <https://docs.mitmproxy.org/stable/howto-transparent/#2-disable-icmp-redirects>
sysctl -w net.ipv4.conf.all.send_redirects=1

iptables_check_or_add_nat_rule() {
    local iptables=$1; shift
    "$iptables" -t nat -C "$@" || "$iptables" -t nat -A "$@"
}

# Redirect HTTP/HTTPS traffic from a specific group to mitmproxy.
# Based on the “Work-around to redirect traffic originating from the machine itself”,
# but redirects traffic from a specific primary gid (excluding supplementary groups),
# instead of traffic not from a specific uid.
# <https://docs.mitmproxy.org/stable/howto-transparent/#3-create-an-iptables-ruleset-that-redirects-the-desired-traffic-to-mitmproxy>
for iptables in iptables ip6tables; do
    iptables_check_or_add_nat_rule $iptables OUTPUT -p tcp -m owner --gid-owner mitmproxy --dport 80 -j REDIRECT --to-port 8080
    iptables_check_or_add_nat_rule $iptables OUTPUT -p tcp -m owner --gid-owner mitmproxy --dport 443 -j REDIRECT --to-port 8080
    $iptables -vnt nat -L -Z | rg "owner GID match $proxied_gid "
done
echo

if [ "$mode" = record ]; then
    echo 'First we need to record the requests with `mitmproxy --save-stream-file`:'
    echo '1. When you finish reading, press <Enter>'
    echo '2. Open the original URL in a browser running in a `newgrp mitmproxy` shell, e.g.'
    echo '   $ newgrp mitmproxy'
    echo '   $ google-chrome-stable --ignore-certificate-errors --user-data-dir=$(mktemp -d) --no-first-run https://servo.org'
    echo '3. When you are done, press <q> then <y>'
    printf 'Ready? '
    read -r _
    # `block_global=false` avoids “Warn: [17:51:27.613] Client connection from <IPv6 address> killed by block_global option.”
    # <https://stackoverflow.com/a/52281899>
    mitmproxy --set block_global=false --save-stream-file "$dump_path"
elif [ "$mode" = replay ]; then
    echo 'Now we can replay the requests with `mitmproxy --server-replay`:'
    echo '1. When you finish reading, press <Enter>'
    echo '2. Open the original URL in a browser running in a `newgrp mitmproxy` shell, e.g.'
    echo '   $ newgrp mitmproxy'
    echo '   $ google-chrome-stable --ignore-certificate-errors --user-data-dir=$(mktemp -d) --no-first-run https://servo.org'
    echo '3. When you are done, press <q> then <y>'
    printf 'Ready? '
    read -r _
    # `block_global=false` avoids “Warn: [17:51:27.613] Client connection from <IPv6 address> killed by block_global option.”
    # <https://stackoverflow.com/a/52281899>
    # `--server-replay-extra kill --server-replay-reuse` forces all requests to be served from our replay file only.
    # (<https://github.com/mitmproxy/mitmproxy/discussions/5940> suggests `--set connection_strategy=lazy`, but that doesn’t actually work)
    mitmproxy --set block_global=false --server-replay-extra kill --server-replay-reuse --mode transparent --server-replay "$dump_path"
fi
