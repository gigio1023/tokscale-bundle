# tokscale-bundle

Move Tokscale-discoverable local session data from one device to another, then submit one combined local view with plain `tokscale`.

Use this when multiple devices have independent Claude Code, Codex, Gemini, OpenCode, Kilo, Hermes, Crush, or similar local sessions, but you want Tokscale collection and parsing to stay exactly upstream.

## Agent Prompts

Run these in a local coding agent such as Claude Code or Codex.

### Source Device

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

### Destination Device

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
6. Run `HOME="<fake_home>" tokscale submit --dry-run --no-spinner`.
7. Summarize the clients, totals, and any skipped sources.

Do not run the final non-dry-run submit until I confirm.
```

## Flow

![Combined fake-home flow](docs/diagrams/combined-fake-home-flow.drawio.png)

## Why Not Just Zip and Unzip

Plain zip/unzip can work, but the manual workflow is easy to get wrong.

`tokscale-bundle` automates the parts that matter:

- **Discovery**: reuses `tokscale-core` scanner logic instead of hard-coding paths.
- **Replay layout**: stages imported data under a fake home that plain `tokscale` can scan.
- **Settings**: writes fake-home `settings.json` for extra scan roots and replay paths.
- **Local merge**: adds the destination device's local sessions into the same fake home.
- **Safety**: keeps credentials out of the archive and avoids overwriting fixed-path data.

The key idea: merge before submit, inside the fake home.

## Manual CLI

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
HOME="/tmp/tokscale-bundle-abc123/home" tokscale submit --no-spinner
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
HOME="/tmp/tokscale-bundle-abc123/home" tokscale submit --dry-run --no-spinner
HOME="/tmp/tokscale-bundle-abc123/home" tokscale submit --no-spinner
```

Copy credentials and preview the combined submit:

```bash
mkdir -p "$FAKE_HOME/.config/tokscale"
cp ~/.config/tokscale/credentials.json "$FAKE_HOME/.config/tokscale/credentials.json"

HOME="$FAKE_HOME" tokscale submit --dry-run --no-spinner
```

Submit only after the dry run looks correct:

```bash
HOME="$FAKE_HOME" tokscale submit --no-spinner
```

Clean up after submission:

```bash
cargo run -- cleanup "$UNPACK_ROOT"
```

## Commands

```bash
cargo run -- export --output <bundle.zip>
cargo run -- unpack <bundle.zip>
cargo run -- add-local <unpack_root>
cargo run -- cleanup <unpack_root>
```

## Requirements

- macOS or Linux
- Rust toolchain
- Git submodules initialized
- plain `tokscale` installed separately
- optional: `jq`, `unzip`

## Safety and Limits

- `tokscale-bundle` does not submit by itself.
- The final submit is always `HOME="<fake_home>" tokscale submit`.
- Credentials are never bundled. Copy credentials only on the destination device.
- For more than two devices, run `add-local` against the same `unpack_root` from each additional device. Importing multiple bundle archives into one existing `unpack_root` is not a CLI command yet.
- Some clients use fixed paths inside `HOME`. If imported data already occupies that path, `add-local` skips the local fixed-path source instead of overwriting it.
- Windows is not supported in v1.
