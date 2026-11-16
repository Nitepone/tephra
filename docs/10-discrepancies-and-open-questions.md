# 10 - Discrepancies between documentation and code, and open questions

Target firmware **v1.8.19**, checked 2026-11-16. Rule used throughout: **the v1.8.19 source code wins**; the official statement is quoted so the difference is visible.

## A. Official documentation vs. firmware code

| # | Official statement (source) | What v1.8.19 does | Effect on a server |
|---|---|---|---|
| 1 | "Send a number (`900`), not a string (`"900"`); the firmware reads a string as `0` and wakes again at once" (BYOS page, https://docs.trmnl.com/go/diy/byos.md) | `refresh_rate` is read with ArduinoJson 7.4.2 `doc["refresh_rate"]` into a `uint64_t`; a numeric string **is converted** (`"900"` -> 900); only non-numeric strings give 0. Verified by compiling ArduinoJson `v7.4.2` on a host and running the exact conversion; not run on a device. | Still send numbers; a string probably works. Treat the doc statement as unconfirmed for this version. |
| 2 | `/api/display` OpenAPI response example has `"status": 200` (https://trmnl.com/api-docs/openapi.yaml) | The device acts only on `status` **0**, **202**, **500**. `200` falls into the "ignore" branch: nothing is shown and the refresh interval is not updated. A *missing* `status` counts as 0. | Send `0` (or omit it, as Terminus does). Never send `200` for a normal screen. |
| 3 | BYOS page "`status` -- `0` draws `image_url`; `202` means "not registered yet"" | Also `500` = reset (wipes Wi-Fi + settings). Not mentioned on that page (only the Display API page mentions it for unknown tokens). | Do not use `500` for ordinary errors. |
| 4 | BYOS page "Any `status` other than `200` counts as a failed setup" (`/api/setup`) | `404` (HTTP or JSON) = "MAC not registered" with message/image and **sleep**; other values = failed setup that nevertheless continues to `/api/display`. Non-200 HTTP codes are not parsed for a body. | Use 404 for unknown MAC; see [02](02-api-setup.md). |
| 5 | OpenAPI `/api/setup`: "if a device MAC address is not found, then the HTTP status code will be 200 but the `status` code in the response will be 404" | Handled, but so is a real HTTP 404; the `message`/`image_url` are used only for the HTTP-200 form. LaraPaper and Terminus answer HTTP 404. | Prefer HTTP 200 + JSON `status: 404` if you want the device to display your message/image. |
| 6 | BYOS page lists `special_function` values `none, identify, sleep, add_wifi, restart_playlist, rewind, send_to_me, guest_mode` | Also `toggle_hold` (new in 1.8.19). OpenAPI also lists `refresh`, which the firmware does not know. | Unknown names are treated as `none`. |
| 7 | OpenAPI request header `Special-Function` | The firmware sends **`special_function`** (underscore). | Accept both; check proxies that drop underscores. |
| 8 | OpenAPI `Wake-Time`: "Seconds the device was awake during the last cycle (eg. 12)" | Value is in **milliseconds** (`iPrevWakeTime = millis() - startup_time`). | Divide by 1000 for display. |
| 9 | OpenAPI `Battery-Charging`: "Whether device is plugged into power (eg. true)", type number | Sends `1` / `0` and only on boards with a charger status line; `USB-Connected` sends `true`/`false`. | Parse both. |
| 10 | OpenAPI lists `Panel-Otp`, `BASE64`, `Percent-Charged` etc. | v1.8.19 does **not** send `Panel-Otp` or `BASE64` (they are for newer/other clients). `Percent-Charged` is X-only. | Treat as optional. |
| 11 | OpenAPI `/api/log` response `204` | Firmware accepts 200, 204 and 301 as success. | Either works. |
| 12 | Hosted docs use `https://trmnl.com/...` throughout | The firmware's default base URL is **`https://trmnl.app`** (`API_BASE_URL`). | Matters only if a device is ever left unconfigured. |
| 13 | BYOS page: `reset_firmware` "makes the device erase its WiFi credentials + API key" | Also erases the stored server URL and every other setting. | After a reset the user must re-enter the server URL in the portal. |
| 14 | BYOS page Filenames: "When a response names a file it already has, it shows that copy and downloads nothing." | True for PNG and JPEG only. A BMP is never stored under its filename and is re-downloaded and redrawn every wake. (Code-derived.) | Use PNG when you rely on the cache. |
| 15 | BYOS page Images: "It reads any other response as a BMP." | Correct, but an invalid BMP (including a PNG sent with a wrong `Content-Type`) is dropped **silently**; there is no error screen and the failure is not counted. | Serve exact `image/png` / `image/jpeg`. |
| 16 | BYOD/S page (https://docs.trmnl.com/go/diy/byod-s.md) says to mimic `api/setup` and `api/display` | The firmware also calls `POST /api/log`. The README says "three endpoints". | Implement `/api/log` (or at least answer 200/204). |
| 17 | On-device message and docs mention "PNG images must be at most 90000 bytes and 1 or 2 bpp" | The decoders accept 1/2/4/8-bit gray, palette and RGB PNGs and reduce them. The 90,000-byte cap applies only to non-PSRAM builds (750,000 otherwise). | Follow the ImageMagick guide per model anyway. |
| 18 | BYOS page shows `FW-Version: 1.8.16` in the example; "Our hosted server also sends `touchbar_mode`" | Consistent with the code: `touchbar_mode` is not read. `image_url_timeout`, `verbose_logging`, `action` **are** read but not documented there. | Optional extras. |
| 19 | `include/refresh_interval.h` comment: Wi-Fi retry "60 / 180, then 300", API retry "15 / 30 / 60, then ..." | Code: Wi-Fi 300 s (then 900 s from the 12th failure), API fixed 300 s. | The code is what the device does. |
| 20 | Spec comment (`spec/spec/general/network/http_spec.rb`): setup logo redirect to a **relative** Location fails | The v1.8.19 code (`DeviceSetup::downloadSetupImage`) resolves a relative Location against the image URL's origin. The test comment is stale. | Redirects on the setup image work, one hop. |
| 21 | `https://trmnl.com/api/models` lists `seeed_e1004` as 1600x1200; BYOD page the same | The firmware's own test profile for `reterminal_e1004` is 1200x1600. Real-hardware value not verified. | Use the `Width`/`Height` headers. |
| 22 | TRMNL's OTA channel (`https://trmnl.com/api/firmware`) shows `latest: 1.8.18` for all models on 2026-11-16 | GitHub's latest release is 1.8.19. | Informational. |

## B. Behaviours that are surprising and worth knowing (not documentation errors)

- An unknown or empty `Access-Token` plus a server that answers `status: 500` (the TRMNL cloud, and byos_next's "Device not found") makes the device wipe its Wi-Fi *and server URL*.
- After a **failed** `/api/setup` the device still calls `/api/display` with an empty token.
- `/api/setup` never follows redirects; `/api/display` and image downloads follow one 307/308.
- `refresh_rate` is ignored on `status: 202`, on `empty_state`, and in most special-function answers.
- `update_firmware: true` with no version comparison and no failure timestamp on success can make the device re-flash on every wake.
- `Content-Type` of images is compared exactly (`image/png`).
- Only the first 7 + last 17 characters of a long `filename` identify the cached file.
- The BMP path is validated strictly (800x480, 1-bit, 48,000 data bytes, exact black/white palette), and the pixel data is always read from offset 62.
- The setup logo is validated (BMP exactly 48,062 bytes, or complete PNG) but not displayed after a successful setup; the friendly-ID text is hard-coded and ignores `message`.

## C. Open questions / **Unverified**

These could not be settled from the v1.8.19 source or the public documentation alone. Each needs a real device or the closed-source TRMNL server.

1. What the device actually shows when `refresh_rate` is `0` (the code arms a 0-second timer; the docs say "wakes again at once"). Not measured on hardware.
2. Images **smaller** than the panel on mono SPI panels (OG family): the decoder writes only the lines it receives; behaviour of the remaining controller memory is unspecified here. Always send exact size.
3. Portrait-oriented PNGs (width == panel height): implemented in the X-class and Spectra-6 paths, **not found** in the mono SPI path although a test named "portrait png is rotated" exists (it only asserts a non-empty screenshot).
4. JPEG support per panel: the firmware's own simulator tests mark JPEG as "pending" for UC81xx-controller boards (screen stays blank after a JPEG); only OG and X are confirmed.
5. Black/white/red (3-colour) panels: how the red plane is produced from a PNG; `color-7a` (7-colour) palette support.
6. TLS details: TLS 1.3, SNI, maximum certificate chain size, ALPN. The OG uses ESP-IDF 4.4-era mbedTLS (TLS 1.2); the X environments use a newer platform.
7. Resolution of `.local` (mDNS) hostnames by the HTTP client, although the official examples use `byos.local`.
8. What happens when a `sleep` / `add_wifi` special-function answer carries `filename: null` (the code maps it to the path `/`).
9. The TRMNL cloud's exact answer to a `rewind` request (the code accepts `action: "guest_mode"` with an image) and to `toggle_hold`.
10. TRMNL X 5 GHz modem path (`src/services/device_setup_x.cpp`, modem branch of `http_retry_request.cpp`): read only partially; assumed to have the same protocol semantics; known differences are that BMPs are not drawable on X-class boards and that RSSI/MAC come from the modem.
11. Whether the Arduino core on the X environments (pioarduino platform 55.03.37) writes the same default HTTP headers as 2.0.17 (verified only for 2.0.17).
12. Duplicate `Accept-Encoding` request header (the core adds one; `HttpRetryRequest` adds `identity`): code-derived, not captured on the wire.

## Sources

- All firmware citations are to tag `v1.8.19` of https://github.com/usetrmnl/trmnl-firmware (files named in the pages above).
- Official documents quoted: https://docs.trmnl.com/go/diy/byos.md , https://docs.trmnl.com/go/diy/byod-s.md , https://docs.trmnl.com/go/private-api/screens.md , https://trmnl.com/api-docs/openapi.yaml , https://trmnl.com/api/models , https://trmnl.com/api/firmware (all fetched 2026-11-16).
- ArduinoJson 7.4.2 conversion check: built on a host from https://github.com/bblanchon/ArduinoJson tag `v7.4.2`, same `JsonDocument`/implicit-conversion calls as `parse_response_api_display.cpp`.
- Firmware tests used as evidence: `test/test_parse_api_display/`, `test/test_unclaimed_device_response/`, `spec/spec/general/*`.
