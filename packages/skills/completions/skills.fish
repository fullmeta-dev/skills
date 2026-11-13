# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_skills_global_optspecs
    string join \n h/help V/version
end

function __fish_skills_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_skills_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_skills_using_subcommand
    set -l cmd (__fish_skills_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c skills -n "__fish_skills_needs_command" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c skills -n "__fish_skills_needs_command" -s V -l version -d 'Print version'
complete -c skills -n "__fish_skills_needs_command" -f -a "list" -d 'List discovered skills, optionally applying selectors'
complete -c skills -n "__fish_skills_needs_command" -f -a "use" -d 'Declare a source and selected skills, resolving its immutable pin'
complete -c skills -n "__fish_skills_needs_command" -f -a "upgrade" -d 'Refresh immutable pins for declared sources. This never installs or syncs'
complete -c skills -n "__fish_skills_needs_command" -f -a "install" -d 'Install selected configuration tools through normal mise, then sync links'
complete -c skills -n "__fish_skills_needs_command" -f -a "self-update" -d 'Update the CLI and its compatible backend as one safe pair'
complete -c skills -n "__fish_skills_needs_command" -f -a "doctor" -d 'Diagnose executable collisions and selected mise ownership'
complete -c skills -n "__fish_skills_needs_command" -f -a "sync" -d 'Converge the selected configuration without following update tracks'
complete -c skills -n "__fish_skills_needs_command" -f -a "status" -d 'Show declared skill sources and their link state'
complete -c skills -n "__fish_skills_needs_command" -f -a "remove" -d 'Remove a declaration or selected entries from its include policy'
complete -c skills -n "__fish_skills_needs_command" -f -a "prune" -d 'Remove stale links recorded for the selected configuration root'
complete -c skills -n "__fish_skills_needs_command" -f -a "cache" -d 'Inspect or clean the immutable source cache used by list'
complete -c skills -n "__fish_skills_needs_command" -f -a "validate" -d 'Report strict Agent Skills conformance without blocking normal use'
complete -c skills -n "__fish_skills_needs_command" -f -a "check" -d 'Report strict Agent Skills conformance without blocking normal use'
complete -c skills -n "__fish_skills_needs_command" -f -a "completions" -d 'Generate a completion script for one supported shell'
complete -c skills -n "__fish_skills_needs_command" -f -a "completion" -d 'Generate a completion script for one supported shell'
complete -c skills -n "__fish_skills_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c skills -n "__fish_skills_using_subcommand list" -l track -d 'Branch or tag to inspect without changing any config' -r
complete -c skills -n "__fish_skills_using_subcommand list" -l reference -d 'Explicit Git ref or full commit to inspect without changing any config' -r
complete -c skills -n "__fish_skills_using_subcommand list" -l subpath -d 'Select a source subpath before applying catalog discovery' -r -F
complete -c skills -n "__fish_skills_using_subcommand list" -s i -l include -d 'Include a selector. Repeated values are ORed' -r
complete -c skills -n "__fish_skills_using_subcommand list" -s e -l exclude -d 'Exclude a selector. Exclusions always win' -r
complete -c skills -n "__fish_skills_using_subcommand list" -l tree -d 'Render every selected skill\'s complete artifact tree'
complete -c skills -n "__fish_skills_using_subcommand list" -l json -d 'Emit stable machine-readable JSON'
complete -c skills -n "__fish_skills_using_subcommand list" -l explain -d 'Explain shadowing, selector decisions, and missing selectors'
complete -c skills -n "__fish_skills_using_subcommand list" -l refresh -d 'Re-resolve Git refs instead of using a cached resolution'
complete -c skills -n "__fish_skills_using_subcommand list" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand use" -l env -d 'Select mise\'s environment-specific config (`mise.<env>.toml`)' -r
complete -c skills -n "__fish_skills_using_subcommand use" -l path -d 'Select this mise config file (or mise.toml inside a directory)' -r -F
complete -c skills -n "__fish_skills_using_subcommand use" -l agent -d 'Agent adapters to record in the declaration' -r
complete -c skills -n "__fish_skills_using_subcommand use" -l subpath -d 'Source-relative skill directory, validated against traversal' -r
complete -c skills -n "__fish_skills_using_subcommand use" -l track -d 'Branch or tag to resolve now and record as track. Defaults to the remote\'s default branch' -r
complete -c skills -n "__fish_skills_using_subcommand use" -l reference -d 'Git ref or full commit to resolve now and record as track' -r
complete -c skills -n "__fish_skills_using_subcommand use" -l global -d 'Select the user-owned global mise config'
complete -c skills -n "__fish_skills_using_subcommand use" -l locked -d 'Do not resolve or rewrite an existing immutable version'
complete -c skills -n "__fish_skills_using_subcommand use" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand upgrade" -l env -d 'Select mise\'s environment-specific config (`mise.<env>.toml`)' -r
complete -c skills -n "__fish_skills_using_subcommand upgrade" -l path -d 'Select this mise config file (or mise.toml inside a directory)' -r -F
complete -c skills -n "__fish_skills_using_subcommand upgrade" -l global -d 'Select the user-owned global mise config'
complete -c skills -n "__fish_skills_using_subcommand upgrade" -l locked -d 'Do not resolve or rewrite any declared version'
complete -c skills -n "__fish_skills_using_subcommand upgrade" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand install" -l env -d 'Select mise\'s environment-specific config (`mise.<env>.toml`)' -r
complete -c skills -n "__fish_skills_using_subcommand install" -l path -d 'Select this mise config file (or mise.toml inside a directory)' -r -F
complete -c skills -n "__fish_skills_using_subcommand install" -l global -d 'Select the user-owned global mise config'
complete -c skills -n "__fish_skills_using_subcommand install" -l locked -d 'Require mise\'s existing lockfile data without resolving tool URLs'
complete -c skills -n "__fish_skills_using_subcommand install" -l dry-run -d 'Preview normal mise installation without changing files; skips the direct sync'
complete -c skills -n "__fish_skills_using_subcommand install" -l force -d 'Replace unmanaged or changed targets at desired link paths'
complete -c skills -n "__fish_skills_using_subcommand install" -l accept-collision -d 'Explicitly accept an unrelated existing `skills` executable'
complete -c skills -n "__fish_skills_using_subcommand install" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand self-update" -l path -d 'Update this mise config file' -r -F
complete -c skills -n "__fish_skills_using_subcommand self-update" -l env -d 'Select an environment-specific config' -r
complete -c skills -n "__fish_skills_using_subcommand self-update" -l version -d 'Install a specific release from the compatibility manifest' -r
complete -c skills -n "__fish_skills_using_subcommand self-update" -l global -d 'Update the user-owned global mise config (the default)'
complete -c skills -n "__fish_skills_using_subcommand self-update" -l dry-run -d 'Show the compatible pair and planned changes without installing or editing'
complete -c skills -n "__fish_skills_using_subcommand self-update" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand doctor" -l env -d 'Select mise\'s environment-specific config (`mise.<env>.toml`)' -r
complete -c skills -n "__fish_skills_using_subcommand doctor" -l path -d 'Select this mise config file (or mise.toml inside a directory)' -r -F
complete -c skills -n "__fish_skills_using_subcommand doctor" -l global -d 'Select the user-owned global mise config'
complete -c skills -n "__fish_skills_using_subcommand doctor" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand sync" -l env -d 'Select mise\'s environment-specific config (`mise.<env>.toml`)' -r
complete -c skills -n "__fish_skills_using_subcommand sync" -l path -d 'Select this mise config file (or mise.toml inside a directory)' -r -F
complete -c skills -n "__fish_skills_using_subcommand sync" -l global -d 'Select the user-owned global mise config'
complete -c skills -n "__fish_skills_using_subcommand sync" -l locked -d 'Accepted for the generated postinstall hook; sync never resolves versions'
complete -c skills -n "__fish_skills_using_subcommand sync" -l force -d 'Replace unmanaged or changed targets at desired link paths'
complete -c skills -n "__fish_skills_using_subcommand sync" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand status" -l env -d 'Select mise\'s environment-specific config (`mise.<env>.toml`)' -r
complete -c skills -n "__fish_skills_using_subcommand status" -l path -d 'Select this mise config file (or mise.toml inside a directory)' -r -F
complete -c skills -n "__fish_skills_using_subcommand status" -l global -d 'Select the user-owned global mise config'
complete -c skills -n "__fish_skills_using_subcommand status" -l json -d 'Emit machine-readable status'
complete -c skills -n "__fish_skills_using_subcommand status" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand remove" -l env -d 'Select mise\'s environment-specific config (`mise.<env>.toml`)' -r
complete -c skills -n "__fish_skills_using_subcommand remove" -l path -d 'Select this mise config file (or mise.toml inside a directory)' -r -F
complete -c skills -n "__fish_skills_using_subcommand remove" -l global -d 'Select the user-owned global mise config'
complete -c skills -n "__fish_skills_using_subcommand remove" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand prune" -l env -d 'Select mise\'s environment-specific config (`mise.<env>.toml`)' -r
complete -c skills -n "__fish_skills_using_subcommand prune" -l path -d 'Select this mise config file (or mise.toml inside a directory)' -r -F
complete -c skills -n "__fish_skills_using_subcommand prune" -l global -d 'Select the user-owned global mise config'
complete -c skills -n "__fish_skills_using_subcommand prune" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand cache" -l clean -d 'Remove every cached entry'
complete -c skills -n "__fish_skills_using_subcommand cache" -l json -d 'Emit machine-readable JSON'
complete -c skills -n "__fish_skills_using_subcommand cache" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand validate" -l subpath -d 'Select a source subpath before validation' -r -F
complete -c skills -n "__fish_skills_using_subcommand validate" -l json -d 'Emit stable machine-readable diagnostics'
complete -c skills -n "__fish_skills_using_subcommand validate" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand check" -l subpath -d 'Select a source subpath before validation' -r -F
complete -c skills -n "__fish_skills_using_subcommand check" -l json -d 'Emit stable machine-readable diagnostics'
complete -c skills -n "__fish_skills_using_subcommand check" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand completions" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand completion" -s h -l help -d 'Print help'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "list" -d 'List discovered skills, optionally applying selectors'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "use" -d 'Declare a source and selected skills, resolving its immutable pin'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "upgrade" -d 'Refresh immutable pins for declared sources. This never installs or syncs'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "install" -d 'Install selected configuration tools through normal mise, then sync links'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "self-update" -d 'Update the CLI and its compatible backend as one safe pair'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "doctor" -d 'Diagnose executable collisions and selected mise ownership'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "sync" -d 'Converge the selected configuration without following update tracks'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "status" -d 'Show declared skill sources and their link state'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "remove" -d 'Remove a declaration or selected entries from its include policy'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "prune" -d 'Remove stale links recorded for the selected configuration root'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "cache" -d 'Inspect or clean the immutable source cache used by list'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "validate" -d 'Report strict Agent Skills conformance without blocking normal use'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "completions" -d 'Generate a completion script for one supported shell'
complete -c skills -n "__fish_skills_using_subcommand help; and not __fish_seen_subcommand_from list use upgrade install self-update doctor sync status remove prune cache validate completions help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
