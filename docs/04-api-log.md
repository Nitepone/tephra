# 04 - `POST /api/log`

Error log upload. Target firmware **v1.8.19**, checked 2026-11-16.

## What is logged and when it is sent

Only log entries of severity **error** or **fatal** are ever stored or sent (`store_submit_threshold = LOG_ERROR`, `src/app_logger.cpp`).
Informational and warning messages go to the serial port only. The `verbose_logging` server field and the `Log-Level: debug` header affect the serial output only,
not the upload.

Two paths:

1. **Stored** (the common case): the entry is appended to a ring of **10 slots** in flash (NVS keys `log_0` ... `log_9`): the first 5 slots keep the oldest entries (never overwritten once full), the other 5 form a circular buffer for the newest.
   When all 10 are full the oldest of the newest 5 is overwritten. Entries survive power loss.
2. **Submit-or-store** (`Log_error_submit`, `Log_fatal_submit`): the device tries to POST the single entry immediately; if that fails it is stored as above.

Stored entries are uploaded together, in one request, by `submitStoredLogs()`, which runs:
- right after setup (before the first `/api/display`),
- after the `/api/display` fetch (and again inside the display flow once the image has been received, before Wi-Fi is switched off),
- and just before the device goes to sleep (`enterSleep`).

It needs Wi-Fi to be connected and an `api_key` to be stored. If the POST is considered successful the stored slots are cleared; otherwise they stay and are retried on the next opportunity (so a server that always fails `/api/log` just makes the device keep the newest 10 entries).
An upload costs time and battery on every wake while entries are pending, so make sure the endpoint answers success.

Examples of what ends up in the log (not exhaustive): `HTTPS_UNABLE_TO_CONNECT - Unable to create WiFiClient`, `Receiving failed. Read: ...`, `File writing ERROR`, `error parsing image file - Wrong image format. Did not pass signature check`,
`Cached image is empty or unreadable: ...`, `Connection failed! WL Status: ...`, HTTP failures of the form `HTTP GET failed after 5 attempts: ...`.

## Request

`POST {base}/api/log`, HTTP/1.1, 15 s connect/read timeouts, **no retries** within a wake.

Headers (`buildLogHeaders`, `lib/trmnl/src/api-client/request_headers.cpp`):

| Header | Value |
|---|---|
| `ID` | device MAC, upper-case, colon separated |
| `Accept` | `application/json, */*` |
| `Access-Token` | the device's `api_key` |
| `Content-Type` | `application/json` |

(plus `Host`, `User-Agent: ESP32HTTPClient`, `Connection: close` and `Content-Length`.)

Body: `{"logs":[ <entry>, <entry>, ... ]}` - the stored entries joined with `,`, oldest first. At most 10 entries.
Entries are already serialised JSON strings, so the array is built by string concatenation (`serializeApiLogRequest`, `lib/trmnl/src/serialize_request_api_log.cpp`).

### Log entry fields

From `serialize_log` (`lib/trmnl/src/serialize_log.cpp`). All keys are always present, except `retry`.

| Field | JSON type | Meaning |
|---|---|---|
| `created_at` | integer | Epoch seconds of the log (device clock; `0` if the clock was not valid/NTP not synced) |
| `id` | integer | Monotonic log counter kept in flash (starts at 1), so the server can de-duplicate re-sent entries |
| `message` | string | The log text (truncated to 512 characters) |
| `source_line` | integer | Source line number in the firmware |
| `source_path` | string | Source file path as compiled into the firmware (e.g. `src/display_session.cpp`; can be a longer path depending on the build) |
| `wifi_signal` | integer | RSSI in dBm at log time |
| `wifi_status` | string | Wi-Fi status name, lower case: `no_shield`, `idle_status`, `no_ssid_avail`, `scan_completed`, `connected`, `connect_failed`, `connection_lost`, `disconnected`, `stopped`, or `unknown` (`wifiStatusStr`, `lib/wificaptive/src/wifi-helpers.cpp`) |
| `refresh_rate` | integer | Stored refresh interval (s) |
| `sleep_duration` | integer | Seconds the device slept before this wake (`0` if unknown, e.g. no NTP) |
| `firmware_version` | string | `1.8.19` |
| `special_function` | string | Current special function name (`none`, `identify`, ...) |
| `battery_voltage` | number | Volts, float |
| `wake_reason` | string | Same names as `Update-Source` (`timer`, `button`, `EXT0`, `powercycle`, ...) |
| `free_heap_size` | integer | bytes |
| `max_alloc_size` | integer | largest allocatable block, bytes |
| `level` | string | `debug`, `info`, `warn`, `error` or `fatal` (in practice `error` or `fatal`) |
| `retry` | integer | **Only present** when logged after setup during a retry sequence: the current API-failure retry counter |

Example request:

```http
POST /api/log HTTP/1.1
Host: byos.local:4567
User-Agent: ESP32HTTPClient
Connection: close
ID: A1:B2:C3:D4:E5:F6
Accept: application/json, */*
Access-Token: xK3b9fQ2mZ7vLw1RaT4nYp
Content-Type: application/json
Content-Length: <length of the body>

{"logs":[{"created_at":1787000042,"id":17,"message":"error parsing image file - Wrong image format. Did not pass signature check","source_line":684,"source_path":"src/display_session.cpp","wifi_signal":-58,"wifi_status":"connected","refresh_rate":900,"sleep_duration":902,"firmware_version":"1.8.19","special_function":"none","battery_voltage":4.1,"wake_reason":"timer","free_heap_size":112000,"max_alloc_size":69620,"level":"error","retry":1}]}
```

(Entry values are illustrative; field names, order and types are exact. The unit test fixture is `test/test_serialize_log/`.)

## Response

The firmware looks only at the HTTP status code: **200**, **204** and **301** count as success (the stored logs are then deleted), anything else, or a transport error, counts as failure (logs are kept).
The body is never read. A `307`/`308` redirect is followed once (relative `Location` is appended to the base URL).

Recommended: `204 No Content` (what the official OpenAPI document and Terminus use) or `200` with a small JSON body (what LaraPaper uses).
Be liberal in what you accept: **do not** reject unknown fields, and do not require the `retry`/`level` fields. Terminus' strict schema requires every field except `level` and `retry`, which matches what the firmware sends.

```http
HTTP/1.1 204 No Content
```

## Official documentation (copied)

Source: https://docs.trmnl.com/go/diy/byos (Markdown: https://docs.trmnl.com/go/diy/byos.md), section "Logs".

> #### Logs
>
> When something goes wrong (a failed download, a bad image), the device posts its logs here:
>
> ```bash
> curl -X POST "http://byos.local/api/log" \
>      -H 'ID: <device_mac_address>' \
>      -H 'Access-Token: <device_api_key>' \
>      -H 'Content-Type: application/json' \
>      -d '{"logs": [{"message": "..."}]}'
> ```

From the OpenAPI document (https://trmnl.com/api-docs/openapi.yaml, `POST /api/log`, response `204`): "An array of log entries. Each entry can be any JSON type: string, object, etc." (`logs` required, no additional properties).

> **Corrections for v1.8.19:** the example entry `{"message": "..."}` is a minimal sketch; the firmware always sends the 16 fields listed above (17 with `retry`).

## Sources

- `src/api-client/submit_log.cpp`, `src/app_logger.cpp`, `lib/trmnl/src/serialize_log.cpp`, `lib/trmnl/src/serialize_request_api_log.cpp`,
  `lib/trmnl/src/stored_logs.cpp`, `lib/trmnl/include/trmnl_log.h`, `lib/trmnl/src/api-client/request_headers.cpp`, `src/api-client/submit_log.cpp` - tag `v1.8.19`,
  e.g. https://github.com/usetrmnl/trmnl-firmware/blob/v1.8.19/src/api-client/submit_log.cpp
- Tests: `test/test_serialize_log/`, `test/test_serialize_api_log/`, `test/test_stored_logs/`; `spec/spec/general/network/http_spec.rb` (`ErrorLog`: redirect followed, logs kept while the endpoint fails).
- Official BYOS guide and OpenAPI (copied/cited above).
- Cross-check: Terminus `app/actions/api/log/create.rb` (required-field schema), LaraPaper `DeviceLogController.php`.
