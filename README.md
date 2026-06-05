# obelisk-bevy

A [Bevy](https://bevyengine.org/) plugin that exposes the [`obelisk`](../obelisk) ARPG
libraries (loot / stat / skill-tree / drop-table systems) to Bevy games, extended with a
**3D + temporal skill model**, **hit / hurt boxes**, **skill-usage primitives**, and
**VFX-sequencing hooks**.

obelisk provides the pure-Rust ARPG rules — skills, triggered effects, statuses/ailments,
damage resolution, stats. `obelisk-bevy` grafts an ECS + spatiotemporal + eventing layer
on top: a headless, deterministic, server-authoritative simulation that drives obelisk's
pipelines from Bevy schedules, plus a compile-outable presentation layer that consumes
gameplay events for VFX/audio/animation.

- **Bevy:** 0.17
- **Spatial backend:** Avian3d (sensors for hit/hurt detection, spatial queries for targeting)

## Status

Pre-implementation. See the design spec:

- [docs/superpowers/specs/2026-06-04-obelisk-bevy-plugin-design.md](docs/superpowers/specs/2026-06-04-obelisk-bevy-plugin-design.md)

## License

MIT
