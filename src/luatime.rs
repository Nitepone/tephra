//! The `time` library apps get, and a few string and table helpers Lua lacks.
//!
//! Times are seconds since 1970-01-01 UTC (as `os.time()` returns). Zones are IANA names such
//! as "Europe/London", "UTC", a number of hours such as -5 or 5.5, or "local": the server's
//! zone (the config's `timezone`, else the system's), which is also the default.

use chrono::{DateTime, Datelike, Days, FixedOffset, LocalResult, Months, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone, Timelike, Utc};
use chrono::format::{Item, StrftimeItems};
use chrono_tz::Tz;
use mlua::{Lua, Table, Value};

#[derive(Clone, Copy)]
enum Zone {
    Named(Tz),
    Fixed(FixedOffset),
}

/// The server's zone: TZ (which tephra sets from the config), else the system's, else UTC.
pub fn local_zone_name() -> String {
    std::env::var("TZ")
        .ok()
        .filter(|z| z.parse::<Tz>().is_ok())
        .or_else(|| iana_time_zone::get_timezone().ok().filter(|z| z.parse::<Tz>().is_ok()))
        .unwrap_or_else(|| "UTC".into())
}

fn zone(v: &Value) -> mlua::Result<Zone> {
    match v {
        Value::Nil => Ok(Zone::Named(local_zone_name().parse().unwrap_or(Tz::UTC))),
        Value::Integer(h) => fixed(*h as f64),
        Value::Number(h) => fixed(*h),
        Value::String(s) => {
            let s = s.to_str()?.to_string();
            if s == "local" {
                return zone(&Value::Nil);
            }
            s.parse::<Tz>()
                .map(Zone::Named)
                .map_err(|_| mlua::Error::runtime(format!("unknown time zone {s:?}: use a name such as \"Europe/London\", \"UTC\", \"local\" or hours such as -5")))
        }
        v => Err(mlua::Error::runtime(format!("a time zone is a name or a number of hours, not {}", v.type_name()))),
    }
}

fn fixed(hours: f64) -> mlua::Result<Zone> {
    FixedOffset::east_opt((hours * 3600.0).round() as i32)
        .map(Zone::Fixed)
        .ok_or_else(|| mlua::Error::runtime(format!("a UTC offset of {hours} hours is out of range")))
}

fn utc(t: i64) -> mlua::Result<DateTime<Utc>> {
    DateTime::from_timestamp(t, 0).ok_or_else(|| mlua::Error::runtime(format!("time {t} is out of range")))
}

/// Runs `f` on the time as seen in the zone (generic over the zone's type).
macro_rules! in_zone {
    ($z:expr, $t:expr, |$dt:ident| $body:expr) => {
        match $z {
            Zone::Named(tz) => {
                let $dt = utc($t)?.with_timezone(&tz);
                $body
            }
            Zone::Fixed(off) => {
                let $dt = utc($t)?.with_timezone(&off);
                $body
            }
        }
    };
}

/// A wall-clock time in a zone, as seconds. In a DST gap the time after it; when ambiguous,
/// the earlier.
fn localize(z: Zone, naive: NaiveDateTime) -> mlua::Result<i64> {
    fn pick<T: TimeZone>(r: LocalResult<DateTime<T>>, tz: &T, naive: NaiveDateTime) -> Option<i64> {
        match r {
            LocalResult::Single(d) | LocalResult::Ambiguous(d, _) => Some(d.timestamp()),
            // Skipped when the clocks went forward: the same reading an hour later.
            LocalResult::None => tz.from_local_datetime(&(naive + chrono::Duration::hours(1))).earliest().map(|d| d.timestamp()),
        }
    }
    match z {
        Zone::Named(tz) => pick(tz.from_local_datetime(&naive), &tz, naive),
        Zone::Fixed(off) => pick(off.from_local_datetime(&naive), &off, naive),
    }
    .ok_or_else(|| mlua::Error::runtime(format!("{naive} doesn't exist in that time zone")))
}

const NAIVE_FORMATS: &[&str] = &["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d %H:%M"];

fn parse(s: &str, z: Zone) -> Option<mlua::Result<i64>> {
    let s = s.trim();
    if let Ok(d) = DateTime::parse_from_rfc3339(s).or_else(|_| DateTime::parse_from_rfc3339(&s.replacen(' ', "T", 1))) {
        return Some(Ok(d.timestamp()));
    }
    if let Ok(d) = DateTime::parse_from_rfc2822(s) {
        return Some(Ok(d.timestamp()));
    }
    for f in NAIVE_FORMATS {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, f) {
            return Some(localize(z, n));
        }
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(localize(z, d.and_time(NaiveTime::MIN)));
    }
    None
}

fn check_format(f: &str) -> mlua::Result<()> {
    if StrftimeItems::new(f).any(|i| matches!(i, Item::Error)) {
        return Err(mlua::Error::runtime(format!("bad time format {f:?}: see strftime, such as \"%a %-d %b %H:%M\"")));
    }
    Ok(())
}

fn relative(t: i64, now: i64) -> String {
    let d = t - now;
    let a = d.abs();
    let (n, unit) = if a < 45 {
        return "now".into();
    } else if a < 3600 {
        ((a + 30) / 60, "min")
    } else if a < 86400 {
        ((a + 1800) / 3600, "h")
    } else if a < 86400 * 14 {
        let days = (a + 43200) / 86400;
        (days, if days == 1 { "day" } else { "days" })
    } else {
        let weeks = (a + 86400 * 3) / (86400 * 7);
        (weeks, if weeks == 1 { "week" } else { "weeks" })
    };
    if d > 0 { format!("in {n} {unit}") } else { format!("{n} {unit} ago") }
}

fn int_field(t: &Table, k: &str) -> mlua::Result<Option<i64>> {
    t.get::<Option<i64>>(k)
}

pub fn install(lua: &Lua) -> mlua::Result<()> {
    let time = lua.create_table()?;
    time.set("now", lua.create_function(|_, ()| Ok(Utc::now().timestamp()))?)?;
    time.set("zone", lua.create_function(|_, ()| Ok(local_zone_name()))?)?;

    time.set(
        "parse",
        lua.create_function(|_, (s, z): (String, Value)| {
            let z = zone(&z)?;
            Ok(match parse(&s, z) {
                Some(Ok(t)) => (Some(t), None),
                Some(Err(e)) => (None, Some(e.to_string())),
                None => (None, Some(format!("can't read {s:?} as a date or time"))),
            })
        })?,
    )?;

    time.set(
        "format",
        lua.create_function(|_, (t, f, z): (i64, String, Value)| {
            check_format(&f)?;
            let z = zone(&z)?;
            Ok(in_zone!(z, t, |dt| dt.format(&f).to_string()))
        })?,
    )?;

    time.set(
        "parts",
        lua.create_function(|lua, (t, z): (i64, Value)| {
            let z = zone(&z)?;
            let out = lua.create_table()?;
            in_zone!(z, t, |dt| {
                out.set("year", dt.year())?;
                out.set("month", dt.month())?;
                out.set("day", dt.day())?;
                out.set("hour", dt.hour())?;
                out.set("min", dt.minute())?;
                out.set("sec", dt.second())?;
                out.set("weekday", dt.weekday().number_from_monday())?;
                out.set("yday", dt.ordinal())?;
                out.set("offset", dt.offset().fix().local_minus_utc())?;
                out.set("zone", dt.format("%Z").to_string())?;
            });
            Ok(out)
        })?,
    )?;

    time.set(
        "make",
        lua.create_function(|_, (p, z): (Table, Value)| {
            let z = zone(&z)?;
            let need = |k: &str| int_field(&p, k)?.ok_or_else(|| mlua::Error::runtime(format!("time.make needs {k}")));
            let (y, m, d) = (need("year")?, need("month")?, need("day")?);
            let (h, mi, s) = (int_field(&p, "hour")?.unwrap_or(0), int_field(&p, "min")?.unwrap_or(0), int_field(&p, "sec")?.unwrap_or(0));
            let date = NaiveDate::from_ymd_opt(y as i32, m as u32, d as u32).ok_or_else(|| mlua::Error::runtime(format!("{y}-{m}-{d} isn't a date")))?;
            let tm = NaiveTime::from_hms_opt(h as u32, mi as u32, s as u32).ok_or_else(|| mlua::Error::runtime(format!("{h}:{mi}:{s} isn't a time")))?;
            localize(z, date.and_time(tm))
        })?,
    )?;

    time.set(
        "start_of_day",
        lua.create_function(|_, (t, z): (i64, Value)| {
            let z = zone(&z)?;
            let date = in_zone!(z, t, |dt| dt.date_naive());
            localize(z, date.and_time(NaiveTime::MIN))
        })?,
    )?;

    time.set(
        "add",
        lua.create_function(|_, (t, by, z): (i64, Table, Value)| {
            let z = zone(&z)?;
            let get = |k: &str| int_field(&by, k).map(|v| v.unwrap_or(0));
            let (months, days) = (get("months")? + 12 * get("years")?, get("days")? + 7 * get("weeks")?);
            // Calendar steps on the wall clock in the zone, so "1 day" across a DST change is
            // still the same time of day; then exact seconds.
            let mut naive = in_zone!(z, t, |dt| dt.naive_local());
            naive = if months >= 0 { naive.checked_add_months(Months::new(months as u32)) } else { naive.checked_sub_months(Months::new((-months) as u32)) }
                .ok_or_else(|| mlua::Error::runtime("time.add: out of range"))?;
            naive = if days >= 0 { naive.checked_add_days(Days::new(days as u64)) } else { naive.checked_sub_days(Days::new((-days) as u64)) }
                .ok_or_else(|| mlua::Error::runtime("time.add: out of range"))?;
            let base = if months != 0 || days != 0 { localize(z, naive)? } else { t };
            Ok(base + get("hours")? * 3600 + get("minutes")? * 60 + get("seconds")?)
        })?,
    )?;

    time.set(
        "relative",
        lua.create_function(|_, (t, now): (i64, Option<i64>)| Ok(relative(t, now.unwrap_or_else(|| Utc::now().timestamp()))))?,
    )?;

    lua.globals().set("time", time)?;
    lua.load(HELPERS).set_name("=helpers").exec()
}

/// String and table helpers, in Lua.
const HELPERS: &str = r#"
function string.split(s, sep, plain)
  local out = {}
  if sep == nil then
    for w in s:gmatch("%S+") do out[#out + 1] = w end
    return out
  end
  local i = 1
  while true do
    local a, b = s:find(sep, i, plain)
    if not a or b < a then out[#out + 1] = s:sub(i); return out end
    out[#out + 1] = s:sub(i, a - 1)
    i = b + 1
  end
end
function string.trim(s) return (s:gsub("^%s+", ""):gsub("%s+$", "")) end
function string.starts(s, prefix) return s:sub(1, #prefix) == prefix end
function string.ends(s, suffix) return suffix == "" or s:sub(-#suffix) == suffix end
function table.map(t, f) local out = {} for i, v in ipairs(t) do out[i] = f(v, i) end return out end
function table.filter(t, f) local out = {} for i, v in ipairs(t) do if f(v, i) then out[#out + 1] = v end end return out end
function table.find(t, f) for i, v in ipairs(t) do if f(v, i) then return v, i end end return nil end
function table.keys(t)
  local out = {}
  for k in pairs(t) do out[#out + 1] = k end
  table.sort(out, function(a, b) return tostring(a) < tostring(b) end)
  return out
end
function table.slice(t, i, j)
  local out = {}
  for k = i or 1, j or #t do out[#out + 1] = t[k] end
  return out
end
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &str) -> String {
        let lua = Lua::new();
        install(&lua).unwrap();
        lua.load(src).eval::<String>().unwrap_or_else(|e| format!("error: {e}"))
    }

    #[test]
    fn parses_iso_and_formats_in_zones() {
        // Abu Dhabi 2026: Sunday 6 December, 13:00 UTC.
        assert_eq!(eval(r#"local t = time.parse("2026-12-06T13:00:00Z") return tostring(t)"#), "1796562000");
        assert_eq!(eval(r#"return time.format(time.parse("2026-12-06T13:00:00Z"), "%a %-d %b %H:%M %Z", "Asia/Dubai")"#), "Sun 6 Dec 17:00 +04");
        assert_eq!(eval(r#"return time.format(time.parse("2026-12-06T13:00:00Z"), "%-I:%M %p", "America/New_York")"#), "8:00 AM");
        assert_eq!(eval(r#"return time.format(time.parse("2026-12-06 13:00", "Europe/London"), "%H:%M", "UTC")"#), "13:00");
        assert_eq!(eval(r#"return time.format(time.parse("2026-07-06 13:00", "Europe/London"), "%H:%M", "UTC")"#), "12:00", "BST");
        assert_eq!(eval(r#"return time.format(time.parse("2026-12-06", "UTC"), "%H:%M %Y-%m-%d", 5.5)"#), "05:30 2026-12-06");
        assert_eq!(eval(r#"local t, e = time.parse("next tuesday") return tostring(t) .. " " .. e"#), "nil can't read \"next tuesday\" as a date or time");
    }

    #[test]
    fn parts_make_and_add_respect_dst() {
        assert_eq!(eval(r#"local p = time.parts(time.parse("2026-03-29T00:30:00Z"), "Europe/London") return p.hour .. " " .. p.zone .. " " .. p.weekday"#), "0 GMT 7");
        assert_eq!(eval(r#"local p = time.parts(time.parse("2026-03-29T01:30:00Z"), "Europe/London") return p.hour .. " " .. p.zone"#), "2 BST");
        // A day after noon the day before the clocks change is noon again, 23 hours later.
        assert_eq!(eval(r#"local t = time.make({ year = 2026, month = 3, day = 28, hour = 12 }, "Europe/London")
            local u = time.add(t, { days = 1 }, "Europe/London")
            return time.format(u, "%d %H:%M", "Europe/London") .. " " .. (u - t) // 3600"#), "29 12:00 23");
        // 01:30 doesn't happen in London on 29 March 2026: it is 02:30 BST instead.
        assert_eq!(eval(r#"return time.format(time.make({ year = 2026, month = 3, day = 29, hour = 1, min = 30 }, "Europe/London"), "%H:%M %Z", "Europe/London")"#), "02:30 BST");
        assert_eq!(eval(r#"local t = time.make({ year = 2026, month = 1, day = 31 }, "UTC") return time.format(time.add(t, { months = 1 }, "UTC"), "%Y-%m-%d", "UTC")"#), "2026-02-28");
        assert_eq!(eval(r#"local t = time.parse("2026-12-06T13:00:00Z") return time.format(time.start_of_day(t, "Asia/Tokyo"), "%Y-%m-%d %H:%M", "Asia/Tokyo")"#), "2026-12-06 00:00");
    }

    #[test]
    fn relative_and_errors() {
        assert_eq!(eval(r#"return time.relative(1000 + 3 * 86400, 1000) .. ", " .. time.relative(1000 - 7200, 1000) .. ", " .. time.relative(1010, 1000)"#), "in 3 days, 2 h ago, now");
        assert!(eval(r#"return time.format(0, "%H", "Mars/Olympus")"#).contains("unknown time zone"));
        assert!(eval(r#"return time.format(0, "%Q")"#).contains("bad time format"));
    }

    #[test]
    fn helpers() {
        assert_eq!(eval(r#"return table.concat(("a, b,,c"):split(","), "|")"#), "a| b||c");
        assert_eq!(eval(r#"return table.concat(("  two   words "):split(), "|")"#), "two|words");
        assert_eq!(eval(r#"return ("  x  "):trim() .. tostring(("abc"):starts("ab")) .. tostring(("abc"):ends("bc"))"#), "xtruetrue");
        assert_eq!(eval(r#"return table.concat(table.map(table.filter({1,2,3,4}, function(v) return v % 2 == 0 end), function(v) return v * 10 end), ",")"#), "20,40");
        assert_eq!(eval(r#"return table.concat(table.keys({b=1, a=2}), ",") .. table.concat(table.slice({1,2,3,4}, 2, 3), ",")"#), "a,b2,3");
    }
}
