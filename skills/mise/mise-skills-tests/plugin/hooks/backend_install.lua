function PLUGIN:BackendInstall(ctx)
    local json = require("json")
    local file = require("file")
    local log_path = os.getenv("SKILLS_COMPAT_LOG")

    if log_path then
        local log = io.open(log_path, "a")
        log:write(json.encode({
            event = "backend-install",
            tool = ctx.tool,
            version = ctx.version,
            options = ctx.options or {},
        }))
        log:write("\n")
        log:close()
    end

    local bin_path = file.join_path(ctx.install_path, "bin")
    os.execute("mkdir -p " .. bin_path)

    -- The compatibility test's managed CLI is deliberately tiny. The source
    -- tool has no executable; this conditional keeps the two tool roles clear.
    if ctx.tool == "mise-skills" then
        local executable = io.open(file.join_path(bin_path, "mise-skills"), "w")
        executable:write("#!/bin/sh\n")
        executable:write("printf '%s\\n' cli >> \"$SKILLS_COMPAT_LOG\"\n")
        executable:write("for arg in \"$@\"; do printf 'arg=%s\\n' \"$arg\" >> \"$SKILLS_COMPAT_LOG\"; done\n")
        executable:close()
        os.execute("chmod +x " .. file.join_path(bin_path, "mise-skills"))
    end

    return {}
end
