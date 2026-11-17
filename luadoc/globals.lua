--- Everything else an app can use.
--
-- Each config, check, fetch and render runs in a fresh Lua 5.4 state, so nothing carries over
-- between calls except what `fetch` returns. The standard `string`, `table`, `math` and `utf8`
-- libraries are there, and of `os` only `os.time`, `os.date`, `os.clock` and `os.difftime`
-- (in the server's timezone; @{time} does more, in any zone). There is no file access,
-- `require`, `io` or process control.
--
-- tephra adds a few string and table helpers that Lua lacks, listed below. The string ones
-- work as methods too: `s:split(",")`.
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

--- Splits a string.
-- @string s
-- @string[opt] sep A Lua pattern (or plain text, with `plain`) to split at. Left out: runs of
-- whitespace, and empty pieces are dropped.
-- @bool[opt=false] plain Treat `sep` as plain text.
-- @treturn {string,...}
-- @usage ("a,b,,c"):split(",")   --> { "a", "b", "", "c" }
-- @usage ("  two   words "):split() --> { "two", "words" }
function string.split(s, sep, plain) end

--- Removes whitespace from both ends.
-- @string s
-- @treturn string
function string.trim(s) end

--- Whether a string starts with a prefix.
-- @string s
-- @string prefix
-- @treturn bool
function string.starts(s, prefix) end

--- Whether a string ends with a suffix.
-- @string s
-- @string suffix
-- @treturn bool
function string.ends(s, suffix) end

--- A new list of `f(value, index)` for each item.
-- @tab t A list.
-- @func f
-- @treturn table
-- @usage table.map(drivers, function(d) return d.name end)
function table.map(t, f) end

--- A new list of the items for which `f(value, index)` is true.
-- @tab t A list.
-- @func f
-- @treturn table
function table.filter(t, f) end

--- The first item for which `f(value, index)` is true, and its index; nil if none is.
-- @tab t A list.
-- @func f
-- @return The item.
-- @treturn ?int Its index.
function table.find(t, f) end

--- A table's keys, sorted.
-- @tab t
-- @treturn table
function table.keys(t) end

--- Items `i` to `j` of a list, as a new list.
-- @tab t
-- @int[opt=1] i
-- @int[opt=#t] j
-- @treturn table
function table.slice(t, i, j) end
