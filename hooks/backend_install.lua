local file = require("file")
local plugin_dir = RUNTIME and RUNTIME.pluginDirPath
if plugin_dir == nil then
    error("skills backend: pluginDirPath is unavailable")
end
return dofile(file.join_path(plugin_dir, "skills", "mise", "plugin", "hooks", "backend_install.lua"))
