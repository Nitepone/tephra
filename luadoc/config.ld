-- LDoc configuration for tephra's Lua API reference.
-- Regenerate with scripts/luadoc.sh; the HTML in web/luadoc is served at /docs/lua/.
project = "tephra"
title = "tephra Lua API"
description = "The API tephra apps are written against."
full_description = [[
Apps are Lua code that returns a table. tephra runs an instance's **config** first, then the app's
optional **config check**, then the app's `fetch` (on a schedule, for data) and `render` (for every
image, to draw). Start with the guide, then look things up in the modules.

## Linking an AI agent

An AI agent can write and test apps for you. Give it a link: it opens one page with everything
here, plus a token that lets the agent use this server's API for 24 hours. Tokens can be
revoked, or made to last longer, in **Devices › Security**.

<div id="agent-link"></div>
<script src="/static/agent-link.js"></script>
]]
file = { "app.lua", "ui.lua", "time.lua", "http.lua", "json.lua", "globals.lua" }
topics = { "guide.md", "dev-api.md" }
dir = "../web/luadoc"
format = "markdown"
sort_modules = false
no_lua_ref = true
no_space_before_args = true
