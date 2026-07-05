# Phase 4: obelisk-arena Migration to the Reformed Sim + Skill-Mode Editor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate obelisk-arena onto the reformed obelisk-bevy sim (trigger-reform `d72fe9f`) and the built-in Skill mode (`bevy_modal_editor` `skill-mode`), so arena skills are authored as the three-artifact triad (rules TOML + `.cast.ron` behavior + `CueBinding` presentation), causality lives in obelisk rules triggers, the client renders cues via `bevy_effect`, and the reference skills (firebolt→firebolt_explosion, chain_lightning, blizzard) are real reformed content.

**Architecture:** The arena stays server-authoritative (obelisk owns casts/damage/triggers). The server relays a fired `CueEvent` over the lightyear wire as an engine-neutral `CueMessage` carrying `skill_id + cue_id(slot) + charge + end_reason`; the client resolves `timelines[skill_id].cues[cue_id]` → `CueBinding` → `bevy_effect` (EffectLibrary-first-then-VfxLibrary), mirroring the editor preview. Acquisition (what a cast aims at) is resolved host-side from the client's aim ray against the timeline's authored `Acquisition`, producing a `CastAim` the sim validates. The `arena_skills` crate (`.skillfx.ron` lane model) is deleted; `arena_editor` collapses to a thin host shell over the editor's built-in Skill mode.

**Tech Stack:** Rust, Bevy 0.18.1, Avian3d 0.5, lightyear 0.26.4, obelisk-bevy (git, post-reform `main`), `bevy_modal_editor`/`bevy_vfx`/`bevy_effect` (git, post-`skill-mode`-merge `main`), stat_core/loot_core (git `vothuul/obelisk` master `bf9f026`), RON + TOML content.

---

## Global Constraints

Copied verbatim from the reformed source; every task's requirements implicitly include these.

- **`vfx_cues` and `cues` COEXIST — `vfx_cues` was NOT renamed.** `obelisk_bevy::assets::CastTimeline` has BOTH `vfx_cues: HashMap<String,String>` (the SIM reads it — a slot fires a `CueEvent` iff `vfx_cues[slot]` exists, naming the fired `CueEvent.cue_id`) AND `cues: HashMap<String,CueBinding>` (PRESENTATION reads it; inert to the sim). Reference: `obelisk-bevy/src/assets/mod.rs:28,79`, `src/vfx.rs::cue_for`.
- **Cue-id equals slot name.** The editor's `derive_vfx_cues` sets `vfx_cues[slot] = slot`, so a fired `CueEvent.cue_id` equals its slot (`on_cast`, `on_window_{id}`, `on_hit`, `on_end_{id}`, `emit_{id}`). Any hand-authored arena `.cast.ron` MUST populate `vfx_cues[slot]=slot` for every slot it authors a `cues` binding for, or that cue never fires.
- **The five slot patterns** (`src/assets/mod.rs:57-63`): `on_cast`; `on_window_{window_id}`; `on_hit`; `on_end_{window_id}`; `emit_{window_id}` (an emitted `Template` instance fires `emit_*` ONLY, never `on_window_*`).
- **`CueKind` has five variants** (`src/events.rs:202`): `OnCast, OnWindow, OnHit, OnEnd, OnEmit`. `OnEmit` is new vs. the arena's current 4-variant mirror.
- **`CueEvent` payload** (`src/events.rs:217`, after Task A1): `{ cue_id: String, source: Entity, position: Vec3, position_from: Option<Vec3>, kind: CueKind, charge: Option<u8>, end_reason: Option<EndReason>, skill_id: String }`. `charge` is set on every slot; `end_reason: Some(_)` only on `OnEnd`; `position_from: Some(_)` only on a beam window's open cue.
- **`Acquisition`** (`src/assets/mod.rs:165`): `Aim` (default, no cast point) | `SelfPoint` | `HitscanEntity { range: f32, filter: HitFilter, fallback: AcqFallback }` | `GroundPoint { range: f32, fallback: AcqFallback }`. `AcqFallback` = `Fizzle | Then(Box<Acquisition>)`.
- **`CastAim`** (`src/timeline/cast.rs:10`): `Entity(Entity) | Point(Vec3) | Direction(Dir3)`. The host produces a candidate; `validate_casts` checks it against `Acquisition` and walks fallbacks. obelisk is world-agnostic — it never raycasts; producing the `CastAim` from an aim ray is the host's job.
- **`VolumeMotion`** (`src/assets/mod.rs:295`): `Static | Linear{speed} | Ballistic{speed,gravity} | Beam`.
- **`CollisionWindow`** (`src/assets/mod.rs:210`, `#[serde(deny_unknown_fields)]`): `{ id, spawn: WindowSpawn, anchor: WindowAnchor=Caster, anchor_offset: Vec3=ZERO, strikes: bool=true, active_duration, shape, motion: VolumeMotion=Static, motion_direction: MotionDirection=Inherit, hit_filter, hit_mode, rehit_interval: Option<f32>, emitter: Option<Emitter> }`. `WindowSpawn` = `Scheduled{phase,offset=0} | Template`. `WindowAnchor` = `Caster | CastPoint`. `CastTimeline` is `#[serde(deny_unknown_fields)]` — every v1 field (`spawn_phase`, `spawn_offset`, `on_end`, `targeting`, `delivery`, `WindowPhase::Chained`) fails LOUD at load.
- **`CueBinding`** (`src/assets/mod.rs:110`, `#[serde(deny_unknown_fields)]`): `{ effect: Option<String>, attach: CueAttach=World, anim: Option<String>, params: Vec<CueParam> }`. `CueAttach` = `World | Follow`. `CueParam` = `{ param: String, source: ParamSource }`; `ParamSource` = `Charge`.
- **Skill-condition trigger TOML** (`stat_core::damage::triggers::SkillCondition`): `[[conditions]]` block with `trigger_skill = "<id>"`, `additional = <bool>` (default false — MUST be `true` for a condition whose `trigger_skill` names a timeline skill, else flagged invalid), and the flattened `type = "<snake_case TriggerCondition>"`. Lifecycle triggers: `type = "on_impact"` (HitWorld), `type = "on_expire"` (Fuse); hit-phase: `type = "always"` (HitEntity, any confirmed hit). Reference: `obelisk-bevy/tests/fixtures/skills/fireball.toml`.
- **Chain rules** (`stat_core`): `[damage] can_chain = true`, `chain_count = <u32>`; the timeline supplies `chain_radius: f32` and a `Beam` window. A liveness gap exists in `nearest_retarget_candidate` (carried ticket — see Task C9).
- **Determinism**: the sim path draws only from seeded `CombatRng`; emitters draw from a separate `SpawnRng`. Cosmetics (client) NEVER draw sim RNG, spawn a `Hitbox`, or run `ObeliskSet::ResolveHits` (Stage-A invariant, `arena_game/CLAUDE.md` §4).
- **Single-drain rule**: `MessageReceiver::receive()` drains; only `consume_replicated_cues` may drain `CueWireMessage`.
- **Trace `extra` fields must not use the key `kind`** — use `cue_kind` (the harness merges `extra` over a base carrying the top-level `kind`).

---

## Prerequisites (external — NOT tasks in this plan)

These are gates the user controls. The plan is ordered so the wire/acquisition/content/net-test work (Tasks A1, C1, C2, C4–C7) proceeds against **P2 alone**; only the client `bevy_effect` render (C3) and the editor shell (C8) additionally require **P1**.

- **P1 — `skill-mode` merged to `bevy_modal_editor` main.** The arena pins `bevy_modal_editor`/`bevy_vfx`/`bevy_editor_game` at `branch="main"`. `bevy_effect` and the built-in `EditorMode::Skill` exist ONLY on the unmerged `skill-mode` branch (`merge-base main 3448f80 == main HEAD`). Until the user merges `skill-mode` → main (or repoints the pin/patch), C3 and C8 cannot compile. *If the user prefers not to merge yet:* repoint the three pins to a `skill-mode` rev for the duration, or defer C3/C8 to a follow-up and ship C1/C2/C4–C7 first (the game still runs; the client falls back to no cosmetics until C3).
- **P2 — obelisk-bevy `main` is the repin target, with Task A1 landed.** obelisk-bevy `main` (`06046f6`) carries trigger-reform + the cue-payload work. Task A1 (below) adds `CueEvent.skill_id` and must be merged to `main` before C2 consumes it. The arena currently pins the pre-reform `68618a8` (deliberate — `arena_game` HEAD `9df5f37` and the pin commit `f6472e4`).

---

## Decisions settled in this plan (with rationale)

These resolve the stub's open questions. A reviewer who disagrees should raise it against the rationale, not silently.

1. **Shared acquisition resolver → arena-local rewrite, no new shared crate.** obelisk is world-agnostic (it never raycasts; `HitboxWorldHit` is host-fired — `src/events.rs:191`). The shared *semantics* already live in obelisk-bevy: the `Acquisition` enum + `validate_casts`/`resolve_acquisition`. Each host produces its own candidate `CastAim` from its own aim source (editor: stage dummies via `resolve_stage_acquisition`; server: the client aim ray via avian `SpatialQuery`). The arena server therefore rewrites its EXISTING hitscan (`server/cast_pipeline.rs:84-108`, currently keyed on the deleted `CastTargeting::SingleEntity`) onto `Acquisition`. No obelisk-bevy helper, no `arena_sim` helper — those would either violate the world-agnostic invariant or add an abstraction for a ~40-line function used once per host.
2. **`arena_skills` → deleted.** Its entire reason for being — the `.skillfx.ron` `LaneEvent` lane model + `SkillFxRegistry`/`resolve_cue` + `SkillFx` asset/loader + `VfxParamBinding` math — is replaced by obelisk `CueBinding`/`CueParam`. The one thing worth keeping, the engine-neutral wire type `CueMessage`, MOVES into `arena_game/src/net/protocol.rs` (it is a wire type; it belongs with the protocol) and is extended (Task C2).
3. **`CueEvent.skill_id` (Task A1) is the wire's skill key.** Because cue-id == slot, `cue_id` is not unique across skills; the multi-skill client needs the skill to index `timelines[skill_id].cues[cue_id]`. `CueEvent` has no `skill_id` today; all four `cue_on_*` firing sites already hold `e.skill_id`. Adding the field (A1) is the clean, spec-aligned enabler. (Alternative — arena-local globally-unique cue-id values — was rejected: it diverges from `derive_vfx_cues` and breaks consuming editor-authored content verbatim.)
4. **Spatial `TriggerFired` is a ticket, not a task.** The reform's hit-trigger executor (`src/combat/system.rs:325`) fires no `TriggerFired`, but the triggered skill surfaces its own `DamageResolved` + `CueEvent`s, which already replicate. The net-test asserts the trigger through `firebolt_explosion`'s `DamageResolved` (Task C7). A `TriggerFired` for the spatial-trigger edge (observability) is recorded in C9.
5. **Effect (ailment) authoring is out of Skill-mode scope — accepted loss.** v1 `arena_editor` edited `stat_core` ailments (`config/effects/*.toml`) via `effects_panel.rs`/`stat_ui.rs`; the Skill mode authors the triad, not ailments. Those TOMLs stay hand-authored; the shell loads them via one `add_obelisk_effects(config/effects)` call so previews that apply `burn` resolve. A future editor Effect-authoring mode is a C9 ticket.
6. **`arena_editor` stays its own cargo workspace.** The empty `[workspace]` exclusion (heavy-feature unification with `arena_game`) still holds. The shell just enables the `obelisk` feature on `bevy_modal_editor` and repins. Rig-less preview (arena `character.glb` isn't consumed by the ported stage) is accepted for now (C9 ticket).

---

## Reference material (read before authoring content)

obelisk-bevy already ships the canonical pattern this migration adopts. Do NOT invent the shapes — adapt these:

- **`obelisk-bevy/assets/skills/fireball.cast.ron`** — the v2 behavior: `acquisition: Aim`, `chargeable: true`, one `Ballistic` `bolt` window (no inline explosion), `cues` with `on_cast`(World)/`on_window_bolt`(Follow).
- **`obelisk-bevy/tests/fixtures/skills/fireball.toml`** — the rules: three `additional = true` conditions (`type = "always"` / `"on_impact"` / `"on_expire"`), all `trigger_skill = "fireball_explosion"`. Comment: "Only one fires per ending: HitEntity→Always; HitWorld→OnImpact; Fuse→OnExpire."
- **`obelisk-bevy/tests/fixtures/skills/fireball_explosion.toml`** — the triggered half: `mana_cost = 0.0`, its own `[damage]`.
- **`bevy_modal_editor-skill/src/skill/preview/cosmetics.rs`** (`spawn_cue_effect:323`, the `On<CueEvent>` observer `:199`, `window_motion_for_cue:173`) — the exact client-render mirror for Task C3: resolve `tl.cues.get(&ev.cue_id)`, `EffectLibrary`-first-then-`VfxLibrary`, `CueAttach::Follow` flies a proxy along `window_motion_for_cue`, `ParamSource::Charge` baked from `ev.charge`.
- **`bevy_modal_editor-skill/src/skill/preview/stage.rs`** (`resolve_stage_acquisition:716`) — the acquisition-resolution shape to mirror server-side in Task C1.

---

## Task A1 (obelisk-bevy): add `skill_id` to `CueEvent`

**Repo:** `~/src/obelisk-bevy` (branch off `main`). Land + merge to `main` before Task C2.

**Files:**
- Modify: `src/events.rs:217-235` (the `CueEvent` struct)
- Modify: `src/vfx.rs` (the four `cue_on_*` constructors: `cue_on_cast`, `cue_on_window`, `cue_on_end`, `cue_on_hit`)
- Test: `src/vfx.rs` `#[cfg(test)]` (add) or the existing cue tests in `src/scenario/library.rs`

**Interfaces:**
- Produces: `CueEvent { …, pub skill_id: String }`. Every emitted cue carries the originating skill id (from `CastBegan.skill_id` / `HitWindowOpened.skill_id` / `HitboxEnded.skill_id` / `HitConfirmed.skill_id`, all already in scope at the four sites).

- [ ] **Step 1: Write the failing test.** In `src/vfx.rs` tests, cast a one-window skill on a headless app and assert the captured `CueEvent` (via `observe_cue`) has `skill_id == "<the cast skill>"`.

```rust
#[test]
fn fired_cue_carries_skill_id() {
    use crate::events::CueEvent;
    use std::sync::{Arc, Mutex};
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let mut app = crate::scenario::library::minimal_sim_app(); // existing test harness helper
    app.add_observer(move |ev: On<CueEvent>| sink.lock().unwrap().push(ev.event().skill_id.clone()));
    // load a skill whose vfx_cues has an on_cast slot, cast it, run a few ticks
    crate::scenario::library::cast_named(&mut app, "fireball"); // existing helper; adapt to a fixture with vfx_cues
    for _ in 0..10 { app.update(); }
    assert!(seen.lock().unwrap().iter().any(|s| s == "fireball"));
}
```

- [ ] **Step 2: Run it — expect FAIL** (`skill_id` field missing → compile error, or empty).

Run: `cargo test -p obelisk-bevy fired_cue_carries_skill_id`
Expected: FAIL (no field `skill_id` on `CueEvent`).

- [ ] **Step 3: Add the field.** In `src/events.rs`, after `end_reason`:

```rust
    /// The originating skill id, so a multi-skill presentation host can resolve the right
    /// timeline's `cues` binding for this cue (cue_id == slot is not unique across skills).
    pub skill_id: String,
```

- [ ] **Step 4: Populate at all four sites** in `src/vfx.rs`. Each `commands.trigger(CueEvent { … })` gains `skill_id: e.skill_id.clone(),` (the source event already carries it):
  - `cue_on_cast`: `e` is `CastBegan` → `skill_id: e.skill_id.clone()`
  - `cue_on_window`: `e` is `HitWindowOpened` → `skill_id: e.skill_id.clone()`
  - `cue_on_end`: `e` is `HitboxEnded` → `skill_id: e.skill_id.clone()`
  - `cue_on_hit`: `e` is `HitConfirmed` → `skill_id: e.skill_id.clone()`

- [ ] **Step 5: Fix any other `CueEvent { … }` constructors** the compiler flags (e.g. editor/test fixtures) by adding `skill_id`. Search: `rg 'CueEvent \{' src tests`.

- [ ] **Step 6: Run tests — expect PASS.**

Run: `cargo test -p obelisk-bevy` (at least the cue + vfx tests)
Expected: PASS.

- [ ] **Step 7: Commit.**

```bash
git add src/events.rs src/vfx.rs
git commit -m "feat(cue): carry skill_id on CueEvent for multi-skill presentation resolution"
```

> After merge to `main`, note the new `main` rev; Task C1 repins to it.

---

## Task C1 (obelisk-arena): repin obelisk-bevy + minimal game-lib compile-fix

**Repo:** `~/src/obelisk-arena` (branch off `master`, e.g. `phase4-arena-migration`).

**Files:**
- Modify: `Cargo.toml` (root workspace — the `obelisk-bevy` dep), then `cargo update -p obelisk-bevy`
- Modify: `crates/arena_skills/src/lib.rs:119-126,378-387` (the `CueKind` mirror + `From<ObeliskCueKind>` — add `OnEmit`; temporary, this crate is deleted in C2)
- Modify: `crates/arena_game/src/server/cast_pipeline.rs` (rewrite acquisition off `CastTargeting` onto `Acquisition`)
- Test: `crates/arena_game/src/server/cast_pipeline.rs` `#[cfg(test)]` (acquisition-shape unit test)

**Interfaces:**
- Consumes: post-reform `obelisk_bevy::assets::{Acquisition, AcqFallback, HitFilter}`, `obelisk_bevy::timeline::cast::{CastAim, PendingCast}`, `CueEvent.skill_id` (A1).
- Produces: `drain_cast_requests` inserts a `PendingCast { skill_id, aim: CastAim, charge: Some(_), muzzle_offset }` whose `aim` is `Entity`/`Point`/`Direction` chosen from `timeline.acquisition`; the sim validates.

- [ ] **Step 1: Repin.** In root `Cargo.toml`, point `obelisk-bevy` at the post-A1 `main` (keep `branch = "main"`; the lock advances):

```bash
cargo update -p obelisk-bevy
cargo build -p arena_game 2>&1 | head -40   # observe the breaks
```

Expected breaks: `unresolved import obelisk_bevy::assets::CastTargeting` (cast_pipeline.rs:13), `no field targeting` (cast_pipeline.rs:86), non-exhaustive `From<ObeliskCueKind>` match (arena_skills — `OnEmit` unhandled).

- [ ] **Step 2: Add `OnEmit` to the arena_skills `CueKind` mirror** (temporary — C2 deletes this crate). `crates/arena_skills/src/lib.rs`, the `enum CueKind` (add `OnEmit,`) and the `From<ObeliskCueKind>` impl (add `ObeliskCueKind::OnEmit => CueKind::OnEmit,`).

- [ ] **Step 3: Write the failing test** for acquisition-shape selection in `cast_pipeline.rs`:

```rust
#[cfg(test)]
mod acq_tests {
    use super::*;
    use obelisk_bevy::assets::{Acquisition, AcqFallback, HitFilter};
    // A pure helper (extracted in Step 4) that picks the CastAim SHAPE from an Acquisition,
    // given a raycast closure that returns Some(entity)/Some(point). No ECS needed.
    #[test]
    fn hitscan_entity_hit_yields_entity_aim() {
        let acq = Acquisition::HitscanEntity { range: 15.0, filter: HitFilter::Enemies,
            fallback: AcqFallback::Fizzle };
        let e = Entity::from_raw(7);
        let aim = resolve_cast_aim(&acq, Dir3::NEG_Z, |_range| Some(RayHit::Entity(e)), |_r| None);
        assert!(matches!(aim, CastAim::Entity(x) if x == e));
    }
    #[test]
    fn hitscan_miss_falls_through_to_direction() {
        let acq = Acquisition::HitscanEntity { range: 15.0, filter: HitFilter::Enemies,
            fallback: AcqFallback::Fizzle };
        let aim = resolve_cast_aim(&acq, Dir3::NEG_Z, |_r| None, |_r| None);
        assert!(matches!(aim, CastAim::Direction(_))); // sim's fallback (Fizzle) does the rejecting
    }
    #[test]
    fn ground_point_hit_yields_point_aim() {
        let acq = Acquisition::GroundPoint { range: 20.0, fallback: AcqFallback::Fizzle };
        let p = Vec3::new(1.0, 0.0, 2.0);
        let aim = resolve_cast_aim(&acq, Dir3::NEG_Z, |_r| None, |_r| Some(p));
        assert!(matches!(aim, CastAim::Point(x) if x == p));
    }
    #[test]
    fn aim_and_selfpoint_yield_direction() {
        for acq in [Acquisition::Aim, Acquisition::SelfPoint] {
            let aim = resolve_cast_aim(&acq, Dir3::NEG_Z, |_r| None, |_r| None);
            assert!(matches!(aim, CastAim::Direction(_)));
        }
    }
}
```

- [ ] **Step 4: Rewrite `cast_pipeline.rs`.** Replace the `CastTargeting` block (lines 13, 84-127) with an `Acquisition`-driven producer. Extract a pure `resolve_cast_aim` (testable) that the system calls with real raycast closures.

```rust
use obelisk_bevy::assets::{Acquisition, HitFilter};
use obelisk_bevy::timeline::cast::{CastAim, PendingCast};

/// What a host raycast can return along the aim ray.
enum RayHit { Entity(Entity), }

/// Pick the candidate `CastAim` SHAPE from the timeline's authored `Acquisition`, using host
/// raycast closures. The sim (`validate_casts`) does the real range/filter/LOS check + fallback
/// walk against this candidate — we only choose which shape to attempt:
///   HitscanEntity -> raycast for an entity in range; hit => Entity, miss => Direction (let the
///                    sim's authored fallback reject/redirect).
///   GroundPoint   -> raycast to the ground; hit => Point, miss => Direction (ditto).
///   Aim/SelfPoint -> Direction (never fails; SelfPoint's cast point comes from the sim).
fn resolve_cast_aim(
    acq: &Acquisition,
    dir: Dir3,
    mut cast_entity: impl FnMut(f32) -> Option<RayHit>,
    mut cast_ground: impl FnMut(f32) -> Option<Vec3>,
) -> CastAim {
    match acq {
        Acquisition::HitscanEntity { range, .. } => match cast_entity(*range) {
            Some(RayHit::Entity(e)) => CastAim::Entity(e),
            None => CastAim::Direction(dir),
        },
        Acquisition::GroundPoint { range, .. } => match cast_ground(*range) {
            Some(p) => CastAim::Point(p),
            None => CastAim::Direction(dir),
        },
        Acquisition::Aim | Acquisition::SelfPoint => CastAim::Direction(dir),
    }
}
```

Then in `drain_cast_requests`, replace the old `acquired` block: read `tl.acquisition` (not `tl.targeting`), build the two closures from the existing avian `SpatialQuery` + hurtbox exclusion (the entity raycast reuses the current ray logic at lines 91-108; the ground raycast casts `dir` from the eye against the floor collider), call `resolve_cast_aim`, and insert one `PendingCast { skill_id: req.skill_id.clone(), aim, charge: Some(req.charge), muzzle_offset }`. Keep the eye-muzzle offset (`Vec3::Y * ARENA_EYE_HEIGHT`) and the `active.get(caster)` already-casting skip. Preserve the `cast_request_accepted` trace; replace `cast_hitscan_acquired` with a shape-labelled `cast_acquired` trace (`json!({ "skill_id":…, "aim": "<Entity|Point|Direction>" })`).

- [ ] **Step 5: Run the unit tests — expect PASS.**

Run: `cargo test -p arena_game acq_tests`
Expected: PASS (4/4).

- [ ] **Step 6: Build the whole game lib — expect green.**

Run: `cargo build -p arena_game`
Expected: 0 errors. (Runtime casts will not resolve until content migrates in C4 — the net-test is RED here by design; it is re-greened in C7.)

- [ ] **Step 7: Commit.**

```bash
git add Cargo.toml Cargo.lock crates/arena_skills/src/lib.rs crates/arena_game/src/server/cast_pipeline.rs
git commit -m "feat(arena): repin obelisk-bevy to reformed main; cast acquisition onto Acquisition"
```

---

## Task C2 (obelisk-arena): move + extend the cue wire into `arena_game`; delete `arena_skills`

**Files:**
- Create: `crates/arena_game/src/net/cue.rs` (the wire `CueMessage` + `CueKind` mirror + `From<ObeliskCueKind>` + the pure egress helper + a serde round-trip test)
- Modify: `crates/arena_game/src/net/mod.rs` (add `pub mod cue;`)
- Modify: `crates/arena_game/src/net/protocol.rs:164` (`CueWireMessage(pub crate::net::cue::CueMessage)`) and drop the `arena_skills` reference
- Modify: `crates/arena_game/src/skills.rs` (`capture_cue_event` stamps `skill_id`/`charge`/`end_reason`; `consume_replicated_cues` + `predicted_local_cast` use `crate::net::cue::*`)
- Modify: `crates/arena_game/src/client/cosmetics.rs` (`LocalCue` wraps `crate::net::cue::CueMessage`)
- Delete: `crates/arena_skills/` (whole crate) + its workspace member in root `Cargo.toml` + the `arena_skills` dep in `crates/arena_game/Cargo.toml` (+ any `arena_editor` dev-dep)
- Delete: `assets/skills/firebolt.skillfx.ron`, `assets/skills/chain_lightning.skillfx.ron`
- Test: `crates/arena_game/src/net/cue.rs` (serde round-trip incl. new fields)

**Interfaces:**
- Produces: `crate::net::cue::CueMessage { cue_id: String, skill_id: String, source_id: String, position: Vec3, aim_dir: Vec3, position_from: Option<Vec3>, charge: Option<u8>, end_reason: Option<EndReasonWire>, kind: CueKind }`; `enum CueKind { OnCast, OnWindow, OnHit, OnEnd, OnEmit }`; `enum EndReasonWire { HitEntity, HitWorld, Fuse }`; `From<obelisk_bevy::events::EndReason>` + `From<obelisk_bevy::events::CueKind>`; pure `cue_event_to_message(&CueEvent, source_id: &str, aim_dir: Vec3) -> CueMessage`.
- Consumes (C3): the client resolves `CueBinding` from `(skill_id, cue_id)`; this task carries the fields but keeps the existing lane-render alive only long enough to compile — the render swap is C3.

> **Ordering note for the executor:** C2 deletes `arena_skills`, but `client/cosmetics.rs` still contains the lane/`resolve_cue` render. To keep the tree GREEN at the C2 boundary, C2 also removes the lane-render code paths that referenced `arena_skills` types and leaves cosmetics emitting nothing for cues (a temporary no-op) — C3 restores rendering via `CueBinding`. If you prefer one green step, fold C2+C3 into a single task; they are split here so the wire and the render review independently.

- [ ] **Step 1: Write the failing serde test** in the new `crates/arena_game/src/net/cue.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::Vec3;
    #[test]
    fn wire_roundtrips_with_skill_charge_endreason() {
        let m = CueMessage {
            cue_id: "on_end_bolt".into(), skill_id: "firebolt".into(),
            source_id: "player_1".into(), position: Vec3::new(1.0, 2.0, 3.0),
            aim_dir: Vec3::NEG_Z, position_from: None, charge: Some(200),
            end_reason: Some(EndReasonWire::HitWorld), kind: CueKind::OnEnd,
        };
        let bytes = bincode::serialize(&m).unwrap();
        let back: CueMessage = bincode::deserialize(&bytes).unwrap();
        assert_eq!(m, back);
    }
}
```

(Use whatever serde codec the crate already tests with; `serde_json` round-trip is equally fine.)

- [ ] **Step 2: Run it — expect FAIL** (module/type absent).

Run: `cargo test -p arena_game wire_roundtrips_with_skill_charge_endreason`
Expected: FAIL (unresolved `crate::net::cue`).

- [ ] **Step 3: Write `crates/arena_game/src/net/cue.rs`.** Port the engine-neutral pieces from the old `arena_skills` (`CueMessage`, `CueKind`, `From<ObeliskCueKind>`, `cue_event_to_message`), extended:

```rust
//! The engine-neutral cue wire type (moved here from the deleted `arena_skills`). `arena_game`
//! owns the lightyear `CueWireMessage`/`LocalCue` wrappers around it.
use bevy::prelude::Vec3;
use obelisk_bevy::events::{CueEvent, CueKind as ObeliskCueKind, EndReason as ObeliskEndReason};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CueKind { OnCast, OnWindow, OnHit, OnEnd, OnEmit }

impl From<ObeliskCueKind> for CueKind {
    fn from(k: ObeliskCueKind) -> Self {
        match k {
            ObeliskCueKind::OnCast => CueKind::OnCast,
            ObeliskCueKind::OnWindow => CueKind::OnWindow,
            ObeliskCueKind::OnHit => CueKind::OnHit,
            ObeliskCueKind::OnEnd => CueKind::OnEnd,
            ObeliskCueKind::OnEmit => CueKind::OnEmit,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndReasonWire { HitEntity, HitWorld, Fuse }
impl From<ObeliskEndReason> for EndReasonWire {
    fn from(r: ObeliskEndReason) -> Self {
        match r {
            ObeliskEndReason::HitEntity => EndReasonWire::HitEntity,
            ObeliskEndReason::HitWorld => EndReasonWire::HitWorld,
            ObeliskEndReason::Fuse => EndReasonWire::Fuse,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CueMessage {
    /// The fired cue id == its slot (`on_cast`/`on_window_{id}`/`on_hit`/`on_end_{id}`/`emit_{id}`).
    pub cue_id: String,
    /// The originating skill id — the client indexes `timelines[skill_id].cues[cue_id]`.
    pub skill_id: String,
    /// Stable `ObeliskId` of the cue's source (caster for OnCast/OnWindow, target for OnHit).
    pub source_id: String,
    pub position: Vec3,
    /// Caster's normalized aim when the cue fired (observers fly Follow proxies the right way).
    pub aim_dir: Vec3,
    #[serde(default)]
    pub position_from: Option<Vec3>,
    /// The cast's charge, forwarded on every slot (drives `ParamSource::Charge` bindings).
    #[serde(default)]
    pub charge: Option<u8>,
    /// Set only on OnEnd cues (from `HitboxEnded.reason`).
    #[serde(default)]
    pub end_reason: Option<EndReasonWire>,
    pub kind: CueKind,
}

/// Pure egress: build the wire message from a fired `CueEvent` + the resolved stable source id +
/// the caster aim. `arena_game` supplies `source_id` (via `ObeliskEntityIndex`) and `aim_dir`.
pub fn cue_event_to_message(ev: &CueEvent, source_id: &str, aim_dir: Vec3) -> CueMessage {
    CueMessage {
        cue_id: ev.cue_id.clone(),
        skill_id: ev.skill_id.clone(),
        source_id: source_id.to_string(),
        position: ev.position,
        aim_dir,
        position_from: ev.position_from,
        charge: ev.charge,
        end_reason: ev.end_reason.map(Into::into),
        kind: ev.kind.into(),
    }
}
```

- [ ] **Step 4: Rewire `skills.rs`.** `capture_cue_event` now calls `crate::net::cue::cue_event_to_message(cue, source_id, aim)` (dropping the old positional call). `predicted_local_cast` constructs `crate::net::cue::CueMessage { cue_id, skill_id: cast.skill_id.clone(), source_id, position, aim_dir, position_from: None, charge: Some(cast.charge), end_reason: None, kind: CueKind::OnCast }` (it already reads `vfx_cues.get("on_cast")` — keep; add `skill_id` from `cast.skill_id`, `charge` from the predicted cast). `consume_replicated_cues` de-dup unchanged (`kind == OnCast && is_own`).

- [ ] **Step 5: Delete `arena_skills`.** Remove the crate dir, its root `[workspace] members` entry, and the `arena_skills` dependency lines in `crates/arena_game/Cargo.toml` (and `arena_editor` if present). Update `crates/arena_game/src/lib.rs` re-exports and `client/scene.rs::load_skillfx_registry` (delete that loader — C3 replaces the registry). Delete the two `.skillfx.ron` files.

- [ ] **Step 6: Make cosmetics compile** by removing the `arena_skills`-typed lane render from `client/cosmetics.rs` (the `resolve_cue`/`LaneEvent`/`SkillFxRegistry`/`ProjectileCosmetic`-via-lane paths). `LocalCue` now wraps `crate::net::cue::CueMessage`. Leave `spawn_cue_cosmetics` as a temporary stub that only handles the `OnEnd` end-cue despawn of any live `CosmeticProjectile` (keep `CosmeticProjectile`/`fly_cosmetic_projectiles`/`age_lifetimes` — C3 reuses them for `Follow`). Emit the `lane_event`→rename `cue_dispatch` trace so the net-test still sees cue flow.

- [ ] **Step 7: Run the serde test + build — expect green.**

Run: `cargo test -p arena_game wire_roundtrips_with_skill_charge_endreason && cargo build -p arena_game`
Expected: PASS + 0 errors.

- [ ] **Step 8: Commit.**

```bash
git add -A
git commit -m "refactor(arena): move+extend cue wire into arena_game; delete arena_skills + .skillfx.ron"
```

---

## Task C3 (obelisk-arena): client `CueBinding` rendering via `bevy_effect`

**Prerequisite: P1 (skill-mode merged).** Repin `arena_game`'s `bevy_vfx` dep (root `Cargo.toml`) to the merged `main`, and add `bevy_effect` as a dep.

**Files:**
- Modify: root `Cargo.toml` (repin `bevy_vfx`; add `bevy_effect`), `crates/arena_game/Cargo.toml` (add `bevy_effect`)
- Modify: `crates/arena_game/src/client/cosmetics.rs` (replace the stub with `CueBinding` resolution mirroring the editor)
- Modify: `crates/arena_game/src/client/scene.rs` (load `EffectLibrary` from `assets/effects` + `VfxLibrary` from `assets/vfx`/`assets/skills`; build the client `CastTimeline` handle set already exists via `CastTimelineHandles`)
- Delete: `crates/arena_game/src/client/vfx_bind.rs` (the `bake_bindings`/`VfxParamBinding` baker — replaced by `CueParam`/`ParamSource::Charge`) if nothing else uses it
- Test: `crates/arena_game/src/client/cosmetics.rs` (a pure `cue_binding_for(skill_id, cue_id, &Assets<CastTimeline>, &handles)` resolution test)

**Interfaces:**
- Consumes: `crate::net::cue::{CueMessage, CueKind}`, `obelisk_bevy::assets::{CastTimeline, CueBinding, CueAttach, CueParam, ParamSource, VolumeMotion}`, `bevy_effect::{EffectPlugin, EffectLibrary, load_effects_from_dir}`, `bevy_vfx::VfxLibrary`.
- Produces: on each `LocalCue`, resolve the binding and spawn the effect; `Follow` cues attach a `CosmeticProjectile` flown along the window motion and torn down by the matching `OnEnd`.

- [ ] **Step 1: Write the failing resolution test.**

```rust
#[test]
fn resolves_cue_binding_by_skill_and_slot() {
    // Build a CastTimeline with cues={"on_cast": CueBinding{effect:Some("Fire"),..}} and
    // vfx_cues={"on_cast":"on_cast"}; register under "firebolt" in a CastTimelineHandles-like map.
    // cue_binding_for("firebolt","on_cast",..) -> Some(binding with effect=="Fire").
    // cue_binding_for("firebolt","on_hit",..) -> None (no binding authored).
    // cue_binding_for("unknown","on_cast",..) -> None.
    // (Construct via helpers; assert the Some/None + effect name.)
}
```

- [ ] **Step 2: Run it — expect FAIL.**

Run: `cargo test -p arena_game resolves_cue_binding_by_skill_and_slot`
Expected: FAIL.

- [ ] **Step 3: Add `EffectPlugin` + library loading** to the windowed client composition (`client/app_windowed.rs` / `scene.rs`): `app.add_plugins(bevy_effect::EffectPlugin)`, seed `EffectLibrary` via `load_effects_from_dir(&mut lib, &root.join("assets/effects"))`, keep `VfxLibrary` seeding. Headless client does NOT add `EffectPlugin` (no render) — the resolver must no-op cleanly when neither library resolves the name.

- [ ] **Step 4: Rewrite `spawn_cue_cosmetics`** mirroring `bevy_modal_editor-skill/src/skill/preview/cosmetics.rs`:
  - Add a pure `fn cue_binding_for<'a>(skill_id, cue_id, timelines, handles) -> Option<&'a CueBinding>` = `handles.0.get(skill_id).and_then(|h| timelines.get(h)).and_then(|tl| tl.cues.get(cue_id))`.
  - For each `LocalCue(m)`: on `OnEnd`, despawn any `CosmeticProjectile` whose `end_cue == Some(m.cue_id)` (keep the C2 behavior); resolve `cue_binding_for(&m.skill_id, &m.cue_id, …)`; if `None`, no-op (trace `cue_unbound`). Else port `spawn_cue_effect` (EffectLibrary-first-then-VfxLibrary, warn-once on a name in neither).
  - `CueAttach::Follow`: look up `window_motion_for_cue(tl, &m.cue_id)` (strip `on_window_`/`emit_` off cue_id → the `CollisionWindow` → its `VolumeMotion` → `speed`/`gravity`); attach a `CosmeticProjectile { velocity: m.aim_dir * speed, gravity, end_cue: Some("on_end_" + window_id) }` so the sim's `OnEnd` tears it down. `World` (and OnCast/OnHit/OnEnd) spawn at `m.position` (raise `OnCast` by `MUZZLE_HEIGHT_OFFSET`).
  - Bake `ParamSource::Charge` params from `m.charge` (`charge.unwrap_or(0) as f32 / 255.0`), the same normalization obelisk documents; stat sources don't exist in `ParamSource` v1.
  - Keep the `cue_dispatch`/`lane_event`→`cue_effect` trace (harness observability).

- [ ] **Step 5: Run tests + build — expect green.**

Run: `cargo test -p arena_game resolves_cue_binding_by_skill_and_slot && cargo build -p arena_game`
Expected: PASS + 0 errors.

- [ ] **Step 6: Commit.**

```bash
git add -A
git commit -m "feat(arena/client): render cues via CueBinding + bevy_effect (EffectLibrary-first-then-VfxLibrary)"
```

---

## Task C4 (obelisk-arena): migrate `firebolt` → v2 triad + author `firebolt_explosion`

Adapt `obelisk-bevy`'s canonical `fireball`/`fireball_explosion` pair (see Reference material). Keep firebolt's existing arena identity (fire damage 20, applies `burn`, first-person ballistic bolt).

**Files:**
- Rewrite: `assets/skills/firebolt.cast.ron` (v2 behavior)
- Modify: `config/skills/firebolt.toml` (add the three trigger conditions)
- Create: `assets/skills/firebolt_explosion.cast.ron`, `config/skills/firebolt_explosion.toml`
- (Already deleted in C2: `assets/skills/firebolt.skillfx.ron`)
- Test: a headless load-test in `arena_game` (or `arena_sim`) that both timelines parse (no `deny_unknown_fields` reject) and the firebolt→firebolt_explosion trigger resolves to a second `DamageResolved`.

- [ ] **Step 1: Write the failing load+trigger test.** Boot a headless obelisk sim (mirror `arena_sim`/the reform's scenario harness), load `config/skills` + `assets/skills`, cast `firebolt` at a dummy in range, tick, and assert two `DamageResolved` events: `skill_id == "firebolt"` (direct) and `skill_id == "firebolt_explosion"` (triggered).

- [ ] **Step 2: Run it — expect FAIL** (current v1 `firebolt.cast.ron` fails `deny_unknown_fields`; no explosion skill).

- [ ] **Step 3: Rewrite `assets/skills/firebolt.cast.ron`** to v2 (note `vfx_cues` populated slot==value so cues actually fire — the obelisk fixture omits it because it's a sim-only test):

```ron
// Firebolt (v2): a ballistic fire bolt that triggers firebolt_explosion however it ends
// (enemy hit / world impact / fuse) via the rules conditions in config/skills/firebolt.toml.
// Causality is rules-side now — no inline chained blast window (that was v1).
(
  skill_id: "firebolt",
  phase_durations: ( windup: 0.3, active: 0.1, recovery: 0.2 ),
  collision_windows: [
    ( id: "bolt", spawn: Scheduled( phase: Active, offset: 0.0 ), active_duration: 2.0,
      shape: Sphere( radius: 0.5 ), motion: Ballistic( speed: 20.0, gravity: 9.8 ),
      hit_filter: Enemies, hit_mode: FirstOnly ),
  ],
  acquisition: Aim,
  chargeable: true,
  max_hold: 1.5,
  vfx_cues: {
    "on_cast": "on_cast",
    "on_window_bolt": "on_window_bolt",
    "on_hit": "on_hit",
    "on_end_bolt": "on_end_bolt",
  },
  cues: {
    "on_cast": ( effect: Some("Fire"), attach: World, anim: None,
      params: [ ( param: "scale", source: Charge ) ] ),
    "on_window_bolt": ( effect: Some("firebolt_trail"), attach: Follow, anim: None, params: [] ),
    "on_hit": ( effect: Some("Sparks"), attach: World, anim: None, params: [] ),
    "on_end_bolt": ( effect: Some("Explosion"), attach: World, anim: None, params: [] ),
  },
)
```

- [ ] **Step 4: Add the trigger conditions** to `config/skills/firebolt.toml` (append; keep the existing `[damage]`/`[[effect_applications]]` burn block):

```toml
# Trigger firebolt_explosion however the bolt ends. Exactly one fires per ending:
#   HitEntity -> always ; HitWorld -> on_impact ; Fuse -> on_expire.
# additional = true is REQUIRED for a timeline-target condition (else flagged invalid).
[[conditions]]
trigger_skill = "firebolt_explosion"
type = "always"
additional = true

[[conditions]]
trigger_skill = "firebolt_explosion"
type = "on_impact"
additional = true

[[conditions]]
trigger_skill = "firebolt_explosion"
type = "on_expire"
additional = true
```

- [ ] **Step 5: Create `config/skills/firebolt_explosion.toml`** (triggered-only AoE, its own damage):

```toml
id = "firebolt_explosion"
name = "Firebolt Explosion"
tags = ["spell", "fire"]
targeting = "single_enemy"
delivery = "projectile"
mana_cost = 0.0

[damage]
base_damages = [{ type = "fire", min = 15.0, max = 15.0 }]
```

- [ ] **Step 6: Create `assets/skills/firebolt_explosion.cast.ron`** (a Static AoE sphere at the trigger position — `WindowAnchor::CastPoint` resolves to the trigger's payload position):

```ron
// The triggered explosion: a brief static AoE sphere at the trigger position (the bolt's end
// point). Anchored CastPoint so it lands where the bolt actually stopped.
(
  skill_id: "firebolt_explosion",
  phase_durations: ( windup: 0.0, active: 0.05, recovery: 0.0 ),
  collision_windows: [
    ( id: "blast", spawn: Scheduled( phase: Active, offset: 0.0 ), anchor: CastPoint,
      active_duration: 0.05, shape: Sphere( radius: 1.5 ), motion: Static,
      hit_filter: Enemies, hit_mode: OncePerTarget ),
  ],
  acquisition: SelfPoint,
  vfx_cues: { "on_window_blast": "on_window_blast" },
  cues: {
    "on_window_blast": ( effect: Some("Explosion"), attach: World, anim: None, params: [] ),
  },
)
```

- [ ] **Step 7: Run the test — expect PASS** (both parse; two `DamageResolved`). Verify the exact `type` serialization against `obelisk-bevy/tests/fixtures/skills/fireball.toml` if the loader rejects.

Run: `cargo test -p arena_game firebolt_triggers_explosion` (name your test)
Expected: PASS.

- [ ] **Step 8: Commit.**

```bash
git add assets/skills/firebolt.cast.ron assets/skills/firebolt_explosion.cast.ron config/skills/firebolt.toml config/skills/firebolt_explosion.toml
git commit -m "content(firebolt): v2 triad — ballistic bolt triggers firebolt_explosion on any ending"
```

---

## Task C5 (obelisk-arena): re-key `chain_lightning` to the reform

**Files:**
- Rewrite: `assets/skills/chain_lightning.cast.ron` (v2: a `Beam` window + `chain_radius`; cues)
- Modify: `config/skills/chain_lightning.toml` (add `[damage] can_chain = true`, `chain_count`)
- (Already deleted in C2: `chain_lightning.skillfx.ron`)
- Test: a headless chain test (cast at one of two in-range enemies; assert a hop damages the second).

- [ ] **Step 1: Write the failing hop test** (two enemies within `chain_radius`; cast chain_lightning at the first; assert `DamageResolved` on BOTH).

- [ ] **Step 2: Run it — expect FAIL** (v1 content / no chaining).

- [ ] **Step 3: Set chain rules** in `config/skills/chain_lightning.toml` under `[damage]`:

```toml
[damage]
base_damages = [{ type = "lightning", min = 12.0, max = 18.0 }]
base_crit_chance = 0.08
can_chain = true
chain_count = 2
```

- [ ] **Step 4: Rewrite `assets/skills/chain_lightning.cast.ron`** to a `Beam` window with `chain_radius` (a beam hop re-keys to the struck target and searches `chain_radius` for the next):

```ron
(
  skill_id: "chain_lightning",
  phase_durations: ( windup: 0.25, active: 0.1, recovery: 0.15 ),
  collision_windows: [
    ( id: "arc", spawn: Scheduled( phase: Active, offset: 0.0 ), active_duration: 0.1,
      shape: Sphere( radius: 0.4 ), motion: Beam,
      hit_filter: Enemies, hit_mode: FirstOnly ),
  ],
  acquisition: HitscanEntity( range: 20.0, filter: Enemies, fallback: Fizzle ),
  chain_radius: 6.0,
  vfx_cues: { "on_window_arc": "on_window_arc", "on_hit": "on_hit" },
  cues: {
    "on_window_arc": ( effect: Some("Sparks"), attach: World, anim: None, params: [] ),
    "on_hit": ( effect: Some("Sparks"), attach: World, anim: None, params: [] ),
  },
)
```

(`Beam` needs a designated target — `HitscanEntity` acquisition supplies it; the arena server's C1 rewrite produces `CastAim::Entity` for this skill.)

- [ ] **Step 5: Run — expect PASS.**

Run: `cargo test -p arena_game chain_lightning_hops`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add assets/skills/chain_lightning.cast.ron config/skills/chain_lightning.toml
git commit -m "content(chain_lightning): re-key to reformed chaining (can_chain/chain_count + Beam + chain_radius)"
```

---

## Task C6 (obelisk-arena): author `blizzard` (GroundPoint + Emitter + Template)

A fresh reference exercising the acquisition + emitter surface the other two don't.

**Files:**
- Create: `assets/skills/blizzard.cast.ron`, `config/skills/blizzard.toml`
- Modify: `crates/arena_game/src/server/spawn.rs` (grant `blizzard` alongside `firebolt` so it's castable in the arena) — OR leave ungranted and exercise via the editor/test only (choose: the net-test only needs firebolt; grant blizzard for playtest but keep it out of the automated gate).
- Test: a headless emitter test (cast blizzard at a ground point; assert ≥1 shard `HitConfirmed`/`DamageResolved` from the emitted `Template` window).

- [ ] **Step 1: Write the failing emitter test.**

- [ ] **Step 2: Run it — expect FAIL** (no blizzard).

- [ ] **Step 3: Create `config/skills/blizzard.toml`:**

```toml
id = "blizzard"
name = "Blizzard"
tags = ["spell", "cold"]
targeting = "ground"
delivery = "area"
mana_cost = 12.0
cooldown = 6.0

[damage]
base_damages = [{ type = "cold", min = 6.0, max = 10.0 }]
```

- [ ] **Step 4: Create `assets/skills/blizzard.cast.ron`** (a carrier cloud that emits falling shard `Template` instances; `GroundPoint` acquisition with a `SelfPoint` fallback so an unaimed cast still lands overhead):

```ron
// Blizzard: a ground-targeted storm. A non-striking carrier cloud hangs above the cast point and
// rains `shard` Template instances (Down motion) on the emitter clock; each shard strikes.
(
  skill_id: "blizzard",
  phase_durations: ( windup: 0.4, active: 3.0, recovery: 0.3 ),
  collision_windows: [
    ( id: "cloud", spawn: Scheduled( phase: Active, offset: 0.0 ), anchor: CastPoint,
      anchor_offset: ( 0.0, 8.0, 0.0 ), strikes: false, active_duration: 3.0,
      shape: Sphere( radius: 3.0 ), motion: Static,
      hit_filter: Enemies, hit_mode: OncePerTarget,
      emitter: Some(( rate: 8.0, jitter: 3.0, window: "shard" )) ),
    ( id: "shard", spawn: Template, anchor: CastPoint, active_duration: 1.2,
      shape: Sphere( radius: 0.5 ), motion: Linear( speed: 12.0 ), motion_direction: Down,
      hit_filter: Enemies, hit_mode: OncePerTarget ),
  ],
  acquisition: GroundPoint( range: 25.0, fallback: Then(SelfPoint) ),
  vfx_cues: { "emit_shard": "emit_shard", "on_hit": "on_hit" },
  cues: {
    "emit_shard": ( effect: Some("Snow"), attach: World, anim: None, params: [] ),
    "on_hit": ( effect: Some("Sparks"), attach: World, anim: None, params: [] ),
  },
)
```

(Verify `MotionDirection::Down` is the exact variant name in `src/assets/mod.rs:337` — the enum was read partially; confirm `Down` exists or use the correct downward variant.)

- [ ] **Step 5: Run — expect PASS.**

Run: `cargo test -p arena_game blizzard_emits_shards`
Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add assets/skills/blizzard.cast.ron config/skills/blizzard.toml crates/arena_game/src/server/spawn.rs
git commit -m "content(blizzard): ground-targeted storm — GroundPoint acquisition + emitter/Template shards"
```

---

## Task C7 (obelisk-arena): extend the net-test to assert the trigger

The integration gate. Confirms firebolt's direct hit AND its triggered explosion both cross the wire to both observers.

**Files:**
- Modify: `crates/arena_game/tools/net-test/summarize.py` (add explosion assertions 5–6)
- No code change if the trace kinds already carry `skill_id` (they do: `server_net_damage_resolved`/`client_net_damage_resolved` include `skill_id`).

- [ ] **Step 1: Add the explosion assertions** in `summarize.py`, after the existing firebolt block:

```python
    # --- (5) server DamageResolved(skill=firebolt_explosion) — the trigger fired end-to-end ---
    server_expl = [e for e in server
                   if e.get("kind") == "server_net_damage_resolved"
                   and e.get("skill_id") == "firebolt_explosion"
                   and e.get("caster") == caster_id]
    if not server_expl:
        failures.append(
            f"server emitted no DamageResolved(caster={caster_id}, skill=firebolt_explosion) "
            f"— firebolt's on-hit trigger did not fire")

    # --- (6) BOTH observers received the triggered explosion damage ---
    for name, evs in (("observer-0", obs0), ("observer-1", obs1)):
        got_expl = [e for e in evs
                    if e.get("kind") == "client_net_damage_resolved"
                    and e.get("skill_id") == "firebolt_explosion"
                    and e.get("caster") == caster_id]
        if not got_expl:
            failures.append(f"{name} received no DamageResolved(firebolt_explosion)")
```

Add the two counts to the printed verdict block for visibility.

- [ ] **Step 2: Run the net-test — expect PASS.**

Run: `bash crates/arena_game/tools/net-test/run_session.sh`
Expected: `PASS` (all six assertion groups). If the explosion doesn't reach the target, verify the bolt actually connects at the duel gap (the harness aims via `ARENA_CAM_YAW=-1.5707963`; the ballistic arc must still land — the v1 gate already relied on this).

- [ ] **Step 3: Commit.**

```bash
git add crates/arena_game/tools/net-test/summarize.py
git commit -m "test(net): assert firebolt_explosion trigger replicates to both observers"
```

---

## Task C8 (obelisk-arena): thin `arena_editor` to a host shell

**Prerequisite: P1.** Repin `arena_editor/Cargo.toml`'s `bevy_modal_editor`/`bevy_vfx`/`bevy_editor_game` to the merged `main` and enable the `obelisk` feature on `bevy_modal_editor`.

**Files:**
- Modify: `crates/arena_editor/Cargo.toml` (repin; `bevy_modal_editor = { …, features = ["obelisk"] }`; drop deps only used by deleted modules)
- Rewrite: `crates/arena_editor/src/main.rs` (thin shell)
- Modify: `crates/arena_editor/src/lib.rs` (drop the deleted module decls; keep `io::editor_root` if still referenced, else inline)
- Delete: the ~24 skill-designer modules (`SkillDesignerPlugin` + `preview_*`, `effects_panel.rs`, `stat_ui.rs`, `trigger_ui.rs`, `vfx_bind.rs`, `derived.rs`, `preview_controller/socket/scrub/cosmetics`, the effect-config half of `io.rs`, etc.), and `sim_config.rs`'s constants/skills/RNG seeding (the built-in stage seeds those)
- Keep: `character.glb` registration, `PhysicsGizmos` init, and ONE arena line: `add_obelisk_effects(config/effects)` (ailment registry for previews) — fold into `main.rs` or a 3-line plugin
- Delete: the `arena_sim::preview::ArenaSimPreviewPlugin` usage (the built-in stage owns preview physics+sim)
- Test: a headless smoke test that the shell boots, registers the arena content root, and lists ≥1 skill (`firebolt`).

- [ ] **Step 1: Write the failing smoke test** (or a `--smoke-frames` harness like the existing editor smoke): boot the shell headless, run N frames, assert the `SkillLibrary` contains `firebolt`/`firebolt_explosion`/`chain_lightning`/`blizzard` and no panic.

- [ ] **Step 2: Rewrite `main.rs`:**

```rust
use bevy::prelude::*;
use bevy_editor_game::RegisterGltfLibraryExt;
use bevy_modal_editor::{recommended_image_plugin, EditorPlugin, EditorPluginConfig,
    GamePlugin, skill::RegisterObeliskContentExt};
use obelisk_bevy::prelude::ObeliskConfigExt;

fn main() {
    let root = arena_editor::io::editor_root();
    App::new()
        .add_plugins(DefaultPlugins.set(recommended_image_plugin()).set(AssetPlugin {
            file_path: root.join("assets").to_string_lossy().into_owned(),
            ..default()
        }))
        // EditorPlugin with the `obelisk` feature auto-wires Skill mode + bevy_effect + EffectLibrary.
        .add_plugins(EditorPlugin::new(EditorPluginConfig {
            add_physics: false, add_egui: true, ..default()
        }))
        .add_plugins(GamePlugin)
        .register_gltf_library("character.glb")
        // The built-in preview stage seeds obelisk constants + RNG + sim itself, and
        // register_obelisk_content loads the skill triad + assets/effects + assets/vfx presets.
        // The one thing it does NOT load is stat_core AILMENT effects (config/effects/*.toml),
        // which previews that apply `burn` need — supply them here.
        .add_obelisk_effects(&root.join("config/effects"))
        .register_obelisk_content(root.clone())
        .init_gizmo_group::<avian3d::prelude::PhysicsGizmos>()
        .run();
}
```

- [ ] **Step 3: Delete the designer modules** and prune `lib.rs`/`Cargo.toml`. Build iteratively, deleting until green.

- [ ] **Step 4: Run the smoke test + build — expect green.**

Run: `cargo build -p arena_editor --features obelisk && cargo test -p arena_editor editor_shell_lists_arena_skills`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add -A
git commit -m "refactor(arena_editor): thin to a host shell over the built-in Skill mode; delete the designer"
```

---

## Task C9 (obelisk-arena): follow-ups doc (carried tickets + accepted gaps)

**Files:**
- Create: `docs/superpowers/plans/2026-07-05-phase4-followups.md`

- [ ] **Step 1: Record the carried tickets + accepted scope, then commit.** Content:
  - **Effect (ailment) authoring gap** (Decision 5): Skill mode does not author `stat_core` ailments; `config/effects/*.toml` stay hand-authored (loaded via `add_obelisk_effects`). Ticket: a future editor Effect-authoring mode.
  - **Rig-less preview** (Decision 6): the ported stage doesn't consume `character.glb`; the editor caster is a capsule. Ticket: wire a host-provided rig into the preview stage (a `bevy_modal_editor` enhancement); until then `anim` cue bindings are inert in editor preview and unused by the client host.
  - **Spatial-trigger `TriggerFired`** (Decision 4): obelisk-bevy fires no `TriggerFired` for the hit-trigger executor (`src/combat/system.rs:325`) or the lifecycle-trigger site; the arena observes triggered skills via their own `DamageResolved`/cues. Ticket (obelisk-bevy, observability): emit `TriggerFired` at the spatial-trigger edges.
  - **Trigger-reform tickets that touch arena content** (from `2026-07-03-trigger-reform-followups.md`): #4 `nearest_retarget_candidate` no liveness check (chains can hop to corpses — now gameplay-visible via `chain_lightning`); #7 facade `Vec3::ZERO` fallback places transform-less triggered executions at the origin (relevant to triggered explosions). File upstream.
  - **`chargeable`/`max_hold`** are authored metadata only — the arena's hold-to-charge already flows through the `charge: u8` cast param (`ChargeState`/`charge_byte_from_frac`), unchanged by the reform.

```bash
git add docs/superpowers/plans/2026-07-05-phase4-followups.md
git commit -m "docs: phase-4 follow-up tickets + accepted scope reductions"
```

---

## Self-Review

**Spec coverage (stub §3.4 items a–i):**
- (a) thin `arena_editor` → **C8**. (b) kill `arena_skills` + `.skillfx.ron` → **C2**. (c) cue wire contract → **C2** (wire) + **A1** (skill_id enabler). (d) shared acquisition resolver → **C1** (settled arena-local; Decision 1). (e) client `bevy_effect` render → **C3**. (f) reference content (firebolt pair / chain_lightning / blizzard) → **C4/C5/C6**. (g) extended net-test → **C7**. (h) flip obelisk-bevy pin → **C1**. (i) carry 4 open tickets → **C9**. ✅ all covered.

**Corrections to the stub baked into the plan** (verified against source, 2026-07-05): `vfx_cues` coexists with `cues` (not renamed) — Global Constraints; cue-id == slot → wire needs `skill_id` → **A1**; spatial `TriggerFired` not required → net-test asserts via explosion damage (**C7**, **C9**); acquisition resolver is arena-local (Decision 1); `register_obelisk_content` + `EditorPlugin(obelisk)` are the real host API (**C8**); ailment authoring out of scope (Decision 5, **C9**).

**Placeholder scan:** no "TBD"/"add validation"/"similar to Task N". Two explicit verify-against-source steps remain (the exact `TriggerCondition` TOML tag in C4-Step 7 — cross-checked to `type = "on_impact"` from the shipped fixture; `MotionDirection::Down` in C6-Step 4) — both name the exact reference to confirm, not a guess to fill in.

**Type consistency:** `CueMessage` fields (`cue_id, skill_id, source_id, position, aim_dir, position_from, charge, end_reason, kind`) are identical across C2 (definition), C3 (consumer), and C7 (trace). `resolve_cast_aim`/`CastAim`/`Acquisition` variant names match `src/assets/mod.rs`/`src/timeline/cast.rs`. `cue_binding_for(skill_id, cue_id, …)` in C3 matches the wire's `skill_id`/`cue_id`. `firebolt_explosion` id is identical across C4's TOML, RON, trigger condition, and C7's assertion.

**Ordering:** A1 → (P2 merge) → C1 → C2 → [C4,C5,C6 content, order-independent] → C7 gate; C3 and C8 gate on P1 and can land any time after C2/C1 respectively. Each task ends `cargo build` + unit-test green; the net-test (C7) is the integration gate (RED between C1 and C7 by design, noted at C1-Step 6).
