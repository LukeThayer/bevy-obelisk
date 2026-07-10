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
    decay_surfaces, on_paint_surface, patch_contains, PaintSurface, SurfacePainted, SurfacePatch,
    SurfaceRemoveReason, SurfaceRemoved, SurfaceSeq,
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
            .init_resource::<SurfaceSeq>();
        app.init_resource::<systems::StandingState>();
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
