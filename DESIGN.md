# Design: a small self-hosted TRMNL server

Name: **tephra**. One Rust binary that serves TRMNL devices, runs apps written in Lua,
and has a web UI in the style of bps-src.

Decisions so far:

- **Server:** Rust.
- **Apps:** Lua apps return a layout tree, and Rust draws it. No headless browser.
- **Arrangement:** each screen uses a preset layout (full, halves, ⅔|⅓, quadrants, …), with one app in each slot.
- **Rotation:** one screen per device. Playlists are deferred.
- **Simplicity:** keep it simple. When in doubt, leave it out.

The device-facing protocol is documented in `docs/`, against firmware v1.8.19. It is the reference
for the firmware endpoints; see "Protocol rules" below for what it means for this design.

## Shape

```
firmware ──HTTP──▶ device API ──▶ composer ──▶ renderer ──▶ 1-bit image (cached by hash)
                                     ▲
web UI ──▶ control API ──▶ SQLite    │
                                     │
             scheduler ──▶ Lua apps (fetch data on their own interval)
```

One crate, with a few modules:

| Module | Job |
|---|---|
| `device_api` | The three firmware endpoints: `GET /api/setup`, `GET /api/display`, `POST /api/log`. Records battery, RSSI, firmware version and last-seen time from the headers. |
| `control_api` | JSON API for the web UI under `/api/v1`. |
| `web` | The HTML, CSS and JS, embedded with `include_str!` as in bps. No build step. |
| `apps` | Loads `apps/<name>/app.lua`, runs the Lua in a sandbox, and caches each app's data. |
| `render` | Layout tree → layout → raster → dithered 1-bit PNG. |
| `compose` | Device + preset + slots → one image. The image's filename is its content hash, so the device redraws only when something changed. |
| `db` | rusqlite: devices, screens, app instances, settings, device log. |

Crates: `axum`, `tokio`, `rusqlite` (bundled), `mlua` (Lua 5.4, vendored), `ab_glyph` (text),
`png`, `image` (decoding app images), `ureq` (app HTTP), `serde`/`serde_json`, `toml` (config),
`tracing`. The raster is a plain byte buffer; text uses the bundled Noto Sans (OFL).

## Apps

Built-in apps are directories under `apps/` with an `app.lua` (and optionally a `validate.lua`),
read-only in the UI. Apps written, forked or imported in the UI are stored in the database (`apps`
table). Both return the same kind of table: refresh interval, documented settings with defaults, an
optional `fetch(config)` and a `render(data, slot, config)`. The API apps are written against is in
the Lua API reference served at `/docs/lua/`: LDoc stubs and a guide in `luadoc/`, generated into `web/luadoc/` by
`scripts/luadoc.sh` and embedded in the binary. `apps/clock`, `apps/weather` and `apps/image` are the examples.

- **Instances and configs.** One app can be added several times. A slot holds an *instance*, and each
  instance has its own **config**: Lua run before the app, whose assignments become `config`. Settings
  the config leaves out take the app's defaults, and a new instance's config is written from them.
- **Config check.** Optional per app: Lua run after the config and before the app. A config that fails
  it can't be saved, and a slot whose config fails shows the message.
- **Fetch and render are separate.** `fetch` does the network and slow work on the app's own schedule,
  and its result is cached in the database. `render` is fast and runs on every render, at the slot's
  actual size, so the preview re-renders instantly without touching the network. A failed fetch keeps
  the last good data, and the UI shows the error.
- **Sandbox.** Every call gets a fresh Lua state: tables, strings, math, utf8 and the time/date part of
  `os`, plus tephra's `time` library (chrono and chrono-tz: parsing, formatting and calendar
  arithmetic in any IANA zone) and a few string and table helpers. No `io`, `require`, `os.execute` or environment. `http` exists only in `fetch`. A 64 MB memory
  limit and a time limit (60 s for fetch, 2 s for render) stop runaway scripts. Apps written in the UI
  can only show images they stored with `blob()`, not files.
- **Authoring.** The app page has name and description fields, a code editor (line numbers, Lua
  colours), the optional config check, and **Try it**, which runs the unsaved code once. Any app can be
  forked ("Name Copy N"; names are unique, ignoring case). Export and import use a file of
  base64-encoded JSON.

### Layout tree

A small, flexbox-ish vocabulary is enough:

| Node | Notes |
|---|---|
| `column`, `row` | `gap`, `align`, `justify`; children may set `grow`. |
| `text` | `style` = small/body/label/title/large/huge (fixed type scale) or `size`, `font` (sans, serif, mono, overpass), `bold`, `italic`, `wrap`, `align`, `max_lines`, `fit = "shrink"` with `min_size`, `smooth`. `ui.measure` sizes a node before drawing. |
| `image` | App asset or fetched bytes (`blob`). Dithered to 1 bit, `fit` = contain/cover. |
| `rule` | Horizontal or vertical line. |
| `box` | `border`, `padding`, `invert` (white on black). |
| `spacer` | Takes up the remaining space. |
| `table` | *Not built yet.* Rows of cells with column alignment, for calendars and departure boards. |

The renderer owns the fonts and the type scale, so all apps look consistent on the panel.
Text is drawn with a threshold, not dithered, so it stays crisp.

### Developer API

`/api/v1/dev/fetch` and `/api/v1/dev/render` run app code that isn't saved, for scripts and coding
agents: the PNG (one size or a sheet of all four), a per-node layout report flagging cut-off text
and overflow, timings against the limits, and failures with chunk, line and traceback. They run any
Lua with network access, so they answer only loopback clients. Documented in the Lua reference
("Testing from the command line", `luadoc/dev-api.md`).

## Screens and layouts

- Each device has exactly one **screen**: a preset plus an app instance per slot, plus an optional
  per-slot title bar.
- Presets are fractions of the display (full, `½|½`, `½/½`, `⅔|⅓`, `⅔|⅓ split`, quadrants, …).
  Slot sizes in pixels come from the device's resolution, so the same screen works on different panels.
- The refresh interval the device is told to use is the shortest `refresh` among its apps, clamped
  to the device's own min/max settings.
- A screen with no apps shows a welcome screen with the device's name.

## Web UI

The same frame and stylesheet as bps-src: a title bar with a two-way **workspace** switch, pages chosen
by `#/hash`, panels, list/detail splits, and light/dark tokens. It talks to `/api/v1` and polls for updates.

### Screens workspace (`/`, the default)

This workspace assumes every device is connected and configured. Device setup doesn't appear here.
**Placeholder** devices (added in the Devices workspace, with no hardware behind them) appear here like
any other, so screens can be designed before a device exists.

- **Screens.** On the left, a list of screens, one per device, by name with a thumbnail. On the right,
  the editor:
  - a **live preview** of the selected screen. It is the exact image the device will get, shown at the
    panel's aspect ratio.
  - a **layout picker**: the preset thumbnails as a segmented control.
  - **slots** you click on the preview. Choosing one opens its panel: pick an app instance (or create
    one) and edit its settings in the form generated from the app's schema. The preview re-renders as
    you type, and **Save** sends the change to the device on its next poll.
- **Apps.** The installed apps and their instances, with last-fetch time, any error, a **Fetch now**
  button, and **Reload apps**.

### Devices workspace (`/devices`)

- **Devices** (list/detail): name, MAC, friendly ID, model and resolution, firmware, battery, RSSI, last
  seen, refresh bounds, and what a double-click of the button does (see `docs/06-special-functions.md`).
  **Add placeholder** creates a device with no hardware: a name and a panel size.
- **Log.** Device log entries and server events (new devices, failed fetches).
- Server settings (public URL, timezone) stay in the config file for now.

## Protocol rules

The traps from `docs/10-discrepancies-and-open-questions.md` that shape the server:

- **Never send `status: 500` or `reset_firmware`** unless a person asked for a reset. Either one
  wipes the device's Wi-Fi and server URL. An unknown device gets a 404 status, not 500.
- **`status` in `/api/display`:** send 0 or leave it out. The firmware ignores 200 and anything else.
- **Images are PNG only**, served as exactly `image/png` with no parameters. The firmware caches
  PNGs by `filename`, but never caches BMPs. Render at the size from the `Width`/`Height` headers and
  stay under the model's image size limit (90 kB on the OG).
- **Hash-named filenames**, and never starting with `screen_wiper`, which makes the device wipe its panel.
- **Special functions** come in the `special_function` request header (underscore), and the
  response must echo `action`.
- **Firmware updates:** send `update_firmware` only when the device's `FW-Version` differs from
  the target. The device doesn't compare versions itself.
- **Send numbers and booleans as JSON numbers and booleans.** A string such as `"false"` reads as true.

## Out of scope for now

Playlists and rotation, the free-grid layout, grayscale or colour panels, multiple users and
logins (meant for LAN use, like bps), and installing apps from the UI (apps are files on disk).

## Milestones

1. ~~**Talk to a real device.**~~ Device API done and tested against simulated firmware requests.
   **Still to do:** confirm the setup → display → log loop with the real OG.
2. ~~**Renderer.**~~ Layout tree → 1-bit PNG, fonts and type scale, presets and composition,
   hash-named images.
3. ~~**Lua apps.**~~ Sandbox, fetch/render split, scheduler, instances. Example apps: clock, weather
   (wttr.in), image.
4. ~~**Screens workspace.**~~ Preview, layout picker, slot and app configuration, live preview of
   unsaved settings, placeholder devices.
5. ~~**Devices workspace.**~~ Device detail and settings, placeholders, log.

Next ideas: the `table` node, more apps (calendar from ICS, notes), letting a newly registered device
take over a placeholder's screen, and a playlist of screens per device.
