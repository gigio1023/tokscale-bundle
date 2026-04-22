# Tokscale Bundle Design

**Date:** 2026-04-22
**Purpose:** Design a companion tool that exports tokscale-discoverable session data from one machine and replays it on another machine for manual imported-data submission.
**Scope:** macOS and Linux source/destination hosts. Windows excluded.

## Problem

The user wants a workflow that:

1. Auto-discovers the exact on-disk data that tokscale would scan.
2. Packages that data into a single transferable artifact.
3. Replays that artifact on another machine.
4. Lets a human run plain `tokscale` against the replayed imported data.

## Decision

Build a **Rust companion CLI** in a separate repository, not a TypeScript wrapper.

Why:

- `tokscale` discovery, parsing, graph generation, and payload shape already live in Rust.
- Reusing those types directly is safer than bridging through JSON or CLI text output.
- A Rust CLI can stay very small if it focuses only on packaging and replay materialization.

## Architecture

### 1. Export

`tokscale-bundle export`

- Uses `tokscale-core` scanner logic to discover the same files and databases tokscale would parse.
- Copies discovered artifacts into a deterministic replay layout:
  - supported extra-root clients under `.tokscale-bundle/extra/<client>/...`
  - unsupported special cases into fake-home defaults:
    - Kilo DB
    - Hermes DB
    - Crush synthetic registry + DB copies
    - OpenCode SQLite DBs
- Writes `manifest.json` with metadata, replay paths, and checksums.
- Produces a single `.zip` archive.

### 2. Replay / Cleanup

`tokscale-bundle unpack <archive.zip>`

- Unpacks the archive into a persistent temp fake-home.
- Writes `home/.config/tokscale/settings.json` so plain `tokscale` can rediscover the replayed imported data.
- Prints the exact manual submit commands:
  - create fake-home config dir
  - copy credentials into the fake-home
  - run `HOME=<fake_home> tokscale submit --no-spinner`
- Does not read credentials, merge destination-local data, or make network requests.

`tokscale-bundle cleanup <unpack-root>`

- Removes an unpack root created by `tokscale-bundle unpack`.
- Refuses recursive deletion unless the target path canonicalizes to a `tokscale-bundle-*` root containing both `manifest.json` and `home/`.

### 3. Reuse Boundary

Reused directly from `tokscale-core`:

- client definitions and path resolution
- scanner settings semantics
- scan result generation

Implemented locally in the companion:

- bundle manifest
- zip packaging/unpacking
- fake-home replay layout
- fake-home settings materialization
- unpack-root cleanup guardrails

## Non-Goals

- No attempt to patch the public tokscale service.
- No Windows support in v1.
- No remote provider refresh parity beyond what is already cached on disk.
- No automatic submit in v1.
- No merge with destination machine local data in v1.
- No publishing/distribution automation in v1.

## Main Risks

- Some provider data freshness depends on caches populated by tokscale or the upstream client.
- Crush replay needs a synthetic registry because tokscale discovers its DBs indirectly.
- Fake-home replay must materialize scanner settings in the exact place plain `tokscale` expects.

## v1 Success Criteria

- Export creates a single archive from a source machine without manual path enumeration.
- Unpack on another machine creates a temp fake-home that plain `tokscale` can scan without extra manual path editing.
- Cleanup safely removes only unpack roots created by this tool.
- Focused tests cover replay layout, unpack settings materialization, and cleanup safety.
