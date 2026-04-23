# fish completion for hoist

set -l __hoist_subcommands validate-config print-config-paths inspect helper-info

complete -c hoist -f
complete -c hoist -l help -d 'Print help'
complete -c hoist -l version -d 'Print version'
complete -c hoist -l config -r -d 'Path to config file'
complete -c hoist -l profile -r -d 'Profile name'

complete -c hoist -n 'not __fish_seen_subcommand_from $__hoist_subcommands' -a 'validate-config' -d 'Validate config file'
complete -c hoist -n 'not __fish_seen_subcommand_from $__hoist_subcommands' -a 'print-config-paths' -d 'Print discovered config paths'
complete -c hoist -n 'not __fish_seen_subcommand_from $__hoist_subcommands' -a 'inspect' -d 'Inspect runtime environment'
complete -c hoist -n 'not __fish_seen_subcommand_from $__hoist_subcommands' -a 'helper-info' -d 'Print helper binary information'

complete -c hoist -n '__fish_seen_subcommand_from validate-config' -l config -r -d 'Path to config file'
