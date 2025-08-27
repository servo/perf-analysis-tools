#!/usr/bin/env zsh
# Based on: <https://testbit.eu/2023/cgroup-cpuset>
# To enable: isolate-cpu-for-shell.sh <pid> <cpu ids ...>
#       e.g. isolate-cpu-for-shell.sh $$ 10 {13..15}
# To disable: undo-cpu-isolation.sh
set -euo pipefail

all_cpu_ids() {
    lscpu --parse=cpu | rg -v '^#'
}

reset_cpu_boost() {
    # For the AMD 7950X, this requires Linux 6.11.
    if [ -f /sys/devices/system/cpu/cpufreq/boost ]; then
        echo 1 > /sys/devices/system/cpu/cpufreq/boost
    elif [ -f /sys/devices/system/cpu/intel_pstate/no_turbo ]; then
        echo 0 > /sys/devices/system/cpu/intel_pstate/no_turbo
    fi
}

echo 2 > /proc/sys/kernel/randomize_va_space
echo 4 > /proc/sys/kernel/perf_event_paranoid
reset_cpu_boost
# for scg in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor ; do
#     echo schedutil > $scg || :
# done

# Online all CPUs.
for cpon in /sys/devices/system/cpu/cpu*/online ; do
    echo 1 > $cpon
done

# Make the shield cgroup no longer a partition root.
echo member > /sys/fs/cgroup/shield/cpuset.cpus.partition || :

# Move all cgroups to all CPUs.
all_cpus=$(all_cpu_ids | tr \\n ,)
all_cpus=${all_cpus%,}
for cpscpus in /sys/fs/cgroup/**/cpuset.cpus ; do
    echo "$all_cpus" > "$cpscpus"
done
