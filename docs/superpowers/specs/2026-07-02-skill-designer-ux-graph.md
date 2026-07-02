# Skill Designer UX — The Graph Is the Editor — Design Spec

**Status: PROPOSED** (interview-grounded; two answers assumed, flagged §2). The current editor
accreted feature-by-feature into an unusable wall of unlabeled combos; this spec rebuilds the
UX around how a human actually thinks about a skill.

**Interview findings (2026-07-02):** all four pains confirmed (unparseable controls,
exact-name typing, invisible skill structure, test-loop friction); workflow is an even mix of
inventing / tuning / visual authoring; layout decision = **node graph canvas**; test loop
decision = **scrub-first** (Play demoted to occasional full-fidelity check).

## 1. North star

**The skill reads on screen the way you think about it, and the scrubber never lies.**

A skill IS a graph (anatomy spec): Cast → windows via edges (schedule, end-reactions,
retarget hops), with visuals bound to the moments. Today that graph exists only in the data;
the UI shows a flat list of 12-widget rows. Invert it: the graph is the primary surface, every
number lives in a labeled inspector for the selected node, and dragging the scrub head steps
the REAL deterministic sim so what you see at t=0.42s is exactly what the game plays.

## 2. Decisions

| # | Decision | Source |
|---|---|---|
| D1 | Bottom dock becomes a **node-graph canvas**: Cast node + one node per window; edges = spawn schedule (Cast→scheduled windows) and end-reactions (hit/world/fuse → Chain / Retarget-with-hop-badge). | user |
| D2 | **Scrub-first**: a scrub bar under the graph drives a **sim-backed ghost preview** — the deterministic sim stepped to t, rendering true hitbox positions, trajectory trace, hit markers + damage numbers, cue vfx at true moments. Play stays for full-fidelity (anim/audio) checks. | user + ASSUMED (sim-backed) |
| D3 | Nodes show **structure + visual badges**: key facts (motion, fuse, shape) plus bound cosmetics (⚡trail, 💥Explosion) as badges on the node/edge they bind to. Clicking a badge selects that lane in the inspector. | ASSUMED |
| D4 | **Selection inspector** (right panel, the mode already registers PanelSide::Right): labeled, vertical, grouped sections for whatever is selected — node, edge, lane, or the skill root. Kills the 12-widget horizontal rows. | follows D1 |
| D5 | **Never type a registry name again**: vfx effects, anim clips, sockets, chain targets are pickers fed by the live registries (VfxLibrary keys, AnimationLibrary clips, RigSockets, window ids). Free-text stays available behind an "edit as text" affordance. | user (pain #2) |
| D6 | **Rules split by relevance**: mana/cooldown/damage/applied-effects appear in the Cast node's inspector (advanced trigger plumbing collapsed); shared effect DEFINITIONS (burn) keep a small separate surface — they are cross-skill content, not part of one skill's graph. Tabs otherwise die. | ASSUMED |
| D7 | Direct manipulation v1: **drag a wire** from a node's end-port onto another node to set chain/retarget (drop on empty = new Chained window; delete wire = clear reaction); **drag node timing** (spawn offset/duration) on a mini-bar under each scheduled node. Viewport gizmo dragging (radius rings, arc apex) is v2. | ASSUMED |
| D8 | **Feedback is inline**: validation (unknown target, non-Chained target, cycles, orphan Chained windows) renders ON the graph (red wire, warning badge) live, not as a status string after Save. Save stays explicit; dirty state per-node badge. | pain #1/#4 |

## 3. The surfaces

```
┌────────────────────────────────────────────┬──────────────────┐
│                                            │ INSPECTOR        │
│                 3D VIEWPORT                │ (selection-driven│
│   duel + ghost preview at scrub time       │  labeled groups) │
│   trajectory trace · hit markers · rings   │                  │
│                                            │ ▸ bolt (window)  │
├────────────────────────────────────────────┤  Shape  Sphere ▾ │
│ GRAPH                              firebolt│   radius   0.5   │
│ ┌─CAST───┐    ┌─bolt─────────┐  hit ┌─blast──┐ Motion Ballistic│
│ │0.3s ▸5⚡│───▶│● Ballistic    │──┬──▶│○ Sphere │  speed    20   │
│ │Fire    │    │20/9.8 · 2s ⏱ │world │r1.5·hit│  gravity  9.8  │
│ │        │    │⚡trail        │──┘   │💥Explos.│ Fuse      2.0s │
│ └────────┘    └──────────────┘ fuse──▶        │ On end          │
│                                     └────────┘  hit   chain ▾  │
│ scrub ▕──────●────────────▏ t=0.42s  ▶ play │  world chain ▾  │
└────────────────────────────────────────────┴──────────────────┘
```

- **Graph canvas** (bottom dock): auto-laid-out left→right by causality (Cast, then scheduled
  windows, then chained). Retarget self-loops render as a loop arrow with `×3` hop badge.
  Node selection ↔ viewport gizmo highlight. `+` on empty canvas = add window (template
  choices: strike / projectile / zone / beam).
- **Scrub bar** (under graph): the primary verb. Sim-backed ghost (D2): scrubbing runs the
  preview world's fixed schedule to t deterministically (same seed), so charge scaling,
  retarget picks, ground impacts are all TRUE. Time markers on the bar for every discrete
  event (window open, each hit, each end) — click a marker to jump. Play = handoff to the
  live duel as today.
- **Inspector** (right): sections per selection kind. Window → Shape / Motion / Hits /
  Timing / On End. Cast → Phases / Costs / Damage / Effects applied / (collapsed) Triggers.
  Lane → anim picker, effect picker, socket picker, param bindings. Skill root (nothing
  selected) → id, tags, targeting (Direction = free aim, SingleEntity = hitscan — say so in
  the label), delivery.
- **Viewport**: existing gizmos survive, keyed to selection; ghost preview entities at scrub
  time; hit markers with floating damage.

## 4. What dies

The Timeline/Rules/Effects tab strip; the 12-widget window rows; the cosmetic-lane text
fields; the phase-strip-as-property-editor (phases become the Cast node + inspector); the
"saved; N skills reloaded" status line as the only feedback channel.

## 5. Phases (each independently shippable, editor suite green at each gate)

- **P1 — Inspector + pickers** (foundation, valid under any layout): selection model
  (node/lane/root), right-panel labeled inspector, registry-fed pickers, inline validation
  in the inspector. The old horizontal rows die the moment the inspector exists.
- **P2 — Graph canvas**: read/select/layout + wire rendering + badges + add-window
  templates; wire DRAGGING for chain/retarget; node timing drag. Old phase strip demoted to
  the scrub bar only.
- **P3 — Sim-backed scrub**: ghost world stepped to t + event markers + hit/damage
  overlays; Play unchanged.
- **P4 — Rules integration + polish**: Cast-node costs/damage sections, effects mini-surface,
  undo for skill edits (snapshot the triad per gesture), keyboard (del = remove node/wire,
  ctrl-Z), empty-state hints ("press K", "drag from a port").

## 6. Open questions for Luke

1. D3/D2/D6/D7 are assumed — confirm or redirect.
2. Graph auto-layout only, or user-draggable node positions persisted (needs a sidecar file —
   `.cast.ron` stays layout-free)?
3. Scrub ghost: render as translucent duplicates of the real meshes, or schematic (wireframe
   spheres + trace lines)?
