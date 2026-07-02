# Portable Dev Environment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (this plan is credential-gated and sequential — inline execution recommended over subagents). Steps use checkbox (`- [ ]`) syntax.

**Goal:** obelisk-arena builds from a bare `git clone` on any machine (git deps replace `../` path deps), inside a nix devShell, with a documented sibling-checkout escape hatch for co-development.

**Architecture:** Hosting first (git deps can't resolve until code is pushed), then convert manifests bottom-up (bevy_modal_editor pin → obelisk-bevy → obelisk-arena → arena_editor), then the co-dev patch tooling, the flake, and the clean-clone proof gate.

**Tech Stack:** cargo git dependencies + config-level `[patch]`, nix flake (rust-overlay, flake-utils), gh CLI (via `nix run`).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-02-portable-dev-env-design.md`. No code/behavior changes — manifests, configs, docs, flake only.
- Git dep URLs are **https** (public repos, anonymous fetch): `https://github.com/vothuul/obelisk` (branch `master`), `https://github.com/LukeThayer/bevy-obelisk` (branch `main`), `https://github.com/LukeThayer/bevy_modal_editor` (branch `main`). Pushes use SSH remotes.
- `bevy_egui` pinned to rev `81904dac9a09d49563e39962bf3039afc47016dc` in bevy_modal_editor + arena_editor manifests.
- arena_editor stays its own standalone workspace; build it from `crates/arena_editor` only. Plain cargo on this mac outside the flake still works (rustup toolchain remains).
- Gates after each conversion: the repo's own build/tests; after obelisk-arena conversion additionally obelisk-bevy goldens byte-identical + net-test PASS (retry ≤3×). 12 stat_core dead-code warnings allowed.
- Same-repo path deps (arena_skills/arena_sim within obelisk-arena; `crates/*` within bevy_modal_editor) are NOT converted.
- Pushing is now part of the workflow for these four repos (user-approved convention change).

---

### Task 1: Hosting — fork, create, repoint, push everything

**Files:** none (git/GitHub state only). **USER-INTERACTIVE:** gh auth.

- [ ] **Step 1: auth gh (user does the browser dance once):**
```bash
nix run nixpkgs#gh -- auth login --hostname github.com --git-protocol ssh --web
nix run nixpkgs#gh -- auth status
```
Expected: logged in as LukeThayer. (vothuul pushes ride the existing SSH key on `git@github.com:vothuul/obelisk.git` — verify in Step 3; if the key isn't authorized for vothuul, ask the user whether to auth vothuul in gh too or add the key.)

- [ ] **Step 2: fork bevy_modal_editor + create obelisk-arena repo:**
```bash
nix run nixpkgs#gh -- repo fork Nub/bevy_modal_editor --org "" --clone=false --default-branch-only
nix run nixpkgs#gh -- repo create LukeThayer/obelisk-arena --public --description "1v1 arena fighter — obelisk combat + lightyear netcode + in-editor skill designer"
```

- [ ] **Step 3: repoint remotes + push all four repos:**
```bash
cd /Users/luke/src/bevy_modal_editor && git remote rename origin upstream && git remote add origin git@github.com:LukeThayer/bevy_modal_editor.git && git push -u origin main
cd /Users/luke/src/obelisk && git push origin master
cd /Users/luke/src/obelisk-bevy && git push origin main
cd /Users/luke/src/obelisk-arena && git remote add origin git@github.com:LukeThayer/obelisk-arena.git && git push -u origin master
```
Expected: all four up to date; `git status -sb` shows no `[ahead …]` anywhere.

- [ ] **Step 4 (no commit — state change only):** re-run `git remote -v` in all four repos and record the mapping in the execution notes.

---

### Task 2: Pin bevy_egui in bevy_modal_editor + push

**Files:** Modify `/Users/luke/src/bevy_modal_editor/Cargo.toml:13,95`

- [ ] **Step 1:** replace both occurrences of
```toml
bevy_egui = { git = "https://github.com/vladbat00/bevy_egui", branch = "main" }
```
with
```toml
# Pinned: bevy_egui main moved to Bevy 0.19 (needs rustc 1.95); this rev is the last Bevy 0.18-compatible one.
bevy_egui = { git = "https://github.com/vladbat00/bevy_egui", rev = "81904dac9a09d49563e39962bf3039afc47016dc" }
```
(line 13 = `[workspace.dependencies]`, line 95 = the `[patch.crates-io]` table — both must move together or the patch mismatches.)

- [ ] **Step 2:** `cd /Users/luke/src/bevy_modal_editor && cargo build` — expected: resolves + builds clean (rev == what main pointed at when the lock was made, so no version drift).
- [ ] **Step 3:** commit + push:
```bash
git add Cargo.toml Cargo.lock && git commit -m "build: pin bevy_egui to last Bevy-0.18 rev (main moved to 0.19)" && git push
```

---

### Task 3: obelisk-bevy → git deps on obelisk + push

**Files:** Modify `/Users/luke/src/obelisk-bevy/Cargo.toml:22-25`

- [ ] **Step 1:** replace
```toml
stat_core = { path = "../obelisk/stat_core" }
loot_core = { path = "../obelisk/loot_core" }
skill_tree = { path = "../obelisk/skill_tree" }
tables_core = { path = "../obelisk/tables_core" }
```
with
```toml
stat_core = { git = "https://github.com/vothuul/obelisk", branch = "master" }
loot_core = { git = "https://github.com/vothuul/obelisk", branch = "master" }
skill_tree = { git = "https://github.com/vothuul/obelisk", branch = "master" }
tables_core = { git = "https://github.com/vothuul/obelisk", branch = "master" }
```
- [ ] **Step 2 (gate):**
```bash
cd /Users/luke/src/obelisk-bevy && cargo test --features test-support --lib --tests && cargo test --features test-support --test golden
```
Expected: suite green, goldens byte-identical (the fetched rev == local master — Task 1 pushed it).
- [ ] **Step 3:** commit + push: `git add Cargo.toml Cargo.lock && git commit -m "build: obelisk crates via git deps (portable clone; was ../obelisk path deps)" && git push`

---

### Task 4: obelisk-arena root workspace → git deps

**Files:** Modify `/Users/luke/src/obelisk-arena/Cargo.toml` (workspace.dependencies), `/Users/luke/src/obelisk-arena/crates/arena_skills/Cargo.toml:18`

- [ ] **Step 1:** in root `[workspace.dependencies]` replace the three cross-repo lines (keep their doc comments):
```toml
obelisk-bevy = { git = "https://github.com/LukeThayer/bevy-obelisk", branch = "main" }
stat_core = { git = "https://github.com/vothuul/obelisk", branch = "master" }
bevy_vfx = { git = "https://github.com/LukeThayer/bevy_modal_editor", branch = "main" }
```
In `crates/arena_skills/Cargo.toml:18` (dev-dependency) replace with:
```toml
obelisk-bevy = { git = "https://github.com/LukeThayer/bevy-obelisk", branch = "main", features = ["test-support"] }
```
- [ ] **Step 2 (gate):**
```bash
cd /Users/luke/src/obelisk-arena && cargo build && cargo test
pkill -f arena-server; pkill -f arena-client; sleep 1; bash crates/arena_game/tools/net-test/run_session.sh   # retry ≤3×
```
Expected: build/tests green; net-test `session PASS`.
- [ ] **Step 3:** commit (push at Task 6 with the tooling): `git add Cargo.toml Cargo.lock crates/arena_skills/Cargo.toml && git commit -m "build: cross-repo deps via git (portable clone; was ../ path deps)"`

---

### Task 5: arena_editor workspace → git deps + bevy_egui pin

**Files:** Modify `/Users/luke/src/obelisk-arena/crates/arena_editor/Cargo.toml`

- [ ] **Step 1:** replace the cross-repo deps (arena_sim/arena_skills stay `path` — same repo):
```toml
obelisk-bevy = { git = "https://github.com/LukeThayer/bevy-obelisk", branch = "main" }
bevy_modal_editor = { git = "https://github.com/LukeThayer/bevy_modal_editor", branch = "main" }
bevy_editor_game = { git = "https://github.com/LukeThayer/bevy_modal_editor", branch = "main" }
bevy_vfx = { git = "https://github.com/LukeThayer/bevy_modal_editor", branch = "main" }
stat_core = { git = "https://github.com/vothuul/obelisk", branch = "master" }
loot_core = { git = "https://github.com/vothuul/obelisk", branch = "master" }
bevy_egui = { git = "https://github.com/vladbat00/bevy_egui", rev = "81904dac9a09d49563e39962bf3039afc47016dc" }
```
dev-dependency `obelisk-bevy … features = ["test-support"]` gets the same git source. The `[patch.crates-io]` bevy_egui entry gets the same `rev` (replacing `branch = "main"`).
- [ ] **Step 2 (gate):** `cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo build && cargo test` — expected: ≥71 green, no new warnings. Check `git diff Cargo.lock` — bevy/bevy_egui versions must NOT drift (sources change, versions stay 0.18.x/0.39).
- [ ] **Step 3:** commit: `git add crates/arena_editor/Cargo.toml crates/arena_editor/Cargo.lock && git commit -m "build(editor): git deps + bevy_egui rev pin (portable clone)"`

---

### Task 6: co-dev escape hatch — dev-siblings.sh + gitignored patch configs

**Files:** Create `/Users/luke/src/obelisk-arena/tools/dev-siblings.sh`; Modify `.gitignore` in obelisk-arena and obelisk-bevy (add `.cargo/config.toml`)

- [ ] **Step 1:** create `tools/dev-siblings.sh` (mode +x):
```bash
#!/usr/bin/env bash
# Co-dev escape hatch: redirect the git deps to ../ sibling checkouts via config-level [patch]
# (git-ignored .cargo/config.toml — daily edit-lib-rebuild-game loop without push/update ceremony).
#   tools/dev-siblings.sh        clone-or-update siblings + write patch configs
#   tools/dev-siblings.sh --off  remove patch configs (back to pure git deps)
# Lockfile convention: commit Cargo.locks only with patches OFF (sync points).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; SRC="$(dirname "$ROOT")"
CONFIGS=("$ROOT/.cargo/config.toml" "$ROOT/crates/arena_editor/.cargo/config.toml" "$SRC/obelisk-bevy/.cargo/config.toml")

if [[ "${1:-}" == "--off" ]]; then rm -fv "${CONFIGS[@]}"; exit 0; fi

declare -A REPOS=(
  [obelisk]="https://github.com/vothuul/obelisk"
  [obelisk-bevy]="https://github.com/LukeThayer/bevy-obelisk"
  [bevy_modal_editor]="https://github.com/LukeThayer/bevy_modal_editor"
)
for name in "${!REPOS[@]}"; do
  if [[ -d "$SRC/$name/.git" ]]; then echo "sibling $name: present"; else git clone "${REPOS[$name]}" "$SRC/$name"; fi
done

obelisk_patch() { local rel="$1"; cat <<EOF
[patch."https://github.com/vothuul/obelisk"]
stat_core = { path = "$rel/obelisk/stat_core" }
loot_core = { path = "$rel/obelisk/loot_core" }
skill_tree = { path = "$rel/obelisk/skill_tree" }
tables_core = { path = "$rel/obelisk/tables_core" }
EOF
}
mkdir -p "$ROOT/.cargo" "$ROOT/crates/arena_editor/.cargo" "$SRC/obelisk-bevy/.cargo"
{ obelisk_patch ".."; cat <<'EOF'
[patch."https://github.com/LukeThayer/bevy-obelisk"]
obelisk-bevy = { path = "../obelisk-bevy" }
[patch."https://github.com/LukeThayer/bevy_modal_editor"]
bevy_vfx = { path = "../bevy_modal_editor/crates/bevy_vfx" }
EOF
} > "$ROOT/.cargo/config.toml"
{ obelisk_patch "../../../.."; cat <<'EOF'
[patch."https://github.com/LukeThayer/bevy-obelisk"]
obelisk-bevy = { path = "../../../../obelisk-bevy" }
[patch."https://github.com/LukeThayer/bevy_modal_editor"]
bevy_modal_editor = { path = "../../../../bevy_modal_editor" }
bevy_editor_game = { path = "../../../../bevy_modal_editor/crates/bevy_editor_game" }
bevy_vfx = { path = "../../../../bevy_modal_editor/crates/bevy_vfx" }
EOF
} > "$ROOT/crates/arena_editor/.cargo/config.toml"
obelisk_patch ".." > "$SRC/obelisk-bevy/.cargo/config.toml"
echo "patch configs written (run with --off to remove)"
```
(Relative `path` values in a config-level `[patch]` resolve against the directory containing the `.cargo` directory — verify empirically in Step 2 and fix the `rel` args if cargo resolves them differently.)

- [ ] **Step 2 (verify the mechanism):** run `tools/dev-siblings.sh`, then `cd /Users/luke/src/obelisk-arena && cargo metadata --format-version 1 | grep -c "path+file"` — expected: obelisk-bevy/stat_core/bevy_vfx show as `path+file://` sources (patch active). Then `tools/dev-siblings.sh --off`, re-run — back to `git+https`. Restore patches ON afterward (this machine keeps the local-dev loop) but `git checkout -- Cargo.lock` any churn before committing.
- [ ] **Step 3:** add `.cargo/config.toml` to both repos' `.gitignore`; commit obelisk-arena (`git add tools/dev-siblings.sh .gitignore && git commit -m "tools: dev-siblings co-dev patch script"`), commit + push obelisk-bevy's `.gitignore` change.

---

### Task 7: nix flake in obelisk-arena

**Files:** Create `/Users/luke/src/obelisk-arena/flake.nix` (+ generated `flake.lock`)

- [ ] **Step 1:** write `flake.nix`:
```nix
{
  description = "obelisk-arena dev environment (game + skill-designer editor)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        darwinDeps = pkgs.lib.optionals pkgs.stdenv.isDarwin [
          pkgs.apple-sdk_15
          pkgs.libiconv
        ];

        # Bevy runtime/link deps on Linux (windowed client + editor: winit/wgpu/audio/input)
        linuxDeps = pkgs.lib.optionals pkgs.stdenv.isLinux (with pkgs; [
          vulkan-loader
          xorg.libX11 xorg.libXcursor xorg.libXi xorg.libXrandr
          libxkbcommon
          wayland
          alsa-lib
          udev
          libglvnd # EGL for wayland
        ]);
      in {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = darwinDeps ++ linuxDeps;
          packages = [ rustToolchain pkgs.cargo-watch pkgs.cargo-edit pkgs.git ];

          # Bevy dlopens vulkan/x11/wayland at runtime on Linux
          LD_LIBRARY_PATH = pkgs.lib.optionalString pkgs.stdenv.isLinux
            (pkgs.lib.makeLibraryPath linuxDeps);

          shellHook = ''
            echo "obelisk-arena dev shell — $(rustc --version)"
          '' + pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            export WAYLAND_DISPLAY=''${WAYLAND_DISPLAY:-wayland-1}
            export XDG_RUNTIME_DIR=''${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
          '';
        };
      });
}
```
- [ ] **Step 2 (gate, on this mac):** `cd /Users/luke/src/obelisk-arena && nix develop -c cargo build -p arena_sim` (a small crate proves toolchain+SDK; full builds already gated). Expected: builds. If the darwin SDK hook misbehaves (DEVELOPER_DIR errors), fall back to BTDD's stub-derivation `inputsFrom` trick (see `/Users/luke/src/beneath_the_dreaming_deep/flake.nix` devShell).
- [ ] **Step 3:** commit: `git add flake.nix flake.lock && git commit -m "nix: devShell flake (linux+darwin; rust toolchain + bevy system libs; no graphviz)"`

---

### Task 8: DEVELOPING.md + clean-clone proof gate + push + docs

**Files:** Create `/Users/luke/src/obelisk-arena/DEVELOPING.md`

- [ ] **Step 1:** write `DEVELOPING.md`:
```markdown
# Developing obelisk-arena

## New machine
1. SSH keys authorized for github.com/LukeThayer (and vothuul if pushing obelisk).
2. `git clone git@github.com:LukeThayer/obelisk-arena.git && cd obelisk-arena`
3. `nix develop` (Linux needs nix + flakes enabled; macOS also works, or use rustup + plain cargo)
4. `cargo build && cargo test` — the game workspace.
5. `cd crates/arena_editor && cargo build` — the editor is its OWN cargo workspace (never `-p arena_editor` from the root); run it with `cargo run --bin arena-editor`, press `K` for Skill mode.

All external deps are git dependencies (obelisk, bevy-obelisk, bevy_modal_editor fork) pinned by the committed Cargo.locks — no sibling checkouts needed to build.

## Co-developing the libraries
`tools/dev-siblings.sh` clones the three library repos as `../` siblings and writes git-ignored
`.cargo/config.toml` `[patch]` files redirecting the git deps to them. Edit libs + game together with
instant rebuilds; `tools/dev-siblings.sh --off` returns to pure git deps.
**Sync point** (publishing lib changes): commit+push the lib, then in consumers `cargo update -p <crate>`
with patches OFF and commit the lock. Never commit a Cargo.lock generated with patches ON.

## Verification
- Golden combat traces (in ../obelisk-bevy): `cargo test --features test-support --test golden` — byte-identical, no UPDATE_GOLDEN.
- Net-test: `pkill -f arena-server; pkill -f arena-client; sleep 1; bash crates/arena_game/tools/net-test/run_session.sh` (flaky: retry ≤3×, one PASS = green).
- Editor suite: `cd crates/arena_editor && cargo test`.

## Pinned things (do not "fix")
- `bevy_egui` rev `81904da` everywhere (its main moved to Bevy 0.19 / rustc 1.95).
- avian3d 0.5 (pinned by lightyear_avian3d 0.26).
- arena_editor's Cargo.lock (bevy 0.18 set) — no blanket `cargo update` there.
```
- [ ] **Step 2:** commit + push obelisk-arena: `git add DEVELOPING.md && git commit -m "docs: DEVELOPING.md (new-machine setup, co-dev workflow, pins)" && git push`
- [ ] **Step 3 (PROOF GATE — clean clone, no siblings, patches off):**
```bash
D=$(mktemp -d) && git clone git@github.com:LukeThayer/obelisk-arena.git "$D/obelisk-arena"
cd "$D/obelisk-arena" && cargo build && (cd crates/arena_editor && cargo build)
```
Expected: both builds succeed with deps fetched purely from git. On this mac also: `nix develop -c cargo build -p arena_sim` in the clone. Clean up `$D`.
- [ ] **Step 4:** re-enable local patches (`tools/dev-siblings.sh`), run the standard gates once more from the real checkout (obelisk-bevy goldens, net-test, arena_editor suite) to confirm the daily-driver mode is intact. Update project memory + the handoff doc (portable env done; never-push convention retired for these repos; commit the docs in obelisk-bevy).
```

## Self-review notes
- Spec coverage: hosting→T1, conversion+pin→T2-5, escape hatch→T6, flake→T7, docs+proof→T8. ✔
- The one empirical unknown is flagged where it bites: config-level `[patch]` relative-path resolution (T6 Step 2 verifies, with instruction to adjust).
- Type/name consistency: URLs and rev strings identical across T2/T4/T5/T6/T8. ✔
