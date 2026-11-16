# Writing tephra apps

An app is a directory here with an `app.lua`. The directory name is the app's id. Press
**Reload apps** on the Apps page (or restart tephra) after adding or changing one.

`app.lua` returns a table:

```lua
return {
  name = "Weather",                  -- shown in the UI
  description = "One line about it.",
  refresh = "30m",                   -- how often fetch runs: "90s", "15m", "1h", "1d", or seconds
  settings = {                       -- the UI builds the settings form from this
    { key = "location", label = "Location", default = "Seattle", help = "Shown under the field." },
    { key = "units", label = "Units", type = "choice", options = { "metric", "imperial" }, default = "metric" },
  },
  fetch = function(settings) ... end,             -- optional
  render = function(data, slot, settings) ... end, -- required
}
```

Setting types: `string` (the default), `text` (several lines), `number`, `bool`, `choice` (with
`options`). Settings reach Lua with their defaults filled in.

## fetch(settings)

Does the slow work: network requests, parsing. It runs on its own schedule (`refresh`), when the
instance is saved, and when someone presses **Fetch now**. What it returns must be plain data
(tables, strings, numbers, booleans). It is stored and handed to `render` as `data`.

If `fetch` raises an error (`error("...")`), the previous data is kept and the error is shown in
the UI. It may run for up to 60 seconds.

Only `fetch` can use `http`:

- `http.get(url, { headers = { ["Accept"] = "..." } })` and `http.post(url, body, opts)` return
  `{ status = 200, ok = true, body = "..." }`. Non-2xx answers are returned, not raised.
  Bodies over 16 MB, or requests over 20 s, fail.
- `http.urlencode(s)` escapes a string for use in a URL.
- `blob(bytes)` stores binary data, such as a downloaded image, and returns a `"blob:..."`
  reference to put in `data` and pass to `ui.image`.

## render(data, slot, settings)

Returns the layout tree for one slot. `data` is the last fetch result (nil until the first one, or
for apps without `fetch`). `slot` is `{ w = ..., h = ..., size = ... }`: the pixels available and
one of `full`, `large`, `half` or `small`, so an app can show less in small slots. It must finish
within 2 seconds and has no network.

## The ui tree

Build it with the `ui` constructors. Each takes a table of children (positional) and options (named),
or a single value: `ui.text "hi"`. A `nil` or `false` child is skipped, so
`cond and ui.text { ... } or nil` works.

| Node | Options |
|---|---|
| `ui.column { ... }`, `ui.row { ... }` | `gap` (px), `align` = `start`/`center`/`end`/`stretch` (across the axis, default stretch), `justify` = `start`/`center`/`end`/`between` (along it) |
| `ui.box { ... }` | a column with `padding` (default 10), `border` (default 2) and `invert` (white on black) |
| `ui.text { "words" }` | `style` = `small` 16, `body` 22 (default), `label` 16 bold, `title` 26 bold, `large` 44 bold, `huge` 96 bold; or `size` (px) and `bold`; `wrap`, `max_lines`, `align` = `left`/`center`/`right` |
| `ui.image { src }` | `src` is a `blob(...)` reference or a file in the app's directory. `fit` = `contain`/`cover`, `dither` (default true; turn it off for line art) |
| `ui.rule {}` | a line across the parent: horizontal in a column, vertical in a row. `thickness` (default 2) |
| `ui.spacer {}` | takes the remaining space |

Every node also takes `grow` (a share of the parent's leftover space), `width` and `height` (px).

Everything is drawn in pure black and white. Text is crisp; images are dithered.

## Also available

`json.decode(s)` and `json.encode(v)`; `os.time`, `os.date` and `os.clock` (in the server's
timezone, set by `timezone` in the config); `string`, `table`, `math` and `utf8`; `log(...)` and
`print(...)` write to the server's log. There is no file access, `require` or process control.
