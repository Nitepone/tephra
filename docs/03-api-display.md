# 03 - `GET /api/display`

The main loop of the protocol: every wake, the device asks what to show and how long to sleep.
Target firmware **v1.8.19**, checked 2026-11-16.

## Request

`GET {base}/api/display`, HTTP/1.1, no body, one TCP connection per request (`Connection: close`).

Transport behaviour (from `src/services/http_retry_request.cpp`, `lib/trmnl/include/http_client.h`):

- 15 s connect timeout and 15 s read timeout.
- The whole request is retried up to **5 attempts**, 2 s apart, but only for transport failures (no HTTP response), HTTP `408`, HTTP `5xx`, and bodies cut short. Other failures (e.g. 404) are **not** retried.
- Accepted HTTP status codes: **200**, **429** (body is processed exactly like a 200), and **301** (accepted, but a redirect body is useless, see below). Redirects `307` and `308` are followed **once** (relative `Location` is resolved against the scheme://host[:port] of the request URL; the API headers are re-sent only if the redirect target still starts with the configured base URL). `302`/`303` are *not* followed (they count as an invalid response code). Every other code (404, 401, 403, 204, 500...) is an error.
- `Accept-Encoding: identity` is requested: do not compress the response.
- HTTPS certificates are **not validated** (see [08](08-provisioning-and-transport.md#http-vs-https)).
- A body without `Content-Length` (chunked or close-delimited) is accepted. Body size limit: 90,000 bytes on boards without PSRAM, 750,000 bytes with PSRAM (applies to the JSON answer as well).

### Request headers

Built by `buildDisplayHeaders` (`lib/trmnl/src/api-client/request_headers.cpp`) and `createApiHeaders` (`src/api-client/header.cpp`),
sent in this order. "Cond." = only sent when the condition holds. The `HTTPClient` also adds `Host` (with `:port` if not 80/443), `User-Agent: ESP32HTTPClient`, `Connection: close`, and `Accept-Encoding`.

| Header | Value format | When | Example / notes |
|---|---|---|---|
| `ID` | Wi-Fi STA MAC, upper-case, colon separated | always | `A1:B2:C3:D4:E5:F6` |
| `Content-Type` | constant `application/json` | always | (sent even though the request has no body) |
| `Update-Source` | wake cause, see [01](01-overview-and-lifecycle.md#3-the-wake-cycle-step-by-step) | always | `timer`, `powercycle`, `button`, `EXT0` |
| `Access-Token` | the `api_key` from `/api/setup` | always; **empty value** if the device has no key | |
| `Refresh-Rate` | stored refresh interval in seconds, integer | always | `900` (300 if never set) |
| `Battery-Voltage` | decimal volts with 2 decimals (Arduino `String(float)`), measured before Wi-Fi/panel power-up | always | `4.10`. Boards that cannot measure it report a constant (`0.00` on some, `4.20` on others) |
| `Battery-Charging` | `1` charging, `0` not charging | cond.: board has a charger status line (TRMNL X, gen-2 OG) | |
| `USB-Connected` | `true` / `false` | cond.: board can detect VBUS (same boards) | |
| `Battery-Count` | integer | cond.: **TRMNL X only** | `0` (none detected), `1` or `2` |
| `Percent-Charged` | integer 0-100 (fuel-gauge state of charge) | X only | `-1` if the gauge gave no valid reading |
| `Battery-Health` | integer (state of health) | X only | `-1` if invalid |
| `Battery-Current` | integer (gauge current) | X only | `-1` if invalid |
| `Battery-Temp` | decimal degrees C (`String(float)`) | X only | |
| `Battery-Capacity` | `remaining/full` | X only | `1500/1800`; `-1/-1` in the fault case |
| `Gauge-SOC` | integer | X only (raw gauge state of charge, for comparison) | |
| `Gauge-Health` | integer | X only | |
| `Gauge-Capacity` | `remaining/full` | X only | |
| `FW-Version` | `MAJOR.MINOR.PATCH` | always | `1.8.19` |
| `FW-Commit` | short git hash of the build | cond.: non-empty (builds made with `scripts/extra/git_version.py`, e.g. the X) | `a14914c` |
| `Model` | board model string ([05](05-images-and-caching.md#models-and-panel-sizes)) | always | `og`, `x`, ... |
| `Panel-Rev` | e-paper controller revision, 8 lower-case hex digits | cond.: read successfully (boards with known SPI pins; 0 = unknown -> omitted) | `00000040` |
| `Panel-Vcom` | VCOM code programmed into the panel OTP (integer); `-1` = none programmed | cond.: only `Model: og` with an UltraChip-controller panel, once its OTP was read. **New in 1.8.19** | `-1`, `35` |
| `Image-Cached` | whether the previous wake's image came from the flash cache | always | `true` / `false` |
| `Wake-Time` | how long the **previous** cycle stayed awake, in **milliseconds** (integer) | always | `10520` (see [10](10-discrepancies-and-open-questions.md): OpenAPI says seconds) |
| `Reset-Reason` | ESP-IDF reset reason of **this** boot | always. **New in 1.8.19** | `ESP_RST_POWERON`, `ESP_RST_DEEPSLEEP`, `ESP_RST_PANIC`, `ESP_RST_SW`, `ESP_RST_BROWNOUT`, `ESP_RST_INT_WDT`, `ESP_RST_TASK_WDT`, `ESP_RST_WDT`, `ESP_RST_EXT`, `ESP_RST_SDIO`, `ESP_RST_UNKNOWN`; newer IDF reasons are sent as their decimal number |
| `RSSI` | Wi-Fi signal strength in dBm | always | `-58` (on the X's 5 GHz modem path: read from the modem) |
| `WiFi-Band` | `2.4` or `5` (channel >= 36 -> `5`) | cond.: non-empty | |
| `Temperature-Profile` | constant `true` | always | tells the server this firmware understands `temperature_profile` |
| `Width` | panel width in pixels | always | `800` |
| `Height` | panel height in pixels | always | `480` |
| `Log-Level` | `debug` if verbose logging is enabled, else `info` | always. **New in 1.8.19** | see `verbose_logging` below |
| `special_function` | `true`, or a function name | cond.: a special function is active, see [06](06-special-functions.md). Note the **underscore** in the header name | `true`, `identify`, `rewind` |
| `SENSORS` | environment sensor records, see below | cond.: a sensor was read (boards with an I2C sensor bus) | |

Never sent by v1.8.19: `Special-Function` (hyphen), `BASE64`, `Panel-Otp`, `Battery-Percent` (these appear in TRMNL's OpenAPI document or are used by other clients; see [10](10-discrepancies-and-open-questions.md)).

Image downloads (`image_url`) carry only `ID` and `Access-Token` (plus `Accept-Encoding: identity`), and only when the image URL starts with the configured base URL.

#### `SENSORS` header format

Zero or more records separated by `,`; each record is `key=value` pairs separated by `;` with the keys
`make`, `model`, `kind`, `value`, `unit`, `created_at` (epoch seconds of the sample). `kind` is one of
`carbon_dioxide` (`parts_per_million`), `temperature` (`celsius`, printed with `%f`), `humidity` (`percent`), `pressure` (`hectopascal`).
Source: `src/misc/sensor/environment_sensor.cpp`. Example (CO2 sensor SCD41 only):

```
SENSORS: make=Sensirion;model=SCD41;kind=carbon_dioxide;value=612;unit=parts_per_million;created_at=1787000000,make=Sensirion;model=SCD41;kind=temperature;value=23.400000;unit=celsius;created_at=1787000000,make=Sensirion;model=SCD41;kind=humidity;value=41;unit=percent;created_at=1787000000
```

### Example request (OG, timer wake)

```http
GET /api/display HTTP/1.1
Host: byos.local:4567
User-Agent: ESP32HTTPClient
Connection: close
Accept-Encoding: identity;q=1,chunked;q=0.1,*;q=0
Accept-Encoding: identity
ID: A1:B2:C3:D4:E5:F6
Content-Type: application/json
Update-Source: timer
Access-Token: xK3b9fQ2mZ7vLw1RaT4nYp
Refresh-Rate: 900
Battery-Voltage: 4.10
FW-Version: 1.8.19
Model: og
Panel-Rev: 00000040
Image-Cached: false
Wake-Time: 10520
Reset-Reason: ESP_RST_DEEPSLEEP
RSSI: -58
WiFi-Band: 2.4
Temperature-Profile: true
Width: 800
Height: 480
Log-Level: info
```

(Header order after the HTTP client's own lines is exactly the order of the builder. The duplicated `Accept-Encoding` is what the code produces: the Arduino core writes its own line and `HttpRetryRequest` adds another; **Unverified** on the wire.)

## Response

HTTP 200, `Content-Type: application/json`. Parsed by `parseResponse_apiDisplay` (`lib/trmnl/src/parse_response_api_display.cpp`) with ArduinoJson 7.4.2.
Invalid JSON is an error ("JSON parse failed") and counts as a failed request (see [09](09-errors-and-retries.md)); **no error screen** is shown for it.
Unknown fields are ignored. All fields are optional from the parser's point of view, but see the defaults: a missing `status` is `0` (success).

| Field | Type | Parsed as | Default | Semantics |
|---|---|---|---|---|
| `status` | integer | unsigned 64-bit | `0` | `0` = normal answer. `202` = device not claimed yet. `500` = reset the device. **Any other value (including `200`) is ignored**: nothing is shown, the refresh interval is not changed. See [Status handling](#status-handling). |
| `image_url` | string (absolute URL, `http://` or `https://`) | string | `""` | Where to download the image. Empty/null: no image, the screen is left as it is. Max 1023 characters (fixed buffer). |
| `filename` | string | string | `""` | Cache key and identity of the image. Special values: `empty_state`; names starting with `screen_wiper`. See [05](05-images-and-caching.md#caching-by-filename). |
| `refresh_rate` | integer seconds | unsigned 64-bit | `0` | Seconds to sleep until the next wake. Applied on `status: 0`. Missing/null/negative = `0` = wake immediately again. |
| `reset_firmware` | boolean | bool | `false` | `true`: wipe Wi-Fi settings and stored settings (including server URL and API key) and reboot into the portal. |
| `update_firmware` | boolean | bool | `false` | `true` together with a non-empty `firmware_url`: download and flash it. See [07](07-firmware-update-and-reset.md). |
| `firmware_url` | string (absolute URL) | string | `""` | Firmware `.bin` for **this** device model. Max 1023 characters. |
| `special_function` | string | one of `none`, `identify`, `sleep`, `add_wifi`, `restart_playlist`, `rewind`, `send_to_me`, `guest_mode`, `toggle_hold`; any other string (or absent) = `none` | `none` | What a double-click / the X's menu does. **Stored in flash on every normal answer**, so send the same value every time. See [06](06-special-functions.md). |
| `action` | string | string | `""` | Only meaningful when the request carried `special_function`; must echo the requested function ([06](06-special-functions.md)). |
| `maximum_compatibility` | boolean | bool | `false` | `true`: always use full panel refreshes (no partial/fast refresh) for images and message screens. Fixes display problems on some e-paper controllers at the cost of flicker/speed. |
| `temperature_profile` | string | `"a"` -> 1, `"b"` -> 2, anything else (`"default"`, `"c"`, absent) -> 0 | `0` | Selects a panel waveform/temperature profile; saved to flash when it changes, used for subsequent draws (SPI panels). Leave `default` unless images look washed out. |
| `image_url_timeout` | integer seconds | unsigned 32-bit | `0` | Read timeout for the image download (`0` = 15 s). Useful if the server renders the image on demand. Not in the official docs. |
| `verbose_logging` | string | `"debug"` enables serial/verbose logging on the device; any other non-empty string disables it; empty/absent = leave unchanged | `""` | Persisted in flash. Echoed back in the `Log-Level` request header. Only affects the device's serial output; **it does not change what logs are uploaded** (only errors and above are ever uploaded). New in 1.8.19. Not in the official docs. |

Type rules (ArduinoJson conversions, verified by compiling the pinned library 7.4.2 on a host; not run on a device):
- Booleans: JSON `true`/`false` behave as expected. A **number** works (`0` false, anything else true). A **string is always true** (`"false"`, `"0"` and `""` are all `true`) - never send booleans as strings.
- Integers: JSON numbers (a fraction is truncated; negative values for unsigned fields become `0`). A numeric **string** such as `"900"` is also parsed as a number, a non-numeric string is `0`.
- `null`, a missing key, or a wrong JSON type gives the default.

Fields the TRMNL cloud sends that the firmware does **not** read: `touchbar_mode` (X), `rendered_at`, `error`, `message`, ...

### Status handling

`handleApiDisplayResponse` (`src/display_session.cpp`) switches on the JSON `status`:

| `status` | Device behaviour |
|---|---|
| `0` (or missing) | Normal answer; see [What happens on a normal answer](#what-happens-on-a-normal-answer). |
| `202` | "Not registered/claimed yet": **no image is shown or fetched**. `refresh_rate` is ignored; the device sleeps per the *fast-poll* schedule (5 s, 5 s, ... see [01](01-overview-and-lifecycle.md#5-sleep-and-refresh-timing)). On a power-on/button wake (and only the first time per such wake) it draws the "Please visit trmnl.com/start with Friendly ID XXXXXX to finish setup" screen. The retry-failure counter is reset. |
| `500` | Treated as **reset**: wipes Wi-Fi settings and *all* stored settings, reboots into the Wi-Fi portal ([07](07-firmware-update-and-reset.md)). The cloud sends `{"status":500,"error":"Device not found","reset_firmware":true}` for an unknown access token. **A BYOS server must never send this by accident** (for example for a stale token); prefer HTTP 404 for "unknown device". |
| anything else (`200`, `404`, `429`...) | Silently ignored. No image change, **no refresh-interval change** (the device keeps its previously stored interval) and no error screen. |

The HTTP status is a separate layer: HTTP 200 (and 429, and 301) are parsed as above; any other HTTP code is a failed request and the JSON `status` is never read.

### What happens on a normal answer

For `status: 0` with no special function active (the usual case):

1. `special_function` is parsed and stored in flash (overwriting the previous one).
2. `reset_firmware` is remembered.
3. If `image_url` is non-empty:
   - **`filename == "empty_state"`**: the device treats this as "no plugin/playlist content yet". If it has not already recorded this state, it downloads and draws `image_url` and records it; otherwise it draws nothing. Either way the wake ends with the fast-poll schedule (5 s...), ignoring `refresh_rate`. The record is cleared by every non-timer wake and by every normal answer.
   - otherwise: the fast-poll streak is reset. `filename` is looked up in the flash cache. **Cached** -> redraw from flash without downloading (or do nothing if it is already on screen). **Not cached** -> download `image_url`, draw it, cache it.
4. `refresh_rate` is stored (unless step 3 chose fast-poll).
5. If `reset_firmware` is `true` the device resets after this wake's processing (nothing is downloaded or drawn).

If `image_url` is empty the screen is left untouched, but `refresh_rate` is still applied. This is a legitimate way to answer "nothing new".

After the answer: `filename` beginning with **`screen_wiper`** makes the device first clear the panel by flashing it black/white many times (about 60 full refreshes on mono panels, 2 on the BWRY OG, 100 cycles on the X; 2-3.5 minutes), then immediately request `/api/display` again (once per wake only) and show the next item. See [05](05-images-and-caching.md).

Then (in `showScreen`, `src/bl.cpp`) pending logs are uploaded, and `update_firmware` is handled ([07](07-firmware-update-and-reset.md)).

### Examples

Normal answer (new image):

```json
{
  "status": 0,
  "image_url": "http://byos.local:4567/images/plugin-3f2a9c-1787000000.png",
  "filename": "plugin-3f2a9c-1787000000",
  "refresh_rate": 900,
  "reset_firmware": false,
  "update_firmware": false,
  "firmware_url": "",
  "special_function": "identify",
  "maximum_compatibility": false,
  "temperature_profile": "default"
}
```

Same image as last time (device draws nothing and downloads nothing; note: only for PNG/JPEG, see [05](05-images-and-caching.md#caching-by-filename)): identical body, same `filename`.

"Nothing new, sleep an hour":

```json
{"status": 0, "image_url": null, "filename": null, "refresh_rate": 3600}
```

Device not claimed yet (HTTP 200):

```json
{"status": 202, "image_url": null, "filename": null, "refresh_rate": 900, "reset_firmware": false, "update_firmware": false, "firmware_url": "", "special_function": "identify", "maximum_compatibility": false, "temperature_profile": "default"}
```

(this is the body the TRMNL cloud sends, captured in `test/test_unclaimed_device_response/`).

Server asks the device to forget everything:

```json
{"status": 0, "reset_firmware": true}
```

or `{"status": 500, "error": "Device not found", "reset_firmware": true}`.

Firmware update:

```json
{"status": 0, "image_url": "http://byos.local:4567/images/plugin-3f2a9c-1787000000.png", "filename": "plugin-3f2a9c-1787000000", "refresh_rate": 900, "update_firmware": true, "firmware_url": "http://byos.local:4567/firmware/FW1.8.19-og.bin"}
```

Rate limiting (the device processes the body of a 429 like a 200, so this works as the cloud's "rate limited" screen):

```http
HTTP/1.1 429 Too Many Requests
Content-Type: application/json

{"status": 0, "image_url": "http://byos.local:4567/images/rate-limited-1787000000.png", "filename": "rate-limited-1787000000", "refresh_rate": 600}
```

Special-function request/response pairs are in [06](06-special-functions.md).

## Official documentation (copied)

Source: https://docs.trmnl.com/go/diy/byos (Markdown: https://docs.trmnl.com/go/diy/byos.md), section "Display".

> #### Display
>
> The device calls this on every wake. It also sends battery, WiFi + screen details as headers (`Battery-Voltage`, `RSSI`, `Width`, `Height`, `Refresh-Rate` and more); see [Screens](/go/private-api/screens.md) for the full list.
>
> ```bash
> curl "http://byos.local/api/display" \
>      -H 'ID: <device_mac_address>' \
>      -H 'Access-Token: <device_api_key>' \
>      -H 'FW-Version: 1.8.16' \
>      -H 'Content-Type: application/json'
> ```
>
> Example response:
>
> ```json
> {
>   "status": 0,
>   "image_url": "http://byos.local/images/screen-3f2a9c-1781785364.png",
>   "filename": "screen-3f2a9c-1781785364",
>   "refresh_rate": 900,
>   "reset_firmware": false,
>   "update_firmware": false,
>   "firmware_url": null,
>   "special_function": "identify",
>   "maximum_compatibility": false,
>   "temperature_profile": "default"
> }
> ```
>
> The firmware reads these fields:
>
> * `status` -- `0` draws `image_url`; `202` means "not registered yet" and the device polls again in a few seconds
> * `image_url` + `filename` -- see [Filenames](05-images-and-caching.md#caching-by-filename)
> * `refresh_rate` -- seconds to sleep before the next request. Send a number (`900`), not a string (`"900"`); the firmware reads a string as `0` and wakes again at once
> * `reset_firmware` -- `true` makes the device erase its WiFi credentials + API key
> * `update_firmware` + `firmware_url` -- `true` + a `.bin` URL triggers an OTA update. Firmware builds are per model, so only send a TRMNL X build to a TRMNL X
> * `special_function` -- what a button press does: `none`, `identify`, `sleep`, `add_wifi`, `restart_playlist`, `rewind`, `send_to_me` or `guest_mode`. This is a setting, so send the same value on every response
> * `maximum_compatibility` -- `true` fixes display issues on some ePaper driver chips but turns off fast refresh. Optional, defaults to `false`
> * `temperature_profile` -- `default`, `a` or `b`. Leave it at `default` unless images look washed out
>
> Our hosted server also sends `touchbar_mode` to a TRMNL X. Current firmware ignores it, so you can leave it out.

> **Corrections for v1.8.19:**
> - `status`: `0` draws the image, but also `202` (fast poll, no image) and **`500` (reset)**; every other value, including `200`, is ignored. See [Status handling](#status-handling). The doc does not mention `500`.
> - `refresh_rate`: sending a number is correct and recommended, but **the claim that a string is read as `0` is not what the code does**: the pinned ArduinoJson 7.4.2 parses a numeric string (`"900"` -> 900). Treat as "do not rely on it" ([10](10-discrepancies-and-open-questions.md)).
> - `reset_firmware`: erases more than "WiFi credentials + API key": the stored server URL and all other settings go too.
> - `special_function`: `toggle_hold` is also understood by 1.8.19. The same value should be sent every time because the device stores it on every normal answer.
> - Not documented there but read by the firmware: `image_url_timeout`, `verbose_logging`, `action`.
> - The example request lists only three headers; see the full table above.

Also from https://docs.trmnl.com/go/private-api/screens.md (hosted-server behaviour; informational for a BYOS author):

> After setup, each device gets 40 requests to `/api/display` per 5 minutes. Past that we answer with a "rate limited" system screen and a 5-15 minute `refresh_rate` instead of your content.
>
> A request with an unknown `access-token` answers `{"status": 500, "error": "Device not found", "reset_firmware": true}`. Our firmware reads that as "erase WiFi credentials + API key", so double check the key before pointing real hardware at it.
>
> **Note**: TRMNL devices send a few additional values in the request headers by default, such as your WiFi connection strength (`RSSI`), firmware version (`FW-Version`, ex: 1.7.1), battery voltage, screen `Width` + `Height`, and more. [...] These attributes impact the response content by instructing the device to either update firmware, change its refresh rate, and so on. Newer firmware versions also get extra fields like `maximum_compatibility` and `temperature_profile`.

Request-header descriptions from the OpenAPI document (https://trmnl.com/api-docs/openapi.yaml, `GET /api/display`), condensed:

| OpenAPI header | OpenAPI description | v1.8.19 firmware |
|---|---|---|
| `Battery-Voltage` | Device battery voltage (eg. 3.7) | sent |
| `Percent-Charged` | Device percent charged (eg. 69.4) | X only, integer |
| `Battery-Count` | Number of batteries in device (e.g. 1-2) | X only |
| `Battery-Charging` | Whether device is plugged into power (eg. true) | `1`/`0`, not `true` |
| `Battery-Health`, `Battery-Current`, `Battery-Temp`, `Battery-Capacity` | quality (-1-100), current, celsius, ratio `current/max` (`-1/-1` in fault case) | X only |
| `USB-Connected` | "true" or "false" | sent where known |
| `WiFi-Band` | "2.4" or "5" | sent |
| `Panel-Rev`, `Panel-Vcom` | panel identifier; VCOM_DC code (-1 when none) | sent where known |
| `Panel-Otp` | 98 hex digits from the panel's OTP | **not sent by 1.8.19** |
| `FW-Version`, `FW-Commit` | version (eg. 0.0.1); commit hash for the development channel | sent |
| `RSSI`, `Height`, `Width` | eg. -69, 480, 800 | sent |
| `Special-Function` | enum `true`, `identify`, `sleep`, `restart_playlist`, `rewind`, `send_to_me`, `guest_mode`, `refresh`, `toggle_hold` | firmware sends the header as **`special_function`** (underscore); `refresh` and `add_wifi` handling differs, see [06](06-special-functions.md) |
| `BASE64` | Encode image function | **not sent** |
| `Sensors` | Environmental sensor data | sent as `SENSORS` |
| `Image-Cached` | true / false | sent |
| `Wake-Time` | "Seconds the device was awake during the last cycle (eg. 12)" | sent in **milliseconds** |

OpenAPI response example for `/api/display` (`status: 200`, `special_function`, `action`, ...) differs from the firmware: see [10](10-discrepancies-and-open-questions.md).

## Sources

- `src/api-client/display.cpp` (request), `src/api-client/header.cpp` (header inputs, reset-reason names), `lib/trmnl/src/api-client/request_headers.cpp` (header list and order),
  `lib/trmnl/src/parse_response_api_display.cpp`, `lib/trmnl/include/api_types.h` (response parsing), `src/display_session.cpp` (`handleApiDisplayResponse`, `downloadAndShow`),
  `src/services/http_retry_request.cpp` (transport, retries, redirects), `lib/trmnl/src/http_utils.cpp` (`shouldFastRetry`), `src/misc/sensor/environment_sensor.cpp` (`SENSORS`),
  `src/epd_panel_rev.cpp` (`Panel-Rev`, `Panel-Vcom`), `src/request_handling.cpp`, `src/http_result_handling.cpp` - all at tag `v1.8.19`, e.g.
  https://github.com/usetrmnl/trmnl-firmware/blob/v1.8.19/src/display_session.cpp
- Unit tests: `test/test_parse_api_display/`, `test/test_request_headers/`, `test/test_unclaimed_device_response/`; integration specs `spec/spec/general/refresh/`, `faults_spec.rb`.
- ArduinoJson type conversions: checked by building a small host program against ArduinoJson tag `v7.4.2` (the version pinned in `platformio.ini`); **not executed on a device**.
- Official docs (copied above): https://docs.trmnl.com/go/diy/byos.md ; https://docs.trmnl.com/go/private-api/screens.md ; https://trmnl.com/api-docs/openapi.yaml
