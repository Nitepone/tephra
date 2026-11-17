--- Building what an app draws.
--
-- `render` returns a tree of nodes made with these functions. Each takes a table: its children
-- (or, for `text` and `image`, its content) in order, and its options by name. A single value
-- also works: `ui.text "hello"`.
--
-- A `nil` or `false` child is skipped, so a child can be conditional:
--
--     ui.column {
--       ui.text { "always" },
--       show_more and ui.text { "sometimes" } or nil,
--     }
--
-- Containers place their children one after another along their axis. Children that set `grow`
-- share the space left over; otherwise `justify` places them. Across the axis, `align` places
-- each child, stretching it to the full width (or height) by default.
--
-- A screen has 2 grays (black and white) or 4 (also dark and light gray), by the device's
-- **Grays** setting; `slot.grays` says which. Text edges are shaded gray, and images are
-- dithered to the grays there are. A device whose screen shows gray does a full refresh (the
-- panel flashes); one in black and white only, a fast one. To keep a 4-gray device's text-only
-- screens black and white, set `smooth = false` on their text.
-- @module ui

local ui = {}

--- Options every node takes.
-- @table Node
-- @tfield[opt=0] number grow A share of the space the parent has left over.
-- @tfield[opt] number width A fixed width in pixels.
-- @tfield[opt] number height A fixed height in pixels.
-- @tfield[opt] string fill A background: `"black"`, `"dark"` (dark gray), `"light"` (light
-- gray) or `"white"`; or a pattern of black on white, the same with 2 grays as with 4:
-- `"hatch"` (diagonal lines), `"crosshatch"` or `"dots"`. What is drawn on it is white on
-- `"black"` and `"dark"`, black on the others. With 2 grays, `"dark"` is drawn black and
-- `"light"` white.
-- @tfield[opt] string color The color of the node's text and lines, and its children's: the
-- same four names. With 2 grays, `"dark"` is black and `"light"` white.

--- Children one under another.
-- @tparam table t The children, then these options:
-- @tparam[opt=6] number t.gap Pixels between children.
-- @tparam[opt="stretch"] string t.align Across: `"start"`, `"center"`, `"end"` or `"stretch"`.
-- In a row, also `"baseline"`: the first lines of text line up on their baseline, whatever their
-- sizes (children without text go at the top).
-- @tparam[opt="start"] string t.justify Along: `"start"`, `"center"`, `"end"` or `"between"`.
-- Ignored when a child grows. Growing children share the space by `grow`, whatever their own
-- size: two with `grow = 1` are equally wide.
-- @treturn Node
-- @usage ui.column { justify = "center", ui.text "a", ui.text "b" }
function ui.column(t) end

--- Children side by side. Takes the same options as @{column}, with a default `gap` of 12.
-- A @{rule} in a row is vertical.
-- @tparam table t The children, then options.
-- @treturn Node
-- @usage ui.row { align = "center", ui.text { "72°", style = "large" }, ui.text "Sunny" }
-- @usage ui.row { align = "baseline", ui.text { "72°", style = "huge" }, ui.text "feels 70" }
function ui.row(t) end

--- A column with a frame.
-- @tparam table t The children, then the options of @{column} and:
-- @tparam[opt=10] number t.padding Pixels between the frame and the children.
-- @tparam[opt=2] number|table t.border Width of the frame; 0 for none. Or each side's:
-- `{ left = 3 }`, `{ top = 2, bottom = 2 }` (sides left out have none).
-- @tparam[opt=false] bool t.invert White on black.
-- @treturn Node
-- @usage ui.box { invert = true, ui.text { "ALERT", style = "title" } }
function ui.box(t) end

--- Text.
--
-- Styles: `small` 16 px, `body` 22 px (the default), `label` 16 px bold, `title` 26 px bold,
-- `large` 44 px bold, `huge` 96 px bold. A size is the height of a line, in pixels, in every
-- font. Accents on capitals and low descenders may reach a little above or below the line.
--
-- Fonts: `sans` (Noto Sans, the default), `serif` (Noto Serif), `mono` (Noto Sans Mono, every
-- character the same width, for columns of numbers and times) and `overpass` (Overpass). Each
-- has regular and bold; only `serif` has italic, and the others ignore it.
--
-- A character a font lacks is drawn from Noto Sans, and failing that from DejaVu Sans, which has
-- most symbols: ● ○ ✓ ✗ ▶ ★ ━ → ← ↑ ↓ ☀ ☁ ☂ and many more. A character no font has (most
-- emoji, Chinese and Japanese) shows as an empty box; the developer API's layout report and
-- @{measure} name such characters. Text is put in Unicode's composed form first, so a letter
-- followed by a separate accent is drawn as the accented letter.
--
-- Text is smooth by default. On a screen with 4 grays, the edges of letters are shaded gray,
-- which suits text of any size. With 2 grays, text of 60 px and up has its edges dithered,
-- which softens its curves, and smaller text is drawn crisp: each pixel black or white by how
-- much of it the letter covers, as dithering would make it grainy.
--
-- `smooth = false` draws text crisp everywhere. `smooth = true` dithers it with 2 grays even
-- when small.
-- @tparam table t The text (a string or a number), then options:
-- @tparam[opt="body"] string t.style One of the styles above.
-- @tparam[opt] number t.size A size in pixels instead of the style's.
-- @tparam[opt] bool t.bold Bold or not, instead of the style's.
-- @tparam[opt="sans"] string t.font `"sans"`, `"serif"`, `"mono"` or `"overpass"`.
-- @tparam[opt=false] bool t.italic Italic, for `serif`.
-- @tparam[opt] bool t.smooth `false` for crisp text; `true` to dither small text too with 2
-- grays. Left out: smooth as above.
-- @tparam[opt] string t.fit `"shrink"`: use the largest size, from the style's (or `size`) down
-- to `min_size`, at which the text fits its space without being cut: no "…", no lines left
-- out, no words broken. If none fits, `min_size`, cut as usual.
-- @tparam[opt=10] number t.min_size The smallest size `fit = "shrink"` may use.
-- @tparam[opt=false] bool t.wrap Break into lines to fit the width. Without it, a line that
-- doesn't fit ends in "…". Newlines always break.
-- @tparam[opt] int t.max_lines At most this many lines; the last ends in "…" if cut.
-- @tparam[opt="left"] string t.align `"left"`, `"center"` or `"right"`.
-- @tparam[opt=0] number t.tracking Extra space between letters, in pixels: `"RACE WEEKEND"` set
-- wide, as on a ticket.
-- @tparam[opt="proportional"] string t.digits `"tabular"`: every digit as wide as the widest,
-- so numbers line up in columns in any font.
-- @treturn Node
-- @usage ui.text { "A long forecast for the week…", wrap = true, max_lines = 3 }
-- @usage ui.text { data.race_name, style = "huge", fit = "shrink", min_size = 24 }
-- @usage ui.text { "08:00", font = "mono", bold = true }
function ui.text(t) end

--- An image, scaled to its space and dithered to the screen's grays.
--
-- PNG, JPEG, GIF and WebP work. Transparent areas become white.
-- @tparam table t The source, then options. The source is a reference from @{globals.blob},
-- or, for built-in apps only, a file in the app's directory.
-- @tparam[opt="contain"] string t.fit `"contain"` shows all of it; `"cover"` fills the space
-- and crops the edges.
-- @tparam[opt=true] bool t.dither Dither for photos; turn off for line art and text, which
-- then take the nearest gray.
-- @tparam[opt] table t.crop Only part of the image: `{ x = …, y = …, w = …, h = … }`, in the
-- image's own pixels, from its top left corner.
-- @tparam[opt] table t.colors Gray for each color, instead of the colors' brightness: a list of
-- `{ "#rrggbb", gray }`, the gray a number (0 black to 255 white) or `"black"`, `"dark"`,
-- `"light"` or `"white"`. Every pixel takes the gray of the nearest color listed. For maps and
-- charts whose colors mean something, such as radar.
-- @tparam[opt=false] bool t.transparent Keep the image's transparency: see-through parts leave
-- what is under them, for @{layers}. Without it, transparency is drawn white.
-- @tparam[opt=0] number t.blur Soften the image after scaling, by this many pixels: coarse data
-- (radar, a heat map) then comes out as smooth shapes instead of blocks.
-- @tparam[opt] number t.threshold Black below this gray (0 to 255), white from it, after
-- scaling: for line art such as track maps, without dithering's speckle.
-- @tparam[opt=false] bool t.invert Light for dark and dark for light.
-- @treturn Node
-- @usage ui.image { data.photo, fit = "cover", grow = 1 }
-- @usage -- Light rain dark gray, heavy rain black, the white map left white:
-- ui.image { data.radar, dither = false, crop = { x = 0, y = 26, w = 600, h = 494 },
--   colors = { { "#ffffff", "white" }, { "#04e9e7", "dark" }, { "#fd0000", "black" } } }
function ui.image(t) end

--- The size a node would take, and whether it fits, before drawing it.
--
-- Use it to decide what fits instead of guessing at letter widths. `fits` and `cut` come from
-- laying the node out in the space given exactly as a render would, so they agree with what is
-- drawn: a node given `w` × `h` (or, with no limits, `needed_w` × `needed_h`) is drawn with
-- nothing cut. Text with `fit = "shrink"` is measured at its full size.
-- @tparam Node|string node A node made with these functions, or a string (as body text).
-- @tparam[opt] number max_w The width available: wrapping text breaks to fit it.
-- Unlimited if left out.
-- @tparam[opt] number max_h The height available. Unlimited if left out.
-- @treturn table In whole pixels:
--
-- - `w`, `h`: the size it takes within the limits (what a column or row would give it);
-- - `needed_w`: the width it would take with no limit (for wrapping text, one long line);
-- - `needed_h`: the height it needs at `max_w`;
-- - `fits`: whether it fits the limits with nothing cut;
-- - `cut`: why not, as in the developer API's layout report (`too_wide`, `max_lines`,
--   `too_tall`, `word_split`, `no_space`, `overflow`; and `missing_glyph`, which doesn't stop
--   it fitting);
-- - `missing`: characters no font has, if any;
-- - for text, `lines` and `line_height`.
-- @usage local title = ui.text { data.name, style = "large", wrap = true }
-- local m = ui.measure(title, slot.w)
-- local room_left = slot.h - m.h
-- @usage -- Show the table only if it fits beside the map:
-- if ui.measure(timetable, slot.w - 200, room_left).fits then ... end
-- @usage -- The widest label, to size a column:
-- local w = 0
-- for _, s in ipairs(sessions) do w = math.max(w, ui.measure(s.label).w) end
function ui.measure(node, max_w, max_h) end

--- A line across its parent: horizontal in a column, vertical in a row.
-- @tparam[opt] table t Options:
-- @tparam[opt=2] number t.thickness In pixels.
-- @tparam[opt] bool|table t.dash Dashed: `true` (6 on, 4 off), or `{ on, off }` in pixels.
-- @treturn Node
function ui.rule(t) end

--- The first child that fits the space, drawn alone; the last if none does.
--
-- "Fits" is as @{measure} has it: nothing cut, no "…", no lines left out, no words broken.
-- List the versions from the most to the least, and let the space decide; this replaces
-- measuring each version and keeping the first that fits.
-- @tparam table t The versions, then options.
-- @treturn Node
-- @usage ui.first {
--   ui.text { race.name, style = "huge" },
--   ui.text { race.name, style = "large" },
--   ui.text { race.short, style = "large" },
-- }
-- @usage -- With an optional piece, then without:
-- ui.first {
--   ui.column { header, table, footnote },
--   ui.column { header, table },
-- }
function ui.first(t) end

--- Children drawn on top of each other, each over the whole space, the first at the bottom.
--
-- For maps: a picture, then lines and marks over it. Use images with `transparent = true` above
-- the first, so what is under them shows through; a column or row draws nothing of its own, so
-- one can place a mark (`justify = "center", align = "center"`).
-- @tparam table t The children, then options.
-- @treturn Node
-- @usage ui.layers { grow = 1,
--   ui.image { data.rain, transparent = true, fit = "cover" },
--   ui.image { data.coast, transparent = true, fit = "cover" },
--   ui.column { justify = "center", align = "center", ui.text "✚" },
-- }
function ui.layers(t) end

--- Empty space that takes whatever is left over (as if `grow = 1`).
-- @tparam[opt] table t Options.
-- @treturn Node
-- @usage ui.column { ui.text "top", ui.spacer {}, ui.text "bottom" }
function ui.spacer(t) end

return ui
