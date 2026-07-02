# Event-Driven Skill Phases — Window End Events + Chaining — Design Spec

**Status: SHIPPED 2026-07-02** (obelisk-bevy 6afa6ba + arena firebolt v2). Kept as the design rationale.

**Goal:** Skills are physics-driven event sequences, not static animations. A moving hit volume
*ends* somewhere — on an enemy, on the world, or on a timer — and that ending, with its world
position, is a first-class event the skill reacts to: it can chain the next hit volume there
(explosion splash), and it drives what the moment looks like (the explosion renders where the
bolt actually stopped, never "always on an enemy"). The proving case is firebolt v2:
`bolt` (ballistic projectile) → ends for any reason → `blast` (AoE sphere at the end position).

**Architecture:** The event model lives in obelisk-bevy (the sim owns termination + chaining, so
it is deterministic and identical on server/preview); the *world* itself stays the host's job —
obelisk learns "this hitbox hit the world at P" through a host-fired trigger, exactly as physics
stays the host's job today. `arena_sim` supplies the arena's flat-floor world hit; `arena_game`
replicates end cues over the existing `CueWireMessage`; `arena_editor` authors the reactions and
previews the sequence (timeline, trajectory gizmo, scrub).

**Background:** Groundwork already landed (2026-07-02, unpushed): `VolumeMotion::Ballistic
{ speed, gravity }` + `Projectile.gravity` (semi-implicit Euler, golden-clean),
`arena_sim::ballistics::ballistic_launch_dir`, and an arena `ground_stop_projectile_hitboxes`
system that silently despawns grounded bolts — that silent despawn is precisely the hole this
spec fills: it becomes the `HitWorld` ending.

---

## Problem (what the current model cannot say)

Today `CastTimeline` is a fixed schedule: phases → windows spawn at authored times anchored to
the caster → cues fire at three hardcoded moments (`on_cast`, `on_window_{id}` at open,
`on_hit` anchored to a struck enemy). Concretely, for firebolt:

1. A bolt that hits the ground or times out mid-air just vanishes. No event, no explosion, no
   sound hook — content cannot react to the physics outcome.
2. "Explosion" is only expressible as an on-enemy cue. There is no splash damage and no
   at-the-impact-point anything.
3. Nothing can *cause* the next thing. No projectile→AoE, no AoE→lingering-field, no sequences.
4. The cosmetic projectile flies open-loop; it cannot know when/where the authoritative bolt
   ended (today it flies for a hardcoded lifetime).

## Decisions (proposed — confirm before implementation)

1. **Every hitbox ends with a reason + position.** `EndReason { HitEntity, HitWorld, Fuse }`:
   - `HitEntity` — terminal entity hit (today: `HitMode::FirstOnly` after its first confirm).
   - `HitWorld` — host-reported world impact (arena: the flat floor; later: walls/obstacles).
   - `Fuse` — `active_duration` elapsed. The existing window duration IS the fuse timer —
     "explode after a period of time, wherever it is" needs no new field.
   All three funnel into ONE obelisk-owned termination path that despawns the hitbox, fires the
   end event/cue, and spawns any chained window. (Non-moving windows also end — usually `Fuse` —
   so a melee sweep can chain too; `HitWorld` simply never fires for them.)

2. **World collision is a host trigger, not obelisk knowledge.** Obelisk gains
   `HitboxWorldHit { hitbox: Entity, position: Vec3 }` (a `Message`/trigger the host fires).
   `arena_sim` replaces `ground_stop_projectile_hitboxes`'s silent despawn with firing it at the
   floor-plane crossing. A future host could raycast real geometry. Obelisk never asks "what is
   the world" — mirroring how `ObeliskSpatialPlugin` is already omitted in both hosts.

3. **Window schema: `on_end` reactions per reason, v1 reaction = `Chain(window_id)`.**
   ```ron
   ( id: "bolt", spawn_phase: Active, ..., motion: Ballistic(speed: 20.0, gravity: 9.8),
     hit_mode: FirstOnly,
     on_end: ( hit: Some(Chain("blast")), world: Some(Chain("blast")), fuse: Some(Chain("blast")) ) ),
   ( id: "blast", spawn_phase: Chained, active_duration: 0.05,
     shape: Sphere(radius: 1.5), motion: Static, hit_filter: Enemies, hit_mode: OncePerTarget ),
   ```
   - `on_end` is `#[serde(default)]` — every existing `.cast.ron` parses unchanged.
   - `WindowPhase` gains a `Chained` variant: the window never auto-spawns from the phase
     schedule; it spawns only when a parent's `on_end` names it, **at the end position**
     (aim/facing inherited from the parent for directional shapes; `spawn_offset` reused as a
     post-end delay if we want delayed detonation later — v1: immediate).
   - `EndReaction` is an enum with one variant (`Chain`) so later reactions (fork to multiple
     windows, apply effect in radius, spawn persistent zone) are additive.
   - Attribution: the chained `Hitbox` carries the ORIGINAL caster + the cast's charge
     (charge-scaled damage flows through unchanged).
   - Loader validation: `Chain` targets must exist and the chain graph must be acyclic
     (load-time error, like the existing `trigger_skill` referential check).

4. **End cues are position-anchored.** New locked cue slot per window: `on_end_{id}` →
   `"{skill}_end_{id}"`, fired at the END position, and `CueKind` gains `OnEnd`. `CueEvent` /
   `CueMessage` carry the `EndReason` (serde-defaulted for wire back-compat) so a lane can
   theoretically differ per reason later; v1 lanes don't branch on it. The existing `on_hit`
   cue stays (victim-anchored hit-flash is still useful) but the explosion moves to `on_end_bolt`.

5. **Cosmetic projectiles close the loop on the end cue.** The client/preview cosmetic bolt for
   window `w` terminates when `"{skill}_end_{w}"` arrives (snap to the cue position, then hand
   off to the end-cue lanes). The hardcoded 2 s cosmetic lifetime becomes the no-cue fallback.
   This kills visual/sim drift by construction.

6. **Determinism + goldens:** existing golden traces are untouched (no old content uses
   `on_end`); a NEW golden scenario covers each end reason + chaining. The termination funnel
   runs inside the existing `ObeliskSet` chain (end handling in `Advance`/`ResolveHits` order —
   exact placement decided at implementation against the set graph) so replays stay byte-stable.

## Changes by crate

**obelisk-bevy** (`src/assets/mod.rs`, `src/timeline/advance.rs`, `src/spatial/detect.rs`,
`src/events.rs`, `src/vfx.rs`):
- Schema: `OnEnd { hit, world, fuse: Option<EndReaction> }`, `EndReaction::Chain(String)`,
  `WindowPhase::Chained`, serde-defaulted; loader validation (existence + acyclicity).
- `HitboxEnded { caster, skill_id, window_id, position, reason, charge }` event; single
  `end_hitbox` funnel called from: detect (FirstOnly confirm), expire (fuse), and the
  `HitboxWorldHit` host trigger. It despawns, emits, and chain-spawns (reusing the window-spawn
  recipe with position override).
- `cue_on_end` observer mirroring `cue_on_hit`, keyed `on_end_{id}`.
- New golden scenario + unit tests per reason.

**arena_sim:** `ground_stop_projectile_hitboxes` → fire `HitboxWorldHit` (floor plane y=0)
instead of despawning; keep the system name honest (`report_ground_hits`).

**arena_game:** cue egress already generic (`capture_cue_event` forwards any cue) — verify
`OnEnd` + reason serialize through `CueMessage`; client cosmetics terminate the flying bolt on
its end cue (decision 5); net-test grows an assertion: both observers echo the
`firebolt_end_bolt` cue with the server's position.

**arena_editor:** window row gains the `on_end` → chain picker (window-id dropdown per reason,
v1 can collapse to one "on end → chain X" control since all three point at the same target);
`derive_vfx_cues` adds `on_end_{id}`; timeline strip draws chained windows sequenced after
their parent (strip span already extends to window closes; chained windows render as a segment
starting at the parent's close); trajectory gizmo draws the chained blast sphere at the
predicted landing point (`ballistic_launch_dir` + analytic flight already compute it); scrub
fires `on_end_{id}` at that predicted point; preview cosmetic bolt adopts decision 5.

**Content (firebolt v2):** `bolt` gains `on_end: all → Chain("blast")`; new `blast` window
(Chained, Sphere 1.5, 0.05 s, Enemies, OncePerTarget); `firebolt.skillfx.ron` moves the
Explosion lane from `firebolt_impact` to `firebolt_end_bolt` (keeping a small victim hit-flash
on `on_hit` if desired); rules TOML unchanged (damage rides the existing skill).

## Milestones

- **M-A obelisk core:** schema + funnel + events + cues + goldens. Gate: golden suite
  (old byte-identical, new scenario locked), unit tests per end reason.
- **M-B arena wiring:** ground trigger, wire verification, cosmetic termination. Gate: net-test
  with the end-cue assertion.
- **M-C editor authoring:** on_end UI, chained-window timeline rendering, gizmo blast preview,
  scrub end cues. Gate: editor suite + manual Play/scrub.
- **M-D firebolt v2 content** + balance pass (blast radius vs direct-hit damage — same damage
  for v1, splash tuning is a rules follow-up).

## Open questions

1. Does `blast` do the same damage as the direct hit (v1: yes — one skill damage roll applied
   by whichever window connects), or should chained windows have their own damage multiplier in
   the rules TOML? (Multiplier feels right eventually; punt to M-D?)
2. Should `on_end` reactions collapse in the UI to a single "on end → chain X" (all reasons
   same target) with per-reason overrides hidden behind an "advanced" toggle? (Schema keeps
   per-reason regardless.)
3. `HitWorld` for the preview/game currently means the floor plane only. Is wall/obstacle
   collision (spatial raycast against static colliders) worth doing in M-B, or wait for actual
   obstacle content?
