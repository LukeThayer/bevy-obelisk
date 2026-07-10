# Beams, Retarget Hops, Hitscan Acquisition — Increment 2 — Design Record

> **⚠️ SUPERSEDED (schema v1) — kept for design rationale only.** The authoring surface in this doc —
> `EndReaction::Retarget { window, radius, max_hops }`, `WindowPhase::Chained`, `targeting: SingleEntity`
> — was DELETED in the **schema-v2** rework (~2026-07-09). Under v2's `#[serde(deny_unknown_fields)]` that
> RON no longer parses. The *behavior* survives, but the live authoring is rules `can_chain`/`chain_count`
> + timeline `chain_radius` + a `Beam` window + `acquisition: HitscanEntity{range,filter,fallback}` (see
> `chain_lightning`). **Author from:** `src/assets/mod.rs` (the schema),
> `2026-07-02-skill-editor-reimplementation-design.md` (APPROVED), and
> `obelisk-arena/.claude/skills/arena-skill-design`. Corrected palette + editor gaps:
> `obelisk-arena/docs/superpowers/reviews/2026-07-09-obelisk-skill-system-and-editor-review.md`.

**Status: SHIPPED 2026-07-02** (built directly per user decision; this doc records the design
as implemented). Proving case: `chain_lightning` — charged release, server hitscan of the
looked-at target, lightning arcs caster→T₀, then hops to the nearest un-struck enemy N times.

**Decisions locked with the user:** 3 hops at FULL damage (falloff later as a rules knob);
charge scales damage only (the byte already inherits through the chain); a miss COSTS mana +
cooldown and no-ops (the "paid fizzle").

## The model (extends the skill-anatomy spec)

1. **Beam = `VolumeMotion::Beam`**: an instantaneous link to the hitbox's DESIGNATED target —
   no overlap test, the target IS the payload (`Hitbox.beam_target`). `resolve_beam_hits`
   (ResolveHits, server-only, beside `detect_overlaps` which skips beams) moves the hitbox
   onto the victim, applies the faction filter, registers the hit → `FirstOnly` ends it
   `HitEntity` AT the victim. A beam with no designated target strikes nothing and fuses out —
   this IS the paid fizzle, for free.

2. **`EndReaction::Retarget { window, radius, max_hops }`**: on end, search a sphere around
   the end position for the NEAREST combatant passing the target window's hit filter that the
   chain has not already struck; spawn `window` (Chained, normally Beam) targeted onto it.
   Chain state rides the hitbox: `hop: u8` (bound) + `visited: Vec<Entity>` (∪ own `hit_log`
   at search time — a chain never revisits). Deterministic pick: min (distance², entity
   index). UNLIKE `Chain`, Retarget may name its own window (self-hop) — validation allows
   retarget cycles because the hop counter bounds them; `max_hops >= 1`, `radius > 0`.

3. **Two-anchor cues**: `CueEvent`/`CueMessage` gain `position_from: Option<Vec3>`
   (serde-defaulted). A beam window's OPEN cue carries origin→victim so a lightning-arc lane
   renders between them. `arena_skills` gains the `beam:` lane spec `{ effect, color,
   segments, lifetime }` — v1 renders `segments` vfx bursts sampled along the segment (game
   client + editor preview identically); a dedicated stretched-beam renderer can replace the
   rendering later without touching the authoring format.

4. **Hitscan acquisition is the HOST's** (like world collision): the arena server, when a
   skill's timeline declares `CastTargeting::SingleEntity`, raycasts along the client's aim
   from the eye (range = targeting range, excluding the caster + its own hurtboxes); a hit
   acquires `CastAim::Entity(target)`, a miss falls back to the direction cast → the beam
   fizzles after costs. `SingleEntity` targeting now MEANS "acquires an entity" — free-aim
   skills must declare `Direction` (firebolt corrected accordingly).

5. **LOS self-block fix** (obelisk): the entity-aim LOS ray now excludes the caster's own
   hurtbox entities (child sensors self-blocked every entity cast on compound bodies), and a
   ray meeting the target's BODY collider counts as seeing the target (compound hosts can
   return either collider first). This unblocks entity-aimed casts for arena combatants —
   including in the editor preview, which now casts entity-aimed for beam skills.

## Editor

Motion picker gains `Beam`; the window row's `end→` combo gains `hop <window>` entries
(including self) with radius/max_hops drag-fields; the preview spawns a SECOND dummy for
retargeting skills and casts entity-aimed; the gizmo draws the beam line + hop-radius circle;
the scrubber stages beam window cues with both anchors.

## Verification

obelisk-bevy: `tests/beam_retarget.rs` (chain order + visited exclusion + hop bound, paid
fizzle, charge ×2 across the chain vs same seed, determinism, validation rules; the caster in
these tests HAS its own hurtbox — the LOS regression is pinned). Goldens byte-identical.
Arena: editor preview test `play_chain_lightning_hops_to_the_second_dummy` (real
chain_lightning asset end-to-end); all suites green; net-test PASS (firebolt unchanged over
the wire; the zero-damage failure seen once during verification was the harness's known
wall-clock flake — a clean re-run passed with the expected 7 casts → 8 damage shape).

## Deferred

- Damage falloff per hop (rules knob), charged bonus hops, `stat_core` `can_chain`/
  `chain_count` reconciliation (those fields remain rules-side declarations with the graph
  owning the geometry).
- A real stretched-beam/lightning renderer for `beam:` lanes.
- Client-side prediction of beam arcs (currently server cues only — the short windup covers
  most of the latency).
