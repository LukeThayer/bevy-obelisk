# obelisk-arena — Phase 1 Context Brief

**Date:** 2026-06-25
**Status:** Design grounding (pre-brainstorm). Not a spec.
**Umbrella:** `docs/superpowers/specs/2026-06-24-obelisk-arena-design.md` (sections 4, 7, 10–13 are the architectural source for this brief).
**Purpose:** Ground a brainstorming conversation for **Phase 1 — the 1v1 online game (+ AI monster)**. Phase 0 (the obelisk-bevy 0.17→0.18 / Avian 0.4→0.5 migration) is the hard prerequisite and is assumed done before any Phase 1 code compiles.

This brief inventories what already exists across four sibling repos, sketches how the pieces compose into `arena_game`, and consolidates the open decisions into clarifying questions for the brainstorm.

---

## 0. The cast of repos

| Repo | What it lends Phase 1 | Versions |
|---|---|---|
| **wisp** (`../wisp`) | lightyear netcode scaffold (server-auth loop, replication, interpolation, input, headless server + observer test harness) **and** the player rig / animation / models (`character.glb`). | Bevy 0.18 / Avian 0.5 / lightyear 0.26 |
| **bevy_modal_editor** (`../bevy_modal_editor`) | navmesh stack (`bevy_landmass` + `bevy_rerecast`) + the `ai_demo` agent-steering reference + arena level prototype. | Bevy 0.18 / Avian 0.5 |
| **obelisk-bevy** (`.`, migrated) | deterministic combat: cast timelines, hit/hurt boxes, `resolve_one_hit` funnel, `CombatRng`, `NetEvent` egress, `CueEvent`, facades + verbs. | Bevy 0.18 / Avian 0.5 (post-Phase-0) |
| **arena_game** (new) | the crate we are building: composes all of the above. | Bevy 0.18 / Avian 0.5 / lightyear 0.26 |

Reuse verdicts use four levels: **copy** (lift the code ~as-is), **path-dep** (depend on the crate, don't fork), **reference** (study the pattern, write our own), **rebuild** (the concern exists but the implementation is wrong for arena).

---

## 1. Reusable inventory

### 1a. wisp-net (lightyear netcode scaffold)

| Unit | Path | Verdict | Why |
|---|---|---|---|
| Protocol definition (replicated components + messages) | `wisp/src/net/protocol.rs` | reference | Registration pattern is the template; arena extends with combat-event messages (obelisk `NetEvent`→lightyear bridge). |
| Server-auth controller (Input→FixedUpdate→Physics→Replicate) | `wisp/src/net/server.rs:446-514` | copy w/ adaptation | The loop shape is reusable; the movement constants are wisp-specific and combat-movement differs. |
| Lightyear plugin wiring (Client/ServerNetPlugin) | `wisp/src/net/client.rs:29-53`, `server.rs:39-87` | reference | Plugin composition + `TICK_HZ=60` setup is the pattern; arena wires obelisk + damage into the same structure. |
| Manual sender refresh (late-joiner fix) | `wisp/src/net/server.rs:196-232` | **copy** | `Replicate::manual` re-poll on client-count change. Essential: a mid-match joiner must get current arena state. Reuse exactly. |
| Input drain / message handler pattern | `wisp/src/net/server.rs:414-444` | reference | Generalizes to any client→server message; arena drains cast requests the same way. |
| Position sync (Transform → NetworkedPosition) | `wisp/src/net/server.rs:519-551` | **copy** | Arena adds obelisk cast-state fields to the replicated position component. |
| Client replication visuals + smoothing | `wisp/src/net/replication.rs:30-146` | reference | Render-delay interpolation + teleport snap; arena hooks the same observer, extends with combat state. |
| Headless server binary | `wisp/src/bin/server.rs` | reference | MinimalPlugins + no-render template; arena adds `ObeliskSimPlugin` + AI + arena scene. |
| Headless observer client (scripted, trace-emitting) | `wisp/src/bin/observer.rs` | reference | Template for arena's net-test harness; extend script commands with skill casts. |
| Networked health + damage attribution | `wisp/src/spells/damage.rs:151-186`, `server.rs:101-102` | copy w/ extension | hp-mirror + `ClientPlayerMap` attribution are directly reusable; obelisk's `DamageResolved` writes the mirror. |
| Child-cast / body-trigger integration | `wisp/src/spells/triggers.rs`, `server.rs:1030-1090` | reference | Template for obelisk skill-composition layer; arena routes obelisk `NetEvent`→lightyear instead of per-spell messages. |
| Trace plugin (JSONL cross-process correlation) | `wisp/src/trace.rs` | **copy** | Reusable as-is; arena adds obelisk events to the stream. |
| Net-test harness (run_session.sh + summarize.sh) | `wisp/tools/net-test/` | reference | Regression-suite template; adapt scripts + queries for skill/hit/damage events. |
| `add_avian_with_lightyear` plugin glue | `wisp/src/lib.rs:57-70` | **copy** | `LightyearAvianPlugin(Position)` + disabled conflicting physics plugins. Reuse as-is. |

### 1b. wisp-rig / anim (character + animation)

| Unit | Path | Verdict | Why |
|---|---|---|---|
| Character model + rig (`character.glb`) | `wisp/assets/character.glb` | reference w/ adaptation | Unified mesh + Polysplit skeleton (`pelvis→waist→chest→neck`) reusable for third-person; **fork** PartSelection for combat loadout, not cosmetics. Preserve `chest_joint` bone name (aim lean). |
| Locomotion animation clips | `wisp/assets/character.glb` | copy w/ minimal change | idle/walk_*/falling/casting_* clips + direction-weighted blend math reusable. Obelisk needs NEW per-phase cast clips (windup/active/recovery) authored separately. |
| Animation graph + playback system | `wisp/src/player/visuals.rs` | reference; rebuild binding | Load-named-clips + per-frame blend-weights pattern is sound. Obelisk needs a NEW binding layer driven by `ActiveCast.phase` + elapsed, not velocity+casting-flag. |
| Player controller (input→action, spine lean) | `wisp/src/player/controller.rs` | rebuild | First-person + pre-netcode. Reuse only the `apply_aim_pitch_to_local_spine` (`chest_joint` Z-rotate, lines 219-249) pattern + bone names. Write a third-person orbiter. |
| Bone attachment (chest spine socket) | `wisp/src/player/controller.rs:208-212` | reference w/ adaptation | Query bones by **Name** + ancestor walk (never Entity id). Add `wand_tip`/`sword_tip` named bones for VFX sockets the same way. |
| Avian character body (kinematic capsule) | `wisp/src/player/mod.rs:320-330` | copy w/ mods | Capsule (r=0.4, h=1.2) + ground-check pattern reusable. Arena may add a Static **hurtbox** (obelisk's model) separate from the kinematic view body. |
| Scene + visibility hierarchy (render layers) | `wisp/src/player/mod.rs:198-282` | reference | Arena is pure third-person → one scene root, no viewmodel. `propagate_self_body_render_layer` still useful for multi-rig layer stamping. |
| Model loading + caching | `wisp/src/player/visuals.rs:121-159` | rebuild | wisp swaps costumes at runtime; arena has fixed per-player models. Keep the programmatic `AnimationGraph`-from-`named_animations` pattern; bake the model paths. |
| Recolor system (material tint swaps) | `wisp/src/player/recolor.rs` | reference | Sound `named_materials` tint-mutation pattern if arena wants team colors / dyes. Orthogonal to animation. |
| Local vs remote animation driving | `wisp/src/player/visuals.rs:548-610`, `replication.rs:1059-1136` | copy w/ large adaptation | Dual-state (local + remote) exponential-easing pattern is sound. Rebind from velocity/casting-flag to `ActiveCast.phase` + elapsed; locomotion still blends during cast. |
| wisp CastPhase enum / CastInstance | `wisp/src/spells/engine.rs:75-102` | **reference only — do NOT reuse** | wisp casts are input-reactive (Fire-hold→Charging); obelisk casts are timeline-deterministic. Map obelisk phases to clips, not wisp phases. |

### 1c. editor-navmesh (bevy_landmass + bevy_rerecast)

| Unit | Path | Verdict | Why |
|---|---|---|---|
| `NavigationPlugin` + `NavmeshState` resource | `bevy_modal_editor/src/navigation/mod.rs:75-89,26-69` | **copy** (plugin: path-dep on landmass/rerecast) | Full navmesh-gen + landmass pipeline; works with paused physics. `NavmeshState` struct is self-contained — copy directly. |
| `gt_collider_backend` (GlobalTransform backend) | `navigation/mod.rs:98-125` | reference | Needed only if baking with paused physics; arena's server has running physics, so the default backend may suffice. |
| Navmesh baking flow (generate + on_ready observer) | `navigation/mod.rs:128-280` | copy | Lifecycle portable; replace the UI-button trigger with a match-start trigger. |
| `extract_wireframe(navmesh)` | `navigation/mod.rs:178-202` | copy | Self-contained; useful for debug viz + random on-mesh points. |
| Navmesh wireframe gizmo drawer | `navigation/mod.rs:157-174` | copy | Drop-in debug visual. |
| AI-mode panel UI | `bevy_modal_editor/src/ui/ai_editor.rs` | reference | Editor-only; not Phase 1 (Phase 2). Shows slider/button layout. |
| `RuntimeAgent` spawning | `ai_demo/src/agent.rs:29-138` | copy w/ mods | Replace SpawnPoint/Waypoint queries with monster/player queries; replace waypoint-find with player-chase. |
| `move_agents` (DesiredVelocity→Transform) | `ai_demo/src/agent.rs:145-157` | **copy** | Canonical "apply navmesh steering to transform" for landmass. After `LandmassSystems::Output`. |
| Agent retarget-on-arrival + `random_navmesh_point` | `ai_demo/src/agent.rs:160-223` | reference (retarget) / copy (helper) | Replace wander logic with chase; the random-point helper is a reusable utility. |
| Agent path gizmo drawer | `ai_demo/src/agent.rs:226-246` | copy | Debug visual. |
| Arena level prototype + `level_gen` helpers | `ai_demo/src/levels/{arena,level_gen}.rs` | reference / copy-with-mods | Geometry-as-primitives example; reuse `spawn_cube/spawn_cylinder` to build the Phase 1 hard-coded arena. |
| SpawnPoint/Waypoint custom entity types | `ai_demo/src/main.rs:21-92` | copy w/ mods | Editor registration — relevant Phase 2; for Phase 1 just hard-code spawn markers. |

### 1d. obelisk-seams (combat engine integration points)

| Unit | Path | Verdict | Why |
|---|---|---|---|
| `ObeliskSimPlugin` (Validate→Advance→Projectiles→ResolveHits→TickEffects) | `obelisk-bevy/src/lib.rs:93-141` | path-dep | Add to **both** server + client apps. Server authoritative; client runs same sim for prediction. Headless, no render deps. |
| `CombatRng` + `seed_combat_rng(u64)` | `core/config.rs:14-72` | path-dep, **server-only** | Seeded ChaCha8. Lives on server ONLY; arena wires lightyear's session seed → `app.seed_combat_rng()`. Clients never touch RNG. |
| `resolve_one_hit` funnel | `combat/resolve.rs:78-150` | reference (server) | Deterministic damage funnel; runs only on server via `on_hit_confirmed`. |
| `ObeliskCombat` facade (resolve_skill_hit / resolve_aoe) | `facade/combat.rs:19-92` | reference (server) | Programmatic skill resolution for AI; deterministic, stable-sorted targets. |
| Validate→Advance→detect_overlaps FixedUpdate pipeline | `timeline/advance.rs`, `spatial/detect.rs` | path-dep | Deterministic state machine; server is authoritative, clients corrected by replication. |
| `NetEvent` enum + `ObeliskNetPlugin` | `net.rs:1-221` | path-dep | serde-stable String-id wire format (CastBegan/DamageResolved/EffectApplied/EntityDied/CastRejected). Arena drains `MessageReader<NetEvent>` → lightyear messages. |
| `CueEvent` + `ObeliskCuePlugin` + `observe_cue` | `vfx.rs:1-119` | path-dep | Fires from `.cast.ron` `vfx_cues`; deterministic (no RNG). Server emits; arena replicates a separate `CueMessage` so clients bind locally. |
| `ActiveCast` / `PendingCast` + cast verbs | `timeline/state.rs`, `timeline/cast.rs:14-59` | path-dep | `cast_skill_at[_point/_dir]` insert `PendingCast`; validate consumes it. Server authoritative. |
| `ObeliskSpatial` facade (nearest_enemy, in_range, cone, raycast, los_clear) | `facade/spatial.rs` | reference (server) | Target acquisition for AI + cast validation. Returns targets; does NOT resolve hits. |
| `Hitbox` / `Hurtbox` | `spatial/boxes.rs` | path-dep, server-owned | Server spawns hitboxes during hit windows + owns overlap detection. Client renders hurtbox for debug only. |
| `ObeliskRead` facade (life/mana/has_effect/can_cast) | `facade/read.rs` | reference | Read-only; HUD/AI/prediction on both sides. |
| `Combatant` + `Attributes` + `SkillSlots` + `Faction` | `core/components.rs` | path-dep | Core bundle; lightyear replicates to clients. Invariant: `ObeliskId == StatBlock.id`. |
| `make_combatant` / `grant_skill` / `apply_obelisk_effect` verbs | `verbs.rs:1-121` | **copy** (usage) | Spawn players + AI, grant skills. `make_combatant` enforces the id invariant. |
| `CastTimeline` asset (.cast.ron) | `assets/mod.rs:1-140` | path-dep | RON: phases + windows + `vfx_cues` + targeting + delivery. Arena adds a `.skillfx.ron` sidecar; `CastTimeline` itself unchanged. |
| `SkillRegistry` | `core/config.rs:9-10` | path-dep | Loaded on both sides from same files; server authoritative. |

---

## 2. Integration architecture (how it composes into `arena_game`)

### 2.1 The server-authoritative loop

The wisp server loop and obelisk's FixedUpdate pipeline **nest cleanly** because both are headless and deterministic, both run at 60 Hz fixed, and both already separate sim from presentation.

- **Server** (headless binary, `MinimalPlugins`): `ServerNetPlugin` (wisp) + `add_avian_with_lightyear` + `ObeliskSimPlugin` + arena scene + AI logic. It owns `CombatRng` (seeded once at match start from a lightyear-provided session seed) and is the sole authority for hit resolution, damage rolls, effect application, cooldowns, mana, and death.
- **Each Update tick** the server drains client→server messages (movement input + one-shot cast requests) into per-player state.
- **Each FixedUpdate tick** the server: runs the movement controller (wisp pattern, adapted) → physics integrates → runs obelisk's `Validate → Advance → Projectiles → ResolveHits → TickEffects` → `detect_overlaps` fires `HitConfirmed` → `on_hit_confirmed` calls `resolve_one_hit` against `CombatRng` → emits `DamageResolved`/`EffectApplied`/`EntityDied` → `ObeliskNetPlugin` mirrors those to the buffered `NetEvent` stream + `vfx_cues` fire `CueEvent`.
- **Egress:** an arena bridge system drains `MessageReader<NetEvent>` + `CueEvent` each Update and converts them to lightyear messages (a combat-event message + a separate `CueMessage`), plus syncs `Transform`→`NetworkedPosition` (extended with cast phase/skill-id) and mirrors hp into `NetworkedHealth`. `refresh_replicate_on_connect` keeps the manual sender list current for late joiners.

### 2.2 Client prediction / interpolation

- **Movement:** follow wisp's current model — interpolate replicated positions with render-delay smoothing + teleport snap (`replication.rs`). wisp does NOT predict-and-reconcile player movement today; whether arena needs to is an **open question** (Stage Q / full Position rollback is registered-but-disabled in wisp's protocol).
- **Combat sim on client:** the client also runs `ObeliskSimPlugin` for local prediction + cosmetic timeline ticking (deterministic phases), but it must accept the server's `HitConfirmed`/`DamageResolved` as canonical and correct on divergence. Client never emits events that feed `on_hit_confirmed`; it only consumes them. Client never spawns Hitboxes.
- **Presentation reactions:** VFX, animation, and projectiles are client-side reactions to replicated `CueMessage` + `DamageResolved` + replicated cast state — the obelisk sim↔present split, exactly as the design doc §11 specifies.

### 2.3 Input → cast path

```
keyboard/mouse  ──►  PlayerInputMessage (extended: cast_request: Option<(skill_id, target_hint)>)
                          │  (client → server; movement on unreliable channel,
                          │   cast request likely on a reliable one — open question)
                          ▼
        [server, Update] drain_player_inputs → per-player input state + queued cast request
                          ▼
        [server, FixedUpdate] arena system: entity.cast_skill_at(skill_id, target)  → PendingCast
                          ▼
        obelisk Validate: range / LOS / mana / cooldown / already-casting gates
                          │
            reject ───────┴─────── accept → ActiveCast + CastBegan
              │                              │
        CastRejected (NetEvent)         Advance phases → hit windows → detect_overlaps
                                             │
                                        HitConfirmed → resolve_one_hit (CombatRng)
                                             │
                                        DamageResolved / EffectApplied / EntityDied
```

### 2.4 End-to-end data flow (ASCII)

```
 CLIENT A                          SERVER (authoritative)                      CLIENT B
 ─────────                         ──────────────────────                      ─────────
 input (move + cast)  ──msg──►  drain inputs → cast_skill_at
                                       │
                                obelisk FixedUpdate sim
                                  Validate→Advance→ResolveHits
                                  (CombatRng — server only)
                                       │
                          ┌────────────┼─────────────────────┐
                          ▼            ▼                      ▼
                   NetworkedPosition   NetEvent          CueEvent
                   + NetworkedHealth   (Damage/Cast/     (on_cast/
                   (replicated state)   Death/Reject)     on_window/on_hit)
                          │            │                      │
                          │       →lightyear msgs        →CueMessage
                          │            │                      │
   ◄──replicate───────────┴────────────┴──────────────────────┴──────────►
        │                                                          │
   present layer:                                            present layer:
   interpolate pos, lerp hp bar,                            (same)
   bind Cue→anim clip + particle + projectile spawn,
   play DamageResolved floating numbers / death anim
```

The seam discipline: **timeline drives animation** (deterministic phase → clip), **cues drive VFX/projectiles** (presentation-facing), **NetEvent drives HUD/damage/death state**. All three are pure client-side reactions; none feed back into the authoritative sim.

---

## 3. The `arena_skills` question (now or later?)

**The choice:** does Phase 1 ship the full `arena_skills` crate + a formal `.skillfx.ron` sidecar format now, or a **minimal inline cue→effect binding** in `arena_game`, deferring the real format + the designer to Phase 3?

**What each skill needs to look + feel right in Phase 1:**
- a `.cast.ron` (`CastTimeline`: phases, hit windows, `vfx_cues`) — obelisk already requires this.
- a binding from each `vfx_cues` key → *play this anim clip* / *spawn this particle effect at this socket* / *spawn this projectile*.

**Recommendation: minimal inline binding in Phase 1; defer the crate + designer to Phase 3.**

Rationale:
- Phase 1's job is to **prove the integration** (netcode + obelisk + rig + AI work end to end), not to deliver a content pipeline. The full `arena_skills` crate earns its keep only when the **designer** (Phase 3) needs a shared data+bridge seam between the game and the editor. With only ~2 skills (one player, one monster), a hand-written `HashMap<cue_id, Binding>` registry behind `app.observe_cue(...)` is enough and far cheaper.
- The umbrella design (§4) already scopes `arena_skills` as "the seam every other crate shares" — its value is **shared** consumption by `arena_game` AND `arena_editor`. In Phase 1 only `arena_game` exists, so the crate boundary buys nothing yet.
- **De-risk the format by deferring it.** The `.skillfx.ron` schema (per-cue lane events: anim/particle/projectile) is exactly the data the Phase 3 timeline UI must author. Designing that format before we know the editor's emitter/socket model risks churning it twice. Phase 1 should keep the binding small and inline so Phase 3 can lift it into `arena_skills` with full knowledge.
- **Keep the cue contract stable now.** What Phase 1 SHOULD lock down is the **cue key naming convention** (`on_cast` / `on_window_<id>` / `on_hit`) and the `CueMessage {cue_id, source_id, position, kind}` wire shape, because those cross the network and constrain the designer. The binding *implementation* can be inline; the cue *interface* should be deliberate.

Net: Phase 1 = a tiny inline `cue_id → {anim_clip, particle_ref, projectile_ref}` table consumed by `observe_cue`, plus a stable `CueMessage`. Promote to `arena_skills` + `.skillfx.ron` when Phase 3 starts.

---

## 4. AI monster (minimal viable)

**Target behavior:** a server-driven monster that **chases the nearest player via the navmesh** and **casts a ranged obelisk skill on a cadence** when in range. This proves obelisk skills work for NPCs, not just players (design §7).

### What it needs from the navmesh stack
- **At match start:** a navmesh baked from the arena's static colliders (copy `handle_generate_navmesh` + `on_navmesh_ready`), an `Archipelago3d` + `Island3dBundle` spawned, and the monster spawned as an `Agent3d` with `ArchipelagoRef3d` pointing at the archipelago (copy `spawn_agents_on_play`, replace spawn-point/waypoint logic).
- **Each FixedUpdate:** set `AgentTarget3d::Entity(nearest_player)` (replace the wander/retarget logic with a nearest-player query), let landmass compute `AgentDesiredVelocity3d`, then `move_agents` applies it to `Transform` (copy `move_agents`, after `LandmassSystems::Output`). The agent is kinematic — not a `RigidBody` — so steering is decoupled from physics.
- **Phase 1 simplification option:** for a small bounded arena with little cover, simple "steer toward player" may obviate the full navmesh bake. Navmesh earns its cost only if the arena has obstacles to path around. (Open question — arena complexity decides.)

### What it needs from obelisk
- spawn with `make_combatant(StatBlock)` + `Faction::Enemy` + `SkillSlots` populated with the monster's ranged skill id(s) + a `Hurtbox`.
- a **monster-action system** (FixedUpdate, after movement): use `ObeliskSpatial::nearest_enemy` / `enemies_in_range` to acquire the target, check range + `Cooldowns` + (optionally) `los_clear`, then `entity.cast_skill_at(skill_id, player)`. obelisk validates and gates the cast; acceptance flows through the same `CastBegan → hit window → resolve_one_hit` path as a player. **No client-side AI logic** — the server decides; clients render the replicated result.
- the monster's ranged skill needs its own `.cast.ron` (phases + a `Projectile` delivery + a collision window) + `vfx_cues`, plus the same inline cue binding as the player skill.

### What it needs replicated
- `Combatant` + `Attributes` (so clients render hp + see the monster), `NetworkedPosition` (so clients interpolate its chase), and the same `NetEvent` + `CueMessage` egress for its casts. Death: `EntityDied` → Phase 1 minimal just despawns the monster (no loot pickup; loot networking deferred per design open-questions).

### Minimal AI decision policy (Phase 1 default, to be confirmed)
- target = **nearest player** (re-evaluated each frame; if it dies, re-target the other).
- cadence = **cast when player in range AND skill off cooldown** (obelisk's range/cooldown gates do most of the work; a coarse aggro radius gate is optional). No rotation, no reactions, no interrupts.

---

## 5. Recommended Phase 1 slice + milestones

Each milestone is the **smallest vertical slice that proves one integration boundary**. Build them in order; each is independently demoable.

- **M1 — single-player move + cast on the new stack.** One client, no server split (or a co-located server). Spawn the wisp rig as an obelisk `Combatant`; third-person camera + controller (rebuilt); cast one skill via `cast_skill_at`; obelisk `ActiveCast.phase` drives the animation blend; `CueEvent` → inline binding → particle + projectile spawn. **Proves:** obelisk combat + wisp rig + animation binding compose at all, headless sim ↔ present split holds.
- **M2 — 2-player server-authoritative online.** Headless server runs `ObeliskSimPlugin` + `CombatRng`; two clients connect (wisp netcode); player casts replicate via `cast_request` → server validates → `NetEvent`/`CueMessage` egress → both clients render damage + VFX. `NetworkedHealth` HUD; late-joiner refresh. **Proves:** the full input→server-obelisk→replicated-state→client-present loop, server authority, the `NetEvent`→lightyear bridge.
- **M3 — AI monster.** Server bakes navmesh, spawns the monster as a landmass `Agent3d` + obelisk `Combatant`, chases nearest player, casts a ranged skill on cadence; replicated to both clients. **Proves:** obelisk skills drive NPCs, navmesh-steering + obelisk-casting compose on the server, AI state replicates with no client decision logic.

**Suggested supporting work (alongside M2/M3):** stand up the trace harness (copy `wisp/src/trace.rs` + adapt `run_session.sh`) so each milestone has a regression assertion (e.g., "player casts firebolt → both peers see `CastBegan → HitConfirmed → DamageResolved`").

### Explicitly OUT of Phase 1
- **The editor** (`arena_editor`, `EditorPlugin`, editor-mode toggle) — Phase 2.
- **The skill designer** (timeline-over-CastTimeline sequencer, "Play Skill" preview, lane UI) — Phase 3.
- **The `arena_skills` crate + `.skillfx.ron` format** — deferred to Phase 3 (see §3); Phase 1 uses inline binding.
- **Editor-authored arena scenes** — Phase 2; Phase 1 arena is hard-coded geometry.
- **Loot pickup / item networking** — deferred; Phase 1 monster death just despawns.
- **Deep AI** (rotations, reactions, interrupts, lead targeting, threat) — Phase 1 is chase + cadence-cast only.
- **Phase 0** (the obelisk-bevy migration) — prerequisite, separate spec, assumed done.

---

## 6. Open design questions for the brainstorm

Consolidated + deduped from all four input areas, phrased as crisp choices. These are the decisions to settle before writing the Phase 1 spec.

1. **Scope & win-condition.** What ends a match? (best-of-N rounds / first-to-X-kills / last-standing / timed?) Does the AI monster threaten both players (shared PvE) while they also fight each other (PvP), or is it a separate mode? Is HP-on-death respawn (round-robin spawn) or elimination?

2. **Reuse model for wisp.** Do we **depend on `wisp` as a path/git crate** and call into its net/rig modules, or **copy the relevant files** into `arena_game` and diverge? (wisp's controller is first-person + pre-netcode-complete; the rig + netcode loop are reusable but need adaptation. Depending couples us to wisp's churn; copying forks maintenance.)

3. **Listen-server vs dedicated.** Dedicated-only (wisp's current model: headless server, both players are clients) — or also a **listen-server** (player 1 hosts) for local/casual play? Listen-server adds host-input-zero-latency-advantage handling + peer-selection complexity.

4. **Client prediction depth.** Movement: keep wisp's interpolate-only (accept slight latency, simplest) or finish Stage Q (full server-auth Position rollback + client reconciliation)? Casts: server-latency-only, or client-side cast prediction (anticipate the hit before server confirms)? This drives how much netcode work Phase 1 carries.

5. **`arena_skills` now or later.** Confirm the recommendation (§3): minimal **inline cue→effect binding in Phase 1**, promote to the `arena_skills` crate + `.skillfx.ron` sidecar at Phase 3 — or stand up the crate + format now to avoid a later lift?

6. **Cast-request reliability + authority boundary.** One-shot cast requests: **reliable channel** (guaranteed, +latency) or unreliable (+client retry)? And does the server trust the client's `target_hint`, or re-acquire the target server-side (re-raycast / `nearest_enemy`) and reject if out of range? (Responsiveness vs cheat-resistance.)

7. **AI depth + targeting.** Confirm Phase 1 AI = **nearest-player chase + cadence-cast ranged skill**, pure melee/ranged/hybrid? Targeting policy (nearest / last-hit-me / random)? And the arena-complexity call: **full navmesh bake** (path around cover) vs **simple bounded steering** (no navmesh, faster)?

8. **Milestone ordering + headless-test investment.** Confirm M1 (single-player rig+cast) → M2 (2-player server-auth) → M3 (AI monster). Do we build the trace/observer regression harness in Phase 1 (front-loads cost, protects every later phase) or defer it?

9. **Animation clip inventory + cast clips.** (Needs verification.) Do all referenced locomotion clips actually ship in `character.glb` (left/right may be synthesized)? And do we author NEW per-phase cast clips (windup/active/recovery) for fidelity, or Phase-1-reuse the existing `casting_*` clips at reduced fidelity (single casting-stance blend)?

10. **Replicated combatant fidelity.** Max HP: replicate per-player from obelisk's `StatBlock` (dynamic, correct) or hardcode for v1? Remote cast fidelity: replicate the full `ActiveCast` timeline (Windup/Active/Recovery — precise remote animation) or just a casting-flag + locomotion (cheaper, no recovery-phase anim on remotes)?
