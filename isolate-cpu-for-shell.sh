#!/usr/bin/env zsh
# Based on: <https://testbit.eu/2023/cgroup-cpuset>
# To enable: isolate-cpu-for-shell.sh <pid> <cpu ids ...>
#       e.g. isolate-cpu-for-shell.sh $$ 10 {13..15}
# To disable: undo-cpu-isolation.sh
set -euo pipefail

all_cpu_and_core_ids() {
    lscpu --parse=cpu,core | rg -v '^#' | sed $'s/,/\t/g'
}

core_id_for_cpu_id() {
    lscpu --parse=cpu,core | rg -v '^#' | rg "^$1," | sed $'s/,/\t/g' | cut -f 2
}

cpu_ids_for_core_id() {
    lscpu --parse=cpu,core | rg -v '^#' | rg ",$1\$" | sed $'s/,/\t/g' | cut -f 1
}

usage() {
    >&2 echo 'Usage: isolate-cpu-for-shell <pid> <cpu ids ...>'
    >&2 printf 'Available cpu ids:'
    all_cpu_and_core_ids | while read -r cpu core; do
        # If this is the first cpu id with the given core id...
        if [ "$(cpu_ids_for_core_id "$core" | head -1)" = "$cpu" ]; then
            >&2 printf ' %s' "$cpu"
        fi
    done
    >&2 echo
    exit 1
}

if [ $# -gt 1 ]; then
    shell_pid=${1:-':???:'}; shift
    for cpu; do
        core=$(core_id_for_cpu_id "$cpu")
        # If this is not the first cpu id with the given core id...
        if [ "$(cpu_ids_for_core_id "$core" | head -1)" != "$cpu" ]; then
            usage
        fi
    done
else
    usage
fi

if test -r /proc/$shell_pid/exe ; then
    # Disable process ASLR.
    echo 0 > /proc/sys/kernel/randomize_va_space

    # Allow any user to run perf.
    echo 0 > /proc/sys/kernel/perf_event_paranoid

    # Disable CPU frequency boost.
    # For the AMD 7950X, this requires Linux 6.11.
    echo 0 > /sys/devices/system/cpu/cpufreq/boost

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

    # Compute the cpu ids to reconfigure.
    selected_cpus=$(printf \%s "$*" | tr ' ' ,)
    other_online_cpus=
    all_cpu_and_core_ids | while read -r cpu core; do
        # If this is the first cpu id with the given core id...
        if [ "$(cpu_ids_for_core_id "$core" | head -1)" = "$cpu" ]; then
            # If this cpu is not one of the selected cpu ids...
            if ! printf \%s " $* " | fgrep -q " $cpu "; then
                other_online_cpus=$other_online_cpus${other_online_cpus:+,}$cpu
            fi
        fi
    done
    >&2 echo "Selected cpu ids: $selected_cpus"
    >&2 echo "Other online cpu ids: $other_online_cpus"

    # Create a “shield” cgroup and assign the dedicated CPUs to it.
    mkdir -p /sys/fs/cgroup/shield
    echo "+cpu" >> /sys/fs/cgroup/shield/cgroup.subtree_control
    echo "+cpuset" >> /sys/fs/cgroup/shield/cgroup.subtree_control
    echo "$selected_cpus"   > /sys/fs/cgroup/shield/cpuset.cpus

    # Move all other cgroups to the remaining CPUs.
    for cpscpus in /sys/fs/cgroup/**/cpuset.cpus ; do
        if [ "$cpscpus" != /sys/fs/cgroup/shield/cpuset.cpus ]; then
            echo "$other_online_cpus" > "$cpscpus"
        fi
    done

    # Make the shield cgroup a “partition root”, giving it exclusive access to its CPUs.
    sleep 0.75
    echo root > /sys/fs/cgroup/shield/cpuset.cpus.partition
    test "$(cat /sys/fs/cgroup/shield/cpuset.cpus.partition)" = root

    # Put the given pid in the shield cgroup.
    echo $shell_pid > /sys/fs/cgroup/shield/cgroup.procs
    ls -al /proc/$shell_pid/exe
    head /sys/fs/cgroup/shield/cpuset.cpus /sys/fs/cgroup/shield/cgroup.procs /sys/fs/cgroup/shield/cpuset.cpus.partition /sys/devices/system/cpu/cpufreq/boost
else
    >&2 echo 'Error: failed to find pid'
fi
