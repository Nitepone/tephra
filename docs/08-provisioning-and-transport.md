# 08 - Provisioning, server URL, and transport (HTTP/HTTPS, redirects, timeouts, time)

Target firmware **v1.8.19**, checked 2026-11-16.

## Captive portal

Started when no Wi-Fi network is saved (first boot, after a reset, after a 5 s button hold), or on an `add_wifi` special function ([06](06-special-functions.md)).
Code: `lib/wificaptive/src/WifiCaptive.cpp`, `WebServer.cpp`, `portal/index.html`.

| Item | Value |
|---|---|
| Access point SSID | `TRMNL-` + 6 upper-case hex digits taken from bytes of the eFuse MAC (e.g. `TRMNL-4D4CF0`), stable per device |
| Security | **open** (no password), channel 6, max 4 clients |
| Network | AP IP `4.3.2.1`, mask `255.255.255.0`; wildcard DNS (TTL 3600) answers every name with `4.3.2.1`; unknown URLs redirect to `http://4.3.2.1` (captive-portal detection URLs for Android/Apple/Windows/Firefox are handled) |
| Portal HTTP server | port 80 on `4.3.2.1` |
| Timeout | **15 minutes**. Then (non-X) "Wi-Fi setup portal timed out. Press the button to try again." and deep sleep; on the X the device enters shipping sleep. |
| Screen while waiting | "Connect your phone or computer to "TRMNL-XXXXXX" Wi-Fi", "or scan the QR code for help" |
| Wi-Fi bands | 2.4 GHz everywhere; 5 GHz on the TRMNL X (via the ESP32-C5 modem) and ESP32-C5 boards |

### Portal HTTP interface

Useful for automation or for a custom provisioning tool. All on `http://4.3.2.1`.

| Request | Purpose |
|---|---|
| `GET /` | the single-page UI (gzip-compressed HTML) |
| `GET /scan` (`?force` to rescan) | `202` while the scan runs; then `200` `{"networks":[{"name":"...","rssi":"-61","open":false,"saved":false,"band":"2.4GHz","enterprise":false}, ...],"mac":"A1:B2:C3:D4:E5:F6"}` (`mac_5ghz` added on the X). `rssi` is a **string**. The `mac` is the value to register on a BYOS server. |
| `POST /connect` (JSON) | Start connecting. Body fields: `ssid`, `pswd`, `server` (see below), `band` (`"2.4GHz"`, `"5GHz"` or `""`), `isEnterprise` (bool), `identity`, `username` (WPA2/3-Enterprise), `useStaticIP` (bool), `staticIP`, `gateway`, `subnet`, `dns1`, `dns2`, `ntpServer1` (custom NTP host, stored as `ntp_server`), `hostname` (custom DHCP hostname). Answers `200 {"ssid":"...","mac":"..."}` (plus `mac_5ghz`). |
| `GET /connection-status` | `{"status":"idle"|"connecting"|"connected"|"authentication_failed"|"network_not_found"|"failed"}` |
| `GET /device-settings` | `{"api_url":"<stored server URL or empty>","verbose_logging":false}` |
| `POST /verbose-logging` | `{"enabled":true}` toggles serial logging |
| `GET /soft-reset` (any method) | erases Wi-Fi and all stored settings (same as the 15 s hold) |
| `GET /run-test` | sensor self-test JSON |

After a successful connection the credentials are saved (up to 5 networks are remembered; the last used is tried first) and **then** the server URL is saved (`saveApiServer`).
If the connection attempt fails on a 2.4 GHz network the device restarts so the portal comes back.

## How the base server URL is configured

- The server URL is the **"Server" field in the portal's "Advanced" section** (`server` in the `/connect` body; hint in the UI: "Point your TRMNL at your own server (BYOS)").
- It is saved in flash (NVS namespace `data`, key `api_url`). If the field is empty, the key is removed and the compiled default **`https://trmnl.app`** applies (`API_BASE_URL`, `include/config.h`).
- The portal page validates it in JavaScript: it must parse as a URL and start with `http://` or `https://`; whitespace is trimmed and trailing `/` characters are stripped. The firmware itself does no validation, so a hand-crafted `/connect` call can store anything.
- Every endpoint is `base + "/api/setup"`, `base + "/api/display"`, `base + "/api/log"`. A path prefix is therefore fine (`https://example.com/trmnl`). **No trailing slash.** Include a non-default port as `:4567`.
- Length: keep the URL well under ~190 characters. `/api/log` builds the URL in a 200-byte stack buffer (`char new_url[200]` in `src/api-client/submit_log.cpp`), so a longer base URL overflows it. (Code-derived; effect on a device **Unverified**.)
- **Changing the server later** means going through the portal again (hold the button 5-15 s to forget Wi-Fi, then re-enter Wi-Fi and the new URL). When the saved server differs from the new one, the stored `api_key` and `friendly_id` are deleted so the device registers again with `/api/setup`. The URL is shown on the TRMNL X's menu Diagnostics screen (line "Server").
- A server-initiated reset (`reset_firmware`, `status: 500`) deletes the URL as well ([07](07-firmware-update-and-reset.md)).
- The URL given in `image_url` / `firmware_url` is independent and can point anywhere; but see the auth-header rule in [05](05-images-and-caching.md#download-behaviour).

## HTTP vs HTTPS

- Both schemes work for all API endpoints and downloads. The scheme is chosen by looking for `https://` in the URL (`withHttp`, `lib/trmnl/include/http_client.h`): HTTPS uses `WiFiClientSecure` with **`setInsecure()`: the server certificate is not verified at all** (no CA store, no hostname check, no expiry check). Self-signed certificates work.
- Because nothing is verified, HTTPS only protects against passive eavesdropping, not against an active impostor. Do not treat it as authentication of the server.
- The access token is sent as a request header on every call; over plain HTTP it is visible to anyone on the path. Most BYOS servers run plain HTTP on the LAN (the official docs say so).
- TLS: the firmware's own HTTPS test server is configured for **TLS 1.2 only, ECDHE-ECDSA-AES256-GCM-SHA384 with a P-384 certificate** (`spec/lib/trmnl_sim/mock_trmnl.rb`). The TLS library is mbedTLS from the Arduino-ESP32 2.0.17 / ESP-IDF 4.4 stack for the OG (X environments use a newer pioarduino platform). A server that offers TLS 1.2 with a common ECDHE/RSA or ECDSA suite should work; **Unverified:** TLS 1.3-only servers, SNI behaviour, and certificate chain size limits.
- Only for the exact prefix `https://trmnl.app` the device uses a special client that resumes the TLS session across deep sleep (`ResumableWiFiClientSecure`); this does not apply to your server.
- The device speaks HTTP/1.1 only, one request per TCP connection (`Connection: close`). A TLS-terminating reverse proxy in front of the server is the normal way to add HTTPS.

Official documentation (copied), https://docs.trmnl.com/go/diy/byos.md, "Securing With HTTPS":

> Most BYOS implementations serve up content over plain HTTP. If you prefer TLS connections, you can put a reverse proxy in front of the server. Caddy is a popular option. You can use self-signed certificates, or real ones from Let's Encrypt. See [this community issue](https://github.com/usetrmnl/trmnl-firmware/issues/58#issue-2826999580) for details on how to set it up yourself.

Official documentation (copied), https://docs.trmnl.com/go/diy/byod-s.md, "Server quickstart" steps:

> 2. Change the base URL to your own server or local network from the WiFi Captive Portal
> 3. Mimic the `api/setup` and `api/display` endpoints per our [firmware README](https://github.com/usetrmnl/trmnl-firmware#web-server-endpoints) + the [BYOS API minimum](/go/diy/byos.md#api)
> 4. Follow our [ImageMagick guide](/go/diy/imagemagick-guide.md) to create TRMNL firmware compatible images

> **Correction (v1.8.19):** the firmware also calls `POST /api/log`; the README sentence lists three endpoints, the byod-s steps list two.

## Redirects, timeouts and retries

| Request | Redirects | Timeouts | Retries |
|---|---|---|---|
| `GET /api/setup` | **none followed** | 15 s connect + 15 s read | none within the wake |
| `GET /api/display` | `307`, `308` once (relative `Location` allowed, resolved against `scheme://host[:port]`) | 15 s + 15 s | 5 attempts, 2 s apart, on transport error / 408 / 5xx / short body |
| `GET image_url` | same as `/api/display` | 15 s (or `image_url_timeout`), 30 s inactivity | same |
| setup-image `GET` | `307`, `308` once | 15 s | none |
| `POST /api/log` | `307`, `308` once | 15 s | none |
| `GET firmware_url` | **none** | default (HTTPClient) | none |

Accepted response codes per request are given on each endpoint page. `301`/`302` are never followed (`301` is accepted as "OK" by the display/image fetch, which then fails on the empty/HTML body).
HTTPClient redirect-following is disabled in the Arduino core by default (`HTTPC_DISABLE_FOLLOW_REDIRECTS`); the firmware implements the 307/308 hop itself.

## Wire-level facts a server should know

- HTTP/1.1 requests with `Host: host[:port]` (the port only if not 80/443), `User-Agent: ESP32HTTPClient`, `Connection: close` (verified in Arduino-ESP32 2.0.17 `HTTPClient.cpp`; the X environments use a newer core: **Unverified** there).
- Request header names are sent in the casing shown in these pages. One of them contains an underscore (`special_function`).
- `/api/display` and image downloads ask for `Accept-Encoding: identity`; do not gzip. Chunked responses are fine.
- Responses should have a correct `Content-Length` when possible (it lets the device size its buffer and detect truncation).
- The device is a constrained client: avoid response bodies larger than the limits in [05](05-images-and-caching.md), avoid slow responses (render images ahead of time; or set `image_url_timeout`).
- All endpoints are called by IP/hostname resolved with the DHCP-provided DNS. Hostnames ending in `.local` (mDNS) are used in TRMNL's own documentation examples (`byos.local`); the firmware starts an mDNS *responder* (`ESPmDNS`, hostname `TRMNL-OG-<friendly_id>`) but whether it *resolves* `.local` names for the API client was **not verified**. Using an IP address or a unicast DNS name is safe.
- Static IP / gateway / DNS can be configured in the portal per saved network.

## Time (NTP)

At each wake (skipped if the last successful sync was less than 24 hours ago) the device runs SNTP against `time.google.com` (or the portal's `ntpServer1`), with `pool.ntp.org` as fallback, waiting up to 10 s.
A failed sync is tolerated, but the clock is then not trustworthy: log `created_at` may be `0`/wrong, the OTA retry timer is skipped, and the image-cache age purge is affected ([05](05-images-and-caching.md#caching-by-filename)).
TLS does not depend on it (no certificate checks). On an isolated LAN, either allow NTP out, redirect UDP 123, or set a local NTP server in the portal.

## Sources

- `lib/wificaptive/src/WifiCaptive.cpp`, `WifiCaptive.h`, `WebServer.cpp`, `WebServer.h`, `portal/index.html` (server field validation), `src/wifi_session.cpp`, `src/wifi_network.cpp` (hostname, mDNS, reconnect),
  `include/config.h` (`API_BASE_URL`), `lib/trmnl/include/http_client.h` (`withHttp`), `src/services/http_retry_request.cpp`, `src/api-client/submit_log.cpp`, `src/api-client/setup.cpp`,
  `src/misc/clock/clock.cpp`, `src/menu.cpp` (Settings > Server) - tag `v1.8.19`, e.g. https://github.com/usetrmnl/trmnl-firmware/blob/v1.8.19/lib/wificaptive/src/WebServer.cpp
- Arduino-ESP32 2.0.17 `libraries/HTTPClient/src/HTTPClient.cpp` (request line/headers, default User-Agent, redirect default): https://github.com/espressif/arduino-esp32/blob/2.0.17/libraries/HTTPClient/src/HTTPClient.cpp
- Tests: `spec/spec/general/network/http_spec.rb`, `https_spec.rb`, `spec/spec/general/setup/portal_spec.rb`; `spec/lib/trmnl_sim/mock_trmnl.rb` (TLS settings).
- Official docs: https://docs.trmnl.com/go/diy/byos.md ; https://docs.trmnl.com/go/diy/byod-s.md
