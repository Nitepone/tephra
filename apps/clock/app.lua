-- The time and date. The device shows the time of its last refresh, so the minutes are only as
-- current as the device's refresh interval (5 minutes by default).
return {
  name = "Clock",
  description = "The time and date, as of the device's last refresh.",
  refresh = "1m",
  settings = {
    { key = "hour24", label = "24-hour clock", type = "bool", default = true },
    { key = "show_date", label = "Show the date", type = "bool", default = true },
  },

  render = function(_, slot, config)
    local now = time.now()
    local clock = time.format(now, config.hour24 and "%H:%M" or "%-I:%M %p")
    -- As large as the slot allows: measure the time at 100 px and scale to the width.
    local at100 = ui.measure(ui.text { clock, size = 100, bold = true })
    local size = math.min(slot.h * (config.show_date and 0.5 or 0.7), 100 * slot.w / at100.w)
    local date_style = slot.size == "small" and "body" or "title"
    return ui.column {
      justify = "center",
      ui.text { clock, size = size, bold = true, align = "center", fit = "shrink" },
      config.show_date and ui.text { time.format(now, "%A, %B %-d"), style = date_style, align = "center" } or nil,
    }
  end,
}
