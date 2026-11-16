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

  render = function(_, slot, s)
    local time
    if s.hour24 then
      time = os.date("%H:%M")
    else
      time = (os.date("%I:%M"):gsub("^0", "")) .. os.date(" %p")
    end
    -- As large as the slot allows: the digits are about half as wide as they are tall.
    local size = math.min(slot.h * (s.show_date and 0.5 or 0.7), slot.w / (#time * 0.6))
    local date_style = slot.size == "small" and "body" or "title"
    return ui.column {
      justify = "center",
      ui.text { time, size = size, bold = true, align = "center" },
      s.show_date and ui.text { (os.date("%A, %B %d"):gsub(" 0", " ")), style = date_style, align = "center" } or nil,
    }
  end,
}
