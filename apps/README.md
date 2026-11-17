# tephra apps

Each directory here is a **built-in app**: an `app.lua`, and optionally a `validate.lua` that
checks an instance's config before the app runs. Built-in apps are read-only in tephra's UI; fork
one there to change it. Press **Reload apps** on the Apps page after editing these files.

- `clock`: render only.
- `weather`: fetches from wttr.in, a smaller layout for small slots, a config check.
- `image`: binary data through `blob`, a config check.
- `forecast`: National Weather Service data from several requests, layouts chosen by the slot's
  width and height, `fill` panels for 4 grays, a radar map built with `ui.layers` from
  transparent, recolored and blurred images, and icon files from its own directory (drawn by
  `scripts/forecast-icons.py`).

The Lua API is documented in the reference tephra serves at `/docs/lua/` (a link is at the foot of
every page). Its source is `luadoc/` in this repository: LDoc stubs and a guide. Regenerate the
HTML with `scripts/luadoc.sh` after changing them.
