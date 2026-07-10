//! Surfaces (ground effects): persistent, typed, paintable ground state — spec
//! `obelisk-arena/docs/superpowers/specs/2026-07-09-surfaces-ground-effects-design.md`.
//! A surface TYPE is content (`config/surfaces/<id>.toml` → [`SurfaceRegistry`], loaded via
//! `ObeliskConfigExt::add_obelisk_surfaces`); a surface PATCH is a sim entity (Task 2). Skills
//! interact through paint (window `paints`), require/consume (acquisition `on_surface`), and
//! react (the surface's own `standing` payload + `on_skill_contact` reactions). A surface never
//! deals damage itself — it applies effects or executes a triggered-only skill, so everything
//! rides the existing combat path.
pub mod types;

pub use types::{
    load_surfaces_dir, ContactReaction, StandingFilter, StandingPayload, SurfaceRegistry,
    SurfaceType, SurfaceVisuals, SURFACE_MATCH_SLACK, SURFACE_Y_TOLERANCE,
};
