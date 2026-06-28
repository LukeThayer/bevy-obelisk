# Obelisk-Arena Play-Test Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix four play-test problems in the obelisk-arena 1v1 game: (A) make the view first-person, (B) make casting aim where the player looks, (C) add a hold-to-charge cast with charge/spell UI, (D) fix the broken multi-layer rig and add a customization UI synced over the network.

**Architecture:** obelisk-arena's client (`crates/arena_game/src/client/`) drives camera/input/HUD/rig; its server (`crates/arena_game/src/server/`) runs obelisk's deterministic combat; `crates/arena_skills` carries the wire-safe skill data. wisp (`../wisp`) is a first-person wizard game using the *byte-identical* `character.glb` — we port its `parts.rs` (rig layer visibility) and first-person/charge patterns. obelisk-bevy (`../obelisk-bevy`) is the deterministic combat crate; it already supports directional casting and gets one small, golden-safe `charge` field.

**Tech Stack:** Bevy 0.18.1, Avian3d 0.5.0, lightyear 0.26.4. Rust. The project gate is `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` green in `../obelisk-arena`, plus obelisk-bevy's 39 golden traces + ~84 tests for the charge change.

---

## Ground Truth (from code maps — cite these, re-read exact code before editing)

**obelisk-arena client:**
- Camera: `client/controller.rs` — `CAM_OFFSET = Vec3(0,2,4)` third-person follow (`follow_local_net_player`), `CameraYaw` (mouse-X, ~line 65), `AimPitch` (mouse-Y, clamped ±85°, ~line 70), pitch applied to `chest_joint` spine (~line 232). Camera spawned `client/mod.rs:520-524` with `FollowCamera` marker.
- Cast input: `client/mod.rs:250-264` Space/LMB → `CastIntent(Some("firebolt"))`. Send: `client/net.rs:185-261` `send_cast_requests` — finds nearest OTHER player, `aim_dir` = toward that target, ships `CastRequestMessage{skill_id,target_hint,aim_dir}` on reliable `CastChannel`. Server: `server/mod.rs:367-427` `drain_cast_requests` re-acquires via `ObeliskSpatial::nearest_enemy` (20u) then `cast_skill_at`.
- Cast message: `net/protocol.rs:136-145` `CastRequestMessage`.
- Cast feedback: animation only — `client/rig.rs:312-327` `LocalAnimBlend`/`step_casting_blend`, `client/rig.rs:407-443` `drive_animation` maps Windup/Active→1.0, Recovery→0.5. HUD `client/hud.rs:1-389` has HP bars + floating damage + round banner, **no cast bar / crosshair / spell indicator**.
- Rig: `client/mod.rs:535-539` `load_rig`; `client/present.rs:31-68` `attach_rig_to_players` spawns `ArenaBody` scene as child (π yaw offset line 59); `client/rig.rs:86-133` `build_graph_when_loaded`; **`client/rig.rs:174-231` `cull_costume` — broken hardcoded allowlist** (`F_eyes0`, `F_mouth0`, …) that does not match the glb node names → all eyes/mouths show.

**wisp (reference, byte-identical `character.glb`, sha 36a70e18…):**
- `../wisp/src/player/parts.rs` — `PartSelection` (7 `u8` slots) + variant tables (TOP/BOTTOM/HEADWEAR/HAIR/EYES/EYEBROWS/MOUTH, ALL_WEAPONS, ALL_CAPES) + `is_visible(mesh_name)` + `apply_local_part_visibility`/`refresh_local_part_visibility_on_change` (toggle `Visibility` on `SkinnedMesh` entities, resolve node name from `Name`/parent walk, cache in `PartMesh`). This is the **known-good** fix for the broken rig.
- `../wisp/src/ui/customization.rs` — `K`-toggled panel, per-slot `<`/`>` rows cycling `PartSelection::advance`, color swatches. Port slot rows; colors optional (see Task D6).
- wisp is first-person: `LocalWizardBody` marker + `hide_self_wizard_body`. Per its CLAUDE.md, the local body is hidden and the camera sits at eye height.

**obelisk-bevy combat API (`../obelisk-bevy`):**
- Cast verbs `src/timeline/cast.rs:23-54`: `cast_skill_at(skill, Entity)`, `cast_skill_at_point(skill, Vec3)`, `cast_skill_dir(skill, Dir3)`. `CastAim` enum `cast.rs:5-12` `{Entity, Point, Direction}`. **`cast_skill_dir` already gives free-aim — no obelisk change needed for Fix B.**
- `validate_casts` `src/timeline/advance.rs:101-121` resolves `CastAim`→`aim_dir: Vec3`, snapshots `ActiveCast.aim_dir` (`advance.rs:176`); projectile velocity = `aim_dir * speed` (`advance.rs:245-267`, `spatial/projectile.rs`), full 3D.
- Damage: skill TOML `[damage]`; `combat/resolve.rs:78-165` `resolve_one_hit(... rng)`. No per-cast power scalar today. Schedule `lib.rs` FixedUpdate `Validate→Advance→Projectiles→ResolveHits→TickEffects`. `CombatRng` server-only, seeded `core/config.rs`.
- firebolt: `assets/skills/firebolt.cast.ron` `phase_durations:(windup:0.3,active:0.1,recovery:0.2)`, `speed:20.0`.

---

## Decisions (locked with the user)
- **First-person:** hidden own body + centered crosshair; opponent fully visible.
- **Aiming:** free aim along the camera ray (can miss). Pitch included (3D).
- **Charge:** hold-to-charge; longer hold = stronger + faster bolt; release fires. Quantized to `u8` so it is a deterministic sim input.
- **Rig:** fix default to one coherent witch AND add a customization UI, with the selection replicated so each client sees the other's appearance.

---

## Milestone A — First-Person Camera (arena only)

**Files:** `crates/arena_game/src/client/controller.rs`, `client/mod.rs`, `client/present.rs`, `client/hud.rs`, `client/rig.rs`.

### Task A1: First-person camera placement

**Files:** Modify `client/controller.rs` (camera follow + yaw/pitch), re-read `client/mod.rs:520-524` (camera spawn).

- [ ] **Step 1:** Re-read `controller.rs` in full and `mod.rs:500-560`. Identify `follow_local_net_player`, `CAM_OFFSET`, `CameraYaw`, `AimPitch`, `FollowCamera`.
- [ ] **Step 2:** Replace the third-person follow with first-person: place the camera at the local predicted player's eye position `player_pos + Vec3::Y * EYE_HEIGHT` (define `const EYE_HEIGHT: f32 = 1.6;`), and set its rotation to `Quat::from_axis_angle(Vec3::Y, yaw) * Quat::from_axis_angle(Vec3::X, pitch)` from `CameraYaw`/`AimPitch`. Remove the `CAM_OFFSET` back-offset. Keep mouse-look accumulation exactly as-is.
- [ ] **Step 3:** Keep `ARENA_CAM_YAW`/`ARENA_TEST_PITCH` debug seeding working (used by screenshot verification).
- [ ] **Step 4:** Build: `cd /Users/luke/src/obelisk-arena && cargo build`. Expected: compiles.
- [ ] **Step 5:** Commit `feat(arena): first-person camera at eye height`.

### Task A2: Hide the local player's own body

**Files:** Modify `client/present.rs` (rig attach) + `client/rig.rs` (or a small new system). Reference `../wisp` `LocalWizardBody` + `hide_self_wizard_body`.

- [ ] **Step 1:** When attaching the rig (`present.rs:31-68`), tag the LOCAL player's body root with a `LocalPlayerBody` marker component (the local player is the one with the local `NetworkOwner`/`Predicted`; re-read how locality is determined in `controller.rs`/`client/net.rs`).
- [ ] **Step 2:** Add a system that sets the local body's `Visibility` (root) to `Hidden` once attached, leaving remote players `Inherited`. In first person you never see your own body; this also prevents the camera being inside the head mesh.
- [ ] **Step 3:** Verify the opponent's body stays visible (do NOT hide non-local rigs).
- [ ] **Step 4:** Build + commit `feat(arena): hide local player body in first person`.

### Task A3: Crosshair HUD

**Files:** Modify `client/hud.rs`.

- [ ] **Step 1:** Add a centered crosshair UI node (a small dot or `+`): an absolutely-positioned `Node` centered on screen (e.g. 4×4 px white square at 50%/50%, or a thin cross). Spawn it in the HUD setup system alongside the existing HP bars.
- [ ] **Step 2:** Build + commit `feat(arena): add center crosshair`.

### Task A4: Drop self spine-lean (cleanup)

- [ ] **Step 1:** The `chest_joint` pitch lean (`controller.rs:~232`) was for third-person self-view aiming. In first person we don't see our own body, so applying pitch to the local body's spine is moot. Keep applying it to REMOTE players (so opponents visibly lean to aim) but skip it for the now-hidden local body. If locality gating is awkward, leaving it is harmless (body is hidden) — only change if clean. Document the choice in a comment.
- [ ] **Step 2:** Build + commit if changed.

**Milestone A gate:** windowed client shows a first-person view (camera at eye height, mouse-look), no own-body occlusion, crosshair centered, opponent visible. Capture `ARENA_SHOT` screenshot.

---

## Milestone B — Free-Aim Directional Casting (arena only)

**Files:** `crates/arena_game/src/client/net.rs`, `crates/arena_game/src/net/protocol.rs`, `crates/arena_game/src/server/mod.rs`. **No obelisk-bevy change.**

### Task B1: Client sends look-direction aim

**Files:** Modify `client/net.rs:185-261` `send_cast_requests`; re-read `net/protocol.rs:136-145` `CastRequestMessage`.

- [ ] **Step 1:** Compute `aim_dir` from the camera forward built from `CameraYaw`+`AimPitch` (same quat as Task A1): `let fwd = (Quat::from_axis_angle(Y, yaw) * Quat::from_axis_angle(X, pitch)) * -Vec3::Z;`. Send that normalized vector as `aim_dir`. Stop computing `aim_dir` from the nearest enemy. `target_hint` may be left as the nearest player or removed — prefer removing reliance on it (Task B2 makes the server ignore it).
- [ ] **Step 2:** Build + commit `feat(arena): cast aim follows camera look direction`.

### Task B2: Server casts in the aim direction

**Files:** Modify `server/mod.rs:367-427` `drain_cast_requests`.

- [ ] **Step 1:** Replace the `ObeliskSpatial::nearest_enemy` re-acquire + `cast_skill_at(target)` with a directional cast: convert the message's `aim_dir` to `Dir3` (`Dir3::new(aim_dir).unwrap_or(Dir3::NEG_Z)`) and call `cast_skill_dir(skill_id, dir)` via the obelisk combat facade. Keep server-side validation that the caster exists / is allowed to cast.
- [ ] **Step 2:** Re-read how the server obtains the `ObeliskCombat`/cast verb handle in the current `drain_cast_requests` to call `cast_skill_dir` the same way `cast_skill_at` was called.
- [ ] **Step 3:** Build + commit `feat(arena): server fires firebolt along client aim direction`.

### Task B3: Headless regression — directional hit still resolves

**Files:** `crates/arena_game/tools/net-test/` (existing harness).

- [ ] **Step 1:** With both players positioned facing each other (slot 0 at +4, slot 1 at -4 — existing spawn), a cast aimed at the opponent should still produce `DamageResolved`. Run the existing net-test session script; confirm `CastBegan` + `DamageResolved` on both observers as before.
- [ ] **Step 2:** If the default aim (straight forward) misses because the bots aren't oriented, set the test client's `ARENA_CAM_YAW` so the look vector points at the opponent, or have the harness face the opponent. Confirm a hit.
- [ ] **Step 3:** Commit any harness tweak `test(arena): directional-cast regression`.

**Milestone B gate:** net-test shows a directional firebolt hitting when aimed at the opponent and missing when aimed away; server uses `cast_skill_dir`.

---

## Milestone C — Hold-to-Charge + Charge/Spell UI

obelisk-bevy charge field first (C1–C2), then arena input/wire/cast (C3–C5), then UI (C6).

### Task C1: obelisk-bevy — add `charge` to the cast pipeline (golden-safe)

**Files (in `../obelisk-bevy`):** `src/timeline/cast.rs` (`PendingCast`, `CastAim`, verbs), `src/timeline/advance.rs` (thread to `ActiveCast`, projectile speed), `src/combat/system.rs` (apply to damage post-resolve). Tests: obelisk's existing golden + unit suites.

- [ ] **Step 1 (write the failing test):** Add a unit test asserting that a charged cast scales damage. In obelisk's combat tests, set up a caster+target, resolve firebolt at `charge = Some(255)` (max) and assert the dealt damage is the base × the max multiplier; resolve at `charge = None` and assert base damage unchanged. Pick the mapping `mult = 0.5 + (charge as f32 / 255.0) * 1.5` → range 0.5×…2.0×, with `None` ≡ 1.0×. Also assert projectile speed scales by the same `mult`.
- [ ] **Step 2 (run, expect fail):** `cd /Users/luke/src/obelisk-bevy && cargo test charge` — fails (field/param missing).
- [ ] **Step 3 (implement):**
  - Add `pub charge: Option<u8>` to `PendingCast` (`cast.rs`). Default all existing verb constructors to `charge: None`. Add charged verb variants: `cast_skill_dir_charged(skill, dir, charge: u8)` (and optionally `_at_charged`) that set `charge: Some(..)`.
  - Snapshot charge onto `ActiveCast` in `validate_casts` (`advance.rs`). At projectile spawn (`advance.rs:245-267`), multiply `speed` by `charge_mult(charge)` where `fn charge_mult(c: Option<u8>) -> f32 { c.map_or(1.0, |c| 0.5 + (c as f32/255.0)*1.5) }`.
  - In `combat/system.rs` `on_hit_confirmed`, after `resolve_one_hit` returns, multiply the outcome's damage applied to the target by `charge_mult(active_cast.charge)`. The scalar enters AFTER the RNG draw but is a pure multiply on the result — it draws no RNG, so determinism holds and goldens (all `charge: None`) are unchanged.
- [ ] **Step 4 (run, expect pass):** `cargo test charge` passes.
- [ ] **Step 5 (golden gate):** Run the full obelisk suite incl. the 39 golden traces: `cargo test`. Expected: all pass, goldens byte-identical (None path is a no-op). If any golden changed, the charge multiply is leaking into the default path — fix so `None` is exactly 1.0 with no float reordering on the default path.
- [ ] **Step 6:** `cargo clippy --all-targets -- -D warnings && cargo fmt --check`. Commit in obelisk-bevy `feat(combat): optional per-cast charge multiplier (damage + projectile speed), default no-op`.

### Task C2: arena_skills / protocol — carry charge on the wire

**Files:** `crates/arena_game/src/net/protocol.rs` `CastRequestMessage`.

- [ ] **Step 1:** Add `pub charge: u8` to `CastRequestMessage` (0 = uncharged/min). Keep serde derives. Re-read the message registration to ensure no extra step is needed.
- [ ] **Step 2:** Build + commit `feat(arena): carry cast charge on the wire`.

### Task C3: arena client — hold-to-charge input

**Files:** `crates/arena_game/src/client/mod.rs:250-264` (cast input), a new `ChargeState` resource/component, `client/net.rs` (send on release).

- [ ] **Step 1:** Re-read the current input → `CastIntent` flow. Replace press-to-cast with hold-to-charge:
  - On cast button DOWN (Space or LMB): start/continue charging — accumulate `charge_secs += dt`, clamp to `MAX_CHARGE_SECS` (define `const MAX_CHARGE_SECS: f32 = 1.5;`).
  - On cast button UP (release): emit a cast request with `charge = (charge_secs / MAX_CHARGE_SECS * 255.0).round() as u8`, then reset `charge_secs = 0`.
  - Expose current normalized charge `0..1` for the HUD (Task C6) and for the animation blend.
- [ ] **Step 2:** Update `send_cast_requests` (`client/net.rs`) to fire on release with the quantized `charge`, not every frame the intent is held.
- [ ] **Step 3:** Build + commit `feat(arena): hold-to-charge cast input`.

### Task C4: arena server — cast with charge

**Files:** `server/mod.rs` `drain_cast_requests` (already directional from Task B2).

- [ ] **Step 1:** Pass the message's `charge` into the directional cast: `cast_skill_dir_charged(skill_id, dir, charge)`. Validate/clamp server-side (charge is already `u8`, inherently bounded).
- [ ] **Step 2:** Build + commit `feat(arena): server applies cast charge`.

### Task C5: charge-driven animation (optional polish, keep if clean)

- [ ] **Step 1:** Feed the normalized charge into `LocalAnimBlend`/`drive_animation` so the casting pose holds during charge-up (windup-like) and releases on fire. If the existing `ActiveCast.phase` blend already reads well once the windup plays on release, leave animation as-is and note it. Build + commit if changed.

### Task C6: charge bar + spell-selected HUD

**Files:** `client/hud.rs`.

- [ ] **Step 1:** Add a charge bar near the crosshair/bottom-center that fills `0..1` from the client charge state while the button is held, and hides/empties when not charging. Use the same `Node` width-driven bar pattern as the existing HP bars.
- [ ] **Step 2:** Add a spell-selected indicator (text/icon showing "Firebolt") — a small fixed HUD label naming the active spell. (Only firebolt exists; a static label is fine, structured so more spells slot in later.)
- [ ] **Step 3:** Build + commit `feat(arena): charge bar + spell indicator HUD`.

**Milestone C gate:** windowed client — holding the cast button fills the charge bar, releasing fires a bolt whose speed visibly scales with hold time; spell label visible; obelisk goldens still green. net-test: a max-charge hit deals more damage than an uncharged hit.

---

## Milestone D — Rig Fix + Customization UI + Netcode Sync

### Task D1: Port wisp `parts.rs` into arena

**Files:** Create `crates/arena_game/src/client/parts.rs` (port of `../wisp/src/player/parts.rs`). Modify `client/mod.rs` (module + plugin wiring).

- [ ] **Step 1:** Copy `PartSelection`, the variant tables (TOP/BOTTOM/HEADWEAR/HAIR/EYES/EYEBROWS/MOUTH, ALL_WEAPONS, ALL_CAPES), `Slot`, `SLOTS`, `is_visible`, `decide_outfit_visibility`, `is_indexed_match`, `PartMesh`, `apply_local_part_visibility`, `refresh_local_part_visibility_on_change`. The glb is byte-identical so tables apply verbatim.
- [ ] **Step 2:** Adapt the body-marker gating: wisp keys on `LocalWizardBody`; arena must apply visibility to BOTH players' rigs (each rig's body root from `present.rs`). Generalize `apply_local_part_visibility` to walk to the arena body-root marker (the marker attached in `present.rs`/Task A2) rather than wisp's local-only marker. For the local player use the local `PartSelection` resource; remote players use their replicated selection (Task D5) — for D1 just drive both from the default selection to prove the rig renders as one coherent witch.
- [ ] **Step 3:** Remove/replace the broken `cull_costume` (`client/rig.rs:174-231`) with the ported visibility system. Ensure no double-hiding conflict.
- [ ] **Step 4 (test):** Add a unit test for `is_visible` mirroring wisp's intent: default selection → `F_Witch_Top` visible, `F_Knight_Top` hidden, `F_eyes0` visible, `F_mouth0` visible, weapons/capes/body hidden. Run `cargo test parts`.
- [ ] **Step 5:** Build + commit `fix(arena): replace broken costume cull with wisp PartSelection visibility (one coherent witch)`.

### Task D2: Verify rig visually

- [ ] **Step 1:** Windowed `ARENA_SHOT` screenshot of both players; confirm each is a single coherent witch (one set of eyes/mouth/hair/outfit), no overlapping eyes/mouths. Read the PNG.

### Task D3: `CharacterColors` recolor (optional within D — include if clean)

**Files:** port `../wisp/src/player/recolor.rs` if it exists; create `client/recolor.rs`.

- [ ] **Step 1:** If wisp has a recolor system (material tinting by channel), port it; else skip colors and note it. Colors are a nice-to-have; slot selection is the core ask.

### Task D4: Customization UI panel

**Files:** Create `crates/arena_game/src/client/customization.rs` (port of `../wisp/src/ui/customization.rs`). Modify `client/mod.rs` (plugin wiring).

- [ ] **Step 1:** Port the `K`-toggled panel with per-slot `<`/`>` rows that call `PartSelection::advance`. Drop wisp's `InputMode`/cursor-grab coupling if arena lacks it (re-read arena's input handling); at minimum free the cursor while the panel is open so buttons are clickable, and restore mouse-look on close. Color swatch rows only if Task D3 landed.
- [ ] **Step 2:** Wire `handle_button_interactions`, `refresh_slot_labels` (and `refresh_swatch_visuals` if colors). The local `PartSelection` resource changing must drive `refresh_local_part_visibility_on_change` (already ported in D1) so the local witch updates live.
- [ ] **Step 3:** Build + commit `feat(arena): character customization panel (K to open)`.

### Task D5: Replicate `PlayerCustomization` so each client sees the other's appearance

**Files:** `crates/arena_game/src/net/protocol.rs` (new replicated component), `server/mod.rs` (store on player), `client/net.rs` or `client/present.rs` (apply remote selection to remote rig). Reference `../wisp/src/net/replication.rs:283-340` + protocol `PlayerCustomization`.

- [ ] **Step 1:** Add `#[derive(Component, Clone, Copy, PartialEq, Serialize, Deserialize)] struct PlayerCustomization { parts: PartSelection }` and register it for replication in `ProtocolPlugin` (component replication — initial value is reliable per the project's notes; appearance rarely changes, so initial-value replication + an occasional update is acceptable). Re-read how existing components (e.g. `NetworkedHealth`) are registered and copy that.
- [ ] **Step 2:** Server: attach `PlayerCustomization::default()` when spawning each networked player (`sync_networked_players`). For now appearance is the default per slot; if/when the client sends a chosen selection, relay it (a `CustomizeMessage` C→S is optional — default-only sync is enough to prove the path and fixes the bug for the opponent's view).
- [ ] **Step 3:** Client: when a remote player's rig is attached, read its replicated `PlayerCustomization.parts` and apply via `is_visible` (the D1 system, parameterized by that player's selection rather than the local resource). Mirror wisp `replication.rs:283-340`.
- [ ] **Step 4:** Build + commit `feat(arena): replicate character appearance between players`.

### Task D6: Optional — send local selection to server

- [ ] **Step 1:** If straightforward, add a `CustomizeMessage(PartSelection)` C→S; server updates the player's `PlayerCustomization`; remote clients re-apply. This makes the customizer affect what the opponent sees. If it expands scope much, defer and note it — D5 already fixes the broken-rig bug for both views.

**Milestone D gate:** net-test — two players each carry a replicated `PlayerCustomization`; each observer materializes the other with a coherent witch. Windowed — `K` opens the panel, cycling a slot updates the local character live; both players render as single coherent witches (bug fixed). Screenshot proof.

---

## Final Review

- [ ] Full arena gate green: `cd /Users/luke/src/obelisk-arena && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`.
- [ ] obelisk-bevy gate green incl. goldens: `cd /Users/luke/src/obelisk-bevy && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`.
- [ ] net-test regression passes (cast + directional hit + charge-damage + appearance sync).
- [ ] Windowed verification screenshots captured for first-person + crosshair + charge bar + coherent rigs.
- [ ] Dispatch a final code-reviewer subagent over the whole diff.

---

## Self-Review Notes
- **Spec coverage:** A=first-person (A1–A4), aiming=B (B1–B3), charge+UI=C (C1–C6), rig+customizer+sync=D (D1–D6). All four play-test issues mapped.
- **Determinism:** charge is a quantized `u8` sim input applied as a post-resolve multiply with `None`≡1.0 default → 39 goldens stay byte-identical (gated in C1 Step 5).
- **No obelisk change for aiming:** `cast_skill_dir` already exists.
- **Type consistency:** `PartSelection` (7 `u8`s) is the single shared shape across parts.rs port (D1), customizer (D4), and replication (D5). `charge: u8` consistent across protocol (C2), input (C3), server (C4), obelisk verb (C1).
