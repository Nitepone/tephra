--- Dates, times and time zones.
--
-- Times are numbers: seconds since 1970-01-01 UTC, the same as `os.time()` returns. Store and
-- compare them as numbers, and turn them into text only to draw them, with @{format}.
--
-- Every function that takes a zone argument accepts:
--
-- - an IANA zone name, such as `"Europe/London"`, `"America/New_York"` or `"Asia/Dubai"`,
--   which follows that place's daylight saving time;
-- - `"UTC"`;
-- - a number of hours from UTC, such as `-5` or `5.5`, which never changes;
-- - `"local"`: the server's zone (tephra's `timezone` setting, else the system's).
--
-- Left out, the zone is `"local"`. Apps whose users may be elsewhere can take a zone in their
-- config and pass it along:
--
--     settings = { { key = "zone", label = "Time zone", default = "local" } },
--     ...
--     ui.text { time.format(data.start, "%a %-d %b %H:%M", config.zone) }
--
-- @module time

local time = {}

--- The current time.
-- @treturn int Seconds since 1970-01-01 UTC.
function time.now() end

--- The server's zone name, such as `"America/Los_Angeles"`.
-- @treturn string
function time.zone() end

--- Reads a date or time.
--
-- Understands ISO 8601 / RFC 3339 (`"2026-12-06T13:00:00Z"`, `"2026-12-06T17:00:00+04:00"`,
-- `"2026-12-06 13:00"`, `"2026-12-06"`) and RFC 2822 (`"Sun, 06 Dec 2026 13:00:00 GMT"`).
-- A text with its own offset (`Z`, `+04:00`) means exactly that moment; one without is read in
-- the zone given. A date alone is midnight.
-- @string s
-- @param[opt="local"] zone For text without an offset.
-- @treturn ?int The time, or nil if `s` can't be read...
-- @treturn ?string ...and why.
-- @usage -- Jolpica gives the date and the time apart:
-- local t = time.parse(race.date .. "T" .. race.time)
-- @usage local t, err = time.parse(config.start)
-- if not t then error("start: " .. err) end
function time.parse(s, zone) end

--- Writes a time as text, in a zone.
--
-- The format is strftime's, such as `%Y-%m-%d`, `%H:%M`, `%a` (Sun), `%A` (Sunday), `%b` (Dec),
-- `%B` (December), `%p` (AM/PM) and `%Z` (the zone's abbreviation, such as BST). A `-` after the
-- `%` drops the leading zero: `%-d` gives "6" rather than "06", `%-I` gives "8" rather than "08".
-- @int t
-- @string fmt
-- @param[opt="local"] zone
-- @treturn string
-- @raise When the format has an unknown `%` code, or the zone is unknown.
-- @usage time.format(t, "%a %-d %b, %H:%M", "Asia/Dubai")   --> "Sun 6 Dec, 17:00"
-- @usage time.format(t, "%-I:%M %p", "America/New_York")    --> "8:00 AM"
function time.format(t, fmt, zone) end

--- A time's calendar fields, in a zone.
-- @int t
-- @param[opt="local"] zone
-- @treturn table `year`, `month` (1-12), `day`, `hour`, `min`, `sec`, `weekday` (1 Monday to 7
-- Sunday), `yday` (1-366), `offset` (seconds east of UTC) and `zone` (the abbreviation).
function time.parts(t, zone) end

--- The time at a wall-clock date and time in a zone.
--
-- A reading in the hour skipped when clocks go forward means the same reading an hour later
-- (01:30 becomes 02:30); one in the hour repeated when they go back means the first of the two.
-- @tparam table parts `year`, `month`, `day`, and optionally `hour`, `min`, `sec` (default 0).
-- @param[opt="local"] zone
-- @treturn int
-- @raise When the date doesn't exist, such as 31 February.
-- @usage time.make({ year = 2026, month = 12, day = 6, hour = 17 }, "Asia/Dubai")
function time.make(parts, zone) end

--- The start (midnight) of the day a time falls on, in a zone.
--
-- Two times are on the same day when their `start_of_day` is equal.
-- @int t
-- @param[opt="local"] zone
-- @treturn int
-- @usage local today = time.start_of_day(time.now(), config.zone)
-- local tomorrow = time.add(today, { days = 1 }, config.zone)
function time.start_of_day(t, zone) end

--- A time plus (or minus) an amount.
--
-- `years`, `months`, `weeks` and `days` step the calendar in the zone, so a day later is the
-- same time of day even across a change of clocks, and a month after 31 January is the last
-- day of February. `hours`, `minutes` and `seconds` are exact. Negative amounts go back.
-- @int t
-- @tparam table amount Any of `years`, `months`, `weeks`, `days`, `hours`, `minutes`, `seconds`.
-- @param[opt="local"] zone For the calendar steps.
-- @treturn int
-- @usage time.add(t, { days = 1, hours = 2 })
function time.add(t, amount, zone) end

--- How far a time is from now, in words: "now", "in 12 min", "3 h ago", "in 2 days",
-- "in 3 weeks".
-- @int t
-- @int[opt] now Compare with this instead of the current time.
-- @treturn string
function time.relative(t, now) end

--- Seconds in a duration: `"90s"`, `"15m"`, `"6h"`, `"2d"`, `"1w"`, or several, `"1h30m"`. A
-- number is taken as seconds already.
-- @tparam string|number d
-- @treturn int
-- @raise When `d` isn't a duration.
-- @usage time.add(t, { seconds = time.duration(config.lead) })
function time.duration(d) end

return time
