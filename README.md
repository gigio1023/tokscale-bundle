# tokscale-bundle

`tokscale-bundle` moves existing `tokscale`-discoverable local data from one machine to another.
It is written in Rust so it can reuse `tokscale-core` discovery and replay semantics directly.

It does three things:

- `export` creates a zip bundle from the source machine's local data
- `unpack` expands that bundle into a temporary fake-home for plain `tokscale`
- `cleanup` removes that temporary replay directory when you are done

It does **not** submit data itself. In v1, the submit step is manual and imported-data-only.

## Quick Start

1. On the source machine, create a bundle:

```bash
cargo run -- export --output /tmp/tokscale-bundle.zip
```

2. On the destination machine, unpack it:

```bash
cargo run -- unpack /tmp/tokscale-bundle.zip
```

3. Follow the commands printed by `unpack`.
   They tell you exactly how to:
   - create the fake-home config directory
   - copy `tokscale` credentials into it
   - run plain `tokscale submit --no-spinner` with `HOME=<fake_home>`

4. When you are done, remove the unpacked replay directory:

```bash
cargo run -- cleanup /tmp/tokscale-bundle-XXXXXX
```

## Notes

- macOS and Linux only
- reads disk-backed local data only
- does not do remote/provider refresh
- does not merge destination-local data with imported data in v1

## Requirements

- Rust toolchain
- Git submodules initialized
- plain `tokscale` installed separately for the manual submit step

```bash
git submodule update --init --recursive
```
