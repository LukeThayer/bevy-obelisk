# obelisk-arena — Phase 1 Design: the 1v1 online game (+ AI monster)

**Date:** 2026-06-25
**Status:** Approved design (brainstorm complete). Ready for implementation planning.
**Umbrella:** `docs/superpowers/specs/2026-06-24-obelisk-arena-design.md` (this is Phase 1 of that vision).
**Context brief:** `docs/superpowers/specs/2026-06-25-obelisk-arena-phase1-context.md` (the reusable inventory + integration architecture this spec builds on).
**Prerequisite:** Phase 0 is **done** — obelisk-bevy is on Bevy 0.18.1 / Avian 0.5.0.

---

## 1. Goal

Build the **`obelisk-arena` Cargo workspace** and a **dedicated-server, server-authoritative 1v1 online duel** on Bevy 0.18 / Avian 0.5 / lightyear 0.26 that composes the three integrations — wisp's netcode + rig/animation, obelisk's deterministic combat, and (for the AI monster) the editor's navmesh — into one playable, replicable template. Phase 1 proves every integration boundary end-to-end and lays the real architectural foundation (including the shared `arena_skills` data crate) that Phases 2–3 (editor + skill designer) build on.

**Definition of done:** the five milestones (M0–M4 in §10) each pass their demo + trace-regression assertion, on a dedicated headless server with two connected clients, with the AI monster PvE scene functional.

---

## 2. Scope & the game

- **Mode:** dedicated-server **1v1 duel**. Two clients connect to a headless server process; **best-of-3 rounds**; a round is won by reducing the opponent's obelisk HP to 0; between rounds both players respawn at fixed spawn markers and HP/cooldowns reset.
- **AI monster:** lives in a **separate PvE scene** (its own server scene/mode), not in the duel — keeping PvP and PvE clean and independently demoable.
- **Arena:** hard-coded geometry (a bounded arena with a few obstacles). Editor-authored scenes are Phase 2.

**Explicitly out of Phase 1:** the editor (`arena_editor`, `EditorPlugin`, editor-mode toggle) → Phase 2; the skill designer (timeline UI, "Play Skill") → Phase 3; editor-authored scenes → Phase 2; loot pickup / item networking; deep AI (rotations, interrupts, lead-targeting, threat). The `arena_skills` crate **is** in Phase 1 (built now, per the approved decision) but stays minimal until the designer fleshes it out.

---

## 3. Workspace & crates

New `obelisk-arena/` workspace (Bevy 0.18.1 / Avian 0.5.0 / lightyear 0.26), **two** members in Phase 1 (`arena_editor` is Phase 2):

| Crate | Responsibility | Depends on |
|---|---|---|
| **`arena_skills`** | Shared skill data + bridge: the `.skillfx.ron` sidecar format, the cue→effect binding layer, the `CueMessage` wire type + cue-key naming contract. App-agnostic (no lightyear/wisp deps). | `obelisk-bevy` (path), `serde`, `ron` |
| **`arena_game`** | The dedicated server + client game: netcode, third-person rig + animation, obelisk combat, the duel, the AI monster, the PvE scene, the trace harness. | `arena_skills`, `obelisk-bevy`, `lightyear`, `bevy_landmass`, `bevy_rerecast` (path), `serde`, `ron` |

**Reuse model:**
- **path-dep** the clean libraries: `obelisk-bevy` (combat), `bevy_landmass` + `bevy_rerecast` (navmesh).
- **copy-and-diverge** the specific reusable wisp files (net loop, rig/anim, trace) into `arena_game` with attribution comments. Rationale: wisp's controller is first-person + pre-netcode-complete and the arena adapts it heavily; a path-dep would couple this *template* to wisp's churn for code we're rewriting anyway. Copying keeps the template self-contained.

The exact wisp files to copy and their adaptation verdicts are enumerated in the context brief §1a–1b (e.g., copy `server.rs:196-232` late-joiner refresh, `lib.rs:57-70` `add_avian_with_lightyear`, `trace.rs`; rebuild the controller and the animation binding).

---

## 4. `arena_skills` + the `.skillfx.ron` format

A skill = obelisk's existing `.cast.ron` (`CastTimeline`: phases, hit windows, `vfx_cues`) **+ a new `.skillfx.ron` sidecar** binding each cue key to presentation lane-events.

### 4.1 Format

```ron
// assets/skills/firebolt.skillfx.ron — sidecar to firebolt.cast.ron
SkillFx(
    bindings: {
        "on_cast":        [ Anim(clip: "cast_windup", layer: Upper), Particle(effect: "charge_fx", socket: "wand_tip") ],
        "on_window_bolt": [ Projectile(effect: "firebolt", socket: "wand_tip", speed: 18.0), Particle(effect: "muzzle_fx", socket: "wand_tip") ],
        "on_hit":         [ Particle(effect: "impact_fx", socket: "hit_point") ],
    },
)
```

Lane-event variants (serde enum `LaneEvent`):
- `Anim { clip: String, layer: AnimLayer }` — play a named animation clip on a body layer (`Upper`/`Full`).
- `Particle { effect: String, socket: String }` — spawn a particle effect at a named socket bone.
- `Projectile { effect: String, socket: String, speed: f32 }` — spawn a cosmetic projectile (the *authoritative* projectile is obelisk's; this is its visual).

### 4.2 What `arena_skills` provides
- The `SkillFx` / `LaneEvent` / `AnimLayer` serde types and a RON loader.
- A `SkillFxRegistry` resource, loaded alongside obelisk's `SkillRegistry` from the same asset directory.
- The **binding layer**: a consumer (registered via obelisk's `observe_cue`) that, on a local `CueEvent` or a replicated `CueMessage`, looks up `cue_id → [LaneEvent]` and dispatches each (play clip / spawn particle at socket / spawn cosmetic projectile). Missing assets **no-op with a logged warning** (never crash — mirrors obelisk's lenient effect-apply).
- The `CueMessage { cue_id: String, source_id: ObeliskId, position: Vec3, kind: CueKind }` wire type (plain serde; `arena_game` wraps it as a lightyear message).

### 4.3 The locked cue contract
Phase 1 fixes the conventions that cross the network and constrain the Phase 3 designer:
- **Cue key naming:** `on_cast`, `on_window_<id>`, `on_hit` (matching obelisk's `vfx_cues` keys).
- **Wire shape:** `CueMessage` as above.

The `.skillfx.ron` *format* is intentionally minimal-but-real: Phase 3's designer becomes a UI that authors these same files (no parallel model), and `arena_skills` is exactly the seam `arena_editor` will later share. The format may grow (more lane-event variants, timing offsets) in Phase 3; the cue contract should not.

---

## 5. Networking

**lightyear 0.26, dedicated server, server-authoritative** (wisp's model). The server is the sole authority for hit resolution, damage, effects, cooldowns, mana, and death; `CombatRng` lives **server-side only** in Stage A (§6.2) and is deterministically reproducible on clients in Stage B (§6.3).

### 5.1 Server loop
Headless `MinimalPlugins` binary: `ServerNetPlugin` (copied wisp) + `add_avian_with_lightyear` + `ObeliskSimPlugin` + arena scene + AI. Per `FixedUpdate` tick: drain client inputs → movement controller (wisp pattern, adapted) → physics → obelisk `Validate→Advance→Projectiles→ResolveHits→TickEffects` → `HitConfirmed`→`resolve_one_hit` → `DamageResolved`/`EffectApplied`/`EntityDied`. An **egress bridge** (Update) drains `MessageReader<NetEvent>` + `CueEvent` and converts them to lightyear messages (a combat-event message + `CueMessage`), syncs `Transform`→`NetworkedPosition` (extended with cast phase/elapsed/skill-id), and mirrors hp into `NetworkedHealth`. `refresh_replicate_on_connect` (copied) keeps the manual sender list current for late joiners.

### 5.2 Input → cast path
Client input → `PlayerInputMessage` (movement on an unreliable channel; `cast_request: Option<(skill_id, target_hint)>` on a **reliable** channel). Server drains inputs into per-player state, then (FixedUpdate) calls `entity.cast_skill_at(skill_id, target)` → `PendingCast`. Obelisk `Validate` gates range / LOS / mana / cooldown / already-casting; **the server re-validates server-side** (treats `target_hint` as a hint, re-acquires/re-checks) and rejects out-of-range/cheating casts with `CastRejected`. Accept → `ActiveCast` + `CastBegan` → hit windows → resolution.

### 5.3 Presentation reactions (the sim↔present split)
VFX, animation, and projectiles are **client-side reactions** to replicated `CueMessage` + `DamageResolved` + replicated cast state. Clients never spawn `Hitbox`es and never feed `on_hit_confirmed`. This is obelisk's sim↔present split, exactly as umbrella §11 specifies.

**Cue sourcing rule (predicted vs replicated):** remote entities' cues (the other player, the monster) bind from replicated `CueMessage`. The local predicting player additionally fires its *own* `on_cast` and projectile-spawn cues from its predicted local sim for zero-latency feedback; these are de-duplicated against the replicated copy by `source_id` so a cue plays once. Resolution-dependent cues (`on_hit`) come from the server (replicated) in Stage A, and from the predicted sim in Stage B.

---

## 6. Client prediction & rollback

Built in two stages (M2 then M3), both reaching the chosen "full prediction/rollback" end-state.

### 6.1 Movement prediction (Stage A, M2)
Standard lightyear: each client owns a *predicted* replica of its player, simulates the kinematic controller forward from local inputs, and rolls back + replays when the server's authoritative `Position` arrives mismatched. wisp has the Position-rollback path registered-but-disabled ("Stage Q"); Phase 1 finishes and enables it. Low risk.

### 6.2 Stage A combat (M2): predict motion, server-authoritative damage
Predict own **cast initiation** (windup animation + timeline start) and **projectile motion** (deterministic, no RNG) for instant feedback. The client runs `ObeliskSimPlugin`'s timeline + projectile systems for the predicting player (so cast phases and projectile motion advance locally) but performs **no hit resolution and touches no `CombatRng`**. **Hit detection + damage resolution are server-authoritative** — the server resolves and the damage number/flash lands on server confirm a few ms later; the client reconciles its predicted cast state against the replicated `ActiveCast`. No `CombatRng` on clients; no obelisk change. This is how many shipping action games handle it, and it makes M2 responsive and playable quickly.

### 6.3 Stage B combat (M3): predicted hit + damage via rollback-safe RNG
Add client-side prediction of **hit + damage**. The hazard is obelisk's `CombatRng` being a stateful *stream* — under rollback, a single mispredicted hit desyncs the stream and cascades. The fix is the standard deterministic-rollback technique: **rollback-safe, stateless-per-event RNG**. Each resolution seeds a fresh RNG from a key that is a pure function of replicated state:

```
seed = hash(match_seed, tick, caster_id, target_id, window_id)
```

Each resolution is then independent (a mispredicted hit cannot corrupt another's roll) and re-simulating a rolled-back tick reproduces identical rolls. This requires:
- **An obelisk addition (`resolve_seeded`):** a resolution facade that takes an explicit per-call seed instead of drawing from the global `CombatRng` stream. A deliberate, testable obelisk-bevy change carried into Phase 1 (like Phase 0's feat branch); it strengthens obelisk's determinism story and is covered by a golden/unit test on the seeding.
- **Rollback-registered combat state:** `ActiveCast`, `Cooldowns`, effect timers, and hp registered with lightyear's rollback so the predicted obelisk sim re-runs correctly on correction.
- The client runs `ObeliskSimPlugin` in its predicted timeline; `match_seed` is replicated; the per-resolution key makes predicted and authoritative resolutions agree when inputs match, and self-heal via rollback when they don't.

---

## 7. Characters & animation

- **Third-person controller** (rebuilt from wisp's first-person base): orbit camera, camera-relative WASD movement, mouse-aim. Keep wisp's `apply_aim_pitch_to_local_spine` (`chest_joint` Z-rotate aim-lean) and the query-bones-by-`Name` pattern.
- **Rig:** the wisp `character.glb` rig, reused with adaptation. Locomotion clips (idle/walk_*/falling) + the direction-weighted blend math copy.
- **Animation binding (rebuilt):** driven by obelisk **`ActiveCast.phase` + elapsed**, *not* wisp's input-reactive casting flag. Lower body always blends locomotion from velocity; when an `ActiveCast` exists, an upper-body cast layer plays the phase clip. **Phase 1 fidelity:** map Windup/Active/Recovery onto the existing `casting_*` clips (a casting-stance blend per phase); bespoke per-phase clips are deferred content work.
- **Sockets:** named bones (e.g. `wand_tip`) queried by `Name`, used as `.skillfx.ron` spawn points.
- **Replication fidelity:** replicate the full `ActiveCast` (phase + elapsed + skill_id) so remote players animate casts precisely; replicate max-HP from obelisk's `StatBlock`.

---

## 8. AI monster + navmesh (the PvE scene)

A **separate server scene**. At scene start the server **bakes a navmesh** from the arena's static colliders (copy the editor's `NavigationPlugin` + bake flow + `extract_wireframe` debug viz) — **full bake**, proving the `bevy_landmass`/`bevy_rerecast` integration (the arena has obstacles to path around). The monster is:
- a kinematic landmass `Agent3d` (steering decoupled from physics), **and**
- an obelisk `Combatant` via `make_combatant` (`Faction::Enemy`, `SkillSlots` with a ranged skill id, a `Hurtbox`).

Each `FixedUpdate`: target = **nearest player** (re-evaluated; re-target the other if one dies) → `AgentTarget3d::Entity(player)` → landmass computes `AgentDesiredVelocity3d` → `move_agents` applies to `Transform`. A **monster-action system** (after movement) uses `ObeliskSpatial::nearest_enemy`/range + `Cooldowns` gates → `entity.cast_skill_at(skill_id, player)`, flowing through the *same* `CastBegan→hit window→resolve_one_hit` path as a player. The monster is **server-authoritative and replicated/interpolated to clients** — it is not client-predicted; clients render its chase + casts from replicated state + `CueMessage`.

**AI depth:** chase-nearest + cadence-cast ranged only (no rotations, interrupts, threat). **Death:** `EntityDied` → despawn (no loot). The monster's ranged skill has its own `.cast.ron` (Projectile delivery + collision window + `vfx_cues`) and `.skillfx.ron`, driven by the same binding as the player skill.

---

## 9. Testing

- **Trace harness (front-loaded, M0):** copy `wisp/src/trace.rs` + adapt `run_session.sh`/`summarize.sh`. Every milestone carries a regression assertion over the JSONL trace (e.g., "player casts firebolt → both peers observe `CastBegan → HitConfirmed → DamageResolved`").
- **Combat correctness:** rides obelisk-bevy's deterministic scenario suite (unchanged; obelisk is path-dep'd).
- **Prediction determinism (M3):** a dedicated test that `resolve_seeded` reproduces identical outcomes for the same key on client and server (the rollback-safety guarantee), plus a same-seed idempotence test.
- **Net regression:** headless server + a scripted, trace-emitting observer client (wisp's `observer` binary as the template), extended with skill-cast script commands.

---

## 10. Milestones (implementation decomposition)

Phase 1 is one spec; each milestone is a tractable build cycle with its own plan and demo + trace assertion.

- **M0 — scaffold + `arena_skills`.** Create the `obelisk-arena` workspace + both crates; `.skillfx.ron` types + loader + binding layer + `CueMessage`; author firebolt `.cast.ron` + `.skillfx.ron`; wire path-deps; trace-harness skeleton (copy `trace.rs`).
- **M1 — single-player move + cast.** Third-person controller + rig; player as obelisk `Combatant`; cast firebolt via `cast_skill_at`; `ActiveCast.phase` drives the animation blend; `CueEvent`→binding→particle + cosmetic projectile. Co-located (no net split). *Proves:* obelisk + rig + animation + skillfx compose; sim↔present split holds.
- **M2 — 2-player online, Stage A prediction.** Dedicated server runs `ObeliskSimPlugin` + `CombatRng`; two clients connect; cast_request→server validate→`NetEvent`/`CueMessage` egress→both clients render damage + VFX; predict own movement + cast + projectile, **server-authoritative damage**; `NetworkedHealth` HUD; best-of-3 round flow; late-joiner refresh; net trace regression. *Proves:* the full input→server-obelisk→replicated→client-present loop, server authority, the `NetEvent`→lightyear bridge.
- **M3 — Stage B combat prediction.** obelisk `resolve_seeded`; deterministic per-resolution seeding; rollback-registered combat state; **predicted hit + damage**; the prediction-determinism test. *Proves:* the deterministic-combat-rollback showcase.
- **M4 — AI monster PvE scene.** Navmesh bake; monster as landmass `Agent3d` + obelisk `Combatant`; chase nearest + cadence-cast ranged; replicated to clients; despawn on death. *Proves:* obelisk skills drive NPCs; navmesh-steering + obelisk-casting compose on the server; AI state replicates with no client decision logic.

---

## 11. Data formats

All RON/TOML, hand-editable and diffable:
- **Skill stats:** obelisk stat TOML (unchanged).
- **Cast timeline:** obelisk `.cast.ron` (`CastTimeline`: phases, windows, `vfx_cues`, targeting, delivery) — unchanged.
- **Skill FX sidecar:** the new `.skillfx.ron` (§4) — lane-events keyed to cues.
- **Arena:** Phase 1 hard-coded in Rust (geometry-as-primitives, `spawn_cube`/`spawn_cylinder` style from the editor's `level_gen`); editor-authored scenes are Phase 2.

---

## 12. Error handling

Load failures (RON/TOML) surface visibly in logs (never silent). A missing animation clip / particle effect / projectile asset logs a warning and the binding layer **no-ops gracefully at runtime** (mirrors obelisk's lenient effect-apply), so a half-authored skill never crashes the game or the server. Cast rejections (range/LOS/mana/cooldown) are explicit (`CastRejected` → client feedback), never silent drops.

---

## 13. Dependencies & carries

- **obelisk-bevy `resolve_seeded` addition** (§6.3): a small, deliberate obelisk-bevy/obelisk change to enable rollback-safe combat prediction (Stage B / M3). Authored on a branch, gated by obelisk's golden + determinism suite, resolved (merge/publish) the same way as Phase 0's feat branch. **Not needed for M0–M2** (Stage A uses no client RNG).
- **Path deps** to `../obelisk-bevy`, `../bevy_landmass`, `../bevy_rerecast` (or the editor's vendored versions) for local dev; portability (git deps) deferred.
- **Copied wisp code** carries attribution; no runtime dep on wisp.

---

## 14. Open / deferred

- **Animation fidelity:** Phase 1 reuses `casting_*` clips; bespoke per-phase cast clips are later content work.
- **Listen-server:** not in Phase 1 (dedicated only); could be added later.
- **Workspace portability:** path deps now; git deps later.
- **arena_skills format growth:** the designer (Phase 3) may extend `.skillfx.ron` lane-events; the cue contract (§4.3) stays stable.
- **Loot / item networking, deep AI:** deferred beyond Phase 1.
