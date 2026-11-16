-- Current conditions and a three-day forecast from wttr.in.
return {
  name = "Weather",
  description = "Current conditions and the next days' forecast, from wttr.in.",
  refresh = "30m",
  settings = {
    { key = "location", label = "Location", default = "Seattle",
      help = "A place name, airport code, or coordinates like 47.6,-122.3." },
    { key = "units", label = "Units", type = "choice", options = { "metric", "imperial" }, default = "metric" },
  },

  fetch = function(s)
    local r = http.get("https://wttr.in/" .. http.urlencode(s.location) .. "?format=j1")
    if not r.ok then error("wttr.in answered HTTP " .. r.status) end
    local w = json.decode(r.body)
    local now = w.current_condition[1]
    local area = w.nearest_area and w.nearest_area[1]
    local out = {
      place = area and area.areaName[1].value or s.location,
      desc = now.weatherDesc[1].value,
      temp_c = tonumber(now.temp_C), temp_f = tonumber(now.temp_F),
      feels_c = tonumber(now.FeelsLikeC), feels_f = tonumber(now.FeelsLikeF),
      humidity = tonumber(now.humidity),
      wind_kmh = tonumber(now.windspeedKmph), wind_mph = tonumber(now.windspeedMiles),
      days = {},
    }
    for _, d in ipairs(w.weather) do
      local rain = 0
      for _, h in ipairs(d.hourly or {}) do rain = math.max(rain, tonumber(h.chanceofrain) or 0) end
      table.insert(out.days, {
        date = d.date,
        max_c = tonumber(d.maxtempC), min_c = tonumber(d.mintempC),
        max_f = tonumber(d.maxtempF), min_f = tonumber(d.mintempF),
        rain = rain,
      })
    end
    return out
  end,

  render = function(w, slot, s)
    if not w then return ui.text { "Waiting for the first forecast…", style = "body" } end
    local imperial = s.units == "imperial"
    local function t(c, f) return string.format("%d°", imperial and f or c) end
    local wind = imperial and (w.wind_mph .. " mph") or (w.wind_kmh .. " km/h")

    local function day_name(date)
      local y, m, d = date:match("(%d+)-(%d+)-(%d+)")
      return os.date("%a", os.time { year = tonumber(y), month = tonumber(m), day = tonumber(d), hour = 12 })
    end

    local today = w.days[1]
    if slot.size == "small" then
      return ui.column {
        ui.text { w.place, style = "label" },
        ui.text { t(w.temp_c, w.temp_f), style = "large" },
        ui.text { w.desc, style = "body", wrap = true, max_lines = 2 },
        ui.spacer {},
        today and ui.text { "High " .. t(today.max_c, today.max_f) .. "  Low " .. t(today.min_c, today.min_f), style = "body" } or nil,
      }
    end

    local details = ui.column {
      gap = 4,
      ui.text { w.desc, style = "title", wrap = true, max_lines = 2 },
      ui.text { "Feels like " .. t(w.feels_c, w.feels_f), style = "body" },
      ui.text { "Humidity " .. w.humidity .. "%", style = "body" },
      ui.text { "Wind " .. wind, style = "body" },
    }
    local forecast = { gap = 0, justify = "between" }
    for i, d in ipairs(w.days) do
      if i > 3 then break end
      table.insert(forecast, ui.column {
        gap = 2, grow = 1,
        ui.text { i == 1 and "Today" or day_name(d.date), style = "label" },
        ui.text { t(d.max_c, d.max_f) .. " / " .. t(d.min_c, d.min_f), style = "title" },
        ui.text { "Rain " .. d.rain .. "%", style = "small" },
      })
    end

    return ui.column {
      gap = 10,
      ui.text { w.place, style = "title" },
      ui.row { align = "center", gap = 24,
        ui.text { t(w.temp_c, w.temp_f), size = slot.size == "half" and 72 or 110, bold = true },
        details },
      ui.spacer {},
      ui.rule {},
      ui.row(forecast),
    }
  end,
}
