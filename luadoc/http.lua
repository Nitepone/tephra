--- Web requests. Only available in `fetch`.
--
-- Requests time out after 20 seconds, and bodies over 16 MB fail. Redirects are followed.
--
-- Rate limits are handled for you: an answer of 429 (too many requests) or 503 is asked for
-- again after the wait its `Retry-After` header gives (a second if none), up to three times, as
-- long as the wait fits in fetch's time limit. To space requests yourself, use @{globals.sleep}.
-- An answer with an error status is returned, not raised: check `ok`.
-- @module http

local http = {}

--- What a request returns.
-- @table Response
-- @tfield int status The HTTP status, such as 200.
-- @tfield bool ok Whether the status is 2xx.
-- @tfield string body The body, as bytes: text, or binary data for @{globals.blob}.
-- @tfield table headers The answer's headers, by lowercase name: `r.headers["last-modified"]`.

--- Sends a GET request.
-- @string url
-- @tparam[opt] table opts Options:
-- @tparam[opt] table opts.headers Request headers, such as `{ ["Accept"] = "application/json" }`.
-- @treturn Response
-- @raise When the server can't be reached or the request takes too long.
-- @usage local r = http.get("https://example.com/data.json")
-- if not r.ok then error("HTTP " .. r.status) end
-- local data = json.decode(r.body)
function http.get(url, opts) end

--- Sends a POST request.
-- @string url
-- @string body The request body.
-- @tparam[opt] table opts Options, as for @{get}.
-- @treturn Response
-- @usage http.post(url, json.encode({ q = "x" }), { headers = { ["Content-Type"] = "application/json" } })
function http.post(url, body, opts) end

--- Escapes a string for use in a URL.
-- @string s
-- @treturn string
-- @usage http.get("https://wttr.in/" .. http.urlencode("New York") .. "?format=j1")
function http.urlencode(s) end

return http
