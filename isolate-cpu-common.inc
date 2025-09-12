all_cpu_and_core_ids() {
    lscpu --parse=cpu,core | rg -v '^#' | sed $'s/,/\t/g'
}

core_id_for_cpu_id() {
    lscpu --parse=cpu,core | rg -v '^#' | rg "^$1," | sed $'s/,/\t/g' | cut -f 2
}

cpu_ids_for_core_id() {
    lscpu --parse=cpu,core | rg -v '^#' | rg ",$1\$" | sed $'s/,/\t/g' | cut -f 1
}
