--- Everything else an app can use.
--
-- Each config, check, fetch and render runs in a fresh Lua 5.4 state, so nothing carries over
-- between calls except what `fetch` returns. The standard `string`, `table`, `math` and `utf8`
-- libraries are there, and of `os` only `os.time`, `os.date`, `os.clock` and `os.difftime`
-- (in the server's timezone). There is no file access, `require`, `io` or process control.
--
-- A call may use up to 64 MB of memory.
-- @module globals

--- The instance's config, also handed to `fetch` and `render`.
--
-- It is made by running the instance's config code, whose assignments land in this table:
--
--     location = "Portland"
--     units = "imperial"
--
-- A config can also `return { location = "Portland" }`. Anything it leaves out takes its
-- default from the app's `settings`. If the app has a config check, the check runs next, with
-- this table already in place.
-- @table config

--- Stores binary data, such as a downloaded image, and returns a reference to it.
--
-- `fetch` can only return plain data, so binary data goes through here: store it, return the
-- reference, and pass that to `ui.image` in `render`.
-- @string bytes
-- @treturn string A reference like `"blob:3f2a…"`.
-- @usage -- in fetch
-- return { photo = blob(http.get(config.url).body) }
-- -- in render
-- return ui.image { data.photo }
function blob(bytes) end

--- Writes its arguments to the server's log.
-- @param ... Values to write, separated by spaces.
function log(...) end

--- The same as @{log}.
-- @param ...
function print(...) end
