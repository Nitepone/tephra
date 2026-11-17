# Writing an app

An app is Lua code that returns a table (see @{app}). It draws one slot of a device's screen:
the whole screen, or a half, third or quarter of it.

## Where apps live

**Built-in apps** are directories in tephra's `apps` directory, each with an `app.lua` and
optionally a `validate.lua` (its config check). They are read-only in the UI.

**Your apps** are written in the UI. On the Apps page, use **New app**, **Create copy (fork)** of
any app, or **Import** an exported one. They are stored in tephra's database. Names are unique,
ignoring case, and copies are named "Name Copy 1", "Name Copy 2" and so on.

**Export** saves an app as a `.tephra-app` file, holding its name, description, code and config
check as base64-encoded JSON. **Import** reads one back.

## What runs, in order

Each time tephra needs an app, it runs in a fresh Lua state:

1. **The instance's config.** Each instance (an app placed somewhere with its own config) has
   config code, plain Lua: `units = "imperial"` sets `config.units`. See @{globals.config}.
2. **The defaults.** Anything the config left out takes its default from the app's `settings`.
3. **The config check**, if the app has one: Lua that stops a bad config with
   `assert(ok, "message")` or `error("message")`. A config that fails can't be saved, and a
   slot whose config fails shows the message.
4. **The app's code**, and then its @{app.fetch|fetch} (on a schedule, to get data) or its
   @{app.render|render} (to draw).

## A first app

    return {
      refresh = "15m",
      settings = {
        { key = "message", label = "Message", default = "Hello" },
      },
      render = function(data, slot, config)
        return ui.column {
          justify = "center",
          ui.text { config.message, style = "title", wrap = true },
        }
      end,
    }

An instance's config for it could be:

    message = "Good morning, " .. os.date("%A")

## Getting data

`fetch` runs on the app's `refresh` schedule and returns plain data, which tephra stores and
passes to `render`. It is the only place @{http} works. Binary data, such as images, goes
through @{globals.blob}.

    fetch = function(config)
      local r = http.get("https://wttr.in/" .. http.urlencode(config.location) .. "?format=j1")
      if not r.ok then error("wttr.in answered HTTP " .. r.status) end
      local w = json.decode(r.body)
      return { temp = w.current_condition[1].temp_C }
    end,

When a fetch fails, the previous data stays and the error is shown on the Apps page.

## Drawing

`render` returns nodes built with @{ui}: columns, rows, boxes, text, images, rules and spacers.
`slot.w` and `slot.h` are the pixels available, and `slot.size` is `"full"`, `"large"`, `"half"`
or `"small"` (see @{app.Slot}). Show less in small slots:

    render = function(data, slot, config)
      if not data then return ui.text "Waiting for data…" end
      return ui.column {
        ui.text { data.temp .. "°", style = "huge" },
        slot.size ~= "small" and ui.text { data.summary } or nil,
      }
    end,

Measure instead of guessing what fits: @{ui.measure} gives the size text (or any node) will
take before drawing it, and `fit = "shrink"` on @{ui.text} picks the largest size that fits.
The panel is black and white. Text is drawn crisp; images are dithered.

## Dates and times

Keep times as numbers and use @{time} to read and write them. It knows time zones and daylight
saving time, so an app can show a race start in its viewer's zone:

    local start = time.parse(race.date .. "T" .. race.time)  -- "2026-12-06T13:00:00Z"
    ui.text { time.format(start, "%a %-d %b, %H:%M", config.zone) }  -- "Sun 6 Dec, 17:00"
    ui.text { time.relative(start) }  -- "in 3 days"

## Trying it

The app's page has a **Try it** panel: it runs a config, the fetch and the render once, at a
chosen slot size, with the code as it stands in the editor, saved or not. The instance page has
the same panel for its config. Errors are drawn where the app would be.

## Examples

tephra's built-in apps are complete examples: **Clock** (render only), **Weather** (fetch from
wttr.in, a different layout for small slots, a config check) and **Image** (binary data through
`blob`). Open one on the Apps page to read its code, or fork it to change it.
