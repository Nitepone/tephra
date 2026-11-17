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
-- Everything is drawn in pure black and white: text is crisp, images are dithered.
-- @module ui

local ui = {}

--- Options every node takes.
-- @table Node
-- @tfield[opt=0] number grow A share of the space the parent has left over.
-- @tfield[opt] number width A fixed width in pixels.
-- @tfield[opt] number height A fixed height in pixels.

--- Children one under another.
-- @tparam table t The children, then these options:
-- @tparam[opt=6] number t.gap Pixels between children.
-- @tparam[opt="stretch"] string t.align Across: `"start"`, `"center"`, `"end"` or `"stretch"`.
-- @tparam[opt="start"] string t.justify Along: `"start"`, `"center"`, `"end"` or `"between"`.
-- Ignored when a child grows.
-- @treturn Node
-- @usage ui.column { justify = "center", ui.text "a", ui.text "b" }
function ui.column(t) end

--- Children side by side. Takes the same options as @{column}, with a default `gap` of 12.
-- A @{rule} in a row is vertical.
-- @tparam table t The children, then options.
-- @treturn Node
-- @usage ui.row { align = "center", ui.text { "72°", style = "large" }, ui.text "Sunny" }
function ui.row(t) end

--- A column with a frame.
-- @tparam table t The children, then the options of @{column} and:
-- @tparam[opt=10] number t.padding Pixels between the frame and the children.
-- @tparam[opt=2] number t.border Width of the frame; 0 for none.
-- @tparam[opt=false] bool t.invert White on black.
-- @treturn Node
-- @usage ui.box { invert = true, ui.text { "ALERT", style = "title" } }
function ui.box(t) end

--- Text.
--
-- Styles: `small` 16 px, `body` 22 px (the default), `label` 16 px bold, `title` 26 px bold,
-- `large` 44 px bold, `huge` 96 px bold. A size is the height of a line, in pixels.
--
-- Fonts: `sans` (Noto Sans, the default), `serif` (Noto Serif), `mono` (Noto Sans Mono, every
-- character the same width, for columns of numbers and times) and `overpass` (Overpass). Each
-- has regular and bold; only `serif` has italic, and the others ignore it. A character a font
-- lacks is drawn from Noto Sans.
--
-- Text is drawn crisp: each pixel is black or white by how much of it the letter covers. With
-- `smooth`, the edges are dithered instead, which softens the curves of large text (about 60 px
-- and up) but makes small text grainy.
-- @tparam table t The text (a string or a number), then options:
-- @tparam[opt="body"] string t.style One of the styles above.
-- @tparam[opt] number t.size A size in pixels instead of the style's.
-- @tparam[opt] bool t.bold Bold or not, instead of the style's.
-- @tparam[opt="sans"] string t.font `"sans"`, `"serif"`, `"mono"` or `"overpass"`.
-- @tparam[opt=false] bool t.italic Italic, for `serif`.
-- @tparam[opt=false] bool t.smooth Dither the edges, for large text.
-- @tparam[opt] string t.fit `"shrink"`: use the largest size, from the style's (or `size`) down
-- to `min_size`, at which the text fits its space without being cut: no "…", no lines left
-- out, no words broken. If none fits, `min_size`, cut as usual.
-- @tparam[opt=10] number t.min_size The smallest size `fit = "shrink"` may use.
-- @tparam[opt=false] bool t.wrap Break into lines to fit the width. Without it, a line that
-- doesn't fit ends in "…". Newlines always break.
-- @tparam[opt] int t.max_lines At most this many lines; the last ends in "…" if cut.
-- @tparam[opt="left"] string t.align `"left"`, `"center"` or `"right"`.
-- @treturn Node
-- @usage ui.text { "A long forecast for the week…", wrap = true, max_lines = 3 }
-- @usage ui.text { data.race_name, style = "huge", fit = "shrink", min_size = 24 }
-- @usage ui.text { "08:00", font = "mono", bold = true }
function ui.text(t) end

--- An image, scaled to its space and dithered to black and white.
--
-- PNG, JPEG, GIF and WebP work. Transparent areas become white.
-- @tparam table t The source, then options. The source is a reference from @{globals.blob},
-- or, for built-in apps only, a file in the app's directory.
-- @tparam[opt="contain"] string t.fit `"contain"` shows all of it; `"cover"` fills the space
-- and crops the edges.
-- @tparam[opt=true] bool t.dither Dither for photos; turn off for line art and text.
-- @treturn Node
-- @usage ui.image { data.photo, fit = "cover", grow = 1 }
function ui.image(t) end

--- The size a node would take, before drawing it.
--
-- Use it to decide what fits instead of guessing at letter widths. The size is what the node
-- asks for within `max_w` × `max_h`, the same way a column or row measures it. Text with
-- `fit = "shrink"` is measured at its full size.
-- @tparam Node|string node A node made with these functions, or a string (as body text).
-- @tparam[opt] number max_w The width available: wrapping text breaks to fit it.
-- Unlimited if left out.
-- @tparam[opt] number max_h The height available. Unlimited if left out.
-- @treturn table `{ w = ..., h = ... }` in pixels; for text also `lines` and `line_height`.
-- @usage local title = ui.text { data.name, style = "large", wrap = true }
-- local m = ui.measure(title, slot.w)
-- local room_left = slot.h - m.h
-- @usage -- The widest label, to size a column:
-- local w = 0
-- for _, s in ipairs(sessions) do w = math.max(w, ui.measure(s.label).w) end
function ui.measure(node, max_w, max_h) end

--- A line across its parent: horizontal in a column, vertical in a row.
-- @tparam[opt] table t Options:
-- @tparam[opt=2] number t.thickness In pixels.
-- @treturn Node
function ui.rule(t) end

--- Empty space that takes whatever is left over (as if `grow = 1`).
-- @tparam[opt] table t Options.
-- @treturn Node
-- @usage ui.column { ui.text "top", ui.spacer {}, ui.text "bottom" }
function ui.spacer(t) end

return ui
