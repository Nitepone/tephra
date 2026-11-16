# 01 - Overview and device lifecycle

Target: firmware **v1.8.19** (checked 2026-11-16). See [README](README.md) for conventions.

## 1. Every network request the firmware makes

Confirmed by searching the whole v1.8.19 source tree for URL construction. The firmware has exactly **three**
server API endpoints, all relative to one configurable base URL, plus a few downloads whose URLs the server hands out.

| # | Method + path | When | Auth headers sent | Page |
|---|---|---|---|---|
| 1 | `GET {base}/api/setup` | Only when the device has no stored API key / friendly ID (first boot after the Wi-Fi portal, or after a reset) | `ID` only (no `Access-Token`) | [02](02-api-setup.md) |
| 2 | `GET {base}/api/display` | Every wake | `ID`, `Access-Token` | [03](03-api-display.md) |
| 3 | `POST {base}/api/log` | When stored error logs exist: right after setup, after the display fetch, just before sleep | `ID`, `Access-Token` | [04](04-api-log.md) |
| 4 | `GET {image_url}` | When `/api/display` returns an image that is not already cached on the device | `ID` + `Access-Token` **only if** `image_url` starts with the base URL (string prefix); otherwise none | [05](05-images-and-caching.md) |
| 5 | `GET {image_url}` from `/api/setup` | Once during setup (and for a "not registered" screen) | none | [02](02-api-setup.md) |
| 6 | `GET {firmware_url}` | When `/api/display` says `update_firmware: true`, at most once per 24 h after a failure | none | [07](07-firmware-update-and-reset.md) |
| 7 | NTP (UDP 123) to `time.google.com`, fallback `pool.ntp.org` (or a custom server) | Each wake, but skipped if the last sync was < 24 h ago | n/a | [08](08-provisioning-and-transport.md#time-ntp) |

Not called by the firmware (confirmed absent from the v1.8.19 source): `/api/current_screen`, `/api/display/current`,
any firmware-check or "latest version" endpoint, any heartbeat/ping endpoint, any endpoint that registers or
claims a device other than `/api/setup`. `/api/current_screen` and `/api/display/current` exist on the TRMNL cloud
and in some BYOS servers for companion apps (e.g. the Chrome extension); they are not part of the device protocol.

The default base URL, used when none has been configured, is `https://trmnl.app` (`include/config.h`,
`API_BASE_URL`). The protocol is **device-initiated only**: the server never contacts the device.

## 2. Request/response shapes at a glance

| Endpoint | Success | Notable non-success the firmware treats specially |
|---|---|---|
| `/api/setup` | HTTP 200 with JSON `status: 200`, `api_key`, `friendly_id`, `image_url`, `message` | HTTP 404, or HTTP 200 with JSON `status: 404`: "MAC not registered" |
| `/api/display` | HTTP 200 with JSON `status: 0` (or absent) and `image_url`, `filename`, `refresh_rate`, ... | JSON `status: 202` (not claimed yet), JSON `status: 500` (reset), `reset_firmware: true`; HTTP 429 body is processed like 200 |
| `/api/log` | HTTP 200 or 204 | anything else keeps the logs for later |

## 3. The wake cycle, step by step

Source: `src/bl.cpp` (`bl_init`, `showScreen`), `src/wake_init.cpp`, `src/wifi_session.cpp`, `src/setup_session.cpp`,
`src/display_session.cpp`, `src/sleep_session.cpp`, `src/request_handling.cpp`.

The device spends nearly all its time in ESP32 deep sleep. Every wake runs `bl_init()` from the top; no RAM
survives (apart from a few RTC variables), so all state that matters is in flash (NVS "preferences" and a
small filesystem).

A wake has one of these causes, which the firmware reports in the `Update-Source` request header:

| Cause | `Update-Source` value | Notes |
|---|---|---|
| Power on / reset / first boot | `powercycle` | shows the TRMNL logo ("wake logo") immediately |
| Sleep timer expired | `timer` | no wake logo; errors are kept quiet (see [09](09-errors-and-retries.md)) |
| Button (ESP32-C3/C5 GPIO wake) | `button` | wake logo; errors shown at once |
| Button (ESP32-S3 EXT0 wake) | `EXT0` | same |
| other | `EXT1`, `touchpad`, `ulp`, `uart`, `wifi`, `cocpu`, `cocpu_trap_trig`, `bt`, `all`; `unknown` if unmapped | |

Sequence (`bl_init` in `src/bl.cpp`):

1. **Early init.** Starts a 20-minute watchdog timer on non-X boards: if the device is still awake after
   20 minutes it deliberately panics and reboots (`AWAKE_TIME_LIMIT_MS`, `src/sleep_session.cpp`). The reason is reported on the next boot as `Reset-Reason: ESP_RST_PANIC`.
2. **Read the button** if the wake was a button/GPIO wake (non-X boards):
   - press > 1 s and <= 5 s, or two short presses within 800 ms: **double click** -> run the stored special function ([06](06-special-functions.md));
   - hold > 5 s and < 15 s: **forget Wi-Fi** (credentials erased, portal will start);
   - hold >= 15 s: **soft reset**: erase Wi-Fi credentials *and* all stored settings including the server URL, API key and friendly ID, then reboot;
   - single short press: plain wake.
   (`src/button.cpp`, `include/config.h`: `BUTTON_HOLD_TIME 5000`, `BUTTON_MEDIUM_HOLD_TIME 1000`, `BUTTON_SOFT_RESET_TIME 15000`, `BUTTON_DOUBLE_CLICK_WINDOW 800`.)
3. **Battery voltage** is measured *before* the display and Wi-Fi are powered, and reported later in `Battery-Voltage`.
4. **Display and filesystem init.** If the wake was not a timer wake, the TRMNL logo ("loading" screen) is drawn.
5. **Wi-Fi.** If credentials are saved: connect (up to 3 attempts per network with 2 s / 4 s back-off; the last used network first, then the other saved networks). If none are saved: start the captive portal ([08](08-provisioning-and-transport.md#captive-portal)). On connect failure: sleep (see [09](09-errors-and-retries.md#3-wi-fi-connect-failure)).
6. **Clock.** NTP sync if the last one is older than 24 h. Failure is tolerated.
7. **Setup.** If **both** `api_key` and `friendly_id` are present in flash, skip. Otherwise call `GET /api/setup` ([02](02-api-setup.md)).
8. **Send stored logs** (`POST /api/log`) if any ([04](04-api-log.md)).
9. **Display.** `GET /api/display` ([03](03-api-display.md)). Depending on the answer: download + draw an image, or redraw a cached one, or leave the screen alone.
10. **Post-processing**, in this order (`showScreen` in `src/bl.cpp`):
    1. if the image was too big: show the "too large" message;
    2. apply the retry policy (may go straight to sleep on a quiet failure);
    3. send stored logs again;
    4. if the server said "not registered" (`status: 202`): show the friendly-ID screen;
    5. if the server asked for a reset: wipe Wi-Fi + settings and reboot;
    6. if `update_firmware` is due: download and flash the firmware, reboot ([07](07-firmware-update-and-reset.md));
    7. otherwise show an error screen if there was an error.
11. **TRMNL X only:** stay awake for touch-bar taps (each tap triggers another `/api/display` request); otherwise
12. **Sleep** for the stored refresh interval, with the button armed as a wake source.

## 4. First boot and registration

1. **No Wi-Fi saved -> captive portal.** The device shows "Connect your phone or computer to "TRMNL-XXXXXX" Wi-Fi" and opens an open access point.
   The user picks the home network and may enter a **custom server URL** (this is how a BYOS server is configured). Full detail in [08](08-provisioning-and-transport.md#captive-portal).
2. **Connected, no API key -> `GET /api/setup`** with the device MAC in `ID`. The server answers with an
   `api_key` (the device's future `Access-Token`) and a `friendly_id`. The device stores both in flash.
   If the portal's server URL differs from the previously stored one, the stored `api_key` and `friendly_id` are discarded first, so the device registers again.
3. **Setup image.** If `image_url` is non-empty the device downloads it, checks it, stores it (as `/logo.bmp` or `/logo.png`), then draws "Please visit trmnl.com/start with Friendly ID XXXXXX to finish setup" over the built-in logo.
   The wording is hard-coded in the firmware; `message` from the server is **not** shown on that screen.
4. **First `GET /api/display`.** A device that the server does not consider "claimed" yet gets JSON `status: 202`; the device shows the friendly-ID screen and polls again quickly (5 s, see below). When the server has content it answers `status: 0` with an image.

If `/api/setup` answers "MAC not registered" (HTTP 404 or JSON `status: 404`), the device shows the server's
`message` (or the server's image if `image_url` is present), **goes to sleep** and, on the next wake, tries `/api/setup` again.

## 5. Sleep and refresh timing

The sleep duration is always the **stored refresh interval** (NVS key `refresh_rate`, seconds, default 300 if never set;
`lib/trmnl/src/refresh_interval.cpp`). It is overwritten by:

| Event | New interval | Source |
|---|---|---|
| `/api/display` answer with `status: 0` | the response's `refresh_rate` (numeric; absent/null/0/negative -> 0, i.e. wake again immediately; a numeric string such as `"900"` is accepted, see [10](10-discrepancies-and-open-questions.md)) | `applyServerRate` |
| `status: 202` (not claimed), `status: 500` (reset), or `status: 0` with `filename: "empty_state"` | "fast poll": 5 s for the first 50 consecutive such answers, then 60 s for 10, then 900 s for 10, then 3600 s forever. The streak is reset by the first normal image answer. The response's `refresh_rate` is **ignored** in these cases. | `applyFastPoll` |
| `/api/display` failed (transport error, 5xx, bad JSON, bad image...) | 300 s (fixed) | `applyApiRetry` -> `applyDefault` |
| Wi-Fi connect failed | 300 s for the first 11 failures, 900 s from the 12th | `applyWifiRetry` |
| `/api/setup` answered "not registered" | 300 s | `applyDefault` |
| Firmware update failed | 30 s (just long enough to replace the failure message), after which the next wake behaves normally | `applyFailedUpdate` |

Other rules:
- The device sends its current stored value as `Refresh-Rate` on the next request.
- There is no lower or upper clamp on the server's value. The official hosted server uses 5 min to 24 h (docs: https://docs.trmnl.com/go/how-it-works.md).
- The timer is armed together with the button wake, so a button press always wakes the device early.
- Panel refresh mode is also affected by the interval: if `refresh_rate >= 1800` s (30 min) a would-be partial refresh is made a "fast" refresh to limit ghosting (`choose_refresh_mode`, `src/display.cpp`). A full refresh is forced every 8th update, or always when `maximum_compatibility: true`.

## 6. What survives power loss, and what a reset removes

Stored in flash (NVS namespace `data`, plus `wificaptive`): `api_key`, `friendly_id`, `api_url` (server base URL), saved Wi-Fi networks (up to 5),
`refresh_rate`, stored special function (`sf`), temperature profile, retry counters, error logs (10 slots), last OTA attempt time, and so on.
Images are cached on the small flash filesystem (see [05](05-images-and-caching.md#caching-by-filename)).

| Trigger | Wipes |
|---|---|
| Hold button 5-15 s | Saved Wi-Fi networks only |
| Hold button >= 15 s, or portal "soft reset" | Wi-Fi networks **and** the whole `data` namespace: API key, friendly ID, server URL, counters... then reboot into the portal |
| `/api/display` `status: 500` or `reset_firmware: true` | the same as the 15 s hold |
| Server URL changed in the portal | only `api_key` and `friendly_id` |

## Sources

- `src/bl.cpp`, `src/wake_init.cpp`, `src/wifi_session.cpp`, `src/setup_session.cpp`, `src/sleep_session.cpp`,
  `src/request_handling.cpp`, `src/display_session.cpp`, `src/button.cpp`, `include/config.h`,
  `lib/trmnl/src/refresh_interval.cpp`, `lib/trmnl/src/special_function.cpp`, `lib/trmnl/src/logging_parsers.cpp` (wake reason names),
  `lib/wificaptive/src/WifiCaptive.cpp` (`saveApiServer`, `resetSettings`), `src/misc/clock/clock.cpp` - all at tag `v1.8.19`,
  e.g. https://github.com/usetrmnl/trmnl-firmware/blob/v1.8.19/src/bl.cpp
- Firmware README wake-cycle flowchart: https://github.com/usetrmnl/trmnl-firmware/blob/v1.8.19/README.md
- Official architecture overview: https://docs.trmnl.com/go/how-it-works.md
- Firmware integration test suite (behavioural confirmation of retry/quiet-failure rules):
  `spec/spec/general/errors_spec.rb`, `spec/spec/general/refresh/special_functions_spec.rb` at tag `v1.8.19`.
