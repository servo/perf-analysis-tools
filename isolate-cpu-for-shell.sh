#!/usr/bin/env zsh
# Based on: <https://testbit.eu/2023/cgroup-cpuset>
# To enable: isolate-cpu-for-shell.sh <pid>
#       e.g. isolate-cpu-for-shell.sh $$
# To disable: isolate-cpu-for-shell.sh
set -xeuo pipefail

if test -r /proc/"${1:-:???:}"/exe ; then
    # Disable process ASLR.
    echo 0 > /proc/sys/kernel/randomize_va_space

    # Allow any user to run perf.
    echo 0 > /proc/sys/kernel/perf_event_paranoid

    # Disable CPU frequency boost.
    # For the AMD 7950X, this requires Linux 6.11.
    echo 0 > /sys/devices/system/cpu/cpufreq/boost

    # Set all CPUs to run at maximum frequency.
    for scg in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor ; do
        echo performance > $scg || :
    done

    # Offline the SMT siblings of the dedicated CPUs.
    echo 0 > /sys/devices/system/cpu/cpu24/online # 8,24
    echo 0 > /sys/devices/system/cpu/cpu25/online # 9,25

    # Create a “shield” cgroup and assign the dedicated CPUs to it.
    mkdir -p /sys/fs/cgroup/shield
    echo "+cpu" >> /sys/fs/cgroup/shield/cgroup.subtree_control
    echo "+cpuset" >> /sys/fs/cgroup/shield/cgroup.subtree_control
    echo 8-9   > /sys/fs/cgroup/shield/cpuset.cpus

    # Move all other cgroups to the remaining CPUs.
    for cpscpus in /sys/fs/cgroup/**/cpuset.cpus ; do
        if [ "$cpscpus" != /sys/fs/cgroup/shield/cpuset.cpus ]; then
            echo 0-7,10-31 > "$cpscpus"
        fi
    done

    # Make the shield cgroup a “partition root”, giving it exclusive access to its CPUs.
    sleep 0.75
    echo root > /sys/fs/cgroup/shield/cpuset.cpus.partition
    test "$(cat /sys/fs/cgroup/shield/cpuset.cpus.partition)" = root

    # Put the given pid in the shield cgroup.
    echo $1 > /sys/fs/cgroup/shield/cgroup.procs
    ls -al /proc/$1/exe
    head /sys/fs/cgroup/shield/cpuset.cpus /sys/fs/cgroup/shield/cgroup.procs /sys/fs/cgroup/shield/cpuset.cpus.partition /sys/devices/system/cpu/cpufreq/boost
else
    echo 2 > /proc/sys/kernel/randomize_va_space
    echo 4 > /proc/sys/kernel/perf_event_paranoid
    echo 1 > /sys/devices/system/cpu/cpufreq/boost
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
    for cpscpus in /sys/fs/cgroup/**/cpuset.cpus ; do
        echo 0-31 > "$cpscpus"
    done
fi
