# tokscale-bundle

`tokscale-bundle` helps submit one Tokscale view from multiple devices.

The preferred mode is a payload proxy hub. Each device runs official `tokscale submit`, but points it at your server. The device sends the normal Tokscale submit JSON payload only, not raw Codex sessions or local SQLite files.

Use it when Tokscale's remote submit path still merges by client instead of by device, and you want one server-owned merge point for all machines.

## Recommended Flow

```text
Device A
  TOKSCALE_API_URL=https://hub.example.com/devices/macbook
  bunx tokscale@latest submit
  -> sends submit payload JSON to the hub

Device B
  TOKSCALE_API_URL=https://hub.example.com/devices/workstation
  bunx tokscale@latest submit
  -> sends submit payload JSON to the hub

Hub
  -> stores the latest payload per device
  -> merges date/client/model/provider rows
  -> submits one merged payload to https://tokscale.ai/api/submit
```

The older fake-home bundle flow is still available as a fallback when you need to move raw Tokscale-discoverable data between machines and let upstream `tokscale` scan one combined fake home.

## Docker Quick Start

Use Docker when you want the hub to stay available on a server. The container stores all hub state as files under `/data`; keep `/data` on a Docker volume or bind mount and the payload history survives container stops, removals, image rebuilds, and later direct non-Docker runs.

Build the image:

```bash
git clone --recurse-submodules https://github.com/gigio1023/tokscale-bundle.git
cd tokscale-bundle
docker build -t tokscale-bundle .
```

If you already cloned without submodules, run this before building:

```bash
git submodule update --init --recursive
```

Run with a named Docker volume:

```bash
docker volume create tokscale-bundle-data

docker run -d \
  --name tokscale-bundle \
  --restart unless-stopped \
  -p 4787:4787 \
  -v tokscale-bundle-data:/data \
  tokscale-bundle
```

Or run with an explicit host directory, which is convenient if you may later run the binary directly:

```bash
mkdir -p "$HOME/.local/share/tokscale-bundle"

docker run -d \
  --name tokscale-bundle \
  --restart unless-stopped \
  -p 4787:4787 \
  -v "$HOME/.local/share/tokscale-bundle:/data" \
  tokscale-bundle
```

The default container command is:

```bash
tokscale-bundle serve --bind 0.0.0.0:4787 --storage /data --submit
```

That means incoming device payloads are stored at:

```text
/data/payloads/<tokscale-token-hash>/devices/<device-id>.json
```

You can stop and replace the container without losing records:

```bash
docker rm -f tokscale-bundle

docker run -d \
  --name tokscale-bundle \
  --restart unless-stopped \
  -p 4787:4787 \
  -v tokscale-bundle-data:/data \
  tokscale-bundle
```

To inspect the server:

```bash
curl http://localhost:4787/health

curl -H "Authorization: Bearer $TOKSCALE_API_TOKEN" \
  http://localhost:4787/api/hub/status
```

For a public server, put HTTPS in front of this process with a reverse proxy or load balancer. Each device should use the public HTTPS origin plus a stable device id:

```bash
export TOKSCALE_API_URL="https://your-server.example.com/devices/macbook-pro"
bunx tokscale@latest submit
```

Keep the same Tokscale API token and the same device id to update the existing device file. Changing the Tokscale token creates a separate token-hash directory; changing the device id creates another device file.

## Agent Prompts

Run these in a local coding agent such as Claude Code or Codex.

### Server Payload Proxy

```text
I want to run tokscale-bundle as my multi-device Tokscale payload proxy.

Steps:
1. Clone or reuse https://github.com/gigio1023/tokscale-bundle on my server.
2. Run `cargo run -- serve --bind 0.0.0.0:4787 --storage /var/lib/tokscale-bundle --submit`.
3. Tell me the `TOKSCALE_API_URL` value each device should use, with a unique `/devices/<device-id>` path.
4. Do not copy raw local sessions to the server.
```

### Source Device Payload Submit

```text
I want this device to submit its Tokscale usage through my payload proxy server.

Use this device id: <DEVICE_ID>.
Use this server URL: https://<SERVER_HOST>.

Steps:
1. Set `TOKSCALE_API_URL=https://<SERVER_HOST>/devices/<DEVICE_ID>`.
2. Run `bunx tokscale@latest submit`.
3. Show me whether the proxy accepted the payload.

Do not export raw sessions unless the payload proxy fails.
```

### Bundle Fallback Source Device

```text
I want to export this device's Tokscale-discoverable local session data for transfer to another device.

Use /tmp/tokscale-bundle-device-source.zip as the output archive.

Steps:
1. Clone or reuse https://github.com/gigio1023/tokscale-bundle.
2. Run `git submodule update --init --recursive`.
3. Run `cargo run -- export --output /tmp/tokscale-bundle-device-source.zip`.
4. Show me the archive path and the manifest client summary.

Do not copy Tokscale credentials. Do not run `tokscale submit`.
```

### Bundle Fallback Destination Device

```text
I have a tokscale-bundle archive at <PASTE_ARCHIVE_PATH>.

Use it to prepare one combined Tokscale submission that includes:
- imported sessions from the archive
- this device's local Tokscale-discoverable sessions

Steps:
1. Clone or reuse https://github.com/gigio1023/tokscale-bundle.
2. Run `git submodule update --init --recursive`.
3. Run `cargo run -- unpack <PASTE_ARCHIVE_PATH>` and capture `unpack_root` and `fake_home`.
4. Run `cargo run -- add-local <unpack_root>` from my normal HOME.
5. Copy my existing `~/.config/tokscale/credentials.json` into `<fake_home>/.config/tokscale/credentials.json`.
6. Run the printed `bunx tokscale@latest submit --dry-run` command.
7. Summarize the clients, totals, and any skipped sources.

Do not run the final non-dry-run submit until I confirm.
```

## Flow

![Combined fake-home flow](docs/diagrams/combined-fake-home-flow.drawio.png)

## What This Tool Does

- `serve` receives official Tokscale submit payloads at `/api/submit` or `/devices/<device-id>/api/submit`.
- `serve` stores one latest payload per device and per Tokscale token hash.
- `serve` merges payloads by date, client, model, and provider.
- `serve --submit` forwards the merged payload to upstream Tokscale immediately after each incoming device payload.
- `serve` exposes `/api/hub/status`, `/api/hub/preview`, `/api/hub/push`, and `DELETE /api/hub/device/<device-id>`.
- `upload` creates a raw-data bundle and uploads it to `/api/bundles` for fallback fake-home submission.
- `export`, `unpack`, `add-local`, and `cleanup` support the manual fake-home flow.

## Payload Proxy Quick Start

Run this on your server:

```bash
git clone https://github.com/gigio1023/tokscale-bundle.git
cd tokscale-bundle
cargo run -- serve --bind 0.0.0.0:4787 --storage /var/lib/tokscale-bundle --submit
```

Run this from each device, using a different device id in the URL path:

```bash
export TOKSCALE_API_URL="https://your-server.example.com/devices/macbook-pro"
bunx tokscale@latest submit
```

Example device URL plan:

```bash
# MacBook
export TOKSCALE_API_URL="https://your-server.example.com/devices/macbook-pro"

# Desktop
export TOKSCALE_API_URL="https://your-server.example.com/devices/desktop"

# Work laptop
export TOKSCALE_API_URL="https://your-server.example.com/devices/work-laptop"
```

Important: official `tokscale submit` does not send a device id header. If every device uses `https://your-server.example.com` without a `/devices/<device-id>` path, all devices are stored as `default` and overwrite each other.

The hub authenticates payload proxy requests with the normal `Authorization: Bearer <tokscale api token>` header sent by official `tokscale submit`.

With `--submit`, the hub forwards the merged payload to `https://tokscale.ai/api/submit` using the same bearer token. Without `--submit`, payloads are stored and merged locally until you push manually:

```bash
curl -X POST -H "Authorization: Bearer $TOKSCALE_API_TOKEN" \
  https://your-server.example.com/api/hub/push
```

Inspect stored payload state:

```bash
curl -H "Authorization: Bearer $TOKSCALE_API_TOKEN" \
  https://your-server.example.com/api/hub/status

curl -H "Authorization: Bearer $TOKSCALE_API_TOKEN" \
  https://your-server.example.com/api/hub/preview
```

Payload proxy endpoints:

- `POST /api/submit`
- `POST /devices/<device-id>/api/submit`
- `GET /api/hub/status`
- `GET /api/hub/preview`
- `POST /api/hub/push`
- `DELETE /api/hub/device/<device-id>`

## Bundle Fallback Server

Bundle fallback uploads raw Tokscale-discoverable data to the server. Prefer payload proxy mode unless you specifically need fake-home scanning.

Run this on the authenticated server. The server must already have Tokscale credentials at `~/.config/tokscale/credentials.json`.

```bash
git clone https://github.com/gigio1023/tokscale-bundle.git
cd tokscale-bundle
git submodule update --init --recursive

export TOKSCALE_BUNDLE_TOKEN="$(openssl rand -hex 32)"
cargo run -- serve --bind 0.0.0.0:4787 --storage /var/lib/tokscale-bundle --submit
```

Upload from each device:

```bash
export TOKSCALE_BUNDLE_TOKEN="same-token-as-server"
cargo run -- upload \
  --server https://your-server.example.com \
  --device-name "MacBook Pro"
```

`upload` persists a local device identity at `~/.config/tokscale-bundle/device.json`. The server stores uploads under `storage/devices/<device-id>/`, keeps the latest zip for each device, rebuilds `storage/combined/home`, copies the server's Tokscale credentials into that fake home, and submits if `--submit` is set.

Bundle endpoints:

- `GET /health`
- `POST /api/bundles` with `Authorization: Bearer <token>` and a zip body

## Manual Fake-Home CLI

Run this section step by step. Blocks labeled as example output are not commands.

### Source Device

Set up and export:

```bash
git clone https://github.com/gigio1023/tokscale-bundle.git
cd tokscale-bundle
git submodule update --init --recursive

cargo run -- export --output /tmp/tokscale-bundle-device-source.zip
```

Example output, do not type:

```text
Wrote bundle with 12 entries to /tmp/tokscale-bundle-device-source.zip
```

Inspect the archive:

```bash
unzip -p /tmp/tokscale-bundle-device-source.zip manifest.json \
  | jq '.entries[] | .client' | sort | uniq -c
```

Move `/tmp/tokscale-bundle-device-source.zip` to the destination device.

### Destination Device

Set up:

```bash
git clone https://github.com/gigio1023/tokscale-bundle.git
cd tokscale-bundle
git submodule update --init --recursive
```

Unpack the archive:

```bash
cargo run -- unpack /path/to/tokscale-bundle-device-source.zip \
  | tee /tmp/tokscale-bundle-unpack.txt
```

Example output, do not type:

```text
unpack_root=/tmp/tokscale-bundle-abc123
fake_home=/tmp/tokscale-bundle-abc123/home
settings_path=/tmp/tokscale-bundle-abc123/home/.config/tokscale/settings.json
manual submit: imported-only v1
mkdir -p "/tmp/tokscale-bundle-abc123/home/.config/tokscale"
cp ~/.config/tokscale/credentials.json "/tmp/tokscale-bundle-abc123/home/.config/tokscale/credentials.json"
HOME="/tmp/tokscale-bundle-abc123/home" XDG_CONFIG_HOME="/tmp/tokscale-bundle-abc123/home/.config" XDG_DATA_HOME="/tmp/tokscale-bundle-abc123/home/.local/share" CODEX_HOME="/tmp/tokscale-bundle-abc123/home/.codex" HERMES_HOME="/tmp/tokscale-bundle-abc123/home/.hermes" bunx tokscale@latest submit
```

Capture the generated paths:

```bash
UNPACK_ROOT="$(awk -F= '/^unpack_root=/{print $2}' /tmp/tokscale-bundle-unpack.txt)"
FAKE_HOME="$(awk -F= '/^fake_home=/{print $2}' /tmp/tokscale-bundle-unpack.txt)"
```

Add this device's local sessions:

```bash
cargo run -- add-local "$UNPACK_ROOT"
```

Example output, do not type:

```text
unpack_root=/tmp/tokscale-bundle-abc123
fake_home=/tmp/tokscale-bundle-abc123/home
settings_path=/tmp/tokscale-bundle-abc123/home/.config/tokscale/settings.json
added local entries=8
combined submit: imported + local replay
HOME="/tmp/tokscale-bundle-abc123/home" XDG_CONFIG_HOME="/tmp/tokscale-bundle-abc123/home/.config" XDG_DATA_HOME="/tmp/tokscale-bundle-abc123/home/.local/share" CODEX_HOME="/tmp/tokscale-bundle-abc123/home/.codex" HERMES_HOME="/tmp/tokscale-bundle-abc123/home/.hermes" bunx tokscale@latest submit --dry-run
HOME="/tmp/tokscale-bundle-abc123/home" XDG_CONFIG_HOME="/tmp/tokscale-bundle-abc123/home/.config" XDG_DATA_HOME="/tmp/tokscale-bundle-abc123/home/.local/share" CODEX_HOME="/tmp/tokscale-bundle-abc123/home/.codex" HERMES_HOME="/tmp/tokscale-bundle-abc123/home/.hermes" bunx tokscale@latest submit
```

Copy credentials and preview the combined submit:

```bash
mkdir -p "$FAKE_HOME/.config/tokscale"
cp ~/.config/tokscale/credentials.json "$FAKE_HOME/.config/tokscale/credentials.json"

HOME="$FAKE_HOME" XDG_CONFIG_HOME="$FAKE_HOME/.config" XDG_DATA_HOME="$FAKE_HOME/.local/share" CODEX_HOME="$FAKE_HOME/.codex" HERMES_HOME="$FAKE_HOME/.hermes" bunx tokscale@latest submit --dry-run
```

Submit only after the dry run looks correct:

```bash
HOME="$FAKE_HOME" XDG_CONFIG_HOME="$FAKE_HOME/.config" XDG_DATA_HOME="$FAKE_HOME/.local/share" CODEX_HOME="$FAKE_HOME/.codex" HERMES_HOME="$FAKE_HOME/.hermes" bunx tokscale@latest submit
```

Clean up after submission:

```bash
cargo run -- cleanup "$UNPACK_ROOT"
```

## Commands

```bash
cargo run -- serve --bind 0.0.0.0:4787 --storage /var/lib/tokscale-bundle --submit
cargo run -- upload --server https://your-server.example.com --device-name "MacBook Pro"
cargo run -- export --output <bundle.zip>
cargo run -- unpack <bundle.zip>
cargo run -- add-local <unpack_root>
cargo run -- cleanup <unpack_root>
```

## Requirements

- Docker for container mode
- macOS or Linux with a Rust toolchain for direct binary mode
- Official `tokscale` runnable through `bunx tokscale@latest`
- Git submodules initialized for fake-home bundle operations
- Optional: `jq`, `unzip`

## Safety and Limits

- Payload proxy mode sends only the official Tokscale submit payload JSON to your server.
- Payload proxy storage is partitioned by Tokscale bearer-token hash and device id.
- Docker mode persists payload proxy state only when `/data` is backed by a Docker volume or bind-mounted host directory.
- Put TLS and public auth/rate limits in front of `serve` if it is internet-facing.
- The Docker image is intended for payload proxy mode. The older bundle fallback server path can invoke `bunx tokscale@latest` during server-side fake-home submission, and this image does not install Bun.
- In bundle fallback mode, credentials are never bundled. Copy credentials only on the submitting server or destination device.
- Fixed-path clients such as Kilo, Hermes, Synthetic, and Crush can still collide in fake-home bundle mode because upstream `tokscale` discovers them at fixed paths inside one `HOME`.
- Windows is not supported in v1.
