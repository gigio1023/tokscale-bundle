# tokscale-bundle

`tokscale-bundle` builds a temporary fake home that plain `tokscale` can scan.

Use it when `tokscale` server-side submissions do not merge multiple devices correctly and you need to submit one combined local view instead.

The intended flow is:

```text
Device A local sessions
  -> export to zip
  -> unpack on Device B into fake_home

Device B local sessions
  -> add-local into the same fake_home

same fake_home
  -> HOME=<fake_home> tokscale submit
```

The merge point is the fake home, before `tokscale submit` runs.

## What This Tool Does

- `export` copies Tokscale-discoverable local data from the current machine into a zip bundle.
- `unpack` creates a temporary fake home from a bundle.
- `add-local` copies the current machine's local data into an existing fake home.
- `cleanup` removes the unpacked fake-home directory after submission.

Important constraints:

- `tokscale-bundle` does not submit data by itself.
- Submission is always done by plain `tokscale submit`.
- Plain `tokscale` does not scan two home directories at once.
- To submit combined data, all data must be present in the same fake home before running `tokscale submit`.

## Quick Start

This example submits Device A + Device B data as one combined `tokscale submit` payload from Device B.

### 1. Create a bundle on Device A

```bash
git clone https://github.com/gigio1023/tokscale-bundle.git
cd tokscale-bundle
git submodule update --init --recursive

cargo run -- export --output /tmp/tokscale-bundle-device-a.zip
```

Expected output:

```text
Wrote bundle with 12 entries to /tmp/tokscale-bundle-device-a.zip
```

The entry count depends on what `tokscale` can discover on Device A.

### 2. Move the zip file to Device B

Transfer `/tmp/tokscale-bundle-device-a.zip` to Device B by whatever method works in your environment.

The transfer method is intentionally not prescribed. Use your normal file transfer workflow.

### 3. Unpack the bundle on Device B

```bash
git clone https://github.com/gigio1023/tokscale-bundle.git
cd tokscale-bundle
git submodule update --init --recursive

cargo run -- unpack /tmp/tokscale-bundle-device-a.zip | tee /tmp/tokscale-bundle-unpack.txt
```

Example output:

```text
unpack_root=/tmp/tokscale-bundle-abc123
fake_home=/tmp/tokscale-bundle-abc123/home
settings_path=/tmp/tokscale-bundle-abc123/home/.config/tokscale/settings.json
manual submit: imported-only v1
mkdir -p "/tmp/tokscale-bundle-abc123/home/.config/tokscale"
cp ~/.config/tokscale/credentials.json "/tmp/tokscale-bundle-abc123/home/.config/tokscale/credentials.json"
HOME="/tmp/tokscale-bundle-abc123/home" tokscale submit --no-spinner
```

Do not submit yet if you also need Device B data. The fake home contains Device A data only at this point.

### 4. Add Device B local data to the same fake home

Run `add-local` from the normal Device B shell, not with `HOME` pointed at the fake home.

```bash
cargo run -- add-local /tmp/tokscale-bundle-abc123
```

Example output:

```text
unpack_root=/tmp/tokscale-bundle-abc123
fake_home=/tmp/tokscale-bundle-abc123/home
settings_path=/tmp/tokscale-bundle-abc123/home/.config/tokscale/settings.json
added local entries=8
combined submit: imported + local replay
HOME="/tmp/tokscale-bundle-abc123/home" tokscale submit --dry-run --no-spinner
HOME="/tmp/tokscale-bundle-abc123/home" tokscale submit --no-spinner
```

At this point, the fake home contains:

- Device A data from the unpacked bundle
- Device B data added from the current machine
- scanner settings that point plain `tokscale` at both replay locations

### 5. Copy Tokscale credentials into the fake home

Device B must already have Tokscale credentials for the account you want to submit to.

Run the `mkdir` and `cp` commands printed by `unpack`.

Using the example output above:

```bash
mkdir -p "/tmp/tokscale-bundle-abc123/home/.config/tokscale"
cp ~/.config/tokscale/credentials.json "/tmp/tokscale-bundle-abc123/home/.config/tokscale/credentials.json"
```

Use the actual paths printed on your machine, not the example `tokscale-bundle-abc123` path.

If you copy credentials from a different account, the combined data will be submitted to that different account.

### 6. Check the combined data before submitting

```bash
HOME="/tmp/tokscale-bundle-abc123/home" tokscale submit --dry-run --no-spinner
```

Check that:

- the expected clients or sources from both devices are present
- the token and cost totals are plausible for Device A + Device B
- there is no credentials or authentication error

### 7. Submit once from the combined fake home

```bash
HOME="/tmp/tokscale-bundle-abc123/home" tokscale submit --no-spinner
```

This is the only submit needed for the combined Device A + Device B view.

Do not also run a separate normal `tokscale submit` for Device B if your goal is to avoid server-side multi-device overwrite behavior.

### 8. Clean up the unpacked replay directory

After submission, remove the unpacked directory:

```bash
cargo run -- cleanup /tmp/tokscale-bundle-abc123
```

Pass `unpack_root`, not `fake_home`.

From the example above:

- correct: `/tmp/tokscale-bundle-abc123`
- incorrect: `/tmp/tokscale-bundle-abc123/home`

## How Combined Submission Works

`tokscale` reads one `HOME` at a time.

`tokscale-bundle` makes a new fake home and places replay data inside it:

```text
/tmp/tokscale-bundle-abc123/
  manifest.json
  home/
    .config/tokscale/settings.json
    .tokscale-bundle/
      extra/...        # imported bundle data
      local/...        # add-local data
```

`settings.json` tells plain `tokscale` which replay paths to scan.

When you run:

```bash
HOME="/tmp/tokscale-bundle-abc123/home" tokscale submit --no-spinner
```

plain `tokscale` scans the fake home and produces one submit payload from that combined local view.

## What Gets Bundled or Added

`export` and `add-local` use `tokscale-core` discovery logic.

Included when discovered:

- local session files that plain `tokscale` can scan
- files discovered through scanner settings
- supported extra-root client files
- OpenCode DB replay paths
- Kilo CLI DB when the fake-home fixed path is still empty
- Hermes DB when the fake-home fixed path is still empty
- Crush DB plus replay registry
- Synthetic or Octofriend DB when the fake-home fixed path is still empty

Not included:

- Tokscale credentials
- remote provider refreshes
- Windows-specific paths

Fixed-path caveat:

- Some sources are discovered by plain `tokscale` only at fixed paths inside `HOME`.
- If the imported bundle already occupies that fixed path, `add-local` skips the local fixed-path source instead of overwriting imported data.
- The command prints `skipped ...` lines for those cases.

## Verification Commands

Inspect clients included in a bundle:

```bash
unzip -p /tmp/tokscale-bundle-device-a.zip manifest.json | jq '.entries[] | .client' | sort | uniq -c
```

Inspect generated fake-home scanner settings:

```bash
jq . /tmp/tokscale-bundle-abc123/home/.config/tokscale/settings.json
```

Verify what plain `tokscale` would submit from the fake home:

```bash
HOME="/tmp/tokscale-bundle-abc123/home" tokscale submit --dry-run --no-spinner
```

## Troubleshooting

### `credentials.json` does not exist

Authenticate plain `tokscale` on Device B first.

One way to force the normal Tokscale auth path is:

```bash
tokscale submit --dry-run --no-spinner
```

Then check:

```bash
ls -l ~/.config/tokscale/credentials.json
```

### `add-local` reports skipped sources

This means the fake home already has a fixed-path source from the imported bundle, and adding the local one would overwrite it.

The rest of the local replay data is still added. Review the skipped source before relying on totals for that client.

### `HOME="<fake_home>" tokscale submit` does not read Device B's real home

That is expected.

It should not read Device B's real home. Device B data must be added first with:

```bash
cargo run -- add-local <unpack_root>
```

After that, `HOME="<fake_home>" tokscale submit` reads Device B data from the fake-home replay paths.

### Profile or leaderboard totals go down after submitting

Possible causes:

- you submitted imported data and Device B local data separately instead of submitting once from the combined fake home
- credentials from the wrong Tokscale account were copied into the fake home
- a fixed-path source was skipped during `add-local`
- upstream `tokscale` server-side aggregation is still affecting existing submitted history

What to do:

- run `HOME="<fake_home>" tokscale submit --dry-run --no-spinner` before submitting
- submit once from the combined fake home
- verify the copied credentials belong to the intended account
- review any `skipped ...` lines from `add-local`

### `cleanup` fails

`cleanup` only removes directories that look like valid unpack roots.

Required shape:

- directory name starts with `tokscale-bundle-`
- `manifest.json` exists
- `home/` exists

Wrong:

```bash
cargo run -- cleanup /tmp/tokscale-bundle-abc123/home
```

Correct:

```bash
cargo run -- cleanup /tmp/tokscale-bundle-abc123
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

```bash
git submodule update --init --recursive
```

## Limitations

- no Windows support in v1
- exports disk-backed local data only
- does not run remote provider refreshes
- does not include credentials
- does not merge fixed-path SQLite databases when both imported and local data use the same fixed path
- submit is manual and always done through plain `tokscale`
