# 09 - Errors, retries and what the device shows

Target firmware **v1.8.19**, checked 2026-11-16.

Principle: the device wants to stay quiet and cheap. On a **timer wake** it keeps showing the old picture and retries every 5 minutes; only after about an hour of consecutive failures does it draw an error screen. A **button or power-on wake** shows the error immediately.

## 1. Quiet-failure policy for `/api/display`

State: NVS counter `retry_count`, starts at 1. Code: `applyRetryPolicy` (`src/request_handling.cpp`), `requestFailureAction` (`src/http_result_handling.cpp`).

For each wake, after the `/api/display` exchange (including the image download) has produced a result:

| Result is a "failure"? | Wake type | Action |
|---|---|---|
| no | any | counter reset to 1 |
| yes | button / power-on (not a timer wake) | show the error screen now (if the error has one); counter unchanged; no retry is scheduled, so the device sleeps for the refresh interval it had stored before (the server's last `refresh_rate`) |
| yes | timer wake, counter < 12 | keep the current screen; sleep **300 s**; counter + 1 |
| yes | timer wake, counter >= 12 | show the error screen (if the error has one); sleep 300 s; counter + 1 |

So the first **11** consecutive failed timer wakes are silent; the **12th** (about 1 hour) shows the error. A single success starts over.

Results that are **not** failures (counter reset): success, no error, "not registered" (`status: 202`), reset requests (`status: 500` / `reset_firmware`), "plugin not attached" (`empty_state`), and an image download that got an HTTP error from the image host.
Everything else counts as a failure, including a bad JSON body, a bad image, and an oversized image.

## 2. What each error looks like

"Retries" = the 5-attempt loop in one wake (2 s apart): only for no HTTP response, HTTP 408, HTTP 5xx, and a download cut short.

| Condition | Internal result | Retried in the wake | Failure? | Screen (text shown under the logo) |
|---|---|---|---|---|
| DNS error, refused, reset, connect/read timeout talking to `/api/display` | `HTTPS_RESPONSE_CODE_INVALID` | yes | yes | "Wi-Fi connected, but / can't connect to the API. / Try refreshing, or scan / the QR code for help." (+ QR code), whatever the signal |
| HTTP 5xx / 408 from `/api/display` | same | yes | yes | same |
| HTTP 404, 401, 403, 204, 302 ... from `/api/display` | same | **no** | yes | same |
| HTTP 200 but empty body | `HTTPS_WRONG_IMAGE_SIZE` | no | yes | "Wi-Fi connected, but TRMNL content is malformed. / Wait, or hold the button on the back to reset." (weak-signal screen if RSSI <= -100) |
| HTTP 200, body not valid JSON | `HTTPS_JSON_PARSING_ERR` | no | yes | **none** |
| JSON body or image larger than `MAX_IMAGE_SIZE` (Content-Length, or growth beyond it) | `HTTPS_IMAGE_FILE_TOO_BIG` | no | yes | "The image file from this URL is too large. / <filename, cut to 40 chars> / PNG images must be at most / 90000 bytes and 1 or 2 bpp." (the byte count differs by build). **Drawn on every attempt**, even on quiet timer wakes |
| Download cut short, stalled, connection closed early | `HTTPS_TIMED_OUT` | yes | yes | "Image download timed out. Check your network connection." |
| `image_url` unusable (not `http(s)://`, client cannot be created) | `HTTPS_UNABLE_TO_CONNECT` | no | yes | "Wi-Fi connected, but can't reach the API." / button hint, or "Wi-Fi connected, but the signal is weak." if RSSI <= -100 |
| Image host answers with HTTP error (404, 500, ...) after retries | `HTTPS_IMAGE_DOWNLOAD_FAILED` | yes (5xx) | **no** | "API responded, but the image download failed. / Press the button on the back to advance to the next screen." |
| Out of memory allocating the body | `HTTPS_OUT_OF_MEMORY` | no | yes | none |
| Body served as `image/png` that fails the PNG signature/IHDR check | `HTTPS_WRONG_IMAGE_FORMAT` | no | yes | "The image format is not supported." File not cached; failure logged |
| Body is not PNG/JPEG and not a valid BMP | (none) | no | no | **nothing**: the old screen stays, nothing cached, nothing counted |
| JPEG of the wrong size | (none) | no | no | nothing sensible is drawn; no error screen |
| JSON `status` other than 0 / 202 / 500 | (ignored) | - | no | nothing changes |
| JSON `status` 202 | `HTTPS_NO_REGISTER` | - | no | "Please visit trmnl.com/start / with Friendly ID XXXXXX to finish setup" (once per power-on/button wake) |
| JSON `status` 500 or `reset_firmware` | `HTTPS_RESET` | - | no | none; erases settings and reboots |
| `empty_state` | `HTTPS_PLUGIN_NOT_ATTACHED` | - | no | the image, once; fast polling |

Weak-signal substitution: where the table says "weak-signal screen", the device uses "Wi-Fi connected, but the signal is weak." when the current RSSI is at or below **-100 dBm** (`WIFI_CONNECTION_RSSI`).

Other screens that exist in v1.8.19 (`src/message_screens.cpp`):

| Situation | Text |
|---|---|
| Wi-Fi connect failure (shown on button/power wakes and on the 12th consecutive failure) | "Can't connect to Wi-Fi. Will keep trying. / Hold the button on the back to reset Wi-Fi, or scan the QR code for help." (X: "Tap the middle of the touch bar for Settings > Reset Wi-Fi...") |
| Portal timeout | "Wi-Fi setup portal timed out. / Press the button to try again." |
| `/api/setup` request failed | "Wi-Fi connected, but /api/setup returned an error. / Press the button on the back to retry, / or check your internet connection." |
| Setup image problems / generic API content problems | see [02](02-api-setup.md#failure-handling) |
| Firmware update | "Firmware update available! Starting now...", "Firmware update complete. Device will restart...", "Firmware update failed. Will try again tomorrow.", "Wi-Fi connected, but the firmware update download failed." |
| Not registered on setup | the server's `message` / `image_url` ([02](02-api-setup.md)) |
| Friendly ID | "Please visit trmnl.com/start / with Friendly ID XXXXXX to finish setup" |
| Image download failed (API ok, image host not) | "API responded, but the image download failed. ..." |

All message screens are drawn over the TRMNL logo (or a brand logo stored at the top of flash), 1-bit, with a full refresh.

## 3. Wi-Fi connect failure

`wifiSessionConnect` -> `wifiErrorDeepSleep` (`src/wifi_session.cpp`, `src/sleep_session.cpp`):

1. Up to 3 attempts per saved network (waits 2 s, 4 s between attempts), the last used network first, then a scan and the other saved ones.
2. On failure: on a button/power wake the Wi-Fi failure screen is shown right away. An error is logged (it can only be uploaded on a later wake).
3. NVS counter `wifi_retry` (starts at 1, reset to 1 on every successful connection): sleep **300 s** while the count is below 12; from the 12th consecutive failure show the Wi-Fi failure screen, sleep **900 s** and restart the count at 1. Otherwise the count is incremented.
4. A failed connect also happens when the saved SSID/password is wrong; the user must hold the button 5 s to re-enter them.

## 4. Special cases worth designing for

- **Server down for a long time:** the device shows its last picture for ~1 hour, then the "can't connect to the API" screen, then keeps retrying every 5 minutes. When the server returns, the next successful answer redraws normally.
- **Server answers HTTP 404 for an unknown token:** shows "can't connect to the API", not a reset. A 404 is not retried within the wake.
- **Slow image rendering:** a server that renders on demand and answers the *image* request slowly should set `image_url_timeout` (seconds) or pre-render, because the default read timeout is 15 s and a cut-off download is retried up to 5 times.
- **Per-wake time:** all work (Wi-Fi, TLS, NTP, up to 5 retries x 15 s, panel refresh) happens while the device is awake and on battery; a 20-minute awake limit (non-X) reboots the device. Keep answers fast.

## Sources

- `src/request_handling.cpp`, `src/http_result_handling.cpp` (`requestIsFailure`, `requestFailureAction`, `requestErrorScreen`), `src/bl.cpp` (`showScreen` order),
  `src/sleep_session.cpp` (`wifiErrorDeepSleep`, `AWAKE_TIME_LIMIT_MS`), `src/wifi_session.cpp`, `lib/trmnl/src/refresh_interval.cpp`, `lib/trmnl/include/refresh_interval.h`, `lib/trmnl/include/https_request_err.h`,
  `lib/trmnl/src/http_utils.cpp` (`shouldFastRetry`), `src/services/http_retry_request.cpp`, `src/message_screens.cpp` (texts), `src/status_screens.cpp` - tag `v1.8.19`,
  e.g. https://github.com/usetrmnl/trmnl-firmware/blob/v1.8.19/src/http_result_handling.cpp
- Tests (behavioural confirmation): `spec/spec/general/errors_spec.rb` (`ErrorScreens`, `Retries`), `spec/spec/general/faults_spec.rb`, `test/test_http_result_handling/`, `test/test_refresh_interval/`.
- Firmware README flowchart (copied in [README](README.md)): "Sleep with backoff (error shown on button wake or 12th failed wake)".
- Note: the comment in `lib/trmnl/include/refresh_interval.h` says the Wi-Fi ladder is "60 / 180, then 300" and the API ladder "15 / 30 / 60, then ..."; the **code** uses 300 s then 900 s (Wi-Fi) and a fixed 300 s (API), which is what is documented here.
