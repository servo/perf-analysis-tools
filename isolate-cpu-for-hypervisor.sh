#!/usr/bin/env zsh
# Isolate the CPU for benchmarking work, assuming process affinity will be isolated externally with `isolcpus`.
# Based on: <https://testbit.eu/2023/cgroup-cpuset>
# To enable: isolate-cpu-for-hypervisor.sh <cpu ids ...>
#       e.g. isolate-cpu-for-hypervisor.sh 10 {13..15}
# To disable: undo-cpu-isolation.sh
set -euo pipefail
script_dir=${0:a:h}
. "$script_dir/isolate-cpu-common.inc"

disable_cpu_boost() {
    # For the AMD 7950X, this requires Linux 6.11.
    if [ -f /sys/devices/system/cpu/cpufreq/boost ]; then
        echo 0 > /sys/devices/system/cpu/cpufreq/boost
    elif [ -f /sys/devices/system/cpu/intel_pstate/no_turbo ]; then
        echo 1 > /sys/devices/system/cpu/intel_pstate/no_turbo
    else
        >&2 echo 'Warning: don’t know how to disable CPU boost for this CPU!'
    fi
}

# Disable process ASLR.
echo 0 > /proc/sys/kernel/randomize_va_space

# Allow any user to run perf.
echo 0 > /proc/sys/kernel/perf_event_paranoid

# Disable CPU frequency boost.
>&2 printf 'Disabling CPU frequency boost...\n'
disable_cpu_boost

# Online all CPUs that can be offlined, in case we are rerunning this script with new CPU IDs.
# This is also necessary to avoid write errors with EBUSY in the scaling_governor step.
>&2 echo 'Onlining all cpus'
for online in /sys/devices/system/cpu/cpu*/online; do
    echo 1 > "$online"
done

# Set all CPUs to run at maximum frequency.
for scg in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor ; do
    echo performance > $scg || :
done

# Offline the SMT siblings of the dedicated CPUs.
for cpu; do
    for cpu in $(cpu_ids_for_core_id $(core_id_for_cpu_id "$cpu") | sed 1d); do
        >&2 echo "Offlining cpu $cpu"
        echo 0 > "/sys/devices/system/cpu/cpu$cpu/online"
    done
done
