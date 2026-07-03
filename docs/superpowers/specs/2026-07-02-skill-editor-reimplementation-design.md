# Skill Editor Re-implementation — Obelisk-First, Editor-Native — Design Spec

**Status: APPROVED 2026-07-02** (sections reviewed with Luke; adversarially reviewed ×3 lenses
and revised. Supersedes `2026-07-02-skill-designer-ux-graph.md`; retires the arena_editor
skill-mode implementation).

**Goal:** Rebuild the skill editor so that (1) obelisk is the single source of truth for all
RPG semantics — a fireball's TOML *says* it triggers `fireball_explosion` on hit, and the
explosion is its own separately-designed skill; (2) the 3D behavior layer describes only
where/when contact happens; (3) the editor is a first-class `bevy_modal_editor` mode that
composes the editor's existing particle, effect-sequencer, and scene tools instead of
rebuilding them. Reference skills that MUST be authorable end-to-end: charged ballistic
fireball → explosion (on hit, on world impact, on fuse), chain lightning (hitscan + N hops
driven by rules `chain_count`), blizzard (ground-point-anchored storm raining shard volumes).

**Grounding:** full subsystem surveys taken 2026-07-02 of bevy_modal_editor main (6cd041d),
vothuul stat_core (vendored 54d0837), obelisk-bevy HEAD, and the current arena_editor;
file:line cites are from those.

---

## 1. Locked decisions

| # | Decision |
|---|---|
| D1 | **vothuul/obelisk master is the source of truth.** `~/src/obelisk` (LukeThayer/Obelisk) is a scratch fork; nothing builds against it. obelisk-bevy stops floating: commit `Cargo.lock` (gitignored today, `.gitignore:3`) or pin `rev =` for the obelisk crates. |
| D2 | **The Skill mode is a built-in `bevy_modal_editor` mode on main**, joining Material/Particle/Effect as a hard-coded `EditorMode` variant, feature-gated behind an `obelisk` cargo feature (obelisk-bevy is an SSH-keyed dep; the editor must build without it). The `[skill-designer]` fork branch (pin 4113492, the only home of `CustomModeRegistry`) is abandoned. |
| D3 | **All cross-skill causality lives in obelisk rules.** Hit-driven: existing `SkillCondition` triggers. Non-hit endings: **loot_core** `TriggerCondition` gains `OnImpact`/`OnExpire` under a new `ConditionPhase::Lifecycle` that resolve-time evaluation ignores — vocabulary without spatial semantics, the `can_chain`/`chain_count` precedent. |
| D4 | **A triggered skill with a registered timeline executes spatially** at the trigger payload as a free sub-cast (no mana, no cooldown, no cast state on the caster; original-caster attribution; charge inherited; `depth`-capped at 8, drop + warn at the cap). Triggered skills *without* timelines keep today's inline-damage-packet behavior on hit-phase conditions, so existing golden fixtures stay valid. Timeline-target conditions must author `additional = true` (replace semantics apply only to inline-packet triggers) — enforced by load validation and the editor. |
| D5 | **Chain causality and count are rules-driven; the radius is behavior.** `DamageConfig.can_chain`/`chain_count` (authored today, consumed by nothing — stat_core `skill.rs:383-390`) become real: the sim re-strikes the same skill at the nearest unvisited valid target within the behavior file's `chain_radius`. The authored `EndReaction::Retarget` schema dies. v1 scope: beam skills. |
| D6 | **`CastTimeline` v2** gains authored acquisition, window anchors + roles, emitters, charge authoring (`chargeable`, `max_hold`), and the cue/presentation map; it loses `OnEnd`/`EndReaction`/`WindowPhase::Chained`, `CastDelivery` (never read — `assets/mod.rs:150-155`), and `CastTargeting`-as-authored (subsumed by acquisition). End **events** and end **cues** survive — only end *reactions* die (replaced by Lifecycle triggers). |
| D7 | **Presentation = cue → editor-native Effect/anim bindings** stored in the behavior file. The editor's effect *runtime* is extracted into a reusable `bevy_effect` crate that **owns the `.fx.ron` format and loader** (mirroring `bevy_vfx` owning `VfxSystem`), so game clients load and render the same definitions. `.skillfx.ron` and the `arena_skills` lane model die. **Anim bindings are editor-preview-only in v1** (the game's networked rig has its own animation setup; mapping clip names onto it is future work, stated here so no one assumes parity). |
| D8 | **Skills are library content** (`SkillLibrary`, the `VfxLibrary`/`EffectLibrary` pattern) with one deliberate deviation: **explicit save gated by validation**. Rules TOML writes are format-preserving (`toml_edit` — the file is hand-shared game content; comments survive). `.cast.ron` re-serializes (editor-owned; comment loss accepted, stated here). Save stale-checks the on-disk file (hash vs. scan snapshot; prompt reload-or-overwrite on mismatch). No undo for library edits in v1 — identical to Particle/Effect modes. |
| D9 | **The deterministic preview substrate survives**, re-housed in the Skill mode: persistent stage, sim-backed synchronous scrub, charge slider, replay — with its invariants (vfx despawn grace ladder in render frames, sim-freeze gating, reseeded deterministic restarts). Triggered sub-casts run inside the preview sim like everything else. |
| D10 | **Cards + relationship chips supersede the node-graph canvas.** Scrub-first testing, registry-fed pickers, and task-first rules tiers carry forward from the prior interviews. |

## 2. The skill model (three layers, one id)

| Layer | File | Owner | Answers |
|---|---|---|---|
| Rules | `config/skills/<id>.toml` | vothuul stat_core `Skill` | costs, damage math, effect applications, **all cross-skill causality** (triggers incl. Lifecycle) |
| Behavior | `assets/skills/<id>.cast.ron` | obelisk-bevy `CastTimeline` v2 | phases, charge, acquisition, hit volumes (shape × motion × anchor × role × hit-policy), emitters, `chain_radius`, cue→presentation map |
| Presentation | (`cues:` map inside the behavior file) | edited in Skill mode; rendered by any host via `bevy_effect`/`bevy_vfx` | cue slot → Effect preset + attachment + charge→param bindings (+ editor-only anim clip) |

**Charge** is part of the model: the behavior file authors `chargeable: bool` and
`max_hold: f32`; the host maps hold time → the charge byte; the sim's existing
`charge_mult` (0.5–2.0× damage and projectile speed, `timeline/cast.rs:19-20`) is **ratified
as the v1 scaling rule** — a deliberate, documented exception to "damage math lives in
rules", revisited later as a rules-side charge-scaling entry. Charged-vs-uncharged stays
golden-covered.

Reference decompositions:

- **fireball**: TOML — fire damage on the bolt (nonzero — the acceptance content uses a
  damaging bolt; a zero-damage "pure carrier" bolt is also legal, see §3.2 on empty-packet
  hits), `[[conditions]]` ×3 → `trigger_skill = "fireball_explosion"`, `additional = true`,
  conditions `always` / `on_impact` / `on_expire`. Behavior — `chargeable`, `Aim`
  acquisition, one ballistic `FirstOnly` window. Cues — muzzle Effect on `cast`, trail
  Effect with `attach: Follow` on `window_open:bolt`, nothing on end (the explosion owns
  its visuals).
- **fireball_explosion**: TOML — its own (independently balanceable) fire damage + burn.
  Behavior — `SelfPoint` acquisition, one static sphere anchored `CastPoint`. Cues —
  explosion Effect on `window_open:blast`. Standalone: casting it explodes at the caster's
  feet. Triggered: the payload position is its cast point.
- **chain_lightning**: TOML — lightning damage, `can_chain = true`, `chain_count = 3`.
  Behavior — `HitscanEntity { range, filter }` acquisition, fallback `Fizzle` (paid fizzle),
  one beam window, `chain_radius: 6.0`. Cues — two-anchor beam Effect on `window_open:arc`
  (fires per strike, incl. each hop), impact burst on `hit`.
- **blizzard**: TOML — cold damage + chill application (authored inline on blizzard; the
  shard is a window, not a skill). Behavior — `GroundPoint { range }` acquisition, fallback
  `SelfPoint`; a zone window anchored `CastPoint + (0, 8, 0)` with `strikes: false` (a
  carrier — the storm body itself damages nothing) carrying
  `emitter: { rate, jitter, window: "shard" }`; a `shard` **template** window (small
  sphere, straight-down motion override, `FirstOnly`). Cues — storm Effect on
  `window_open:storm`, shard visual on `emit:shard`.

## 3. Changes by repo

### 3.1 vothuul/obelisk (small PR, vocabulary only)

`TriggerCondition::{OnImpact, OnExpire}` in **loot_core** `types.rs` (~1270-1445) +
`ConditionPhase::Lifecycle`, including the in-repo exhaustive-match sites (`phase()`,
`Display`, `obelisk_editor`'s condition dropdowns). A doc note on `can_chain`/`chain_count`
naming the embedding layer as their consumer. Developed against a **local `[patch]` of the
obelisk git deps** until merged — this is the only viable fallback: `TriggerCondition` is a
closed internally-tagged serde enum, so obelisk-bevy-side variants cannot exist and an
unpatched stat_core hard-fails on `type = "on_impact"` TOMLs.

### 3.2 obelisk-bevy (the reform)

- **Pin** (D1): commit `Cargo.lock` or `rev =` pins.
- **Payload plumbing**: `Hitbox`, `HitConfirmed`, and `HitboxEnded` gain `position` (where
  missing) **and `depth: u8`** — the trigger-generation counter must ride spawned hitboxes
  across ticks (a bolt flies, hits later, the explosion ends later still); the existing
  `MAX_TRIGGER_RESOLUTIONS` bounds only a single hit's inline worklist. Both executor entry
  points read and increment it; at 8, drop + warn (golden-covered: a self-triggering skill
  terminates deterministically).
- **Widen the resolve seam**: `resolve_one_hit_charged` returns the primary `DamagePacket`
  and per-packet `CombatResult`s and exposes pre-hit StatBlock snapshots, so obelisk-side
  condition evaluation matches stat_core's phase semantics (pre-calc evaluates pre-mutation
  state; `DamageOverThreshold` compares pre-mitigation totals). `EveryNthHit` on
  timeline-target conditions is forbidden by validation in v1 (its counter lives inside
  stat_core's calc path).
- **Triggered-timeline executor**: in `on_hit_confirmed`, partition the skill's conditions
  by "trigger target has a registered timeline". With one: strip the condition from the
  `Skill` clone handed to stat_core (no inline packet — the documented double-damage
  hazard), evaluate obelisk-side via `TriggerConditionEval` against the widened outputs,
  and on match execute the target skill's timeline at the payload. Without one: today's
  path, unchanged. Execution = spawn the skill's scheduled windows on a virtual clock from
  the trigger instant (spawn offsets honored), anchors resolved with cast-point = payload
  position, `depth + 1`. **Free means free**: hits from `depth > 0` hitboxes resolve with
  `mana_cost` zeroed on the same `Skill` clone (a sub-cast never bills or fizzles on the
  caster's mana; standalone casts of the same skill stay paid). Needs a pub spawn entry
  (today `spawn_window_hitbox` is pub(crate), `advance.rs:307`).
- **Zero-damage carriers work**: `HitConfirmed` fires from spatial detection regardless of
  packet contents, and condition evaluation does not depend on a nonzero packet — a
  no-damage bolt still triggers its explosion (golden-covered).
- **Lifecycle evaluation**: `end_hitboxes` maps `EndReason::HitWorld → OnImpact`,
  `Fuse → OnExpire`, evaluates the ending skill's Lifecycle conditions, and routes matches
  through the same executor at the end position. A Lifecycle condition whose target has no
  timeline is a **validation error** (there is no defender to packet against at a world
  impact); hit-phase conditions without timelines remain legal (packet path). `HitboxEnded`
  and the end cue slots survive — only authored end *reactions* die.
- **Schema v2** (`assets/mod.rs`):
  - Windows: `spawn: Scheduled { phase, offset } | Template` (a `Template` window never
    self-schedules; it exists to be instantiated by an emitter — the `Chained` replacement
    in spirit, minus causality); `anchor: Caster | CastPoint` (+ local offset);
    `strikes: bool` (default true; `false` = carrier volume, skipped by detection);
    optional `emitter: { rate, jitter, window }` (emitted instances anchor at the jittered
    emission point); optional motion direction override (`Down` for shards).
  - Acquisition: `Aim | HitscanEntity { range, filter } | GroundPoint { range } |
    SelfPoint`; only the fallible variants (`HitscanEntity`, `GroundPoint`) carry
    `fallback: Fizzle | <acquisition>`. The acquired point survives end-to-end (today
    `CastAim::Point` is collapsed to a direction, `advance.rs:118` — the blizzard blocker).
  - `chargeable`, `max_hold`, `chain_radius`.
  - **Cue table** (slots, payloads, attachment):

    | Slot | Fires | Payload | Binding options |
    |---|---|---|---|
    | `cast` | cast begins | caster pos, charge | Effect at caster; anim (editor-only) |
    | `window_open:<id>` | window/instance spawns (incl. each chain strike) | pos, dir, charge; beams: `from`+`to`; projectiles: speed/gravity | Effect at pos; `attach: World \| Follow` (Follow = host flies a proxy along the cue's motion data; the end cue snaps + terminates it — today's pattern, kept) |
    | `hit` | each hit confirm | victim pos, charge | Effect at victim |
    | `end:<id>` | hitbox ends | end pos, `EndReason`, charge | Effect at end pos |
    | `emit:<id>` | emitter instantiates a template | spawn pos, charge | Effect at spawn pos |

  - `validate_timeline` v2: emitter targets exist and are `Template`; `Template` windows
    are referenced by an emitter; acquisition fallback chains terminate; presentation
    strings are opaque to the sim (validated editor-side).
- **Chain-from-rules** (D5): current retarget machinery re-keyed to `can_chain`/
  `chain_count` + `chain_radius`; hop counter + visited set ride the payload.
- **Acquisition contract**: authored spec sim-side; *resolution* host-provided (the arena
  raycasts and passes a resolved `CastAim`; the sim validates against the authored
  acquisition and applies fallbacks). Phase 2 defines the obelisk-bevy-side entry point
  only; arena's helpers move in phase 4.
- **Determinism**: emitter jitter draws from a dedicated `SpawnRng` stream (seeded beside
  `CombatRng`, never interleaved — golden safety). New goldens: triggered execution ×3
  causes, depth-cap termination, zero-damage carrier, chain-from-rules, emitters,
  acquisition fallbacks, charged-vs-uncharged. Existing goldens change only where
  `Chain`-using example content is deliberately migrated (reviewed diffs).
- **Hygiene**: fix 0.17/0.4 doc drift (`lib.rs:1-3`, CLAUDE.md); dead `registry` param in
  `advance_casts` (`advance.rs:289`).

### 3.3 bevy_modal_editor main

- **Extract `crates/bevy_effect`** — scoped honestly, it is not a clean file move:
  - Moves: `EffectMarker`/`EffectStep`/`EffectTrigger`/`EffectAction`/`EffectPlayback` +
    the runtime systems, **plus their editor-scene dependencies**: `PrimitiveShape`
    (`effects/data.rs:13`) and `GltfSource` + its materializing systems (`effects/
    mod.rs:469`) move too (or gain a host spawn-provider seam — decided at implementation).
  - **Reflect type paths are preserved** (`#[type_path]`) — `EffectMarker` is
    scene-serialized (`scene/mod.rs:86`); moving it must not break every saved scene.
  - **`bevy_effect` owns the `.fx.ron` format and an asset loader / library-init helper**
    used by editor and game alike; the editor keeps its auto-save and UI.
  - `SpawnEffect` recursion gains a depth guard (cap 8, drop + warn) — a deliberate
    behavior change *excluded* from the otherwise characterization-identical contract,
    unit-tested with a self-spawning preset.
- **First consumers for two dead seams**: cue bindings bake `VfxParam` (declared
  `data.rs:853-866`, unread today) for charge→scale/intensity; preview cast-anim playback
  becomes `AnimationLibrary`'s first consumer.
- **`EditorMode::Skill`** (key `K`, right panel) behind `feature = "obelisk"`; CI builds
  both with and without. The ~6 hard-coded mode sites (enum + `panel_side`, hotkey block,
  status bar, hints, UiPlugin registration, EditorPlugin composition).
- **`SkillLibrary`**: skill id → three-layer bundle. A **content root** is a directory
  containing `config/skills/` + `assets/skills/` subtrees; hosts register roots via
  `register_obelisk_content(root)`; the first root is the default write target for new
  skills. Palette: browse + "New Skill" archetype templates (strike/projectile/zone/beam —
  playable defaults; templates ship with starter Effect presets registered from the same
  roots so a fresh install isn't born dangling). Lifecycle ops: duplicate, rename, delete —
  each runs a back-reference check first ("3 skills trigger this — really delete?").
- **The panel** (Effect-mode card idiom, exclusive-world system, `panel_frame`, pin
  support): *Rules* (task-first tiers: costs/damage/crit/effect applications + live
  computed readouts; trigger cards read "WHEN on_hit → CAST fireball_explosion", skills
  palette-picked; Advanced drawer; `EnumVariants`-driven dropdowns), *Behavior* (phases +
  charge, acquisition card with plain-language labels, window cards with emitter
  sub-cards), *Presentation* (per-cue rows in timeline order: Effect picker, attachment,
  charge-param bindings, anim picker marked editor-only; jump-to-Effect-mode round-trip via
  pinned panels).
- **Relationship strip**: causality chips (`fireball → fireball_explosion`, `↺ ×3`);
  click = switch the library to that skill.
- **Stage proxies**: selecting a window card materializes an ephemeral gizmo-editable proxy
  (radius/offset/angle drag writes back). Never `SceneEntity`, never serialized.
- **Preview port** (D9): persistent stage, synchronous scrub, charge slider, ⟳ replay,
  event markers (including trigger-firing markers). **Strip extent = the base timeline plus
  a trailing sub-cast region** (dynamic end = last sim entity settled, capped); scrubbing
  fireball past impact shows the explosion because the sub-cast executes inside the same
  frozen-seek sim. **Preview acquisition resolution is stage-provided** (scripted aim at
  the dummy line / ground point at the stage marker), so the Skill mode works in bare
  bevy_modal_editor with no game host. **Dummy auto-sync rules**: target count =
  `chain_count + 1` placed within `chain_radius` for chain skills; `GroundPoint` skills get
  dummies under the default aim point; manual add/move stays as the escape hatch.
- **Validation**: `ValidationRegistry` rules — dangling `trigger_skill`; Lifecycle-target
  missing a timeline (blocking); hit-phase-target missing a timeline (warning only — the
  packet path is legal, D4); timeline-target conditions with `additional = false`;
  unknown Effect/anim preset names; acquisition fallback dead ends; `EveryNthHit` on
  timeline targets. Validation re-runs on library scan and on Effect/Vfx library mutation
  (deleting a preset in Effect mode surfaces dangling skills immediately). **Runtime
  contract for dangling references** (preview and game): skip the cue, warn once per
  (skill, cue), never panic.

### 3.4 obelisk-arena

- `arena_editor` → composition shell: `EditorPlugin` (+`obelisk`) + `GamePlugin` + content
  root registration + arena host bits (flat-floor `HitboxWorldHit` reporter, camera-ray
  acquisition provider — moved here from `arena_game`'s cast pipeline in this phase). The
  skill-designer modules (~15 files) are deleted; preview-substrate tests port upstream
  with the substrate.
- **Cue wire contract** (replaces `arena_skills::CueMessage`/`.skillfx` lookup): the server
  relays obelisk `CueEvent`s as `slot id + payload` (positions/`from`+`to`/charge/reason) —
  it stays generic, so triggered sub-cast, chain-hop, and emitter cues replicate with zero
  special cases. **Clients load `.cast.ron` via the shared obelisk-bevy loader** and
  resolve slot → Effect preset locally through `bevy_effect`; the existing local-cast
  prediction/de-dup story (predicted `cast` cue for the local caster) carries over keyed on
  (caster, skill, slot). `arena_skills`' lane model and `.skillfx.ron` die.
- Content migrates: `firebolt` → `fireball` + `fireball_explosion`, `chain_lightning`
  re-keyed to rules `chain_count`, `blizzard` authored as acceptance content.

## 4. Testing

- **obelisk-bevy**: golden traces per §3.2 (new: triggered ×3, depth cap, zero-damage
  carrier, chain, emitters, fallbacks, charge); existing traces byte-identical except
  deliberate content migrations. Unit tests per feature (`end_events.rs` precedent).
- **bevy_effect**: characterization tests before the move; identical after (cycle guard
  excluded, separately tested).
- **Skill mode**: headless registration/boot tests + unit-tested pure helpers; **save/load
  round-trip tests** (TOML format preservation, `.cast.ron` fidelity, stale-check paths);
  scripted screenshot probes (nothing ships unseen).
- **Preview**: the ported determinism suite + a sub-cast test: seek past bolt impact ⇒
  explosion window spawned + second `DamageResolved` with the explosion's skill id.
- **Acceptance**: all three reference skills authored in the Skill mode, scrubbed, then
  cast in the arena over the network. Net-test asserts: fireball — client renders the
  explosion cue at the server's impact position, damage shape = bolt + explosion as
  separate `DamageResolved` with separate skill ids; chain lightning — N hop cues with
  correct from/to anchor pairs on an observer; blizzard — shard cue positions match the
  server-authoritative shard spawns.

## 5. Sub-projects and order

1. **obelisk PR** — Lifecycle conditions (§3.1). Local `[patch]` unblocks everything.
2. **obelisk-bevy reform** — §3.2 as one series (pin → payload plumbing → resolve-seam
   widening → executor → lifecycle → schema v2 + in-repo content migration →
   chain-from-rules → emitters/acquisition → goldens). **Arena pins its obelisk-bevy rev at
   the pre-reform commit for the duration** (its content still uses v1 schema until
   phase 4).
3. **bevy_modal_editor** — `bevy_effect` extraction (parallel to 2), then the Skill mode +
   preview port (needs 2's schema).
4. **Arena** — thinning + acquisition-helper move, **net protocol migration (cue wire
   contract)**, client `bevy_effect` rendering, reference content, extended net-test; flip
   arena's editor dep from the fork pin to main.

Each sub-project gets its own implementation plan.

## 6. What dies

The `[skill-designer]` bevy_modal_editor fork branch and `CustomModeRegistry`;
`EndReaction::{Chain,Retarget}` + `OnEnd` + `WindowPhase::Chained` + `CastDelivery` +
`CastTargeting`-as-authored; `.skillfx.ron` + the `arena_skills` lane model +
`CueWireMessage`'s lane payload; the arena_editor skill-designer modules; the
node-graph-canvas UX direction.

## 7. Risks

- **vothuul PR timing** — mitigated by the local `[patch]` (the only viable fallback;
  closed serde enums rule out sim-side variants).
- **bevy_modal_editor main drift** — land the mode as one focused series; the `bevy_effect`
  extraction (widest touch: scene deps, reflect type paths, saved-scene compat) goes first
  while the tree is calm.
- **Feature-gate hygiene** — CI builds the editor with and without `obelisk`.
- **Golden churn** — every trace change is a reviewed, explained diff tied to a content
  migration.
- **Resolve-seam widening** (§3.2) is the highest-risk sim change — it touches the exact
  path all 39 golden scenarios exercise; it lands first in the phase-2 series with goldens
  proving byte-identical behavior for non-timeline content.
- **Preview port regressions** — the substrate's invariants are documented in the port and
  its determinism tests come along.
