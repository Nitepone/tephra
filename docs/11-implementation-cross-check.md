# 11 - Cross-check against existing server implementations, and a conformance checklist

Target firmware **v1.8.19**, checked 2026-11-16. The implementations below were used **only to confirm behaviour**; the firmware source is the authority.
They were read from fresh clones on 2026-11-16:

| Implementation | Repository | Version read |
|---|---|---|
| LaraPaper (the user's current server) | https://github.com/usetrmnl/byos_laravel | `0.47.0-3-g113fcab` (commit `113fcabf7ab76b70d97aba9f7d4ae92e2fb90324`) |
| Terminus | https://github.com/usetrmnl/byos_hanami | `0.82.0-6-g448c6be7` (commit `448c6be76f263b5413e77235e7564405523ef3d5`) |
| BYOS Next | https://github.com/usetrmnl/byos_next | `v0.2.15-24-g2511a20` (commit `2511a207b51c3134e027126dd891a82e0e99d103`, 2026-06-30, the oldest of the three) |

The `trmnl-api` Ruby gem (https://github.com/usetrmnl/trmnl-api, `lib/trmnl/api/models/display.rb`) was consulted for Terminus' response shape.

## What each implementation does at the protocol level

### LaraPaper (`routes/api.php`, `app/Http/Controllers/Api/Firmware/*`)

| Topic | Behaviour (code-read) | Compatible with firmware 1.8.19? |
|---|---|---|
| Endpoints | `GET /api/setup` (throttle 10/min/IP), `GET /api/display`, `POST /api/log`, `GET /api/current_screen` (throttle 60/min/IP, shared). | Yes. `/api/current_screen` is not called by the device. |
| `/api/setup` | Uses `ID`; unknown MAC (or a known one that has already been activated and whose owner has no open "auto-join" window) -> **HTTP 404** `{"status":404,"message":"MAC Address not registered or invalid access token"}`; otherwise `{"status":200,"api_key","friendly_id","image_url" (setup-logo or null),"message":"Welcome to TRMNL BYOS"}`. | Yes. HTTP 404 means the device shows its generic "not registered" text without the server's message (body ignored). Note that after a device has been activated (`last_refreshed_at` set by `/api/setup` or `/api/display`), `/api/setup` for that MAC answers 404 again unless the owner's auto-join window is open (`ResolveDeviceByMacAddress`): a device that was factory-reset cannot re-register on its own. |
| `/api/display` auth | `Access-Token` header -> device; unknown token -> **HTTP 404** `{"message": "MAC Address not registered (or not set), or invalid access token"}`. Can auto-create a device when an auto-join window is open. | Yes: the device shows "can't connect to the API", **does not reset**. |
| `/api/display` body | `status: 0`, `image_url`, `filename` (`<prefix:7><id:6>-<unix time:10>` such as `plugin-a9a4d9-1785312389`, 25 chars, time = image mtime), `refresh_rate`, `reset_firmware: false` (always), `update_firmware`, `firmware_url`, `special_function` (setting, default `sleep`), `maximum_compatibility`, and `image_url_timeout` (default 30 s) when configured. Resets `update_firmware` after sending. | Yes, and the filename scheme matches the firmware's purge rules (see [05](05-images-and-caching.md#caching-by-filename)). |
| Request headers read | `rssi`, `battery-percent` / `percent-charged` / `battery_voltage` (sic), `fw-version`, `battery-charging`, `usb-connected`, `sensors`, `id`, `access-token`, `model-id` (setup; the firmware sends `Model`, not `Model-Id`). | `Model` is not read by LaraPaper's setup (it looks at `model-id`), so the device model is not auto-detected from the firmware's header. Observation only. |
| Special functions | The request header `special_function` is **not read anywhere** (grep over `app/` finds only the stored setting). Answers carry no `action`. | The answer to a double click has no `action`, so the device logs "`<function>` failed" and ignores the answer (for the default `sleep` setting too): the special functions are stored on the device but never executed. |
| `/api/log` | Accepts `logs` array (or `log.logs_array`), stores each entry; looks for `creation_timestamp` (the firmware sends `created_at`, so the timestamp falls back to "now"). Answers HTTP 200 `{"status":200}`. | Yes. |
| Other | Per-IP throttling: 60 requests per minute across all devices behind one IP, which matters with many devices or fast polling (5 s polls while unclaimed). Laravel's 429 page has a non-JSON body, which the device treats as bad JSON (silent failure, retries in 5 min). | Observation. |

### Terminus (`config/routes.rb`, `app/actions/api/{setup,display,log}/*`, `trmnl-api` gem)

| Topic | Behaviour | Compatible? |
|---|---|---|
| Endpoints | `GET /api/setup`, `GET /api/display`, `POST /api/log`, plus management APIs. | Yes |
| `/api/setup` | Validates firmware headers against a schema (invalid -> 422 problem+json), creates the device from `Model`, `ID`, `FW-Version`; success body is `{"api_key","image_url":".../assets/setup.bmp","message":"Welcome to Terminus!","status":200}`; failures are problem-details with HTTP 404/422. | Yes (HTTP 404/422 are "failed setup" for the device; 404 = "MAC not registered"). |
| `/api/display` | Looks up the device by token; unknown token -> HTTP 404 problem+json. Body fields from the `trmnl-api` `Display` model: `filename`, `image_url`, `image_url_timeout`, `refresh_rate`, `firmware_url`, `update_firmware`, `reset_firmware`, `maximum_compatibility`, `temperature_profile`, `touchbar_mode`, `action`, ... and **no `status` field** (the firmware defaults a missing `status` to `0`, so this works) and no `special_function`. The `action` echoes the `special_function` request header (or `Update-Source`). | Yes. A missing `status` is fine; a missing `special_function` stores `none`. |
| Special functions | `event = HTTP_SPECIAL_FUNCTION \|\| HTTP_UPDATE_SOURCE`; handles `identify`, `restart_playlist`, `send_to_me`, `guest_mode`, screen navigation and wipe (`filename: screen_wiper.png`); it treats `Update-Source` values `button` / `EXT0` as a button wake when no `special_function` header is present. The `action` in its answer is the same `event` string. | Rack maps both `special_function` and `Special-Function` to `HTTP_SPECIAL_FUNCTION`. **Unverified:** whether it answers a 1.8.19 double click (`special_function: true`) with the `action` the device expects (the code compares the header value, `true`, with function names). |
| `/api/log` | Strict schema: requires `battery_voltage`, `created_at`, `firmware_version`, `free_heap_size`, `id`, `max_alloc_size`, `message`, `refresh_rate`, `sleep_duration`, `source_line`, `source_path`, `special_function`, `wake_reason`, `wifi_signal`, `wifi_status`; optional `level`, `retry`. Returns **204**. Invalid payload -> 422. | Matches the field list in [04](04-api-log.md) exactly. A 422 makes the device keep and resend the logs forever. |

### BYOS Next (`app/api/{setup,display,log}/route.ts`)

| Topic | Behaviour | Compatible? |
|---|---|---|
| `/api/setup` | Requires `ID` and `Model` (HTTP 400 otherwise, `status: 400` in body), may answer 403/503; success `status: 200` with `api_key`, `friendly_id`, `image_url`, `message`. | Yes |
| `/api/display` | Returns **HTTP 200 with JSON `status: 500`** for "Device not found" (`buildErrorResponse(..., status = 500)`), with an error image URL. **That is the device's reset trigger**: the firmware erases Wi-Fi + server URL on `status: 500`. A missing `Access-Token` gives JSON `status: 401` (ignored by the firmware). Success: `status: 0`, `image_url`, `filename`, `refresh_rate`, `reset_firmware: false`, `update_firmware`, `firmware_url`, `special_function: "restart_playlist"` (hard-coded). Claim codes are sent as an image with `status: 0`. | **Hazard:** a stale/unknown token resets the device. Version read is the oldest of the three; behaviour may have changed. |

## Conformance checklist for a new server

Behaviours the firmware requires or that bite when ignored. Each links to the page with the evidence.

**Endpoints**
- [ ] `GET /api/setup`, `GET /api/display`, `POST /api/log` under one base URL, no redirects on `/api/setup`. ([02](02-api-setup.md), [08](08-provisioning-and-transport.md))
- [ ] Case-insensitive header handling; accept `special_function` with underscore. ([06](06-special-functions.md))
- [ ] Identify devices by `ID` (upper-case `AA:BB:CC:DD:EE:FF`) at setup and by `Access-Token` afterwards.

**`/api/setup`**
- [ ] 200 + JSON `status: 200` with `api_key` and `friendly_id` (non-empty); optional `image_url` (48,062-byte BMP or complete PNG, `Content-Length` present).
- [ ] Unknown MAC: HTTP 200 + `status: 404` + `message` (displayed), or HTTP 404 (generic text).

**`/api/display`**
- [ ] `status` is `0` (or absent) for normal answers; `202` for "unclaimed"; **never** `200`; **never** `500` unless you want a factory reset.
- [ ] `refresh_rate` is a JSON number of seconds, always present on `status: 0` answers.
- [ ] Unknown token: HTTP 404, not `status: 500`.
- [ ] `filename` changes iff the image changes; <= 31 characters; `<7 chars><6 chars>-<10-digit unix time>` form. Special names: `empty_state`, `screen_wiper*`.
- [ ] `image_url` absolute, `http://`/`https://`, <= 1023 chars; image served with exact `Content-Type: image/png` (or `image/jpeg`), correct `Content-Length`, no compression, within the size limit (90,000 / 750,000 bytes), at exactly `Width` x `Height`.
- [ ] Per-model image encodings ([05](05-images-and-caching.md)): 1-bit/2-bit gray PNG for the OG, 4-bit gray PNG for the X, palette PNG for colour panels; BMP only 800x480 1-bit with the exact palette.
- [ ] Booleans are JSON booleans (a string `"false"` reads as true).
- [ ] `special_function` setting sent identically on every normal answer; special-function requests answered with the matching `action`.
- [ ] `update_firmware` only when needed (compare `FW-Version`); clear after the device reports the new version; `firmware_url` direct (no redirect), per model.
- [ ] Respect the sleep schedule: do not return `refresh_rate: 0`.
- [ ] Optional: `image_url_timeout`, `maximum_compatibility`, `temperature_profile`, `verbose_logging`.

**`/api/log`**
- [ ] Accept `{"logs":[...]}` with the 16/17 fields of [04](04-api-log.md); answer 200 or 204; do not reject unknown or missing fields.

**Operations**
- [ ] Per-IP throttles must allow several devices behind one NAT and fast polling (5 s) of unclaimed devices.
- [ ] If HTTPS: any certificate works (not verified), TLS 1.2 suites; keep `http` working for `/api/setup` unless you are sure about redirects.
- [ ] Allow NTP from the device, or set a local NTP server in the portal. ([08](08-provisioning-and-transport.md#time-ntp))
- [ ] Keep response times low; the device is awake on battery and has a 20-minute awake limit.

## Sources

- LaraPaper: `routes/api.php`, `app/Http/Controllers/Api/Firmware/{SetupController,DisplayController,DeviceLogController,CurrentScreenController}.php`, `app/Actions/Api/{ResolveDeviceByApiKey,ResolveDeviceByMacAddress,UpdateDeviceTelemetry,RunDeviceDisplayCycle}.php`,
  `app/Services/DeviceScreenFilename.php`, `app/Http/Requests/Api/StoreDeviceLogRequest.php`, `app/Providers/AppServiceProvider.php`, `config/services.php`.
- Terminus: `config/routes.rb`, `app/actions/api/{setup/show,display/show,log/create}.rb`, `app/aspects/firmware/models/setup.rb`, `app/aspects/screens/interrupter.rb`, `app/aspects/firmware/headers/parser.rb`;
  https://github.com/usetrmnl/trmnl-api (`lib/trmnl/api/models/display.rb`).
- BYOS Next: `app/api/setup/route.ts`, `app/api/display/route.ts`, `app/api/display/utils.ts`, `app/api/log/route.ts` (the log route was not read in detail).
- Firmware behaviour per the other pages of this documentation.
