# Testing from the command line

tephra has an HTTP API for trying app code that isn't saved anywhere. It renders the code to the
same PNG a device would get and reports, node by node, what was drawn where and what didn't fit.
It's meant for scripts and coding agents that can't watch the **Try it** panel.

It runs any Lua it's sent, with network access, so **it only answers requests from the machine
tephra runs on** (`127.0.0.1` or `::1`). Anywhere else gets `403`.

`fetch` and `render` take JSON with the app:

- `code`: the app's code (required).
- `validate`: its config check. Runs when not empty.
- `config`: an instance config, such as `"units = 'imperial'"`.
- `now`: the time the app sees as now, in `time.now()`, `os.time()` and `os.date()`: seconds
  since 1970, or text like `"2026-11-22T13:00:00Z"`. For trying a weekend's phases without
  shifting the test data.
- `previous`: what `fetch` gets as its previous result.

## POST /api/v1/dev/fetch

Runs the config, the check and `fetch`, and returns what `fetch` returned. Keep it, and pass it
to `render` as `data` to re-render without fetching again.

    jq -n --rawfile code app.lua \
      '{code: $code, config: "season = 2024"}' > fetch.json
    curl -s localhost:8374/api/v1/dev/fetch \
      -H 'content-type: application/json' -d @fetch.json > fetched.json

Answer (`200`, or `422` when something failed):

    { "ok": true, "data": { ... }, "fetch": { "ms": 812, "limit_ms": 60000 } }

`fetch` also has `refresh_at`, when the app asked to fetch next (`{ refresh = … }`), and `log`,
the lines it wrote with `log()` or `print()`.

## POST /api/v1/dev/render

Runs the config, the check, `fetch` (unless `data` is given) and `render`, at one or more slot
sizes. Besides the app, it takes:

- `data`: render with this instead of running `fetch`.
- `size`: `"full"` (the default), `"large"`, `"half"` or `"small"`. Or `sizes`: a list of them, or
  `"all"`.
- `w`, `h`: the panel, 800 × 480 by default.
- `title_bar`: draw the slot's title bar, as most slots on a device have (default `true`). It
  takes 30 pixels from the app.
- `grays`: `2` (the default) or `4`, as a device's **Grays** setting. With `4`, the image is a
  2-bit PNG when anything on it is gray, and `png_bits` in the report says which it was. A
  2-bit image larger than an OG accepts is sent as 1-bit, dithered, and `problems` says so.
- `format`: `"png"` for an image instead of the JSON report (also `?format=png`). One size gives
  the device image; several give a sheet of them, labelled. If anything fails, the answer is the
  JSON report instead, with status `422`.

- `at`: a list of times (as `now`) to render at: every size at each time, at most 24 renders.
  `fetch`, if it runs, runs once, at the first.
- `slow_ms`: report renders slower than this in `problems` (default 1000, half the limit).

Every size is drawn as its own device screen: the app in the first slot of a layout with that
size, the other slots empty.

    jq -n --rawfile code app.lua --slurpfile f fetched.json \
      '{code: $code, config: "season = 2024",
        data: $f[0].data, sizes: "all"}' > req.json

    # A sheet of all four sizes, to look at:
    curl -s 'localhost:8374/api/v1/dev/render?format=png' \
      -H 'content-type: application/json' -d @req.json -o sheet.png

    # What did not fit, size by size:
    curl -s localhost:8374/api/v1/dev/render \
      -H 'content-type: application/json' -d @req.json \
      | jq '.renders[] | {size, problems}'

### The report

    {
      "ok": true,
      "fetch": null,
      "renders": [{
        "size": "half",
        "slot": { "x": 24, "y": 24, "w": 356, "h": 402 },
        "ms": 3, "limit_ms": 2000,
        "png_bytes": 3181, "png_bits": 1,
        "png": "iVBORw0…",
        "problems": [
          "root/2/1 text \"Max Verstappen\": too_wide"
        ],
        "layout": [
          { "path": "root", "kind": "column",
            "x": 24, "y": 24, "w": 356, "h": 402 },
          { "path": "root/2/1", "kind": "text",
            "x": 74, "y": 70, "w": 72, "h": 22,
            "text": "Max Verstappen", "size": 22,
            "lines": 1, "cut": ["too_wide"] }
        ]
      }]
    }

What the fields mean:

- `fetch`: `null` when `data` was given or the app has no `fetch`; otherwise its time and any error.
- `slot`: the space the app drew into, in panel pixels. `layout` uses the same coordinates.
- `png`: the device image as base64. `jq -r '.renders[0].png' | base64 -d > half.png` saves it.
- `problems`: everything in `layout` worth a look, in words; also a render slower than
  `slow_ms`, and an image larger than the 90,000 bytes an OG accepts.
- `at`: the time the app saw as now, when one was given; `redraw_at`: when render asked to be
  drawn again (`{ redraw_at = … }`); `log`: what it wrote with `log()` or `print()`.
- `layout`: every node, in drawing order. `path` is `root`, then child indexes counted from 0, so
  `root/2/1` is the second child of the root's third child. `kind` is `column`, `row`, `box`,
  `layers`, `first`, `text`, `image`, `rule` or `spacer`. A `first` adds `chosen`: which of its
  children fitted and was drawn (its path continues with that index).
- Text nodes add `text` (its first 80 characters), `size` (its height in pixels) and `lines`
  (lines drawn). Images add `image`: source width and height, then drawn width and height.
- `overflow_px`, on a column: how many pixels taller its children wanted to be than it is.
- `cut`, on a node that didn't fit: the reasons, below.

The reasons in `cut`:

- `too_wide`: a line that doesn't wrap ended in "…".
- `max_lines`: wrapped text had more lines than `max_lines`.
- `too_tall`: lines below the bottom of the node were left out.
- `word_split`: a word wider than the line was broken.
- `no_space`: the node got no room at all.
- `missing_glyph`: characters no bundled font has, listed in the node's `missing`; they show
  as empty boxes.

### Errors

A failure says where it happened. `stage` is `load` (the code itself), `fetch` or `render`, and
`chunk` and `line` give the place in the code (`app`, `config` or `config check`):

    { "stage": "render", "chunk": "app", "line": 14,
      "message": "attempt to index a nil value (field 'driver')",
      "traceback": "stack traceback:\n\t[C]: in metamethod …" }

A `load` failure is the whole answer (`422`). A `fetch` failure is reported in `fetch`, and the
renders then get `nil` as `data`. A `render` failure is in that size's `error`, and its image
shows the message where the app would be.

## POST /api/v1/dev/apps

Saves an app, to try on the Apps page and put on a device. It takes:

- `name` (required) and `description`.
- `code` (required) and `validate`, as above. A non-empty `validate` turns the check on.
- `replace`: overwrite the app already called `name` (default `false`).
- `prototype`: mark the app as a prototype in the UI (default `true`).

The code must load and the check must compile, or the answer is a `load` failure (`422`), as
for `fetch`. Names are unique, ignoring case. A taken name is `409`, unless `replace` is true;
then that app's name, description, code and check are overwritten and its instances keep
running, now with the new code. Built-in apps can't be replaced.

    jq -n --rawfile code app.lua --rawfile check validate.lua \
      '{name: "F1 Lights Out", description: "Countdown to the next race.",
        code: $code, validate: $check, replace: true}' > app.json
    curl -s localhost:8374/api/v1/dev/apps \
      -H 'content-type: application/json' -d @app.json

Answer (`200`):

    { "ok": true, "id": "u3f2a9c1b", "name": "F1 Lights Out",
      "url": "http://localhost:8374/#/apps/app/u3f2a9c1b",
      "replaced": true, "prototype": true }

`url` is the app's page in the UI. It uses the server's `public_url` if set, and otherwise
the host the request was sent to. The UI marks prototypes; unticking **Prototype** in the app's
editor and saving keeps it as a finished app.

Other errors (`400` for a missing name, `409` for a taken one) look like this:

    { "ok": false, "error": { "stage": "request", "message": "..." } }

## GET /api/v1/dev/apps/{id}/export

The app's export file, exactly as the **Export** button saves it, for **Import** elsewhere.
It's the base64 of a JSON object: `format` (`"tephra-app"`), `version` (1), `name`,
`description`, `code`, `validate_code` and `validate_enabled`.

    curl -s localhost:8374/api/v1/dev/apps/u3f2a9c1b/export -o f1-lights-out.tephra-app

## Limits

These are the same as on a device: `render` (with the config and check) has 2 seconds, `fetch`
has 60, and each may use 64 MB. `ms` and `limit_ms` show how close a run came.
