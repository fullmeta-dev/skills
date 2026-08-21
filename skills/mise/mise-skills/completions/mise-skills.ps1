
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'mise-skills' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'mise-skills'
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
        'mise-skills' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List discovered skills, optionally applying selectors')
            [CompletionResult]::new('use', 'use', [CompletionResultType]::ParameterValue, 'Declare a source and selected skills, resolving its immutable pin')
            [CompletionResult]::new('upgrade', 'upgrade', [CompletionResultType]::ParameterValue, 'Refresh immutable pins for declared sources. This never installs or syncs')
            [CompletionResult]::new('sync', 'sync', [CompletionResultType]::ParameterValue, 'Converge the selected configuration without following update tracks')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Show declared skill sources and their link state')
            [CompletionResult]::new('remove', 'remove', [CompletionResultType]::ParameterValue, 'Remove a declaration or selected entries from its include policy')
            [CompletionResult]::new('prune', 'prune', [CompletionResultType]::ParameterValue, 'Remove stale links recorded for the selected configuration root')
            [CompletionResult]::new('validate', 'validate', [CompletionResultType]::ParameterValue, 'Report strict Agent Skills conformance without blocking normal use')
            [CompletionResult]::new('check', 'check', [CompletionResultType]::ParameterValue, 'Report strict Agent Skills conformance without blocking normal use')
            [CompletionResult]::new('completions', 'completions', [CompletionResultType]::ParameterValue, 'Generate a completion script for one supported shell')
            [CompletionResult]::new('completion', 'completion', [CompletionResultType]::ParameterValue, 'Generate a completion script for one supported shell')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'mise-skills;list' {
            [CompletionResult]::new('--subpath', '--subpath', [CompletionResultType]::ParameterName, 'Select a source subpath before applying catalog discovery')
            [CompletionResult]::new('-i', '-i', [CompletionResultType]::ParameterName, 'Include a selector. Repeated values are ORed')
            [CompletionResult]::new('--include', '--include', [CompletionResultType]::ParameterName, 'Include a selector. Repeated values are ORed')
            [CompletionResult]::new('-e', '-e', [CompletionResultType]::ParameterName, 'Exclude a selector. Exclusions always win')
            [CompletionResult]::new('--exclude', '--exclude', [CompletionResultType]::ParameterName, 'Exclude a selector. Exclusions always win')
            [CompletionResult]::new('--tree', '--tree', [CompletionResultType]::ParameterName, 'Render every selected skill''s complete artifact tree')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Emit stable machine-readable JSON')
            [CompletionResult]::new('--explain', '--explain', [CompletionResultType]::ParameterName, 'Explain shadowing, selector decisions, and missing selectors')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'mise-skills;use' {
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Edit this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--agent', '--agent', [CompletionResultType]::ParameterName, 'Agent adapters to record in the declaration')
            [CompletionResult]::new('--subpath', '--subpath', [CompletionResultType]::ParameterName, 'Source-relative skill directory, validated against traversal')
            [CompletionResult]::new('--track', '--track', [CompletionResultType]::ParameterName, 'Branch or tag to resolve. Defaults to the remote''s default branch')
            [CompletionResult]::new('--reference', '--reference', [CompletionResultType]::ParameterName, 'Explicit Git ref to resolve once')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Edit the user-owned global mise config')
            [CompletionResult]::new('--locked', '--locked', [CompletionResultType]::ParameterName, 'Do not resolve or rewrite an existing immutable version')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'mise-skills;upgrade' {
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Edit this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Edit the user-owned global mise config')
            [CompletionResult]::new('--locked', '--locked', [CompletionResultType]::ParameterName, 'Do not resolve or rewrite any declared version')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'mise-skills;sync' {
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Edit this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Edit the user-owned global mise config')
            [CompletionResult]::new('--locked', '--locked', [CompletionResultType]::ParameterName, 'Validate and converge using only already-declared immutable versions')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'mise-skills;status' {
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Edit this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Edit the user-owned global mise config')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Emit machine-readable status')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'mise-skills;remove' {
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Edit this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Edit the user-owned global mise config')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'mise-skills;prune' {
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Edit this mise config file (or mise.toml inside a directory)')
            [CompletionResult]::new('--global', '--global', [CompletionResultType]::ParameterName, 'Edit the user-owned global mise config')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'mise-skills;validate' {
            [CompletionResult]::new('--subpath', '--subpath', [CompletionResultType]::ParameterName, 'Select a source subpath before validation')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Emit stable machine-readable diagnostics')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'mise-skills;check' {
            [CompletionResult]::new('--subpath', '--subpath', [CompletionResultType]::ParameterName, 'Select a source subpath before validation')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Emit stable machine-readable diagnostics')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'mise-skills;completions' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'mise-skills;completion' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'mise-skills;help' {
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List discovered skills, optionally applying selectors')
            [CompletionResult]::new('use', 'use', [CompletionResultType]::ParameterValue, 'Declare a source and selected skills, resolving its immutable pin')
            [CompletionResult]::new('upgrade', 'upgrade', [CompletionResultType]::ParameterValue, 'Refresh immutable pins for declared sources. This never installs or syncs')
            [CompletionResult]::new('sync', 'sync', [CompletionResultType]::ParameterValue, 'Converge the selected configuration without following update tracks')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Show declared skill sources and their link state')
            [CompletionResult]::new('remove', 'remove', [CompletionResultType]::ParameterValue, 'Remove a declaration or selected entries from its include policy')
            [CompletionResult]::new('prune', 'prune', [CompletionResultType]::ParameterValue, 'Remove stale links recorded for the selected configuration root')
            [CompletionResult]::new('validate', 'validate', [CompletionResultType]::ParameterValue, 'Report strict Agent Skills conformance without blocking normal use')
            [CompletionResult]::new('completions', 'completions', [CompletionResultType]::ParameterValue, 'Generate a completion script for one supported shell')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'mise-skills;help;list' {
            break
        }
        'mise-skills;help;use' {
            break
        }
        'mise-skills;help;upgrade' {
            break
        }
        'mise-skills;help;sync' {
            break
        }
        'mise-skills;help;status' {
            break
        }
        'mise-skills;help;remove' {
            break
        }
        'mise-skills;help;prune' {
            break
        }
        'mise-skills;help;validate' {
            break
        }
        'mise-skills;help;completions' {
            break
        }
        'mise-skills;help;help' {
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
