# Trigger Reform — Merge Record + Follow-up Tickets

**Merged 2026-07-03**: `trigger-reform` → main @ `d72fe9f` (20 commits, 13 SDD tasks +
whole-branch review + fix wave). vothuul/obelisk master @ `bf9f026` (lifecycle vocabulary).
obelisk-arena repinned to pre-reform `68618a8` (patches off) until phase 4 migrates it.
Plan: `2026-07-02-obelisk-bevy-trigger-reform.md`; spec: `2026-07-02-skill-editor-reimplementation-design.md`.

## Tickets (from the final whole-branch review; severity as triaged)

1. **CueEvent payload gap** — charge missing on all cue slots, `EndReason` missing on end
   cues; `ParamSource::Charge` bindings cannot be driven from a `CueEvent` as shipped.
   Additive fields on the source events already exist (`HitConfirmed.charge`,
   `HitboxEnded.charge/reason`; `CastBegan` needs `charge`). **FIRST TASK of the phase-3
   plan** — the editor's presentation preview hits this on day one.
2. **GlobalConditional naming a timeline skill** resolves inline unstripped
   (`collect_matching_globals_owned` path) — same partition treatment as skill conditions
   when gear/passive-driven procs become content (later phase).
3. **Template windows' `anchor`/`anchor_offset` silently inert under emission** — add a
   validation error or doc note (editor-visible trap; phase-3 ValidationRegistry).
4. **`nearest_retarget_candidate` has no liveness check** — chains can hop to corpses
   (one `Attributes` aliveness filter; phase 4, gameplay-visible with real arena content).
5. **`additional = false` warn is per-hit, unthrottled** — an EveryTick window with the
   content bug spams; spec's warn-once-per-(skill,cue) contract suggests the shape.
6. **`TriggeredExec.spawned` only grows** — a hot-reloaded timeline that shrinks its window
   list leaves the exec ticking forever (debug_assert or min-clamp when next touched).
7. **Facade `Vec3::ZERO` fallback** for transform-less targets places triggered executions
   at the world origin — skip-or-warn is safer (`src/facade/combat.rs`).
8. **Facade doesn't surface inline `triggered_skill_hits`** as separate `DamageResolved`
   events the way the observer path does (pre-existing, documented divergence).
9. Test-var rename nit: `explosion_depth_zero_free` asserts the depth>0 path.

## Deviations from spec recorded during implementation

- `additional = true` and Lifecycle-target-missing-timeline checks are RUNTIME warn/error
  (not load-time as D4 stated): `CastTimelineHandles` populates after skill load — the
  spec's load-time home cannot see timelines. Phase 3's ValidationRegistry restores
  authoring-time enforcement.
- Executor virtual clock uses UNSCALED authored durations (triggered timelines don't
  inherit caster cast-speed); spec was silent; documented at the spawn site.
- Cue slot names: the sim's runtime vocabulary (`on_cast`/`on_window_{id}`/`on_hit`/
  `on_end_{id}`/`emit_{id}`) is canonical; the spec table's shorthand maps 1:1
  (documented normatively on `CastTimeline::cues`).
