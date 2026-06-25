# obelisk-arena — Design

**Date:** 2026-06-24
**Status:** Proposed (design approved in brainstorming; awaiting written-spec review)
**Companion:** `docs/superpowers/specs/2026-06-24-game-template-context.md` (feasibility brief + the file-level migration survey + online references).

---

## 1. Goal

Build a **new workspace repo, `obelisk-arena`**, that is a reusable **template** for integrating three Bevy pieces into one game:

- **bevy-obelisk** — our deterministic ARPG combat/skill engine.
- **bevy_modal_editor** — a keyboard-first in-Bevy editor with a particle/VFX system and a game-integration API (`bevy_editor_game`).
- **lightyear** — server-authoritative netcode (as wisp uses it).

**The game:** two players fight each other online in an arena, plus a **server-driven AI monster that chases and shoots** them.

**The key deliverable** is a complete **in-editor skill designer** in which the editor's sequencer is the *master source* for authoring obelisk skills — integrating player **animation + particle effects + projectiles**, with a live **"play the real skill"** preview. The designer authors *both* player and monster skills.

Player **models and animations** come from **wisp**; the game builds on **wisp's** lightyear + avian + rig/animation foundation.

---

## 2. What the three pieces provide today

- **obelisk-bevy** — a skill = a **stat TOML** (obelisk stats) + a **`.cast.ron`** (`CastTimeline`: phases `Windup`/`Active`/`Recovery`, collision/hit windows, and `vfx_cues: HashMap<String,String>`). `vfx_cues` → `CueEvent` (keyed `on_cast` / `on_hit` / `on_window_<id>`), routed by `ObeliskCuePlugin`. **Bevy 0.17 / Avian 0.4.** Backed by a 39-golden + 84-test regression suite.
- **bevy_modal_editor** — **Bevy 0.18 / Avian 0.5.** `EditorPlugin` + `crates/bevy_editor_game` (game-facing API: custom entity/component registration, state, events). Has an effects/VFX system (an `EffectMarker` *step-list* + a GPU `VfxSystem` particle pipeline) but **no real timeline editor**. `crates/marble_demo` is the example game. egui UI. Ships `bevy_landmass` + `bevy_rerecast` (**navmesh**).
- **wisp** — **Bevy 0.18 / Avian 0.5 / lightyear 0.26** (server-auth, replication, interpolation, input). A working networked-game scaffold (`net/{client,replication}`, `weapons`, `health`, an `observer` binary) + the **player models** (`character.glb`, `player_meshes.glb`, `wizard.glb`) and animation.

---

## 3. The hard prerequisite — version alignment

obelisk-bevy (**0.17 / Avian 0.4**) will not compile with the editor + wisp + lightyear (**0.18 / Avian 0.5**). The first phase is the **migration**: roughly **3–4 weeks**, *Medium* for Bevy (70+ `On<E>` observers, `#[require]` semantics, `add_message`/`MessageReader` changes) and *HIGH* for Avian (the **`SpatialQuery` rewrite** in `spatial/detect.rs` is the single biggest risk). The **39-golden suite is the regression gate** that makes a scary rewrite safe.

---

## 4. Architecture — the `obelisk-arena` workspace

A new Cargo **workspace** on **Bevy 0.18 / Avian 0.5 / lightyear 0.26**, three member crates, depending on the (migrated) sibling repos:

| Crate | Responsibility | Depends on |
|---|---|---|
| **`arena_skills`** | The shared skill **data + bridge**. Extends obelisk `CastTimeline` as the master source; defines the **`.skillfx.ron` sidecar** (lane events — animation / particle / projectile — bound to cue keys); the **cue → effect binding layer** consumed by *both* the game and the designer. | obelisk-bevy |
| **`arena_game`** | The **1v1 game**: lightyear netcode + avian, server-authoritative combat (obelisk), wisp rig + animation, the arena, two players, and the **AI monster**. Built on **wisp's** net/controller/anim layers, adapted for arena fighting. | obelisk-bevy, arena_skills, lightyear, wisp (reused) |
| **`arena_editor`** | Embeds **bevy_modal_editor** (via `bevy_editor_game` registration) + the **skill-designer** UI + an in-game **editor-mode toggle** (one binary, author ↔ play). | bevy_modal_editor, arena_game, arena_skills |

**Dependencies:** path deps to `../obelisk-bevy` (migrated), `../bevy_modal_editor`, and `../wisp` (reused code) for local dev; switch to git deps for portability later.

**Why this split:** `arena_skills` is the seam every other crate shares, with no Bevy-app concerns; `arena_game` is playable headless/server without the editor; `arena_editor` is the only crate that pulls egui + the editor. Each is understandable and testable on its own.

---

## 5. Phased delivery

Each phase is its own **spec → plan → build** cycle. This document is the **umbrella vision**; implementation starts with Phase 0.

- **Phase 0 — migrate `obelisk-bevy` → Bevy 0.18 / Avian 0.5** (in `../obelisk-bevy`), gated by the 39-golden suite. *The hard prerequisite; nothing else compiles until it's done.*
- **Phase 1 — the 1v1 online game**: wisp netcode + animated characters + obelisk combat + the AI monster.
- **Phase 2 — embed the editor** in-process (editor-mode toggle; register obelisk entity/component types so skills/actors/spawns are editable).
- **Phase 3 — the skill designer**: the timeline-over-`CastTimeline` sequencer + binding + live "play the real skill" preview.

---

## 6. Phase 0 — the migration (first sub-project)

Migrate **obelisk-bevy** in its own repo from Bevy 0.17 → 0.18 and Avian 0.4 → 0.5. (The companion brief enumerates this file-by-file.)

**Change areas**
- **Bevy 0.17 → 0.18:** `On<E>` observer API changes across ~70 observers; `#[require(...)]` required-component semantics; `add_message`/`MessageReader`/`MessageWriter` changes; any 0.18 render/UI/asset breaks (the `present`/`debug_viz`/screenshot/playground paths).
- **Avian 0.4 → 0.5 (highest risk):** the **`SpatialQuery` rewrite** in `spatial/detect.rs` (cone/hitbox detection), plus collider/rigidbody/`ColliderAabb` API changes used in `spatial/boxes.rs`, the hurtbox sync, and the debug-viz gizmos.
- **Carry the in-flight `feat/combat-result-trigger-metadata` obelisk change** (the skill-trigger `CombatResult` metadata, currently unpushed) through the migration, and resolve its publish state.

**Gate (definition of done):** all **39 goldens byte-identical**, all **84 unit/integration tests green**, clippy `-D warnings` + fmt clean, and `cargo build` of the lib + all examples (`playground`, `screenshot`, `headless_server`) on the new versions.

---

## 7. Phase 1 — the 1v1 online game (+ AI monster)

- **1v1 PvP:** two players, **server-authoritative**, lightyear replication, avian movement (wisp's controller, adapted to a third-person arena framing).
- **Characters:** the wisp rig + animation clips (idle / run / cast / hit / death), blended; players are obelisk combatants that cast their authored skills. Hits/damage resolve **server-side**; the obelisk **`NetEvent` egress is wired into lightyear messages**.
- **AI monster (PvE):** server-driven. **Chases** the nearest player via the **navmesh** (`bevy_landmass` + `bevy_rerecast`); **shoots** with an obelisk ranged skill (authored in the designer); replicated to both clients. Proves obelisk skills work for **NPCs**, not just players.
- **Arena:** a simple bounded arena scene (later authorable in the editor, Phase 2).

---

## 8. Phase 2 — embed the editor

- `arena_editor` adds `EditorPlugin` (bevy_modal_editor) plus an **editor-mode toggle** (play ↔ edit) in one binary.
- Register obelisk entity/component types through `bevy_editor_game` so **combatants / skill-grants / spawn points / the arena** are editable in the scene; persist/load via the editor's prefab/scene system.

---

## 9. Phase 3 — the skill designer (the heart)

- **Master source = obelisk `CastTimeline`, grown into a track-based timeline** (scrubber + lanes). The editor today has particle *primitives* but no timeline editor — **building that timeline UI is the bulk of this phase.**
- **Lanes** (all time-anchored to the phases): **Animation** (wisp rig clips), **Particles** (editor `bevy_vfx` emitters at sockets/times), **Projectile** (obelisk projectile + its VFX), **Hit window** (obelisk collision windows), **Cues** (the named `CueEvent` keys).
- **Binding (the chosen seam):** every lane event **compiles down to obelisk `vfx_cues` keys**; the `arena_skills` binding layer routes each `CueEvent` → *play this anim clip* / *spawn this particle effect at this socket* / *spawn this projectile*. **No parallel model** — the timeline is a visual editor over the skill's real `.cast.ron` + the `.skillfx.ron` sidecar.
- **"Play Skill" loop (the payoff of in-process):** ▶ spawns a test caster + dummy and runs the skill through the **real obelisk pipeline** — real phases, real hitbox, real damage/hit-confirm — with the real animation + VFX + projectile. You preview the *actual* skill, and the same data drives it in the networked game.

```
┌─ Skill: firebolt ───────────────────────────[ ▶ Play Skill ]──[ Save ]─┐
│ phases    │ Windup ──────────│ Active ──────│ Recovery ───────────────  │
│ time (s)  0       0.3        0.6            0.9                     1.2   │
│───────────┼──────────────────┼──────────────┼─────────────────────────  │
│ Animation │■ cast_windup ────│■ cast_release │■ recover ──────────────   │
│ Particles │      ◆ charge_fx │   ◆ muzzle_fx │                           │
│ Projectile│                  │   ● firebolt →│                           │
│ Hit window│                  │  ▭ bolt ──────│                           │
│ Cues      │     on_cast      │   on_release  │   on_recover              │
└────────────────────────────────────────────────────────────────────────┘
 inspector ▸ ◆muzzle_fx — emitter preset · socket=right_hand · color · scale
```

---

## 10. Data formats

A skill stays plain, hand-editable, and editor-authored:
- **stat TOML** (obelisk stats) + **extended `.cast.ron`** (`CastTimeline`) + a **`.skillfx.ron` sidecar** listing lane events (anim-clip refs, particle-emitter refs, projectile refs) keyed to cue names. Editor particle effects save in the editor's own effect format, referenced by the sidecar. All RON/TOML — the editor writes them; they remain diffable and hand-editable.

---

## 11. Networking

**lightyear 0.26, server-authoritative** (as wisp does it). The server runs obelisk combat + the AI; obelisk's **`NetEvent` egress → lightyear messages**. Clients **predict** movement + **interpolate** replicated state; **VFX / animation / projectiles are client-side reactions** to replicated cue/damage events (obelisk's sim ↔ present split). The designer's "Play Skill" runs single-process for authoring.

---

## 12. Testing

- **Phase 0:** 39 goldens byte-identical + 84 unit/integration tests + clippy/fmt/builds on 0.18/0.5 — the migration gate.
- **Phase 1+:** combat correctness via obelisk's deterministic scenarios; netcode via a headless server + wisp's `observer` binary; the designer via **round-trip tests** (author → save → load → "Play Skill" produces the expected obelisk event trace).

---

## 13. Error handling

RON/TOML load failures surface **visibly in the editor** (never silent); a missing animation/particle asset shows an editor warning and the binding layer **no-ops gracefully at runtime** (mirrors obelisk's lenient effect-apply), so a half-authored skill never crashes the game.

---

## 14. Online reference patterns (informing the designer)

- **Unreal:** Gameplay Ability System (GAS) + **Gameplay Cues** + Niagara; **AnimNotifies / Montages** sequencing ability events to animation.
- **Unity:** Timeline + VFX Graph; ScriptableObject-driven ability data.
- **Distilled lesson:** ability **phases carry time-ordered events** (play anim / spawn VFX at a socket / spawn projectile / open hitbox) — which is exactly what obelisk's `CastTimeline` grows into. The designer is a **timeline editor over that data**, not a new model. (Source links in the companion brief.)

---

## 15. Scope & sequencing notes

- This document is the **umbrella**. Each phase (0–3) gets its **own** spec → plan → build cycle.
- **First implementation = Phase 0** (the migration), which lives in `../obelisk-bevy` and gets its own detailed plan next.
- The new `obelisk-arena` repo is **created at Phase 1** (Phase 0 is purely in `../obelisk-bevy`).
- **Open / deferred:** workspace portability (path vs git deps); the skill-designer's v1 depth (timeline+preview first, full sequencer as a Phase-3 sub-step); publishing the obelisk `feat` branch (folded into Phase 0).
