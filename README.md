# tephra

A small self-hosted server for TRMNL e-ink devices. Apps are written in Lua. Screens are designed
in the browser with a live preview that is the very image the device gets.

## Run it

```sh
cargo build --release
cp tephra.example.toml tephra.toml   # optional: every setting has a default
./target/release/tephra -c tephra.toml
```

Open `http://SERVER:8374/`. The UI has two workspaces:

- **Screens** (`/`): choose each device's layout, put an app in each slot, and set the apps up.
- **Devices** (`/devices`): connected devices and their health, placeholder devices for designing
  screens without hardware, and the log.

## Point a TRMNL at it

In the device's Wi-Fi setup portal, set the server address to `http://SERVER:8374`. Don't put a
proxy in front that redirects `/api/setup`: the firmware doesn't follow redirects there. The
device registers itself on its next wake and appears under Devices. Set `public_url` in the config
if devices reach the server under a different address than the browser does.

A device moving over from another server (LaraPaper, Terminus, …) keeps the API key it was given
there. tephra registers it under that key on its first request, so there's no need to reset it.

## More

- `/docs/lua/` on a running tephra: the Lua API reference for writing apps (source in `luadoc/`,
  generated with LDoc by `scripts/luadoc.sh`).
- `DESIGN.md`: how tephra is put together.
- `docs/`: the device protocol, for firmware v1.8.19.

Noto Sans is bundled under the SIL Open Font License (`assets/fonts/OFL.txt`).
