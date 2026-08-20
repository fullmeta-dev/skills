function PLUGIN:BackendExecEnv(_ctx)
    -- Skills source artifacts intentionally expose no executable directories.
    return {env_vars = {}}
end
