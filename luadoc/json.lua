--- JSON.
--
-- JSON `null` becomes `nil`, so `if data.field then` works as expected.
-- @module json

local json = {}

--- Parses JSON text into Lua values.
-- @string s
-- @return The value: a table for objects and arrays.
-- @raise When `s` isn't valid JSON.
function json.decode(s) end

--- Writes a Lua value as JSON text. A table with keys 1..n becomes an array.
-- @param v
-- @treturn string
function json.encode(v) end

return json
