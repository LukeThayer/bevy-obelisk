//! Surfaces (ground effects): persistent, typed, paintable ground state — spec
//! `obelisk-arena/docs/superpowers/specs/2026-07-09-surfaces-ground-effects-design.md`.
//! A surface TYPE is content (`config/surfaces/<id>.toml` → [`SurfaceRegistry`], loaded via
//! `ObeliskConfigExt::add_obelisk_surfaces`); a surface PATCH is a sim entity. Skills interact
//! through paint (window `paints`), require/consume (acquisition `on_surface`), and react (the
//! surface's own `standing` payload + `on_skill_contact` reactions). A surface never deals
//! damage itself — it applies effects or executes a triggered-only skill, so everything rides
//! the existing combat path.
//!
//! DATA vs BEHAVIOR split: `add_obelisk_surfaces` loads the registry (any host — including a
//! render-only client that just needs `[visuals]`); [`ObeliskSurfacesPlugin`] adds the systems
//! (sim hosts: server, editor preview) and is included in `ObeliskSimPlugin`.
use bevy::prelude::*;

pub mod patch;
pub mod systems;
pub mod types;

pub use patch::{
    clear_surface_tick_scratch, decay_surfaces, on_paint_surface, patch_contains, PaintSurface,
    SurfacePainted, SurfacePatch, SurfaceRemoveReason, SurfaceRemoved, SurfaceSeq,
    SurfaceTickScratch,
};
pub use systems::{
    apply_standing_payloads, on_hitbox_ended_paint, paint_surfaces, surface_contact_triggers,
    StandingState, SurfaceContacts, TrailPainted,
};
pub use types::{
    load_surfaces_dir, ContactReaction, StandingFilter, StandingPayload, SurfaceRegistry,
    SurfaceType, SurfaceVisuals, SURFACE_MATCH_SLACK, SURFACE_Y_TOLERANCE,
};

pub struct ObeliskSurfacesPlugin;
impl Plugin for ObeliskSurfacesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SurfaceRegistry>()
            .init_resource::<SurfaceSeq>()
            .init_resource::<SurfaceTickScratch>();
        app.init_resource::<systems::StandingState>();
        // NOTE (per-tick scratch clear): the shared [`SurfaceTickScratch`] is reset ONCE per sim
        // tick from INSIDE `apply_standing_payloads` (the last surfaces system in the tick — see
        // its end), NOT via a dedicated system. A standalone clear system — in ANY FixedUpdate set
        // — adds a node to the schedule graph, which perturbs Bevy's topological tie-break between
        // the deliberately-unordered `advance_casts` / `advance_triggered_execs` (see the Task-11
        // note in `lib.rs`) and shifts golden event ORDER (not behavior). Folding the clear into an
        // existing node keeps the graph — and every golden — byte-identical. See
        // [`clear_surface_tick_scratch`] for the timing/correctness argument, including the
        // caveat for hosts that gate these FixedUpdate sets off (e.g. the editor's frozen scrub).
        app.add_systems(
            bevy::app::FixedUpdate,
            decay_surfaces.in_set(crate::ObeliskSet::Advance),
        );
        app.add_systems(
            bevy::app::FixedUpdate,
            (
                paint_surfaces,
                systems::surface_contact_triggers,
                systems::apply_standing_payloads,
            )
                .chain()
                .in_set(crate::ObeliskSet::ResolveHits)
                .before(crate::spatial::detect::detect_overlaps),
        );
        app.add_observer(on_paint_surface);
        app.add_observer(on_hitbox_ended_paint);
    }
}
