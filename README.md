# tokscale-bundle

Multi-device Tokscale submission hub.

The normal path is **payload proxy mode**: each device runs official `tokscale submit`, but sends the generated submit payload to your server first. The server stores the latest payload per device, merges all devices, then submits one combined payload to Tokscale.

Raw session files stay on each device in payload proxy mode.

![Payload proxy flow](docs/diagrams/payload-proxy-flow.drawio.png)

## Quick Start

Run the hub on your server:

```bash
git clone https://github.com/gigio1023/tokscale-bundle.git
cd tokscale-bundle
cargo run -- serve --bind 0.0.0.0:4787 --storage /var/lib/tokscale-bundle --submit
```

Point each device at the hub with a unique device id:

```bash
# MacBook
export TOKSCALE_API_URL="https://your-server.example.com/devices/macbook-pro"
bunx tokscale@latest submit --no-spinner

# Desktop
export TOKSCALE_API_URL="https://your-server.example.com/devices/desktop"
bunx tokscale@latest submit --no-spinner
```

That is the main workflow. Every new device submission updates the server's stored payload for that device, rebuilds the merged payload, and, with `--submit`, immediately forwards it to `https://tokscale.ai/api/submit`.

## The Important Detail

Official `tokscale submit` does not send a device id header. The device id comes from the URL path:

```bash
TOKSCALE_API_URL="https://your-server.example.com/devices/<device-id>"
```

Do not use the same URL on every machine. If all devices submit to `https://your-server.example.com`, they all become the `default` device and overwrite each other.

## Check The Hub

Use the same Tokscale API token that `tokscale submit` uses.

```bash
export TOKSCALE_API_TOKEN="tt_..."

curl -H "Authorization: Bearer $TOKSCALE_API_TOKEN" \
  https://your-server.example.com/api/hub/status

curl -H "Authorization: Bearer $TOKSCALE_API_TOKEN" \
  https://your-server.example.com/api/hub/preview
```

If the hub is running without `--submit`, push the current merged payload manually:

```bash
curl -X POST -H "Authorization: Bearer $TOKSCALE_API_TOKEN" \
  https://your-server.example.com/api/hub/push
```

Remove a stale device:

```bash
curl -X DELETE -H "Authorization: Bearer $TOKSCALE_API_TOKEN" \
  https://your-server.example.com/api/hub/device/old-laptop
```

## Endpoints

Payload proxy:

- `POST /api/submit`
- `POST /devices/<device-id>/api/submit`
- `GET /api/hub/status`
- `GET /api/hub/preview`
- `POST /api/hub/push`
- `DELETE /api/hub/device/<device-id>`

Bundle fallback:

- `GET /health`
- `POST /api/bundles`

## Bundle Fallback

Use this only when payload proxy mode is not enough and you need the server to run upstream Tokscale scanning against a combined fake home. Unlike payload proxy mode, this uploads raw Tokscale-discoverable local data to your server.

Server:

```bash
export TOKSCALE_BUNDLE_TOKEN="$(openssl rand -hex 32)"
cargo run -- serve --bind 0.0.0.0:4787 --storage /var/lib/tokscale-bundle --submit
```

Each device:

```bash
export TOKSCALE_BUNDLE_TOKEN="same-token-as-server"
cargo run -- upload \
  --server https://your-server.example.com \
  --device-name "MacBook Pro"
```

The server keeps the latest bundle per device under `storage/devices/<device-id>/`, rebuilds `storage/combined/home`, copies the server's Tokscale credentials into that fake home, and runs `bunx tokscale@latest submit` when `--submit` is enabled.

## Manual Fake-Home Flow

For one-off transfer without a long-running server:

```bash
# Source device
cargo run -- export --output /tmp/tokscale-bundle-device-a.zip

# Destination device
cargo run -- unpack /tmp/tokscale-bundle-device-a.zip | tee /tmp/tokscale-bundle-unpack.txt
UNPACK_ROOT="$(awk -F= '/^unpack_root=/{print $2}' /tmp/tokscale-bundle-unpack.txt)"
FAKE_HOME="$(awk -F= '/^fake_home=/{print $2}' /tmp/tokscale-bundle-unpack.txt)"

cargo run -- add-local "$UNPACK_ROOT"

mkdir -p "$FAKE_HOME/.config/tokscale"
cp ~/.config/tokscale/credentials.json "$FAKE_HOME/.config/tokscale/credentials.json"

HOME="$FAKE_HOME" \
XDG_CONFIG_HOME="$FAKE_HOME/.config" \
XDG_DATA_HOME="$FAKE_HOME/.local/share" \
CODEX_HOME="$FAKE_HOME/.codex" \
HERMES_HOME="$FAKE_HOME/.hermes" \
bunx tokscale@latest submit --dry-run --no-spinner
```

If the dry run looks correct, rerun the printed command without `--dry-run`.

Clean up:

```bash
cargo run -- cleanup "$UNPACK_ROOT"
```

## Commands

```bash
cargo run -- serve --storage /var/lib/tokscale-bundle --submit
cargo run -- upload --server https://your-server.example.com --device-name "MacBook Pro"
cargo run -- export --output <bundle.zip>
cargo run -- unpack <bundle.zip>
cargo run -- add-local <unpack_root>
cargo run -- cleanup <unpack_root>
```

## Requirements

- macOS or Linux
- Rust toolchain
- Official Tokscale runnable as `bunx tokscale@latest`
- TLS/reverse proxy if the hub is internet-facing

## Notes

- Payload proxy storage is partitioned by Tokscale bearer-token hash and device id.
- Payload proxy mode sends submit JSON only; it does not upload raw sessions.
- Bundle fallback mode can collide for fixed-path clients such as Kilo, Hermes, Synthetic, and Crush because upstream Tokscale discovers them at fixed paths inside one fake `HOME`.
- Windows is not supported in v1.
