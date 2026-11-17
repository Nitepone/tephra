-- A local forecast in the style of the cable-TV weather loops of the 2000s: the time and current
-- conditions down the left, an extended forecast panel, the forecast in words, radar and a
-- ticker. Made for 4 grays; with 2, the gray panels become black and white.
--
-- Data from the US National Weather Service (api.weather.gov): forecasts, the nearest station's
-- observations and alerts; radar and map lines from its map server (opengeo.ncep.noaa.gov). US locations only. Place names are found with
-- Open-Meteo's geocoder.
return {
  name = "Local Forecast",
  description = "Current conditions, a five-day forecast, radar and alerts from the National Weather Service, in the style of a cable-TV weather loop. US only.",
  refresh = "15m",
  settings = {
    { key = "location", label = "Location", default = "Seattle, WA",
      help = "A US place (\"Portland, OR\") or coordinates (\"45.52,-122.68\")." },
    { key = "units", label = "Units", type = "choice", options = { "imperial", "metric" }, default = "imperial" },
    { key = "radar", label = "Radar", type = "choice", options = { "local", "regional", "off" }, default = "local",
      help = "Rain around the location, with a ring at 25 miles (local) or 100 miles (regional)." },
    { key = "zone", label = "Time zone", default = "",
      help = "For the clock. Left empty: the location's own zone." },
  },

  fetch = function(config)
    local function get(url)
      local r = http.get(url, { headers = { Accept = "application/geo+json" } })
      if not r.ok then error(url:match("//([^/]+)") .. " answered HTTP " .. r.status .. " for " .. url) end
      return json.decode(r.body)
    end

    ---- Where ----
    local lat, lon = config.location:match("^%s*(-?[%d.]+)%s*,%s*(-?[%d.]+)%s*$")
    if lat then
      lat, lon = tonumber(lat), tonumber(lon)
    else
      local name, state = config.location:match("^%s*(.-)%s*,%s*(.-)%s*$")
      name = name or config.location:trim()
      local states = {
        AL = "Alabama", AK = "Alaska", AZ = "Arizona", AR = "Arkansas", CA = "California", CO = "Colorado",
        CT = "Connecticut", DE = "Delaware", DC = "District of Columbia", FL = "Florida", GA = "Georgia",
        HI = "Hawaii", ID = "Idaho", IL = "Illinois", IN = "Indiana", IA = "Iowa", KS = "Kansas",
        KY = "Kentucky", LA = "Louisiana", ME = "Maine", MD = "Maryland", MA = "Massachusetts",
        MI = "Michigan", MN = "Minnesota", MS = "Mississippi", MO = "Missouri", MT = "Montana",
        NE = "Nebraska", NV = "Nevada", NH = "New Hampshire", NJ = "New Jersey", NM = "New Mexico",
        NY = "New York", NC = "North Carolina", ND = "North Dakota", OH = "Ohio", OK = "Oklahoma",
        OR = "Oregon", PA = "Pennsylvania", RI = "Rhode Island", SC = "South Carolina", SD = "South Dakota",
        TN = "Tennessee", TX = "Texas", UT = "Utah", VT = "Vermont", VA = "Virginia", WA = "Washington",
        WV = "West Virginia", WI = "Wisconsin", WY = "Wyoming", PR = "Puerto Rico",
      }
      local want = state and (states[state:upper()] or state):lower()
      local g = get("https://geocoding-api.open-meteo.com/v1/search?count=10&countryCode=US&name=" .. http.urlencode(name))
      local hit = table.find(g.results or {}, function(r) return not want or (r.admin1 or ""):lower() == want end)
      if not hit then error("no US place called " .. config.location) end
      lat, lon = hit.latitude, hit.longitude
    end
    local point = string.format("%.4f,%.4f", lat, lon)
    local p = get("https://api.weather.gov/points/" .. point).properties
    local metric = config.units == "metric"
    local here = p.relativeLocation and p.relativeLocation.properties or {}

    ---- The forecast: day and night periods ----
    local fc = get(p.forecast .. (metric and "?units=si" or "")).properties.periods
    local periods = table.map(fc, function(f)
      return {
        name = f.name, start = time.parse(f.startTime), day = f.isDaytime, temp = f.temperature,
        pop = f.probabilityOfPrecipitation and f.probabilityOfPrecipitation.value or 0,
        wind = f.windSpeed, wind_dir = f.windDirection, short = f.shortForecast, detail = f.detailedForecast,
        icon = f.icon,
      }
    end)

    ---- Now: the nearest station's latest observation ----
    local now = {}
    local stations = get(p.observationStations .. "?limit=3")
    for _, st in ipairs(stations.features or {}) do
      local ok, o = pcall(get, st.id .. "/observations/latest")
      o = ok and o.properties
      if o and o.temperature and o.temperature.value then
        local c = o.temperature.value
        local v = function(x) return x and x.value end
        local feels = v(o.windChill) or v(o.heatIndex) or c
        now = {
          station = st.properties.name, at = time.parse(o.timestamp),
          temp = metric and c or c * 9 / 5 + 32, feels = metric and feels or feels * 9 / 5 + 32,
          desc = o.textDescription, icon = o.icon, humidity = v(o.relativeHumidity),
          wind = v(o.windSpeed) and (metric and v(o.windSpeed) or v(o.windSpeed) / 1.609344),
          wind_deg = v(o.windDirection),
          pressure = v(o.barometricPressure) and (metric and v(o.barometricPressure) / 100 or v(o.barometricPressure) / 3386.39),
          visibility = v(o.visibility) and (metric and v(o.visibility) / 1000 or v(o.visibility) / 1609.344),
        }
        break
      end
    end

    ---- Alerts ----
    local alerts = {}
    local ok, a = pcall(get, "https://api.weather.gov/alerts/active?point=" .. point)
    for _, f in ipairs(ok and a.features or {}) do
      local ends = f.properties.ends or f.properties.expires
      table.insert(alerts, { event = f.properties.event, severity = f.properties.severity, ends = ends and time.parse(ends) })
    end

    ---- Radar: rain, and the coast and state lines, in a square centered here ----
    -- From the NWS map server: radar alone (transparent) at low resolution, so it is drawn as
    -- smooth shapes; lines alone at a higher one. The view's half-width is 2.4 times the
    -- range ring's radius (see the app's ring.png): 25 or 100 miles, 40 or 160 km.
    local radar
    if config.radar ~= "off" then
      local ring = metric and (config.radar == "regional" and 160 or 40) or (config.radar == "regional" and 100 or 25)
      local half_km = ring * (metric and 1 or 1.609344) * 60 / 25
      local dlat = half_km / 111.32
      local dlon = half_km / (111.32 * math.cos(math.rad(lat)))
      local bbox = string.format("%.4f,%.4f,%.4f,%.4f", lat - dlat, lon - dlon, lat + dlat, lon + dlon)
      local region = ({ AK = "alaska", HI = "hawaii", PR = "carib", GU = "guam" })[here.state] or "conus"
      local layer = region .. ":" .. region .. "_bref_qcd"
      local wms = "https://opengeo.ncep.noaa.gov/geoserver/ows?service=WMS&version=1.3.0&request=GetMap&styles="
        .. "&crs=EPSG:4326&format=image/png&transparent=true&bbox=" .. bbox
      local rain = http.get(wms .. "&layers=" .. layer .. "&width=" .. (config.radar == "regional" and 192 or 128) .. "&height=" .. (config.radar == "regional" and 192 or 128))
      local lines = http.get(wms .. "&layers=geopolitical&width=360&height=360")
      local caps = http.get("https://opengeo.ncep.noaa.gov/geoserver/" .. region .. "/" .. region .. "_bref_qcd/ows?service=WMS&version=1.3.0&request=GetCapabilities")
      local scan = caps.ok and caps.body:match('Dimension name="time" default="([^"]+)"')
      local is_png = function(r) return r.ok and r.body:sub(2, 4) == "PNG" end
      if is_png(rain) and is_png(lines) then
        radar = { rain = blob(rain.body), lines = blob(lines.body), ring = ring, at = scan and time.parse(scan) or time.now() }
      end
    end

    return {
      place = here.city or config.location, state = here.state, zone = p.timeZone,
      lat = lat, lon = lon, metric = metric,
      now = now, periods = periods, alerts = alerts, radar = radar, fetched = time.now(),
    }
  end,

  render = function(w, slot, config)
    if not w or not w.periods then
      return ui.column { justify = "center", ui.text { "Waiting for the first forecast…", align = "center" } }
    end
    local gray = slot.grays == 4
    local panel_bg = gray and "dark" or "black"
    local zone = config.zone ~= "" and config.zone or w.zone or "local"
    local clock_now = time.now()
    local F = "overpass"

    local function txt(s, opts)
      opts = opts or {}
      opts[1] = s
      opts.font = opts.font or F
      return ui.text(opts)
    end
    local function round(x) return x and math.floor(x + 0.5) end
    local function deg(x) return x and tostring(round(x)) or "--" end
    local speed_unit = w.metric and "km/h" or "mph"

    ---- Pictures: from the NWS icon URL's condition codes ----
    -- ".../icons/land/day/rain,40/sct?size=medium": day, and the codes rain and sct.
    local rank = { storm = 9, snow = 8, sleet = 7, rain = 6, showers = 5, fog = 4, cloudy = 3, partly = 2, sun = 1 }
    local by_code = {
      skc = "sun", few = "sun", hot = "sun", cold = "sun", wind_skc = "sun", wind_few = "sun",
      sct = "partly", wind_sct = "partly", bkn = "partly", wind_bkn = "partly",
      ovc = "cloudy", wind_ovc = "cloudy",
      rain = "rain", rain_showers = "showers", rain_showers_hi = "showers",
      tsra = "storm", tsra_sct = "storm", tsra_hi = "storm", tornado = "storm", hurricane = "storm", tropical_storm = "storm",
      snow = "snow", blizzard = "snow",
      rain_snow = "sleet", rain_sleet = "sleet", snow_sleet = "sleet", fzra = "sleet", rain_fzra = "sleet", snow_fzra = "sleet", sleet = "sleet",
      fog = "fog", haze = "fog", smoke = "fog", dust = "fog",
    }
    local function picture(url)
      url = url or ""
      local night = url:find("/night/", 1, true) ~= nil
      -- Precipitation at any time wins; otherwise the condition that lasts (the last code),
      -- as in the words ("Patchy Fog then Mostly Sunny").
      local best, last
      for code in (url:match("/land/%a+/([^?]+)") or ""):gmatch("[^/]+") do
        local k = by_code[code:match("^[%a_]+")] or "cloudy"
        last = k
        if rank[k] >= rank.showers and (not best or rank[k] > rank[best]) then best = k end
      end
      best = best or last or "cloudy"
      if night and best == "sun" then best = "moon" elseif night and best == "partly" then best = "partly_night" end
      return best
    end
    local function icon(url, bg, size)
      return ui.image { "icons/" .. bg .. "/" .. picture(url) .. ".png", dither = false, width = size, height = size }
    end
    -- "Patchy Fog then Mostly Sunny" -> "Mostly Sunny": the part that lasts.
    local function short(s)
      s = s or ""
      s = s:match(".* then (.*)") or s
      return (s:gsub("Slight Chance", "Chance"):gsub("Showers And Thunderstorms", "T'Storms"):gsub("Thunderstorms", "T'Storms"))
    end

    ---- Days: a daytime period and the night after it ----
    local days = {}
    local today = time.start_of_day(clock_now, zone)
    for i, p in ipairs(w.periods) do
      if p.day then
        local night = w.periods[i + 1]
        local is_today = time.start_of_day(p.start, zone) == today
        table.insert(days, { name = is_today and "Today" or time.format(p.start, "%a", zone),
          hi = p.temp, lo = night and not night.day and night.temp or nil, icon = p.icon, short = p.short, pop = p.pop })
      elseif i == 1 then
        table.insert(days, { name = "Tonight", lo = p.temp, icon = p.icon, short = p.short, pop = p.pop })
      end
    end

    ---- Now ----
    local now = w.now or {}
    local first = w.periods[1]
    local now_temp = now.temp or first.temp
    local now_desc = now.desc and now.desc ~= "" and now.desc or short(first.short)
    local now_icon = now.icon or first.icon
    local function compass(d)
      local names = { "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW" }
      return d and names[math.floor((d % 360) / 22.5 + 0.5) % 16 + 1] or ""
    end
    local wind_now = now.wind and (now.wind < 1 and "Calm" or (compass(now.wind_deg) .. " " .. round(now.wind) .. " " .. speed_unit))
      or (first.wind_dir .. " " .. first.wind)
    local facts = {}
    if now.feels then table.insert(facts, "Feels like " .. deg(now.feels) .. "°") end
    if now.humidity then table.insert(facts, "Humidity " .. round(now.humidity) .. "%") end
    table.insert(facts, "Wind " .. wind_now)
    if now.pressure then table.insert(facts, w.metric and string.format("Pressure %d hPa", round(now.pressure)) or string.format("Pressure %.2f in", now.pressure)) end
    if now.visibility then table.insert(facts, string.format("Visibility %d %s", round(now.visibility), w.metric and "km" or "mi")) end

    ---- Sunrise and sunset (NOAA's sunrise equation; within a minute or two) ----
    local function sun_times()
      local r, d = math.rad, math.deg
      local base = time.start_of_day(clock_now, zone)
      local local_day = time.parts(base, zone)
      local utc_midnight = time.make({ year = local_day.year, month = local_day.month, day = local_day.day }, "UTC")
      local lng_hour = w.lon / 15
      local function at(rising)
        local t = local_day.yday + ((rising and 6 or 18) - lng_hour) / 24
        local m = 0.9856 * t - 3.289
        local l = (m + 1.916 * math.sin(r(m)) + 0.020 * math.sin(r(2 * m)) + 282.634) % 360
        local ra = d(math.atan(0.91764 * math.tan(r(l)))) % 360
        ra = (ra + math.floor(l / 90) * 90 - math.floor(ra / 90) * 90) / 15
        local sin_dec = 0.39782 * math.sin(r(l))
        local cos_dec = math.cos(math.asin(sin_dec))
        local cos_h = (math.cos(r(90.833)) - sin_dec * math.sin(r(w.lat))) / (cos_dec * math.cos(r(w.lat)))
        if cos_h > 1 or cos_h < -1 then return nil end -- no sunrise or sunset today (polar)
        local h = (rising and 360 - d(math.acos(cos_h)) or d(math.acos(cos_h))) / 15
        local ut = (h + ra - 0.06571 * t - 6.622 - lng_hour) % 24
        local s = utc_midnight + math.floor(ut * 3600)
        if s < base then s = s + 86400 end
        return s
      end
      return at(true), at(false)
    end
    local rise, set = sun_times()
    local function hm(t) return t and time.format(t, "%-I:%M%p", zone):lower() or "--" end

    ---- Pieces ----
    local function alert_bar(size)
      local a = w.alerts and w.alerts[1]
      if not a then return nil end
      return ui.row { fill = "black", gap = 8, align = "center",
        ui.box { fill = "white", border = 0, padding = 3, txt("ALERT", { size = size, bold = true }) },
        txt(a.event:upper() .. (#w.alerts > 1 and ("  +" .. (#w.alerts - 1) .. " more") or ""), { size = size, bold = true, grow = 1 }),
      }
    end

    -- The radar's colors (its legend, every 2.5 dBZ) to grays: below 12 dBZ (mist, clutter)
    -- nothing; light rain light gray, moderate dark gray, heavy black. With 2 grays, light
    -- rain is a dither of black and white.
    local dbz_colors = {}
    for _, c in ipairs {
      { "#8d827d", -30 }, { "#918969", -25 }, { "#959053", -22.5 }, { "#a4a16b", -20 }, { "#b2b284", -17.5 },
      { "#c2c39b", -15 }, { "#d1d3b3", -12.5 }, { "#c1c5b4", -10 }, { "#afb5b4", -7.5 }, { "#a2a8b4", -5 },
      { "#949bb5", -2.5 }, { "#7b88ae", 0 }, { "#6275a7", 2.5 }, { "#536aa3", 5 }, { "#435e9f", 7.5 },
      { "#5085b7", 10 }, { "#5dadce", 12.5 }, { "#58c1b8", 15 }, { "#52d5a2", 17.5 }, { "#30d65b", 20 },
      { "#0ed413", 22.5 }, { "#0caf11", 25 }, { "#0b890f", 27.5 }, { "#0a730c", 30 }, { "#0d6008", 32.5 },
      { "#84a004", 35 }, { "#fadf00", 37.5 }, { "#f4ca17", 40 }, { "#eab32d", 42.5 }, { "#f4b217", 45 },
      { "#feb101", 47.5 }, { "#d00808", 50 }, { "#a20f10", 52.5 }, { "#a90808", 55 }, { "#b00000", 57.5 },
      { "#f1b9fd", 60 }, { "#e374fc", 62.5 }, { "#f174fd", 65 }, { "#fe75ff", 67.5 }, { "#8200e7", 70 },
      { "#5e00d4", 72.5 },
    } do
      local dbz = c[2]
      local g = dbz < 12 and 255 or dbz < 30 and (gray and 170 or 140) or dbz < 45 and (gray and 85 or 0) or 0
      table.insert(dbz_colors, { c[1], g })
    end

    -- Rain, the lines, a range ring and a marker here, on top of each other.
    local function radar_panel(label_size)
      local r = w.radar
      if not r or not r.rain then return nil end
      local layer = function(src, opts)
        opts = opts or {}
        opts[1], opts.transparent, opts.fit, opts.dither = src, true, "cover", opts.dither or false
        return ui.image(opts)
      end
      return ui.column { gap = 0, grow = 1,
        ui.row { fill = panel_bg, gap = 8, align = "center",
          ui.box { border = 0, padding = 3, grow = 1, txt("RADAR  " .. hm(r.at), { size = label_size, bold = true }) },
          ui.box { border = 0, padding = 3, txt("ring " .. r.ring .. (w.metric and " km" or " mi"), { size = label_size - 1 }) },
        },
        ui.layers { grow = 1,
          layer(r.rain, { colors = dbz_colors, dither = not gray, blur = 2.5 }),
          layer(r.lines, { colors = { { "#000000", "black" } } }),
          layer("icons/ring.png", { colors = { { "#000000", "black" } } }),
          ui.column { justify = "center", align = "center",
            ui.image { "icons/marker.png", transparent = true, dither = false, width = label_size + 2, height = label_size + 2 } },
        },
      }
    end

    local function ticker(size)
      local has_alerts = w.alerts and #w.alerts > 0
      local items = has_alerts and table.map(w.alerts, function(a)
        return a.event .. (a.ends and (" until " .. time.format(a.ends, "%a ", zone) .. hm(a.ends)) or "")
      end) or facts
      return ui.row { fill = "black", gap = 10, align = "center",
        ui.box { fill = gray and "light" or "white", border = 0, padding = 4, txt(has_alerts and "ALERT" or "NOW", { size = size, bold = true }) },
        txt(table.concat(items, "   ·   "), { size = size, bold = true, grow = 1 }),
      }
    end

    local function day_column(d, number_size, icon_size)
      return ui.column {
        grow = 1, gap = 0,
        txt(d.name, { size = 22, bold = true, align = "center", fill = "black" }),
        ui.column { grow = 1, gap = 0, align = "center", justify = "center",
          icon(d.icon, panel_bg, icon_size),
          txt(short(d.short), { size = 18, align = "center", wrap = true, max_lines = 2, fit = "shrink", min_size = 13 }),
        },
        ui.box { fill = "light", border = 0, padding = 1,
          txt(d.hi and deg(d.hi) or "", { font = "serif", size = number_size, align = "center" }) },
        ui.box { fill = panel_bg, border = 0, padding = 1,
          txt(d.lo and deg(d.lo) or "", { font = "serif", size = number_size, align = "center" }) },
      }
    end

    local function extended(count, number_size, icon_size, show_place)
      local cols = { gap = 3, grow = 1 }
      for i = 1, math.min(count, #days) do table.insert(cols, day_column(days[i], number_size, icon_size)) end
      return ui.box {
        grow = 1, fill = panel_bg, border = 2, padding = 0, gap = 0,
        ui.row { fill = "black", gap = 8, align = "center",
          ui.box { border = 0, padding = 5, grow = 1, txt("Extended Forecast", { size = 26, bold = true }) },
          show_place and ui.box { border = 0, padding = 5, txt(w.place, { font = "serif", size = 20, align = "right" }) } or nil,
        },
        ui.row(cols),
      }
    end

    -- The forecast in words for the period now under way: "TONIGHT'S FORECAST", "TUESDAY".
    local function words_block(text_size, lines)
      local title = first.name:upper()
      if title == "TONIGHT" or title == "TODAY" then title = title .. "'S FORECAST" end
      return ui.column {
        gap = 0,
        ui.box { fill = "light", border = 0, padding = 4,
          ui.row { gap = 10,
            txt(w.place:upper() .. ":", { size = text_size - 2, bold = true }),
            txt(title, { size = text_size - 2, bold = true, color = gray and "dark" or nil, grow = 1 }),
          },
        },
        ui.box { border = 2, padding = 5,
          txt(first.detail, { size = text_size, wrap = true, max_lines = lines, fit = "shrink", min_size = 13 }) },
      }
    end

    -- A table of days: name, picture, forecast, chance of rain, high / low.
    local function day_rows(count, size, icon_size, show_short)
      local rows = { gap = 2 }
      local name_w = 0
      for i = 1, math.min(count, #days) do
        name_w = math.max(name_w, ui.measure(txt(days[i].name, { size = size, bold = true })).w)
      end
      for i = 1, math.min(count, #days) do
        local d = days[i]
        if i > 1 then table.insert(rows, ui.rule { thickness = 1 }) end
        table.insert(rows, ui.row { align = "center", gap = 6,
          txt(d.name, { size = size, bold = true, width = name_w }),
          icon(d.icon, "white", icon_size),
          show_short and txt(short(d.short), { size = size - 1, grow = 1 }) or ui.spacer {},
          (d.pop or 0) >= 20 and txt(d.pop .. "%", { size = size - 2, bold = true }) or nil,
          txt((d.hi and deg(d.hi) or "--") .. " / " .. (d.lo and deg(d.lo) or "--"),
            { font = "serif", size = size + 2, align = "right", width = size * 3.6 }),
        })
      end
      return ui.column(rows)
    end

    local date = time.format(clock_now, "%b %-d", zone)
    local clock = time.format(clock_now, "%-I:%M%p", zone):lower()
    local W, H = slot.w, slot.h

    ---- Full screen ----
    if W >= 700 and H >= 360 then
      local almanac = ui.column { gap = 0,
        ui.box { fill = panel_bg, border = 0, padding = 3, txt("TODAY", { size = 16, bold = true, align = "right" }) },
        ui.box { border = 0, padding = 5, gap = 1,
          txt("Sunrise  " .. hm(rise), { size = 18 }),
          txt("Sunset  " .. hm(set), { size = 18 }),
          now.humidity and txt("Humidity  " .. round(now.humidity) .. "%", { size = 18 }) or nil,
        },
      }
      return ui.column {
        gap = 6,
        alert_bar(20),
        ui.row { grow = 1, gap = 8,
          ui.column { width = 236, gap = 3,
            ui.row { gap = 6, align = "end",
              txt(date, { size = 22, bold = true, grow = 1 }),
              txt(clock, { size = 22, bold = true, align = "right" }),
            },
            ui.box { fill = "light", border = 0, padding = 6, txt(w.place, { size = 28, bold = true, fit = "shrink", min_size = 18 }) },
            ui.row { align = "end", gap = 6,
              txt("now", { size = 26, bold = true, grow = 1 }),
              txt("Feels " .. deg(now.feels or now_temp) .. "°", { size = 18, align = "right" }),
            },
            ui.row { align = "center", gap = 6,
              txt(deg(now_temp) .. "°", { size = 64, bold = true }),
              icon(now_icon, "white", 72),
            },
            txt(now_desc, { size = 20, bold = true, fit = "shrink", min_size = 14 }),
            txt("Wind " .. wind_now, { size = 18 }),
            not w.radar and ui.spacer {} or nil,
            w.radar and radar_panel(16) or almanac,
          },
          ui.column { grow = 1, gap = 6,
            extended(5, 34, 64, true),
            words_block(20, 3),
          },
        },
        ticker(18),
      }
    end

    ---- A wide strip (the top or bottom half) ----
    if W >= 700 then
      return ui.column { gap = 4,
        alert_bar(16),
        ui.row { grow = 1, gap = 8,
          ui.column { width = 200, gap = 2,
            ui.box { fill = "light", border = 0, padding = 4,
              ui.row { gap = 4, txt(w.place, { size = 20, bold = true, grow = 1, fit = "shrink", min_size = 13 }), txt(clock, { size = 16, align = "right" }) } },
            ui.row { align = "center", gap = 4,
              txt(deg(now_temp) .. "°", { size = 50, bold = true }),
              icon(now_icon, "white", 52),
            },
            txt(now_desc, { size = 16, bold = true, fit = "shrink", min_size = 12 }),
            txt("Wind " .. wind_now, { size = 15 }),
          },
          extended(w.radar and 4 or 5, 22, 38, false),
          w.radar and ui.column { width = 180, radar_panel(13) } or nil,
        },
      }
    end

    ---- Two thirds of the screen ----
    if W >= 450 then
      return ui.column {
        gap = 5,
        alert_bar(18),
        ui.row { fill = "light", align = "center", gap = 10,
          ui.box { border = 0, padding = 5, grow = 1, txt(w.place, { size = 24, bold = true, fit = "shrink", min_size = 16 }) },
          txt(deg(now_temp) .. "°  " .. short(now_desc), { size = 20, bold = true }),
          ui.box { border = 0, padding = 5, txt(clock, { size = 20, align = "right" }) },
        },
        extended(4, 30, 54, false),
        H >= 360 and words_block(18, 3) or nil,
        ticker(16),
      }
    end

    ---- Half (tall) or a tall third: now, radar, the days ----
    if H >= 300 then
      local narrow = W < 300
      return ui.column {
        gap = 4,
        alert_bar(14),
        ui.box { fill = "light", border = 0, padding = 5,
          ui.row { gap = 6, align = "center",
            txt(w.place, { size = 22, bold = true, grow = 1, fit = "shrink", min_size = 14 }),
            txt(clock, { size = 16, align = "right" }) } },
        ui.row { align = "center", gap = 8,
          txt(deg(now_temp) .. "°", { size = narrow and 50 or 60, bold = true }),
          icon(now_icon, "white", narrow and 50 or 60),
          not narrow and ui.column { grow = 1, gap = 0,
            txt(now_desc, { size = 18, bold = true, wrap = true, max_lines = 2, fit = "shrink", min_size = 13 }),
            now.feels and txt("Feels like " .. deg(now.feels) .. "°", { size = 16 }) or nil,
            txt("Wind " .. wind_now, { size = 16 }),
            now.humidity and txt("Humidity " .. round(now.humidity) .. "%", { size = 16 }) or nil,
          } or nil,
        },
        narrow and txt(now_desc, { size = 16, bold = true, wrap = true, max_lines = 2 }) or nil,
        narrow and txt("Wind " .. wind_now, { size = 14 }) or nil,
        w.radar and ui.column { grow = 1, radar_panel(13) } or ui.spacer {},
        day_rows(w.radar and (H >= 380 and 4 or 3) or 5, narrow and 15 or 16, narrow and 26 or 30, not narrow),
        not w.radar and not narrow and ui.box { border = 1, padding = 4,
          txt(first.detail, { size = 15, wrap = true, max_lines = 3, fit = "shrink", min_size = 12 }) } or nil,
      }
    end

    ---- Small: now and the next days in a strip ----
    local strip = { gap = 2 }
    for i = 1, math.min(W >= 300 and 4 or 3, #days) do
      local d = days[i]
      table.insert(strip, ui.column { grow = 1, gap = 1,
        txt(d.name, { size = 15, bold = true, align = "center", fill = "black" }),
        ui.row { justify = "center", icon(d.icon, "white", 34) },
        txt((d.hi and deg(d.hi) or "--") .. "/" .. (d.lo and deg(d.lo) or "--"), { font = "serif", size = 18, align = "center" }),
      })
    end
    return ui.column {
      gap = 3,
      alert_bar(13) or
        ui.box { fill = "light", border = 0, padding = 3,
          ui.row { gap = 4, txt(w.place, { size = 17, bold = true, grow = 1, fit = "shrink", min_size = 12 }), txt(clock, { size = 14, align = "right" }) } },
      ui.row { align = "center", gap = 6,
        txt(deg(now_temp) .. "°", { size = 40, bold = true }),
        icon(now_icon, "white", 40),
        ui.column { grow = 1, gap = 0,
          txt(now_desc, { size = 15, bold = true, wrap = true, max_lines = 2, fit = "shrink", min_size = 11 }),
          txt("Wind " .. wind_now, { size = 13 }),
          txt(table.concat({ now.feels and ("Feels " .. deg(now.feels) .. "°") or nil,
            now.humidity and (round(now.humidity) .. "% hum.") or nil }, " · "), { size = 13 }),
        },
      },
      ui.spacer {},
      ui.row(strip),
    }
  end,
}
