# Portable Dev Environment (git deps + nix flake) — Design

**Goal:** obelisk-arena becomes self-contained to clone and build on a new Linux machine (and this mac): `git clone && nix develop && cargo build`. No required sibling-repo layout; no code/behavior changes.

**Date:** 2026-07-02 · **Decided with user:** git deps (not path deps/submodules/monorepo) · devShell-only flake in obelisk-arena · Linux+macOS · keep repo owners, fill gaps, all public.

## 1. Hosting

- Fork `Nub/bevy_modal_editor` → `LukeThayer/bevy_modal_editor` (public; keeps upstream-PR link). Repoint local `origin` to the fork (keep `upstream` = Nub).
- Create `LukeThayer/obelisk-arena` (public), add as `origin`.
- Push all local-ahead work: obelisk (+2, vothuul/obelisk), obelisk-bevy (+19, LukeThayer/bevy-obelisk), bevy_modal_editor (+5, to fork), obelisk-arena (all).
- Convention change: these repos are now pushed at every sync point (consumers fetch from git).

## 2. Cargo conversion

Replace every cross-repo `path = "../…"` dep with `git = "<repo url>", branch = "<repo default>"` (obelisk uses `master`, the rest `main`). The table below is the expected set — the implementer verifies against the actual manifests; the rule "every cross-repo path dep" governs:

| Manifest | Deps converted |
|---|---|
| obelisk-arena `crates/{arena_game,arena_sim,arena_skills}` | obelisk-bevy; stat_core (obelisk); bevy_vfx (bevy_modal_editor fork) |
| obelisk-arena `crates/arena_editor` (standalone workspace — unchanged isolation) | obelisk-bevy; stat_core, loot_core (obelisk); bevy_modal_editor, bevy_editor_game, bevy_vfx (fork) |
| **obelisk-bevy** (cascade: path deps inside a git dep don't resolve) | stat_core, loot_core, skill_tree, tables_core (obelisk) |

Rules:
- Pinning = committed `Cargo.lock`s (git deps lock to exact revs). Sync point: push lib → `cargo update -p <crate>` in consumer.
- **Pin `bevy_egui` to rev `81904da` in the manifests** of bevy_modal_editor and arena_editor (its `main` moved to Bevy 0.19 / rustc 1.95 — a fresh-resolve trap). Keep arena_editor's `[patch.crates-io]` consistent with the pin.
- Multiple crates from one repo is fine (cargo resolves crates within a git repo's workspace by name).

## 3. Co-dev escape hatch

For edit-lib-and-game-together sessions: a **git-ignored** `.cargo/config.toml` in each consumer with `[patch."<git url>"]` tables redirecting to `../` sibling checkouts. New script `tools/dev-siblings.sh` in obelisk-arena: clones/updates the three siblings into `../` and writes the patch configs (`--off` removes them). Verify config-level `[patch]` works with our cargo at implementation time; fallback = documented `[patch]` block pasted into Cargo.toml (never committed).

## 4. Nix flake (obelisk-arena, devShell only)

BTDD's devShell pattern, no packaging, **no graphviz** (the aarch64-darwin breaker):

- Inputs: nixpkgs (nixos-unstable), flake-utils, rust-overlay. `eachDefaultSystem`.
- Toolchain: `rust-bin.stable.latest.default` + rust-src, rust-analyzer (lockfile pins the actual version; ≥1.93 required).
- Linux: alsa-lib, udev, vulkan-loader, xorg.{libX11,libXcursor,libXi,libXrandr}, libxkbcommon, wayland, libglvnd + `LD_LIBRARY_PATH = makeLibraryPath linuxDeps` + Wayland env shellHook (BTDD verbatim).
- Darwin: apple-sdk_15, libiconv (BTDD's stub-derivation `inputsFrom` trick for the SDK hook).
- Common: pkg-config, cargo-watch, cargo-edit, git.
- Commit `flake.nix` + `flake.lock`.

## 5. Docs + verification

- `DEVELOPING.md` in obelisk-arena: new-machine setup (SSH keys for LukeThayer + vothuul, clone, `nix develop`, `cargo build`; arena_editor built from its own dir as ever), co-dev sibling setup, sync-point workflow, net-test/golden commands.
- Gates (all must stay green; the conversion must not change any resolved dependency versions except as pinned): obelisk-bevy goldens byte-identical · net-test PASS (retry ≤3) · arena_editor suite + build · obelisk `cargo test -p stat_core`.
- **Proof gate:** clean clone of obelisk-arena into a temp dir with NO siblings present → `cargo build` (workspace) and `cd crates/arena_editor && cargo build` succeed. On this mac additionally: `nix develop -c cargo build` works.

## Out of scope

Repo merges/renames; nix-built packages; wisp; CI; changing arena_editor's standalone-workspace isolation; any gameplay/editor code.
