-- A picture from the web, dithered for the panel.
return {
  name = "Image",
  description = "Shows a PNG, JPEG, GIF or WebP image from a URL.",
  refresh = "1h",
  settings = {
    { key = "url", label = "Image URL", help = "Fetched again every hour." },
    { key = "fit", label = "Fit", type = "choice", options = { "contain", "cover" }, default = "contain",
      help = "contain shows the whole image; cover fills the slot and crops the edges." },
    { key = "dither", label = "Dither", type = "bool", default = true,
      help = "On for photos. Off for line art and text, which then stay crisp." },
  },

  fetch = function(s)
    if not s.url or s.url == "" then return nil end
    local r = http.get(s.url)
    if not r.ok then error("the image URL answered HTTP " .. r.status) end
    return { image = blob(r.body) }
  end,

  render = function(data, _, s)
    if not data or not data.image then
      return ui.column { justify = "center", ui.text { "No image yet. Set its URL.", style = "body", align = "center" } }
    end
    return ui.column { ui.image { data.image, fit = s.fit, dither = s.dither, grow = 1 } }
  end,
}
