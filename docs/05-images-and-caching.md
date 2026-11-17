# 05 - Images, panel sizes and `filename` caching

Target firmware **v1.8.19**, checked 2026-11-16.

## How the device picks a decoder

After `GET image_url` succeeds (`downloadAndShow`, `src/display_session.cpp`):

1. The response `Content-Type` header is read. If the response has **no** `Content-Type`, the type is sniffed from the first bytes (`BM` = BMP, `89 50 4E 47 0D 0A 1A 0A` = PNG, `FF D8` = JPEG).
2. `isPNG = (Content-Type == "image/png")` and `isJPEG = (Content-Type == "image/jpeg")` - **exact string comparison**. A header such as `image/png; charset=binary`, `image/PNG` or `application/octet-stream` matches neither.
3. If the body starts with `BM`, it is treated as BMP regardless of Content-Type.
4. Anything that is neither PNG nor JPEG is handed to the BMP parser. If it is not a valid BMP it is **dropped silently**: no error screen, no cache entry; the old screen stays and the failure is not counted as a failure for the retry logic. (Only a body served as `image/png` that does not even pass the PNG signature check gets the "image format is not supported" screen.)

So: **serve `image/png` for PNGs and `image/jpeg` for JPEGs, exactly, with no parameters**, and make sure your static-file server/CDN does not rewrite the type.
The display code itself (`draw_image`, `src/display.cpp`) decides by magic bytes (`89 50 4E 47` PNG, `FF D8` JPEG, otherwise BMP/Group5), and uses PNG only if the file is smaller than `MAX_IMAGE_SIZE`.

## Supported formats

| Format | Where | Rules |
|---|---|---|
| **PNG** | all models | Signature + `IHDR` + non-zero size is checked before the image is drawn. Decoded line by line by PNGdec 1.1.6 straight into the panel buffer. Bit depths 1, 2, 4, 8 gray; indexed/palette; RGB(A). Deeper or colour images are *reduced* to what the panel shows (see below). Larger than the panel: cropped at right/bottom. A decoding error mid-image is **not detected** (the header check is the only validation). |
| **JPEG** | all models (see Unverified below) | Must be **exactly** the panel size; decoded with 1-bit dithering on SPI panels, 4-bit dithering on the X. Wrong size: the image is not drawn (no error screen). |
| **BMP** | only 800x480 mono-class panels (OG family etc.); not the X | 1-bit, 800x480, uncompressed, bottom-up, exactly as below. |
| WebP, GIF, 24-bit BMP, top-down BMP | none | Not supported. (TRMNL's cloud serves WebP to a few BYOD colour displays; those are not ESP32 firmware devices.) |

### BMP, byte by byte (`lib/trmnl/src/bmp.cpp`, `src/display.cpp`)

The firmware checks, little endian, in the downloaded buffer:

| Offset | Size | Field | Required |
|---|---|---|---|
| 0 | 2 | magic | `BM` |
| 10 | 4 | pixel data offset | `> 54` (must carry a colour table). The **renderer ignores the value and always reads the pixels at offset 62** (54 + 8-byte palette), so the offset must in practice be 62 |
| 18 | 4 | width | `800` |
| 22 | 4 | height | `480` (positive: rows are stored bottom-up; a negative/top-down BMP fails the check) |
| 28 | 2 | bits per pixel | `1` |
| 34 | 4 | image data size | `48000` |
| 46 | 4 | colours used | `0` or `2` |
| 54 | 8 | palette: 2 entries, B,G,R,0 | exactly `00 00 00 00 FF FF FF 00` (black, white) **or** `FF FF FF 00 00 00 00 00` (white, black; the pixels are inverted so that 0 bits are black). Any other palette (e.g. red/white) fails with "BMP color scheme is invalid" |
| 62 | 48000 | pixels | 100 bytes per row, 480 rows, bottom row first |

Total size of a valid file: 14 + 40 + 8 + 48,000 = **48,062 bytes** (the setup logo must be exactly this size). Failure of any check is silent (see above), except that nothing is cached.
Official ImageMagick recipe (https://docs.trmnl.com/go/diy/imagemagick-guide.md):

```
magick input.png -monochrome -colors 2 -depth 1 -strip bmp3:output.bmp
magick identify output.bmp
output.bmp BMP3 800x480 800x480+0+0 1-bit sRGB 2c 48062B 0.020u 0:00.001
```

### PNG: bit depth and colour handling

(`src/png_render.cpp`, `src/color_reduce.cpp`; mono SPI panels, i.e. OG family.)

- 1-bit PNG (gray or palette): single plane, eligible for a partial (fast, non-flashing) refresh. For a palette PNG with 1-bit indices the firmware compares the brightness of the two palette entries and inverts as needed, so `[black, white]` and `[white, black]` palettes both work.
- 2-bit PNG with exactly **two distinct values** in use: drawn as 1-bit (partial refresh possible).
- 2-bit PNG with 3-4 values: 4-gray mode, **full refresh** (panel's 4-gray waveform), on panels that have one. Gray value 0 = black ... 3 = white (normal PNG gray).
- 4-bit / 8-bit gray, palette, RGB, RGBA: reduced per pixel by thresholding to 1-bit (`gray >= 128` -> white) or to 2-bit (4 levels, top two bits of the gray value) depending on the panel. Colour is first converted to gray with `(R + 2G + B)/4`. No dithering is applied by the firmware: do the dithering on the server (see the ImageMagick commands below).
- Larger than the panel: cropped. Smaller than the panel: **Unverified** on mono SPI panels (unwritten lines are whatever the controller memory holds); on the X, BWRY and Spectra-6 paths the rest is filled white. Always render at exactly the reported `Width` x `Height`.
- Portrait image (width equals the panel *height* and differs from its width): drawn rotated on the X-class (FastEPD) and Spectra-6 paths. **Unverified** on the other panel paths.
- X (16 gray): 1-, 2-, 4-bit gray directly (2-bit is expanded 0/5/10/15 to the 4-bit scale; 4-bit is used as is), 8-bit and colour reduced to 4-bit. Send a 4-bit gray PNG (see below).
- BWRY (`og_4clr` / `og_gen2_4clr`, 4 inks black/white/yellow/red): each pixel's RGB is matched to the nearest ink by `GetBWYRPixel`. Send a palette PNG that uses exactly the panel colours `#000000 #FFFFFF #FFFF00 #FF0000`. (Gray samples: the raw bits are used as an RGB value, so grays become black/white.)
- Spectra 6 (`xiao_epaper_6clr`, `reterminal_e1002`, ...): RGB matched to the six inks `#FF0000 #00FF00 #0000FF #FFFF00 #000000 #FFFFFF`.
- 3-colour black/white/red panels (`xiao_epaper_3clr`): **Unverified** how red is handled; the generic mono path is used by the code.

Official ImageMagick recipes (copied from https://docs.trmnl.com/go/diy/imagemagick-guide.md):

> TRMNL supports BMP3 and PNG images natively, starting with [FW v1.5.2]. [...]
>
> Match your image to the device. Every model's width, height, bit depth + palette is listed at [trmnl.com/api/models](https://trmnl.com/api/models) (palette colors at [trmnl.com/api/palettes](https://trmnl.com/api/palettes)). Keep PNGs under the model's `image_size_limit` -- 90,000 bytes for ESP32 boards without PSRAM like TRMNL OG, 750,000 bytes for TRMNL X. Our firmware won't draw anything larger.
>
> **PNG - 1 bit**
> ```
> magick input.png -monochrome -colors 2 -depth 1 -strip png:output.png
> magick input.png -dither FloydSteinberg -remap pattern:gray50 -depth 1 -strip png:output.png
> ```
> **PNG - 2 bit** (4-gray devices, like the TRMNL OG (2-bit) model on FW 1.6.0+)
> ```
> magick input.png -colorspace Gray -dither None -posterize 4 -alpha off -depth 2 -define png:compression-level=9 -strip png:output.png
> magick input.png -colorspace Gray -dither FloydSteinberg -posterize 4 -alpha off -depth 2 -define png:compression-level=9 -strip png:output.png
> ```
> **PNG - 4 bit** (16-gray devices, like TRMNL X)
> ```
> magick input.png -colorspace Gray -dither None -posterize 16 -alpha off -depth 4 -define png:compression-level=9 -strip png:output.png
> magick input.png -colorspace Gray -dither FloydSteinberg -posterize 16 -alpha off -depth 4 -define png:compression-level=9 -strip png:output.png
> ```
> **PNG - color** (B/W/R, B/W/R/Y, 6- + 7-color; example for the 6-color palette `color-6a`)
> ```
> magick input.png \( -size 1x1 xc:#FF0000 xc:#00FF00 xc:#0000FF xc:#FFFF00 xc:#000000 xc:#FFFFFF +append +write mpr:palette +delete \) -dither None -remap mpr:palette -define png:compression-level=9 -strip png:output.png
> ```

> **Corrections for v1.8.19:** the guide covers the formats the cloud produces. The firmware additionally accepts other depths and reduces them (see above), but the `MSG_TOO_BIG` on-device message "PNG images must be at most 90000 bytes and 1 or 2 bpp" is outdated about depth. The 7-colour palette (`color-7a`) is not handled by any v1.8.19 draw path **(Unverified: only Spectra-6 `EP73` paths were found)**.

## Models and panel sizes

The server learns the panel size from the `Width` and `Height` request headers on every call, and the board from `Model`. **Trust `Width`/`Height`**, not the table below.
Sizes and inks here come from `platformio.ini` (`DEVICE_MODEL` per environment), `src/board_config.cpp` and the firmware's own simulator device profiles `spec/spec/support/devices.rb`; they are *not* a hardware measurement.
Model strings sent by the device differ from the names TRMNL's cloud uses in https://trmnl.com/api/models (`og` vs `og_png`/`og_plus`, `x` vs `v2`, `og_4clr` vs `og_bwry`).

"Max download" is `MAX_IMAGE_SIZE` (`include/config.h`): **750,000** bytes on PSRAM builds (`BOARD_HAS_PSRAM`, defined by the `esp32s3_n16r8` / `esp32c5_*` board files and by several environments in `platformio.ini`), **90,000** bytes otherwise. The per-board values in the table follow the firmware's own test profiles (`psram:` in `spec/spec/support/devices.rb`) cross-checked with `platformio.ini`; for boards built on `seeed_xiao_esp32s3` this relies on the PlatformIO board definition (not in this repository), so treat those as **Unverified**. Larger `Content-Length` (or a body that grows past it) shows the "image file ... too large" message.

| `Model` header | Device | Size (WxH) | Inks / grays | Image formats | Max download |
|---|---|---|---|---|---|
| `og` | TRMNL OG (ESP32-C3) | 800x480 | 1-bit; 4 gray with 2-bit PNG | PNG, JPEG, BMP | 90,000 |
| `og_gen2` | TRMNL OG gen 2 (ESP32-C5) | 800x480 | as `og` | PNG, JPEG, BMP | 750,000 |
| `og_4clr` | TRMNL BWRY (ESP32-C3) | 800x480 | black, white, yellow, red | PNG (palette), JPEG, BMP | 90,000 |
| `og_gen2_4clr` | TRMNL BWRY gen 2 | 800x480 | as `og_4clr` | PNG, JPEG, BMP | 750,000 |
| `x` | TRMNL X (ESP32-S3 + C5 modem) | 1872x1404 | 16 grays | PNG, JPEG (no BMP) | 750,000 |
| `xteink_x4` | Xteink X4 | 800x480 | 1-bit / 4 gray | PNG, JPEG, BMP | 90,000 |
| `xteink_x3` | Xteink X3 | 792x528 | 1-bit / 4 gray | PNG, JPEG | 90,000 |
| `seeed_esp32c3` / `seeed_esp32s3` | XIAO ESP32 + 7.5" panel | 800x480 | 1-bit; 4 gray with 2-bit PNG (EPD_75, as `og`) | PNG, JPEG, BMP | 90,000 / 750,000 |
| `xiao_epaper_display` | TRMNL 7.5" DIY kit | 800x480 | 1-bit; 4 gray with 2-bit PNG (EPD_75, as `og`) | PNG, JPEG, BMP | 750,000 |
| `xiao_epaper_mini` | 4.26" DIY kit | 800x480 | 1-bit / 4 gray | PNG, JPEG | 750,000 |
| `xiao_epaper_3clr` | 7.5" black/white/red kit | 800x480 | 3 colours | PNG | 750,000 (colour handling **Unverified**) |
| `xiao_epaper_6clr` | 7.3" Spectra 6 kit | 800x480 | 6 colours | PNG | 750,000 |
| `reterminal_e1001` | Seeed reTerminal E1001 | 800x480 | 1-bit / 4 gray | PNG, JPEG | 750,000 |
| `reterminal_e1002` | Seeed reTerminal E1002 | 800x480 | Spectra 6 | PNG | 750,000 |
| `reterminal_e1003` | Seeed reTerminal E1003 | 1872x1404 per TRMNL's BYOD docs | 16 grays | PNG | 750,000 (size **Unverified** vs firmware) |
| `reterminal_e1004` | Seeed reTerminal E1004 | 1200x1600 per firmware test profile (TRMNL's model list says 1600x1200) | Spectra 6 | PNG | 750,000 |
| `seeed_sticky` | Seeed Sticky | 800x480 | 1-bit / 4 gray | PNG, JPEG | 750,000 |
| `waveshare_397` | Waveshare 3.97" | 800x480 | 1-bit / 4 gray | PNG, JPEG | 750,000 |
| `waveshare` | generic Waveshare ESP32 driver board | 800x480 (EPD_75) | 1-bit; 4 gray with 2-bit PNG (EPD_75, as `og`) | PNG, JPEG, BMP | 90,000 |
| `crowpanel42` | CrowPanel 4.2" | 400x300 | 1-bit / 4 gray | PNG | 90,000 |
| `m5_paper_mono` | M5Paper | **Unverified** (test profile says 800x480; real panel is 960x540) | mono | PNG | 90,000 |
| `m5_paper_color` | M5Paper Color | 400x600 | Spectra 6 | PNG | 750,000 |
| `m5_papers3` | M5Stack PaperS3 | 960x540 | 16 grays | PNG | 750,000 |
| `lilygo_t5pro` | LilyGo T5 4.7" S3 Pro | 960x540 | 16 grays | PNG | 750,000 |
| `sensoria_c5` | Sensoria C5 | 1280x720 | 16 grays | PNG | 750,000 |
| `sensoria_s3`, `gen-2` | sensor/dev boards | **Unverified** | | | |
| `trmnl_steam` | Waveshare 5.83" ("Steam Machine") | 648x480 | 1-bit / 4 gray | PNG | 90,000 (the firmware's own test notes say this build does not boot yet) |

(The `xiao_epaper_mini_b` profile used for a newer 4.26" panel is internal; the device still reports `xiao_epaper_mini`.)

## Caching by filename

The device keeps downloaded images in a small flash filesystem so it can avoid downloads and redraws. The cache key is the JSON `filename` field.

### The rules, as implemented

1. **Name mapping** (`filesystem_fix_filename`, `src/filesystem.cpp`): the file is stored as `/` + `filename`. If `filename` is longer than 31 characters it is shortened to its **first 7 and last 17 characters** (so the 45-character `mashup-066cc3-weather-and-calendar-1771674964` is stored as `/mashup-lendar-1771674964`: `/` + first 7 characters `mashup-` + last 17 characters). Two different long names with the same ends collide, and the second image will then never be shown (the device believes it already has it). Keep names <= 31 characters.
2. **Lookup**: on a `status: 0` answer with a non-empty `image_url` and `filename != "empty_state"`: if the shortened name exists in flash, the image is **not downloaded**: it is redrawn from flash (or, if the same name is the one currently on the panel, not redrawn at all and the panel is not refreshed). Otherwise it is downloaded.
3. **Store**: only **PNG and JPEG** are written to flash, under the shortened `filename`, after they have been drawn. The device first deletes (`filesystem_purge_old_file`): every file whose first 14 characters equal those of the new file (that is `/` + the first 13 characters of the name: with the recommended `plugin-3f2a9c-1787000000` pattern that is the screen's stable `<name>-<id>`, so a newer render of the same screen replaces the older one), and **every other file in the root whose last 10 characters are not a Unix timestamp younger than 24 hours** (a name with no 10-digit tail counts as timestamp 0 = expired). That also deletes the setup logos (`/logo.bmp`, `/logo.png`) and `/current.bmp`.
4. **BMP is never cached under its filename.** A valid BMP is drawn and a copy is written to `/current.bmp` (only if no `/current.png` exists). Because the lookup in rule 2 finds nothing, a BMP is **downloaded and redrawn on every wake**, even when `filename` is unchanged. (Code-derived; no integration test covers repeated BMP wakes.) Use PNG if you want unchanged screens to cost nothing.
5. A PNG that fails the signature/IHDR check is not drawn and not cached; the failure is logged. A PNG with a valid header but a corrupt body *is* drawn (partially) and cached, so the same `filename` will then be redrawn from the cache forever: give a corrected image a **new** filename.
6. The device remembers which image is currently on the panel across deep sleep. The loading logo drawn on a non-timer wake "covers" it, so a button/power wake redraws the current image even if the filename is unchanged.
7. `Image-Cached` in the next request reports whether the last image came from the cache.
8. Clock matter: the 24 h rule uses the device clock (NTP). With an unsynced clock, `time()` is small and nothing is purged by age, but the 14-character same-screen rule still applies.
9. If writing a file fails (flash full) the filesystem is **formatted** (all cached images lost), and an error is logged.

### Recommendations for a server

- Make `filename` stable while the picture is unchanged, and change it exactly when the picture changes. Use `<7-char prefix><6-char id>-<10-digit unix time>` (e.g. `plugin-3f2a9c-1787000000`, 24 characters) so that replacements and the 24-hour purge work as designed.
- A new `filename` on every response forces a download and a flash write on every wake, which costs battery and wears flash.
- Include the `filename` of the image the device already shows when you answer a special function that must not change the screen ([06](06-special-functions.md)).

### Special filenames

| `filename` | Effect |
|---|---|
| `empty_state` | "No content yet": `image_url` is shown once, then the device fast-polls (5 s ...) instead of using `refresh_rate`. See [03](03-api-display.md#what-happens-on-a-normal-answer). Matched by exact equality. |
| starts with `screen_wiper` (e.g. `screen_wiper.png`, `screen_wiper-1791191234`) | The device clears its panel by cycling it (about 60 full refreshes on mono panels, 2 on BWRY, 100 on the X; minutes) and then immediately fetches `/api/display` again, once per wake. No image is downloaded for the wiper item. Used to remove ghosting. Prefix match since 1.8.19 (earlier: exact `screen_wiper.png`). |

(`setup-logo.bmp` as the tail of `image_url` is only logged; it has no special effect.)

## Download behaviour

- URL scheme `http://` or `https://`; anything else (e.g. `ftp://`) makes the HTTP client refuse the request and the device shows "Wi-Fi connected, but can't reach the API."
- Read timeout 15 s unless `image_url_timeout` (seconds) is set in the `/api/display` answer; the stream is also abandoned after 30 s with no data. Up to 5 attempts (2 s apart) for transport errors, 5xx, 408 and bodies cut short.
- HTTP `200` (and `301`, `429`) accepted; `307`/`308` followed once; any other status gives the "API responded, but the image download failed." screen (and the retry counter is *not* advanced: the API itself answered).
- `Content-Length` is used to size the buffer; when it is missing the device reads up to `MAX_IMAGE_SIZE` bytes. A short body (connection closed early) is treated as a timeout and retried.
- `ID` and `Access-Token` are sent only if `image_url` starts with the configured base URL; if you host images on another host or scheme, they are not sent. Do not rely on image-URL authentication unless you use the same prefix.
- An empty body: error "No data received" (shown as the "TRMNL content is malformed" screen).

## Official documentation (copied)

Source: https://docs.trmnl.com/go/diy/byos (Markdown: https://docs.trmnl.com/go/diy/byos.md), sections "Filenames" and "Images".

> #### Filenames
>
> The device saves each new image to flash under its `filename`. When a response names a file it already has, it shows that copy and downloads nothing. So:
>
> * Change `filename` only when the image changes, never on every request. A new name on every request downloads + writes the image on every wake.
> * Use the format `<name>-<id>-<unix time>`, e.g. `screen-3f2a9c-1781785364`. `<name>-<id>` (13 characters) names the screen and never changes; the time is when this image was made. Each time the device saves a new image, it deletes older images of the same screen, and every saved image that does not end in a 10-digit timestamp or is more than 24 hours old.
> * Keep it to 31 characters or less. The device shortens a longer name to its first 7 + last 17 characters, so two long names can collide and the second image never shows.
>
> #### Images
>
> Render each image at the size the device sends in its `Width` + `Height` headers, and serve it with the right `Content-Type`. The firmware picks the decoder from that header, not from the URL: `image/png` for PNG, `image/jpeg` for JPEG. It reads any other response as a BMP.
>
> | Model (`Model` header) | Size      | Grays  | Formats              | Largest file  |
> | ---------------------- | --------- | ------ | -------------------- | ------------- |
> | TRMNL OG (`og`)        | 800x480   | 2 or 4 | PNG, JPEG, 1-bit BMP | 90,000 bytes  |
> | TRMNL X (`x`)          | 1872x1404 | 16     | PNG, JPEG            | 750,000 bytes |
>
> A BMP must be exactly 800x480 at 1 bit per pixel.

Also from https://docs.trmnl.com/go/diy/byod-s.md: "The firmware treats `filename` as a cache key -- if it already has a file by that name, it skips the download. Give every new image a new `filename`."

> **Corrections / precision for v1.8.19:**
> - "It reads any other response as a BMP": correct, and an invalid BMP (or a PNG sent with a wrong Content-Type) is then dropped **silently** - see above. A response with *no* Content-Type is sniffed by magic bytes first.
> - "When a response names a file it already has, it ... downloads nothing": true for PNG and JPEG only. A BMP is never stored under its filename, so it is downloaded again on every wake.
> - "deletes ... every saved image that does not end in a 10-digit timestamp or is more than 24 hours old": correct; it also deletes the setup logo and `/current.bmp`, and the "older images of the same screen" rule compares the first 14 characters including the leading `/`.
> - "(13 characters)" corresponds to 14 including the leading `/`.
> - The table lists only `og` and `x`; see the full list above. The OG's size limit is 90,000 bytes only on non-PSRAM builds (OG gen 2 and many BYOD boards allow 750,000).

## Sources

- `src/display_session.cpp` (content-type decision, caching, BMP/PNG/JPEG flows, `screen_wiper`), `src/http_utils`/`lib/trmnl/src/http_utils.cpp` (`sniffImageContentType`),
  `lib/trmnl/src/bmp.cpp`, `lib/trmnl/src/png.cpp`, `lib/trmnl/src/png_flip.cpp`, `src/png_render.cpp`, `src/jpeg_render.cpp`, `src/color_reduce.cpp`, `src/display.cpp` (`draw_image`, refresh modes),
  `src/filesystem.cpp` (`filesystem_fix_filename`, `filesystem_purge_old_file`, `filesystem_extract_timestamp`, `writeImageToFile`), `src/displayed_image.cpp`, `src/services/http_retry_request.cpp`,
  `include/config.h` (`MAX_IMAGE_SIZE`, `DISPLAY_BMP_IMAGE_SIZE`), `src/board_config.cpp`, `platformio.ini`, `boards/*.json` (PSRAM), `src/message_screens.cpp` - tag `v1.8.19`,
  e.g. https://github.com/usetrmnl/trmnl-firmware/blob/v1.8.19/src/display_session.cpp
- Tests: `spec/spec/general/images_spec.rb`, `spec/spec/support/devices.rb`, `test/test_bmp/`, `test/test_png/`, `test/test_png_flip/`.
- TRMNL public reference data: https://trmnl.com/api/models and https://trmnl.com/api/palettes (fetched 2026-11-16; palette ids `bw`, `gray-4`, `gray-16`, `gray-256`, `color-3bwr`, `color-3bwy`, `color-4bwry`, `color-6a`, `color-7a`, `color-12bit`, `color-24bit`).
- Official docs (copied above): https://docs.trmnl.com/go/diy/byos.md ; https://docs.trmnl.com/go/diy/imagemagick-guide.md ; https://docs.trmnl.com/go/diy/byod-s.md
- **Unverified (no hardware, not statically provable here):** JPEG on every panel path (the firmware's own tests mark JPEG as failing on UC81xx panels in the simulator); behaviour for images smaller than the panel on mono SPI panels; the `color-7a` palette; 3-colour panels.
