-- LDoc configuration for tephra's Lua API reference.
-- Regenerate with scripts/luadoc.sh; the HTML in web/luadoc is served at /docs/lua/.
project = "tephra"
title = "tephra Lua API"
description = "The API tephra apps are written against."
full_description = [[
Apps are Lua code that returns a table. tephra runs an instance's **config** first, then the app's
optional **config check**, then the app's `fetch` (on a schedule, for data) and `render` (for every
image, to draw). Start with the guide, then look things up in the modules.
]]
file = { "app.lua", "ui.lua", "time.lua", "http.lua", "json.lua", "globals.lua" }
topics = { "guide.md", "dev-api.md" }
dir = "../web/luadoc"
format = "markdown"
sort_modules = false
no_lua_ref = true
no_space_before_args = true
