
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'skills' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'skills'
        for ($i = 1; $i -lt $commandElements.Count; $i++) {
            $element = $commandElements[$i]
            if ($element -isnot [StringConstantExpressionAst] -or
                $element.StringConstantType -ne [StringConstantType]::BareWord -or
                $element.Value.StartsWith('-') -or
                $element.Value -eq $wordToComplete) {
                break
        }
        $element.Value
    }) -join ';'

    $completions = @(switch ($command) {
        'skills' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List discovered skills, optionally applying selectors')
            [CompletionResult]::new('use', 'use', [CompletionResultType]::ParameterValue, 'Declare a source and selected skills, resolving its immutable pin')
            [CompletionResult]::new('upgrade', 'upgrade', [CompletionResultType]::ParameterValue, 'Refresh immutable pins for declared sources. This never installs or syncs')
            [CompletionResult]::new('install', 'install', [CompletionResultType]::ParameterValue, 'Install selected configuration tools through normal mise, then sync links')
            [CompletionResult]::new('self-update', 'self-update', [CompletionResultType]::ParameterValue, 'Update the CLI and its compatible backend as one safe pair')
            [CompletionResult]::new('doctor', 'doctor', [CompletionResultType]::ParameterValue, 'Diagnose executable collisions and selected mise ownership')
            [CompletionResult]::new('sync', 'sync', [CompletionResultType]::ParameterValue, 'Converge the selected configuration without following update tracks')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Show declared skill sources and their link state')
            [CompletionResult]::new('remove', 'remove', [CompletionResultType]::ParameterValue, 'Remove a declaration or selected entries from its include policy')
            [CompletionResult]::new('prune', 'prune', [CompletionResultType]::ParameterValue, 'Remove stale links recorded for the selected configuration root')
            [CompletionResult]::new('cache', 'cache', [CompletionResultType]::ParameterValue, 'Inspect or clean the immutable source cache used by list')
            [CompletionResult]::new('validate', 'validate', [CompletionResultType]::ParameterValue, 'Report strict Agent Skills conformance without blocking normal use')
            [CompletionResult]::new('check', 'check', [CompletionResultType]::ParameterValue, 'Report strict Agent Skills conformance without blocking normal use')
            [CompletionResult]::new('completions', 'completions', [CompletionResultType]::ParameterValue, 'Generate a completion script for one supported shell')
            [CompletionResult]::new('completion', 'completion', [CompletionResultType]::ParameterValue, 'Generate a completion script for one supported shell')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'skills;list' {
            [CompletionResult]::new('--track', '--track', [CompletionResultType]::ParameterName, 'Branch or tag to inspect without changing any config')
            [CompletionResult]::new('--reference', '--reference', [CompletionResultType]::ParameterName, 'Explicit Git ref or full commit to inspect without changing any config')
            [CompletionResult]::new('--subpath', '--subpath', [CompletionResultType]::ParameterName, 'Select a source subpath before applying catalog discovery')
            [CompletionResult]::new('-i', '-i', [CompletionResultType]::ParameterName, 'Include a selector. Repeated values are ORed')
            [CompletionResult]::new('--include', '--include', [CompletionResultType]::ParameterName, 'Include a selector. Repeated values are ORed')
            [CompletionResult]::new('-e', '-e', [CompletionResultType]::ParameterName, 'Exclude a selector. Exclusions always win')
            [CompletionResult]::new('--exclude', '--exclude', [CompletionResultType]::ParameterName, 'Exclude a selector. Exclusions always win')
            [CompletionResult]::new('--tree', '--tree', [CompletionResultType]::ParameterName, 'Render every selected skill''s complete artifact tree')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Emit stable machine-readable JSON')
            [CompletionResult]::new('--explain', '--explain', [CompletionResultType]::ParameterName, 'Explain shadowing, selector decisions, and missing selectors')
            [CompletionResult]::new('--refresh', '--refresh', [CompletionResultType]::ParameterName, 'Re-resolve Git refs instead of using a cached resolution')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;use' {
            [CompletionResult]::new('--env', '--env', [CompletionResultType]::ParameterName, 'Select mise''s environment-specific config (`mise.<env>.toml`)')
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Select this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--agent', '--agent', [CompletionResultType]::ParameterName, 'Agent adapters to record in the declaration')
            [CompletionResult]::new('--subpath', '--subpath', [CompletionResultType]::ParameterName, 'Source-relative skill directory, validated against traversal')
            [CompletionResult]::new('--track', '--track', [CompletionResultType]::ParameterName, 'Branch or tag to resolve now and record as track. Defaults to the remote''s default branch')
            [CompletionResult]::new('--reference', '--reference', [CompletionResultType]::ParameterName, 'Git ref or full commit to resolve now and record as track')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Select the user-owned global mise config')
            [CompletionResult]::new('--locked', '--locked', [CompletionResultType]::ParameterName, 'Do not resolve or rewrite an existing immutable version')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;upgrade' {
            [CompletionResult]::new('--env', '--env', [CompletionResultType]::ParameterName, 'Select mise''s environment-specific config (`mise.<env>.toml`)')
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Select this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Select the user-owned global mise config')
            [CompletionResult]::new('--locked', '--locked', [CompletionResultType]::ParameterName, 'Do not resolve or rewrite any declared version')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;install' {
            [CompletionResult]::new('--env', '--env', [CompletionResultType]::ParameterName, 'Select mise''s environment-specific config (`mise.<env>.toml`)')
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Select this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Select the user-owned global mise config')
            [CompletionResult]::new('--locked', '--locked', [CompletionResultType]::ParameterName, 'Require mise''s existing lockfile data without resolving tool URLs')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'Preview normal mise installation without changing files; skips the direct sync')
            [CompletionResult]::new('--force', '--force', [CompletionResultType]::ParameterName, 'Replace unmanaged or changed targets at desired link paths')
            [CompletionResult]::new('--accept-collision', '--accept-collision', [CompletionResultType]::ParameterName, 'Explicitly accept an unrelated existing `skills` executable')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;self-update' {
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Update this mise config file')
            [CompletionResult]::new('--env', '--env', [CompletionResultType]::ParameterName, 'Select an environment-specific config')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Install a specific release from the compatibility manifest')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Update the user-owned global mise config (the default)')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'Show the compatible pair and planned changes without installing or editing')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;doctor' {
            [CompletionResult]::new('--env', '--env', [CompletionResultType]::ParameterName, 'Select mise''s environment-specific config (`mise.<env>.toml`)')
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Select this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Select the user-owned global mise config')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;sync' {
            [CompletionResult]::new('--env', '--env', [CompletionResultType]::ParameterName, 'Select mise''s environment-specific config (`mise.<env>.toml`)')
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Select this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Select the user-owned global mise config')
            [CompletionResult]::new('--locked', '--locked', [CompletionResultType]::ParameterName, 'Accepted for the generated postinstall hook; sync never resolves versions')
            [CompletionResult]::new('--force', '--force', [CompletionResultType]::ParameterName, 'Replace unmanaged or changed targets at desired link paths')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;status' {
            [CompletionResult]::new('--env', '--env', [CompletionResultType]::ParameterName, 'Select mise''s environment-specific config (`mise.<env>.toml`)')
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Select this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Select the user-owned global mise config')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Emit machine-readable status')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;remove' {
            [CompletionResult]::new('--env', '--env', [CompletionResultType]::ParameterName, 'Select mise''s environment-specific config (`mise.<env>.toml`)')
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Select this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Select the user-owned global mise config')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;prune' {
            [CompletionResult]::new('--env', '--env', [CompletionResultType]::ParameterName, 'Select mise''s environment-specific config (`mise.<env>.toml`)')
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Select this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Select the user-owned global mise config')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;cache' {
            [CompletionResult]::new('--clean', '--clean', [CompletionResultType]::ParameterName, 'Remove every cached entry')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Emit machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;validate' {
            [CompletionResult]::new('--subpath', '--subpath', [CompletionResultType]::ParameterName, 'Select a source subpath before validation')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Emit stable machine-readable diagnostics')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;check' {
            [CompletionResult]::new('--subpath', '--subpath', [CompletionResultType]::ParameterName, 'Select a source subpath before validation')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Emit stable machine-readable diagnostics')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;completions' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;completion' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'skills;help' {
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List discovered skills, optionally applying selectors')
            [CompletionResult]::new('use', 'use', [CompletionResultType]::ParameterValue, 'Declare a source and selected skills, resolving its immutable pin')
            [CompletionResult]::new('upgrade', 'upgrade', [CompletionResultType]::ParameterValue, 'Refresh immutable pins for declared sources. This never installs or syncs')
            [CompletionResult]::new('install', 'install', [CompletionResultType]::ParameterValue, 'Install selected configuration tools through normal mise, then sync links')
            [CompletionResult]::new('self-update', 'self-update', [CompletionResultType]::ParameterValue, 'Update the CLI and its compatible backend as one safe pair')
            [CompletionResult]::new('doctor', 'doctor', [CompletionResultType]::ParameterValue, 'Diagnose executable collisions and selected mise ownership')
            [CompletionResult]::new('sync', 'sync', [CompletionResultType]::ParameterValue, 'Converge the selected configuration without following update tracks')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Show declared skill sources and their link state')
            [CompletionResult]::new('remove', 'remove', [CompletionResultType]::ParameterValue, 'Remove a declaration or selected entries from its include policy')
            [CompletionResult]::new('prune', 'prune', [CompletionResultType]::ParameterValue, 'Remove stale links recorded for the selected configuration root')
            [CompletionResult]::new('cache', 'cache', [CompletionResultType]::ParameterValue, 'Inspect or clean the immutable source cache used by list')
            [CompletionResult]::new('validate', 'validate', [CompletionResultType]::ParameterValue, 'Report strict Agent Skills conformance without blocking normal use')
            [CompletionResult]::new('completions', 'completions', [CompletionResultType]::ParameterValue, 'Generate a completion script for one supported shell')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'skills;help;list' {
            break
        }
        'skills;help;use' {
            break
        }
        'skills;help;upgrade' {
            break
        }
        'skills;help;install' {
            break
        }
        'skills;help;self-update' {
            break
        }
        'skills;help;doctor' {
            break
        }
        'skills;help;sync' {
            break
        }
        'skills;help;status' {
            break
        }
        'skills;help;remove' {
            break
        }
        'skills;help;prune' {
            break
        }
        'skills;help;cache' {
            break
        }
        'skills;help;validate' {
            break
        }
        'skills;help;completions' {
            break
        }
        'skills;help;help' {
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
