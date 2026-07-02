# The Anatomy of a Skill — Parts, Edges, Triggers — Concept Spec

**Goal:** Define what a skill IS — its parts, the edges (events) that connect them, and the
triggers that hook the rules — precisely enough that the editor can author any skill in the
target design space and the sim can execute it deterministically. Reference examples that MUST
be expressible: (1) charged firebolt that lobs and explodes on collision or after a 10 s fuse,
(2) chain lightning that hitscans a looked-at target and hops N times between valid targets,
(3) blizzard placed above a hitscanned point (or the caster) that rains damaging ice shards.

**Relationship to other specs:** [`2026-07-02-event-driven-skill-phases.md`](2026-07-02-event-driven-skill-phases.md)
is increment 1 of this model (end events + chaining). This document is the conceptual frame it
slots into. Grounding: full surveys of `stat_core` @54d0837 (rules vocabulary) and obelisk-bevy
(execution surface) were taken 2026-07-02; file:line cites below are from those.

---

## 1. A skill is three authored layers over one deterministic runtime

| Layer | File | Owns | Answers |
|---|---|---|---|
| **Rules** | `config/skills/<id>.toml` (stat_core `Skill`) | identity/tags, mana cost, cooldown, damage math (crit/conversions/effectiveness), effect applications, use-conditions, skill/effect **triggers** | *What happens on contact, and what it costs* |
| **Behavior graph** | `assets/skills/<id>.cast.ron` (obelisk `CastTimeline`, evolving) | the physical execution: **nodes** that occupy space-time and **edges** (events) that connect them | *Where and when contact happens* |
| **Presentation** | `assets/skills/<id>.skillfx.ron` (+ `.vfx.ron` presets) | cosmetic lanes bound to graph events: anim, particles, projectile visuals | *What each moment looks like* |

The runtime spine executing the graph is obelisk's fixed-tick chain
`Validate → Advance → Projectiles → ResolveHits → TickEffects` (obelisk-bevy `lib.rs:93-141`).
Everything below must stay expressible inside that chain, deterministic under the fixed
timestep and the seeded `CombatRng` (no wall-clock, no unseeded randomness).

## 2. Parts (nodes) — things that exist in space over time

Every node has: a **lifetime** (when it exists), an **anchor** (where it is), and **behavior**
(what it does while alive). The payload that flows into a node at spawn:
`{ position, direction, target?: Entity, charge, hop: u8, rng }` — this is the edge payload
(§3) and it is the ONLY input a node gets, which is what keeps the graph composable.

- **Cast** — the root node; the caster's own body performing the skill.
  Properties: charge-hold (host accumulates, obelisk receives a `u8` → 0.5–2.0× speed+damage,
  `timeline/cast.rs:19`), phase timeline `windup/active/recovery` (attack-speed-scaled),
  interruption (`interrupt_cast`), plus the rules gates at validate: mana, cooldown, range,
  LOS, `use_conditions`. A "must be fully charged to release" rule (firebolt) is a small host
  gate on the charge byte — worth a `min_charge` field on the arena side eventually.

- **Acquisition** — resolving the player's raw aim into the payload, at release time.
  Today this is entirely host code producing a `CastAim` (`Entity(e) / Point(p) / Direction(d)`,
  `timeline/cast.rs:5-12`) — the arena does a camera ray for entity casts; the editor preview
  solves a ballistic loft. The model needs acquisition to become **authored data** so the
  designer (and future AI casters) can express it: `Hitscan { range, filter } → Entity`,
  `GroundPoint { range } → Point` (ray to world), `Aim → Direction`, `Slf → caster` — each with
  an authored **fallback** (`Fizzle` = reject the cast, or a secondary acquisition: blizzard's
  "above the hit point, else above the caster"). Chain lightning's "is the looked-at target
  valid?" is exactly `Hitscan` with `Fizzle` fallback.

- **Volume** — a hit region: `shape × motion × hit-policy × lifetime` (today's
  `CollisionWindow`, `assets/mod.rs:24-38`). This single node kind already covers three
  archetypes by parameter choice:
  - *Strike*: Static motion, short duration, caster-anchored (melee sweep, nova).
  - *Projectile*: Linear/Ballistic motion, `FirstOnly` (bolt, arrow).
  - *Zone/field*: Static or point-anchored, long duration, `EveryTick`/`OncePerTarget` +
    `rehit_interval` (burning ground, blizzard's storm body if it damages directly).
  Missing today: a volume can only spawn **at the caster** (`advance.rs:265`) — spawn-at-
  payload-position is what chaining (increment 1) introduces and everything else reuses.

- **Beam** — an instant two-endpoint link (caster↔target or target↔target): hitscan damage
  plus cosmetics that need BOTH anchors. Chain lightning's hop is a beam. Execution-wise a
  beam is nearly a degenerate volume (instant, guaranteed single target — the acquisition
  already picked it), so it can be modeled as `shape: Beam` or as a `Volume` with
  `motion: Instantaneous { to: payload.target }`; the real novelty is **presentation**
  (two-anchor cue) and **acquisition-driven hitting** (no overlap test — the target IS the
  payload). Decision for the implementation spec.

- **Emitter** — a clock attached to a node that spawns child nodes: blizzard's storm spawning
  shard volumes at `rate` with positional `jitter` (drawn from `CombatRng` — deterministic,
  server-authoritative, replicated to clients as spawn events/cues). An emitter is not a new
  entity kind — it is an **edge kind** (`OnTick`, §3) hanging off a zone volume.

## 3. Edges (events) — how one part causes the next

Every edge fires with the payload `{ position, direction, target?, charge, hop, rng }`; the
spawned node inherits it (charge and caster attribution flow through — increment 1 locks
this). Edge kinds:

| Edge | Fires when | Exists today? |
|---|---|---|
| `At(phase, offset)` | scheduled time in the cast reached | ✅ `spawn_phase`/`spawn_offset` (`advance.rs:235`) |
| `OnEnd(reason)` | a volume terminates: `HitEntity` / `HitWorld` / `Fuse` | ✅ shipped 2026-07-02 (`HitboxEnded`, `EndReaction::Chain`) |
| `OnHit` | EACH hit confirm of a volume/beam (≠ end: an `EveryTick` zone hits many times) | ⚠️ exists as `HitConfirmed` event + rules triggers; not yet an authored graph edge |
| `OnTick(rate, jitter)` | emitter clock fires while its node is alive | ❌ new |
| `OnAcquire` / `OnAcquireFail` | acquisition resolved / found nothing | ❌ new (host-coded today) |

Plus one **edge modifier** that makes chain lightning:

- **`Retarget { filter, radius, exclude_visited, max_hops }`** — before spawning the next
  node, replace `payload.target/position` by searching a sphere around the current payload
  position for the nearest valid entity not in the visited set; increment `hop`; stop the
  edge silently when nothing is found or `hop == max_hops`. The visited set and hop counter
  live on the payload. Note `stat_core` already declares `can_chain`/`chain_count` and
  `pierce_chance`/`pierce_count` on `DamageConfig` (`skill.rs:383-390`) with **no geometry
  behind them** — the graph's `Retarget` edge is the geometry that makes those rules real
  (implementation should reconcile: graph authors the search, rules author the count/damage
  falloff, or the graph owns both and those fields deprecate — decide in the increment spec).

**Cycles are allowed exactly one way:** a node may chain to itself or an ancestor ONLY through
an edge that consumes a bounded counter (`hop`/`max_hops`, or a chain-depth cap like the
existing `MAX_TRIGGER_RESOLUTIONS = 8`, `combat/system.rs:14`). Everything else must be a DAG
(load-time validation, like `trigger_skill` referential checks).

## 4. Triggers — where the rules layer hooks in

The graph decides *where/when* a hit happens; the **rules** decide what the hit does — and
rules can themselves spawn more behavior:

- `TriggerCondition` (~35 variants across 4 pipeline phases — PreCalculation state checks,
  PostCalculation packet checks like `OnCrit`, PostResolution like `OnKill`, defender-side
  like `OnDamageTaken`) gate `SkillCondition { trigger_skill, additional }` — a hit by skill A
  fires skill B's damage against the same target (`triggers.rs`, wired in
  `combat/system.rs:84-174`).
- Effect lifecycle triggers (`OnMaxStacks/OnExpire/OnConsume/OnApply`) detonate stored damage
  or fire skills when effects pop.
- `TriggerFired` is deliberately host-observable for "bespoke routing (on-kill, splash,
  target reselection)" — **the graph model is that routing**: the natural convergence is that
  a triggered skill invokes its OWN behavior graph with the triggering hit as payload (skill B
  explodes where A's hit landed). That unification is future work; increment 1 keeps triggers
  and graph edges separate.

## 5. The three examples, decomposed

**Firebolt** — `Cast(charge, release gated on full) → At(Active,0): Volume bolt
{Sphere 0.5, Ballistic(20, 9.8), FirstOnly, fuse=10s} → OnEnd(any): Volume blast
{Sphere 1.5, Static, Enemies, 0.05s} at payload.position`. Cues: cast (muzzle), bolt flight
(trail), `on_end_bolt` (explosion at position). *Needs: increment 1 only (fuse = the existing
`active_duration`).*

**Chain lightning** — `Cast(charge) → Acquire Hitscan{range, Enemies, fallback Fizzle} →
Beam(caster↔T₀) → OnHit + Retarget{Enemies, radius r, exclude_visited, max_hops n} →
Beam(T₀↔T₁) → … (self-edge, bounded by hop)`. Cues: per-hop two-anchor lightning arc, per-hit
impact. *Needs: authored acquisition, Beam node/presentation, Retarget edge, hop payload.*

**Blizzard** — `Cast(charge) → Acquire GroundPoint{range, fallback: above caster} →
Volume storm {Zone above point + height h, duration D, no direct hits} → OnTick(rate,
jitter r): Volume shard {small, Ballistic(down), FirstOnly} → OnEnd(HitWorld|HitEntity):
impact cue (+ optionally chain a tiny frost patch)`. Cues: placement decal at acquire, storm
loop anchored to the zone, shard trails, shard impacts. *Needs: acquisition with fallback,
point-anchored zone spawn, OnTick emitter edge, deterministic jitter from `CombatRng`.*

## 6. Gap map (existing → increment 1 → next)

| Capability | Status |
|---|---|
| Phases, scheduled windows, shapes, filters, hit modes, rehit | ✅ shipped |
| Charge (byte → speed+damage), Point/Entity/Direction aims, LOS/range validate | ✅ shipped |
| Ballistic motion + gravity, ballistic aim solver, ground plane hook | ✅ landed 2026-07-02 (unpushed) |
| Rules triggers (skill/effect), chain/pierce *fields*, effects/DoT | ✅ shipped (`stat_core`) |
| `OnEnd` events + chain-at-position + end cues + cosmetic termination | ✅ shipped 2026-07-02 (6afa6ba; firebolt v2 is the proving case) |
| Authored acquisition (+fallback/fizzle) | ❌ increment 2 candidate |
| Beam node + two-anchor cues | ❌ increment 2 candidate (chain lightning) |
| `Retarget` edge (visited set, hops) — geometry for `can_chain` | ❌ increment 2 candidate |
| `OnTick` emitter edge + point-anchored zones + RNG jitter | ❌ increment 3 candidate (blizzard) |
| Trigger↔graph unification (triggered skill runs its graph at payload) | 🔮 future |

## 7. Schema + editor direction (non-binding)

- The `.cast.ron` stays a **flat list of nodes referenced by id** (windows today) with edges
  as fields on the source node (`on_end`, `on_tick`, …) — RON-friendly, diff-friendly,
  validated referentially + for boundedness at load. No free-form node-graph soup.
- The editor renders the graph as **sequence lanes**: the cast phase strip (exists) with each
  node a bar starting where its inbound edge fires; chained/spawned nodes indent under their
  parent. The trajectory gizmo generalizes to per-node **anchor preview** (arc landing point,
  beam endpoints from staged targets, storm placement disc), and the scrubber fires edge cues
  at their predicted moments/positions (both already work this way for increment 0).
- Presentation lanes bind to **edges** (cues), and every cue is position-carrying (increment
  1); beams add a second anchor to the cue wire payload.
