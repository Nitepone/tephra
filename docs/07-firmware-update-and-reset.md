# 07 - Firmware update (`update_firmware`) and reset (`reset_firmware`, `status: 500`)

Target firmware **v1.8.19**, checked 2026-11-16.

## Over-the-air firmware update

### Trigger

The `/api/display` answer contains `"update_firmware": true` **and** a non-empty `"firmware_url"`. The check runs at the end of every wake's display processing (`updateFirmwareIfDue`, `src/request_handling.cpp`; `FirmwareUpdateService::isUpdateDue`, `src/services/firmware_update.cpp`), for normal and special-function answers alike.

The device does **not**:
- compare versions. It updates whenever the flag is true, even to the same version it already runs. The server must decide (compare the `FW-Version` request header with the target) and send `update_firmware: true` only when an update is needed, and must stop sending it once the device reports the new version.
- check which model the binary is for beyond what the ESP32 image format itself enforces. Send the binary built for the device's `Model` (chip family: ESP32-C3 for OG/BWRY, ESP32-S3 for X, ESP32-C5 for gen 2).
- authenticate the download. The GET carries **no** `ID`/`Access-Token` headers and no special headers, and the TLS certificate is not validated.

### Rate limit after a failure

The time of the last *failed* attempt is stored in flash (`last_ota`). A new attempt is made only if that was **more than 24 hours** ago (or there is no record). The record is written only on failure, so a *successful* update leaves no timestamp: if the server keeps answering `update_firmware: true` after the reboot, the device updates again on the very next wake, in a loop. If the clock is not synced and an earlier failure is recorded, the attempt is skipped.

### Procedure

1. Shows "Firmware update available! Starting now..." on the panel.
2. Reconnects Wi-Fi if needed, then `GET firmware_url` (HTTP/1.1, plain `HTTPClient`). Only **HTTP 200** is accepted: **redirects are not followed** (so e.g. a GitHub "releases/download" URL, which redirects, fails; give a direct URL). `Content-Length` is used as the update size.
3. Streams the body into the inactive OTA partition (`Update.begin` / `writeStream` / `Update.end(true)`). The file must be an ESP32 **application image** (`firmware.bin` from PlatformIO), not a merged flash image, and must fit the partition. No version, signature or checksum check beyond what the ESP image validator does.
4. On success: shows "Firmware update complete. Device will restart..." and reboots. The new firmware runs a normal wake. (On the OG-family `setup()` the new image is marked valid immediately, no rollback window.)
5. On failure: shows "Firmware update failed. Will try again tomorrow." (or "Wi-Fi connected, but the firmware update download failed." / the weak-signal screen), records the attempt time, and sets the refresh interval to **30 s** so the message is replaced quickly on the next wake.

TRMNL X on a 5 GHz network downloads through the ESP32-C5 modem (120 s timeout) instead of the main radio; the semantics are the same.

### What a server should send

```json
{"status":0,"image_url":"...","filename":"...","refresh_rate":900,"update_firmware":true,"firmware_url":"http://byos.local:4567/firmware/og/FW1.8.19.bin"}
```

and, after the device reports `FW-Version: 1.8.19`, `"update_firmware": false` (and `"firmware_url": ""`).

Where binaries come from (informational): TRMNL publishes per-model binaries listed by the public endpoint https://trmnl.com/api/firmware (checked 2026-11-16: `og_png`, `og_plus`, `og_bwry`, `v2` = TRMNL X; `latest` = 1.8.18; URLs like `https://trmnl-fw.s3.us-east-2.amazonaws.com/trmnl_og/FW1.8.18.bin`, `.../trmnl_bwry/FW1.8.18.bin`, `.../trmnl_x/FW1.8.18.bin`). Self-built: `pio run -e trmnl` etc. The GitHub release for v1.8.19 has no attached binaries.

## Reset

A reset **erases the device's identity and Wi-Fi settings and reboots into the setup portal**. Triggers:

| Trigger | Notes |
|---|---|
| `/api/display` JSON `reset_firmware: true` (with `status: 0`) | nothing is downloaded or drawn for that answer |
| `/api/display` JSON `status: 500` | also fast-poll bookkeeping, then reset |
| Button held >= 15 s ("soft reset"), or "Reset Wi-Fi" from the X menu, or the portal's `/soft-reset` | same erase |
| Button held 5-15 s | only the saved Wi-Fi networks are erased |

What `resetDeviceCredentials` (`src/setup_session.cpp`) removes: all saved Wi-Fi networks (including static-IP settings), then `preferences.clear()` on the whole `data` namespace: **`api_key`, `friendly_id`, the server URL (`api_url`), refresh interval, retry counters, stored special function, temperature profile, error logs**, and so on. The device restarts and, with no Wi-Fi saved, shows the portal screen. The user must re-enter Wi-Fi **and the custom server URL**; otherwise the device falls back to the default `https://trmnl.app`. (Cached images and `/logo.*` on the flash filesystem are not erased by this.)

Consequences for a BYOS server:
- Never answer `status: 500` or `reset_firmware: true` unless you mean "factory-reset this device". In particular do not use them for "unknown/stale access token": the TRMNL cloud does (`{"status":500,"error":"Device not found","reset_firmware":true}`) but it also means the device loses its server URL, which is painful on a self-hosted setup.
- Use HTTP 404 (LaraPaper, Terminus do) or a JSON `status` other than `0`/`202`/`500` to say "I do not know you": the device then shows an error and retries later, without erasing anything. (HTTP 404 is shown as "Wi-Fi connected, but can't connect to the API.", see [09](09-errors-and-retries.md).)
- A device whose `/api/setup` failed continues to `/api/display` with an *empty* `Access-Token` ([02](02-api-setup.md#failure-handling)). If your server maps an empty/unknown token to a reset, a flaky first setup can wipe the device's Wi-Fi.

## Official documentation (copied)

Source: https://docs.trmnl.com/go/diy/byos.md ("Display"):

> * `reset_firmware` -- `true` makes the device erase its WiFi credentials + API key
> * `update_firmware` + `firmware_url` -- `true` + a `.bin` URL triggers an OTA update. Firmware builds are per model, so only send a TRMNL X build to a TRMNL X

Source: https://docs.trmnl.com/go/private-api/screens.md: "A request with an unknown `access-token` answers `{"status": 500, "error": "Device not found", "reset_firmware": true}`. Our firmware reads that as "erase WiFi credentials + API key", so double check the key before pointing real hardware at it."

Source: https://docs.trmnl.com/go/how-it-works.md: "...`update_firmware` (true/false), a direct download link to the firmware's [public] binary package, and whether the device should be reset."

> **Corrections / precision for v1.8.19:** "erase its WiFi credentials + API key" also erases the stored server URL and all other settings. The 24-hour retry limit, the no-redirect rule, the lack of a version comparison and the "loop after success" hazard are not mentioned in the docs. `status: 500` is documented only as a hosted-server behaviour, but it is what triggers the reset.

## Sources

- `src/services/firmware_update.cpp`, `include/services/firmware_update.h`, `lib/trmnl/src/ota_schedule.cpp`, `src/request_handling.cpp` (`updateFirmwareIfDue`, `resetIfRequested`), `src/setup_session.cpp` (`resetDeviceCredentials`),
  `lib/wificaptive/src/WifiCaptive.cpp` (`resetSettings`), `src/display_session.cpp` (status 500 / `reset_firmware`), `src/main.cpp` (`esp_ota_mark_app_valid_cancel_rollback`),
  `lib/trmnl/src/refresh_interval.cpp` (`applyFailedUpdate`) - tag `v1.8.19`, e.g. https://github.com/usetrmnl/trmnl-firmware/blob/v1.8.19/src/services/firmware_update.cpp
- Tests: `spec/spec/general/ota_spec.rb`, `spec/spec/general/refresh/special_functions_spec.rb` ("reset status forgets the device", "reset firmware forgets the device"), `test/test_ota_schedule/`.
- https://trmnl.com/api/firmware (public firmware listing, fetched 2026-11-16).
- Official docs cited above.
- **Unverified:** behaviour when the firmware response has no `Content-Length`; the exact partition size limits per board (the test profile gives OTA slot offsets only).
