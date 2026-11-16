# 02 - `GET /api/setup`

Registration: the device trades its MAC address for an API key and a friendly ID.
Target firmware **v1.8.19**, checked 2026-11-16.

## When the device calls it

Only when the flash does not contain **both** `api_key` and `friendly_id` (`ensureDeviceSetUp`, `src/setup_session.cpp`):
after the first Wi-Fi provisioning, after a reset/wipe, after the server URL was changed in the portal, or
after a previous setup that was aborted before saving. It is called once per wake until it succeeds, at which
point the credentials are stored and `/api/setup` is never called again (until a reset).

It is a plain `GET` with no body. There is **no** `Access-Token`: the server must identify the device by the `ID` header alone.

## Request headers

From `buildSetupHeaders` (`lib/trmnl/src/api-client/request_headers.cpp`). Sent in this order, in addition to the HTTP-client
defaults `Host`, `User-Agent: ESP32HTTPClient` and `Connection: close` (HTTP/1.1):

| Header | Value | Required by firmware to send | Example |
|---|---|---|---|
| `ID` | Wi-Fi station MAC, **upper-case hex, colon separated** (`%02X:%02X:...`) | always | `A1:B2:C3:D4:E5:F6` |
| `Content-Type` | constant | always | `application/json` |
| `FW-Version` | firmware version `MAJOR.MINOR.PATCH` | always | `1.8.19` |
| `Model` | board model string, see the table in [05](05-images-and-caching.md#models-and-panel-sizes) | always | `og` |
| `Panel-Rev` | e-paper controller revision as 8 lower-case hex digits; **omitted** when unknown | optional | `00000040` |
| `Width` | panel width in pixels (decimal) | always | `800` |
| `Height` | panel height in pixels (decimal) | always | `480` |
| `RSSI` | Wi-Fi signal strength in dBm, negative integer | always | `-61` |

Notes:
- On the TRMNL X the `ID` is the MAC of the main ESP32-S3 station interface (the value shown in the portal's `mac` field), not the 5 GHz modem's MAC (`mac_5ghz`, shown by the portal but not sent as `ID`).
- `device_mac_address()` (`src/device_id.cpp`) reads the Wi-Fi STA MAC from eFuse, so it is stable and available before Wi-Fi comes up.

## Response

The firmware accepts the answer in two shapes, and treats them differently:

| HTTP status | What the firmware does |
|---|---|
| **200** | Parses the body as JSON (`parseResponse_apiSetup`). The JSON field `status` then decides (see below). |
| anything else (404, 403, 500, ...) | Does **not** read the body. Treats the HTTP status code as the status. 404 = "MAC not registered", every other code = failed setup. |
| transport error / timeout | Failed setup ("request error"). |

So a "MAC not registered" answer may be either HTTP 404 or HTTP 200 with `"status": 404` in the body. The difference matters:
the `message` and `image_url` fields of an unregistered answer are used **only** in the second form (HTTP 200 body with `status: 404`).
With a real HTTP 404 the device shows its generic text ("...MAC address not registered...") with no server message and no server image.

### Response JSON fields (HTTP 200)

| Field | Type | Meaning / how it is parsed | Default if missing or null |
|---|---|---|---|
| `status` | integer | `200` = success. `404` = MAC not registered. Anything else = failed setup. Parsed with `as<int>()`, so a numeric string works too. | `0` -> failed setup |
| `api_key` | string | Stored verbatim in flash and sent as `Access-Token` on every later request. Read only when `status == 200`. | `""` (an empty key would be stored) |
| `friendly_id` | string | Stored in flash; shown on the "Please visit trmnl.com/start with Friendly ID ..." screen; used in the DHCP/mDNS hostname (`TRMNL-OG-<friendly_id>`). Read only when `status == 200`. | `""` |
| `image_url` | string or null | URL of the "setup logo". Empty/null: no download, the friendly-ID screen is shown immediately. Read for `status` 200 and 404. | `""` |
| `message` | string | Shown **only** on the "not registered" screen (`status` 404). Ignored for 200. | `""` |

Other fields are ignored (the cloud also sends `filename: null`, which is not read).

### Success path (`status: 200`)

1. `api_key` and `friendly_id` are written to flash (before the image is downloaded).
2. If `image_url` is empty: show the friendly-ID screen; done.
3. Otherwise download `image_url` with a plain GET (15 s connect and read timeouts, one redirect `307`/`308` followed; relative `Location` resolved against the image URL's origin; **no** auth headers; **no** retries). It is accepted if:
   - its body is **exactly 48,062 bytes** (the 800x480 1-bit BMP: 62-byte header + 48,000 bytes of pixels) - saved as `/logo.bmp`; **or**
   - it was received completely (`Content-Length` honoured) and begins with the PNG signature (`89 50 4E 47`) - saved as `/logo.png`.
   Content-Type is not checked. A response without `Content-Length` (chunked) is rejected (the size is read from `Content-Length`).
4. Any other body, an HTTP error from the image host, or a connection failure shows an error screen (see below) but the credentials from step 1 **stay saved**.
5. The device then continues the wake normally (logs, then `/api/display`). The downloaded setup logo itself is not drawn after a successful setup; the friendly-ID screen is drawn over the built-in logo (or a brand logo in flash).

The saved `/logo.bmp` / `/logo.png` is drawn only for a later "not registered" answer that carries an `image_url` (see below).
Note that these files have no timestamp suffix, so the next image-cache purge (24 h rule, [05](05-images-and-caching.md#caching-by-filename)) deletes them.

### "MAC not registered" path (`status: 404`)

- If `image_url` is non-empty it is downloaded exactly like the setup logo and, if valid and saved, **drawn full-screen instead of the message**. On parallel-panel (TRMNL X class) boards a downloaded `.bmp` is *not* drawn (their display library cannot draw an uncompressed BMP), so only PNG works there (`showMacNotRegistered`, `src/setup_session.cpp`).
- Otherwise the built-in logo is drawn with `message` (word-wrapped, centred) below it. When no JSON body is available (real HTTP 404) the message is empty.
- The registered flag is cleared, the refresh interval is set to 300 s, and the device **goes to deep sleep**. It does not call `/api/display`. Next wake it asks `/api/setup` again.

### Failure handling

| Failure | Screen | Then |
|---|---|---|
| Cannot create the HTTP client (malformed base URL, ...) | "Wi-Fi connected, but can't connect to the API." | continues to `/api/display` (empty `Access-Token`) |
| Transport failure: DNS error, connection refused/reset, timeout (HTTP client returns a negative code) | "Wi-Fi connected, but /api/setup returned an error." (or the weak-signal screen if RSSI <= -100); an error log is queued | continues to `/api/display` |
| HTTP 200 but the body is not valid JSON | none | continues to `/api/display` |
| HTTP status other than 200/404 (e.g. 500, 403, 503, 301) **or** JSON `status` other than 200/404 | none (no screen, no log) | continues to `/api/display` |
| Setup image: connection or HTTP error | "Wi-Fi connected, but the API couldn't deliver the image." (weak-signal variant if RSSI <= -100) | continues, credentials already saved |
| Setup image: body is neither a 48,062-byte BMP nor a complete PNG | "Wi-Fi connected, but TRMNL content is malformed." (weak-signal variant) | continues, credentials already saved |

**Important consequence:** after most setup failures the device goes on to call `/api/display` with an *empty*
`Access-Token`. If your server answers an unknown token with JSON `status: 500` (as the TRMNL cloud does:
`{"status": 500, "error": "Device not found", "reset_firmware": true}`) the device erases its Wi-Fi and server settings and reboots into the portal.
A server that answers an unknown token with HTTP 404 (LaraPaper, Terminus) only produces the "can't connect to the API" screen.
See [03](03-api-display.md#status-handling) and [07](07-firmware-update-and-reset.md).

## Not followed: redirects

`/api/setup` is fetched with a bare `HTTPClient::GET()`. HTTP redirects are **not** followed for this request (unlike `/api/display`, images and `/api/log`),
so a proxy that redirects `http://` to `https://` or adds a trailing slash breaks registration: the device sees a non-200 status
and reports "/api/setup returned an error". Serve `/api/setup` directly on the configured base URL.

## Example

Request:

```http
GET /api/setup HTTP/1.1
Host: byos.local:4567
User-Agent: ESP32HTTPClient
Connection: close
ID: A1:B2:C3:D4:E5:F6
Content-Type: application/json
FW-Version: 1.8.19
Model: og
Panel-Rev: 00000040
Width: 800
Height: 480
RSSI: -61
```

Registered (HTTP 200):

```json
{
  "status": 200,
  "api_key": "xK3b9fQ2mZ7vLw1RaT4nYp",
  "friendly_id": "4D4CF0",
  "image_url": "http://byos.local:4567/images/setup-logo.bmp",
  "message": "Welcome to my server"
}
```

Unregistered with message shown on the device (HTTP 200, status inside body):

```json
{
  "status": 404,
  "api_key": null,
  "friendly_id": null,
  "image_url": null,
  "message": "MAC A1:B2:C3:D4:E5:F6 is not registered. Add it in the admin UI."
}
```

Unregistered via HTTP status (body ignored):

```http
HTTP/1.1 404 Not Found
Content-Type: application/json

{"status":404,"message":"MAC Address not registered"}
```

## Official documentation (copied)

Source: https://docs.trmnl.com/go/diy/byos (Markdown: https://docs.trmnl.com/go/diy/byos.md), section "Setup".

> #### Setup
>
> The device calls this once, after WiFi setup, to trade its MAC address for an API key.
>
> ```shell
> curl "http://byos.local/api/setup" \
>     -H 'ID: <device_mac_address>' \
>     -H 'FW-Version: 1.8.16' \
>     -H 'Model: og' \
>     -H 'Content-Type: application/json'
> ```
>
> The firmware reads `status`, `api_key`, `friendly_id`, `image_url` + `message`, and stores `api_key` for every request after this one. Any `status` other than `200` counts as a failed setup.
>
> Example response:
>
> ```json
> {
>   "status": 200,
>   "api_key": "<device_api_key>",
>   "friendly_id": "ABC123",
>   "image_url": "http://byos.local/images/setup-logo.bmp",
>   "message": "Register at byos.local with Device ID 'ABC123'"
> }
> ```

> **Corrections for v1.8.19:**
> - The example request omits `Width`, `Height`, `RSSI` and (when known) `Panel-Rev`, which the firmware also sends.
> - "Any `status` other than `200` counts as a failed setup" is incomplete: `status: 404` (and HTTP 404) means "MAC not registered", which is handled differently (message/image, sleep). Other values are a failed setup.
> - `message` is read from the response but is only *displayed* for the `404` case; the success screen text is hard-coded.
> - "stores `api_key` for every request after this one": true; it is sent as `Access-Token`. The `friendly_id` is stored too.

Also relevant, from the OpenAPI document https://trmnl.com/api-docs/openapi.yaml (`/api/setup`, `security: []`):

> Please note that the returned `status` JSON value may NOT always equal the HTTP status code. Notably, if a device MAC address is not found, then the HTTP status code will be 200 but the `status` code in the response will be 404.

(The v1.8.19 firmware handles both that form and a real HTTP 404, as described above.)

Official BYOD note (https://docs.trmnl.com/go/diy/byod.md): "Our server matches your device by the MAC address it sends in the `ID` header of `/api/setup`. If setup answers "MAC not registered," double check step 3."

## Sources

- `src/api-client/setup.cpp` (request, HTTP/JSON handling), `lib/trmnl/src/parse_response_api_setup.cpp` (JSON parsing),
  `lib/trmnl/src/api-client/request_headers.cpp` (`buildSetupHeaders`), `src/services/device_setup.cpp` (flow, image download, outcomes),
  `include/services/device_setup.h`, `src/setup_session.cpp` (screens/sleep), `src/message_screens.cpp` (screen text), `src/device_id.cpp` (MAC format)
  - tag `v1.8.19`, e.g. https://github.com/usetrmnl/trmnl-firmware/blob/v1.8.19/src/services/device_setup.cpp
- `test/test_parse_api_setup/` and `spec/spec/general/errors_spec.rb` (`SetupErrors`) at tag `v1.8.19` (behavioural confirmation).
- Official BYOS guide (copied above): https://docs.trmnl.com/go/diy/byos.md
- OpenAPI: https://trmnl.com/api-docs/openapi.yaml
- Cross-check (not authoritative): `byos_laravel` `app/Http/Controllers/Api/Firmware/SetupController.php`, `byos_hanami` `app/actions/api/setup/show.rb`, `byos_next` `app/api/setup/route.ts` (see [11](11-implementation-cross-check.md)).
- **Unverified:** the TRMNL X 5 GHz-modem variant of setup (`src/services/device_setup_x.cpp`) was not read in full; it routes the same requests through the ESP32-C5 modem.
