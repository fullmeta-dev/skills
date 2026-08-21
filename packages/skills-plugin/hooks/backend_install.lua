local function load_backend()
    local ok, backend = pcall(require, "skills_backend")
    if ok then
        return backend
    end

    local file = require("file")
    local plugin_dir = RUNTIME and RUNTIME.pluginDirPath
    if plugin_dir == nil then
        error("skills backend: pluginDirPath is unavailable")
    end
    return dofile(file.join_path(plugin_dir, "packages", "skills-plugin", "lib", "skills_backend.lua"))
end

function PLUGIN:BackendInstall(ctx)
    return load_backend().install(ctx)
end
