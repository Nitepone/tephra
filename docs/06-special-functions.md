# 06 - Special functions

Target firmware **v1.8.19**, checked 2026-11-16.

A "special function" is an action the user triggers from the device (a double click of the button, or the TRMNL X's touch-bar menu) that the *server* carries out by answering the next `/api/display` request differently.

The firmware knows nine names (`lib/trmnl/src/special_function.cpp`):

`none`, `identify`, `sleep`, `add_wifi`, `restart_playlist`, `rewind`, `send_to_me`, `guest_mode`, `toggle_hold`

Any other string in the response's `special_function` field is parsed as `none`. (The official BYOS page lists the first eight; `toggle_hold` is new in 1.8.19. TRMNL's OpenAPI document also lists a `refresh` value, which this firmware does not know.)

## 1. How a function gets selected

### 1.1 The server assigns it (a setting)

Every normal `/api/display` answer (JSON `status: 0`, no special function active) carries `special_function`. The device **stores it in flash** (NVS key `sf`) after every such answer, overwriting the previous value, including with `none` when the field is missing or unknown. So the server should send the *same configured value on every response* ("button action").

### 1.2 The user triggers it

| Device | Trigger | Request header sent |
|---|---|---|
| OG family and other single-button boards (not the X) | Double click: two presses within 800 ms, **or** one press held more than 1 s and up to 5 s (a 5 s+ hold forgets Wi-Fi instead; 15 s+ resets everything) | `special_function: true` - only if the stored function is not `none` |
| TRMNL X | Touch bar: middle tap opens the menu; "Special" entries: Identify, Restart Playlist, Send to Me, Guest Mode, Toggle Hold Screen. Left tap / left hold ("Back") asks for the previous screen | `special_function: <name>` e.g. `identify`, `restart_playlist`, `send_to_me`, `guest_mode`, `toggle_hold`, `rewind` |

Header name: it is **`special_function` with an underscore**, lower case, not `Special-Function`. (TRMNL's OpenAPI document names the header `Special-Function`; HTTP header names are case-insensitive but `-` and `_` are different characters. Many servers/proxies treat them differently; for example nginx silently drops headers containing underscores unless `underscores_in_headers on;` is set [general nginx behaviour, not verified against this firmware].) A tolerant server should accept both spellings.

The value `true` means "run the function you have configured for this device's button". A name means "run exactly this one". With `true`, the *server* knows the function from its own setting; the *device* handles the response according to the function it has stored from an earlier answer. If the user changed the setting on the server in between, the device and server can disagree until a normal answer has refreshed the stored value; the device then reports "`<function>` failed" in its log (and does nothing) when the `action` does not match.

On the X, a special function request also keeps the device awake for further taps; on other boards the wake continues as usual.

### 1.3 The action handshake

When the request carried `special_function`, the response must contain `"action": "<function name>"` matching what the device expects. If `action` does not equal the expected string, the device logs an error (`<function> failed`), does **not** apply the answer's image or refresh rate, and goes back to sleep with its previous interval. (`action` is ignored for normal requests.)

For `toggle_hold` the device expects the server to answer with `action: "guest_mode"` (comment in `src/display_session.cpp`: "the server answers toggle_hold as guest_mode").

JSON `status` still applies first: `202` and `500` are handled as in [03](03-api-display.md#status-handling); any `status` other than `0`, `202`, `500` is ignored. While a special function is active, `reset_firmware` in the answer is **not** acted on (only `status: 500` resets), `update_firmware` still is, and the stored `special_function` is not overwritten.

## 2. What each function does on the device

All rows assume HTTP 200 and `status: 0`. "Download" means the image at `image_url` is fetched and drawn **without consulting the cache** (the `filename` is not looked up). `empty_state` handling is the same as for normal answers ([03](03-api-display.md#what-happens-on-a-normal-answer)).

| Function | Required `action` | Device behaviour | `refresh_rate` applied? |
|---|---|---|---|
| `none` | - | No special request. | normal rules |
| `identify` | `identify` | If `image_url` is non-empty: download and draw it (typically a "this is device X" screen). | **no** (device keeps its stored interval) |
| `sleep` | `sleep` | No image. The device keeps the current screen and sleeps for the answer's `refresh_rate`. See the cache caveat below. | **yes** |
| `add_wifi` | `add_wifi` | The Wi-Fi captive portal is opened **before** the request is made (see below). The answer needs only `action`. No image. | no |
| `restart_playlist` | `restart_playlist` | Same as `identify`: download and draw `image_url` (the first playlist item). | no |
| `rewind` | `rewind` **or** `guest_mode` | With `action: "rewind"`: the device tries to redraw its local previous image (`/last.bmp` / `/last.png`) - **no code in v1.8.19 ever writes those files**, so this is effectively a no-op (no crash). With `action: "guest_mode"` **and** a non-empty `image_url`: the answer is treated like a normal image answer (flash cache lookup by `filename`, download if missing, draw). This is the form the X's "Back" uses to receive the previous screen from the server. | only for the `guest_mode` form |
| `send_to_me` | `send_to_me` | The device redraws the last image from its own flash cache (the loading/menu screen has covered it). The answer needs nothing else. | no |
| `guest_mode` | `guest_mode` | Same as `identify` (download `image_url`), and `refresh_rate` is applied even when no image is sent. | **yes** |
| `toggle_hold` | `guest_mode` | Same as `guest_mode`. | **yes** |

Notes:

- **`add_wifi` portal:** on a double-click wake the device calls `WifiCaptivePortal.startPortal()` before connecting: it shows the "Connect to TRMNL-XXXXXX" screen, opens the AP, and waits up to 15 minutes. After joining a network (or on a 15-minute timeout, when it goes to deep sleep) the wake continues with the request. See [08](08-provisioning-and-transport.md#captive-portal). The server only has to answer `action: "add_wifi"`.
- **Cache caveat for `sleep`, `add_wifi` (and the `rewind` no-op):** the device sets "no download needed / success". The normal-flow cache step then runs with the answer's `filename`: if that name equals the image currently on the panel, nothing is redrawn (the desired outcome); if it names a cached image, that image is redrawn; if it names nothing the device has (e.g. an uncached BMP), it falls back to `/current.bmp` or keeps the current screen. The firmware's own tests always send the current image's `image_url` and `filename` with these answers. **Recommended:** include the `filename` (and `image_url`) of the image the device is currently showing. **Unverified:** what happens when `filename` is `null` or empty (the code maps it to the path `/`).
- **`send_to_me`:** per the firmware's tests, the server "answers `send_to_me` with nothing but its status and action". Do not change the playlist position on the server for it; it is a "show me what is on the screen again" request.
- **Do not send `refresh_rate: 0`** or omit it in answers where it is applied (`sleep`, `guest_mode`, `toggle_hold` and the `guest_mode` form of `rewind`): the interval becomes 0 s.

## 3. Examples (request/response pairs)

Common request headers are omitted; each request also carries everything listed in [03](03-api-display.md#request-headers).

### 3.1 Normal answer that assigns the button function

```http
GET /api/display HTTP/1.1
...
Update-Source: timer
```
```json
{"status":0,"image_url":"http://byos.local:4567/images/plugin-3f2a9c-1787000000.png","filename":"plugin-3f2a9c-1787000000","refresh_rate":900,"special_function":"identify"}
```

### 3.2 Identify (OG double click)

```http
GET /api/display HTTP/1.1
...
Update-Source: EXT0
special_function: true
```
```json
{"status":0,"action":"identify","image_url":"http://byos.local:4567/images/identify-1787000100.png","filename":"identify-1787000100","refresh_rate":900,"special_function":"identify"}
```

### 3.3 Sleep

```http
special_function: true
```
```json
{"status":0,"action":"sleep","refresh_rate":1800,"image_url":"http://byos.local:4567/images/plugin-3f2a9c-1787000000.png","filename":"plugin-3f2a9c-1787000000","special_function":"sleep"}
```
(`filename` = the image the device already shows, so nothing is redrawn.)

### 3.4 Add Wi-Fi

```http
special_function: true
```
```json
{"status":0,"action":"add_wifi","image_url":"http://byos.local:4567/images/plugin-3f2a9c-1787000000.png","filename":"plugin-3f2a9c-1787000000","special_function":"add_wifi"}
```

### 3.5 Restart playlist

```json
{"status":0,"action":"restart_playlist","image_url":"http://byos.local:4567/images/plugin-aa11bb-1787000200.png","filename":"plugin-aa11bb-1787000200","special_function":"restart_playlist"}
```

### 3.6 Send to me (X menu, request names the function)

```http
special_function: send_to_me
```
```json
{"status":0,"action":"send_to_me"}
```

### 3.7 Guest mode / toggle hold

```http
special_function: guest_mode
```
```json
{"status":0,"action":"guest_mode","image_url":"http://byos.local:4567/images/guest-1787000300.png","filename":"guest-1787000300","refresh_rate":1200}
```
For `toggle_hold` the request header is `special_function: toggle_hold` but the answer still says `"action":"guest_mode"`.

### 3.8 Rewind (X "Back")

```http
special_function: rewind
```
```json
{"status":0,"action":"guest_mode","image_url":"http://byos.local:4567/images/plugin-77ccdd-1786999000.png","filename":"plugin-77ccdd-1786999000","refresh_rate":900}
```
(**Unverified** against the TRMNL cloud: that the cloud answers a `rewind` request with `action: "guest_mode"` is inferred from the firmware comment "Server returned previous screen, or a guest-mode screen" and release note #796 "Query server when navigating back/forward on X".)

### 3.9 Wrong action

```http
special_function: true
```
```json
{"status":0,"action":"nothing","refresh_rate":300}
```
The device ignores this answer completely (logs "... failed", keeps its previous refresh interval).

## 4. Official documentation (copied)

Source: https://docs.trmnl.com/go/diy/byos.md ("Display"): "`special_function` -- what a button press does: `none`, `identify`, `sleep`, `add_wifi`, `restart_playlist`, `rewind`, `send_to_me` or `guest_mode`. This is a setting, so send the same value on every response"; and https://docs.trmnl.com/go/private-api/screens.md: `"special_function": "identify" # what the device button does`.

Source: https://trmnl.com/api-docs/openapi.yaml, request header `Special-Function`: "Run a special function: true for the one set for the button, or a function name", enum `true, identify, sleep, restart_playlist, rewind, send_to_me, guest_mode, refresh, toggle_hold`; response properties `special_function` (string, "nullable: false", example `identify`) and `action` (string, nullable, example `identify`).

> **Corrections for v1.8.19:** the header the firmware sends is `special_function` (underscore). `add_wifi` is a known function but is missing from OpenAPI's header enum, `refresh` is in the enum but unknown to the firmware. `action` is **required** for the device to act on a special-function answer, which the BYOS page does not say.

## Sources

- `lib/trmnl/src/special_function.cpp` / `include/special_function.h` (names), `src/display_session.cpp` (`handleApiDisplayResponse`, the `special_function != SF_NONE` branch; `writeSpecialFunction`),
  `src/button.cpp` (`button_wake_act`, `button_wake_run_special_function`, double-click timing), `lib/trmnl/src/api-client/request_headers.cpp` (header), `src/menu.cpp` / `src/touchbar_actions.cpp` (X menu and Back),
  `src/wake_init.cpp`, `include/config.h` (`BUTTON_*`) - tag `v1.8.19`, e.g. https://github.com/usetrmnl/trmnl-firmware/blob/v1.8.19/src/display_session.cpp
- `spec/spec/general/refresh/special_functions_spec.rb` (firmware integration tests: request header `special_function: true`, actions, 202/500 during a special function, rejected actions).
- Cross-check: Terminus `app/aspects/screens/interrupter.rb` keys on `Update-Source`/`special_function` header values; LaraPaper's `DisplayController` only returns the `special_function` setting and does not dispatch on the request header (see [11](11-implementation-cross-check.md)).
- Official docs: https://docs.trmnl.com/go/diy/byos.md ; https://docs.trmnl.com/go/private-api/screens.md ; https://trmnl.com/api-docs/openapi.yaml
