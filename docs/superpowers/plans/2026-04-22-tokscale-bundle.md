# Tokscale Bundle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust companion CLI that exports tokscale-discoverable session data into a transferable archive and later unpacks it into a fake-home for manual imported-only submission with plain `tokscale`.

**Architecture:** The CLI reuses `tokscale-core` for discovery, stages exported data into a fake-home replay layout, unpacks that layout into a persistent temp directory, materializes fake-home scanner settings for plain `tokscale`, and provides a guarded cleanup command.

**Tech Stack:** Rust, clap, zip, serde, tokscale-core

---

### Task 1: Project Skeleton

**Files:**
- Modify: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/main.rs`

- [ ] Add crate dependencies and module skeleton for CLI, manifest, layout, archive, replay, settings, and export flows.
- [ ] Ensure the binary can parse commands without implementing behavior yet.
- [ ] Run `cargo test` and confirm failing tests identify unimplemented behavior.

### Task 2: Replay Layout + Manifest

**Files:**
- Create: `src/manifest.rs`
- Create: `src/layout.rs`
- Test: `tests/layout.rs`

- [ ] Write failing tests for deterministic replay paths and synthetic scanner settings.
- [ ] Implement bundle manifest types and path mapping rules.
- [ ] Re-run `cargo test --test layout`.

### Task 3: Graph Merge

**Files:**
- Delete: `src/graph_merge.rs`
- Delete: `tests/graph_merge.rs`

- [ ] Remove graph merge from the crate surface and CLI flow.
- [ ] Re-run `cargo test` to confirm no code path still depends on merge logic.

### Task 4: Export / Replay Flow

**Files:**
- Create: `src/tokscale.rs`
- Create: `src/archive.rs`
- Create: `src/replay.rs`
- Modify: `src/main.rs`

- [ ] Implement source discovery using `tokscale-core`.
- [ ] Implement zip pack/unpack and replay materialization.
- [ ] Implement fake-home `settings.json` generation for plain `tokscale`.
- [ ] Implement guarded unpack-root cleanup.
- [ ] Run focused CLI smoke checks.

### Task 5: Verification

**Files:**
- Modify: `README.md` if needed

- [ ] Run `cargo test`.
- [ ] Run `cargo fmt --check`.
- [ ] Run local smoke commands such as `cargo run -- export --output /tmp/example.zip`, `cargo run -- unpack /tmp/example.zip`, and `cargo run -- cleanup <unpack-root>`.
