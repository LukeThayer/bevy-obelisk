//! Surface-type content schema + registry + directory loader. One TOML per surface, loaded by
//! `ObeliskConfigExt::add_obelisk_surfaces` (mirrors `add_obelisk_effects`, but registry-as-
//! Resource like `SkillRegistry` — no stat_core global, no double-init pain). Loader validation
//! fails LOUD (crate convention): unknown effect / skill refs reject the whole directory.
use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Vertical tolerance of the 2.5D overlap test: a point is "in" a patch when its XZ distance is
/// within the patch radius AND its Y is within this many units of the patch (spec §5.2).
pub const SURFACE_Y_TOLERANCE: f32 = 1.5;
/// Acquisition match slack: `on_surface` matches a patch when the aimed point lies within
/// `patch.radius + SURFACE_MATCH_SLACK` in XZ (today's arena `SPIRE_MATCH_RANGE` feel).
pub const SURFACE_MATCH_SLACK: f32 = 0.3;

/// One authored surface type (`config/surfaces/<id>.toml`). The `[visuals]` block is sim-inert
/// data for the host/editor renderer (like `CastTimeline::cues`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceType {
    pub id: String,
    /// Default patch lifetime (secs); a window's `PaintSpec.lifetime` may override per-paint.
    #[serde(default = "default_lifetime")]
    pub lifetime: f32,
    /// Dedup radius: a paint this close (XZ) to an existing same-type patch is skipped.
    #[serde(default = "default_merge_radius")]
    pub merge_radius: f32,
    /// Per-type cap; exceeding it despawns the OLDEST patch (replace-oldest, arena tile semantics).
    #[serde(default = "default_max_patches")]
    pub max_patches: usize,
    /// Default patch radius for paints that don't author one (the `PaintSurface` request path).
    #[serde(default = "default_patch_radius")]
    pub patch_radius: f32,
    /// Payload for entities standing INSIDE the surface (attributed to the painter).
    #[serde(default)]
    pub standing: Option<StandingPayload>,
    /// Reactions to a HITBOX touching this surface ("fire ignites oil").
    #[serde(default)]
    pub on_skill_contact: Vec<ContactReaction>,
    /// Host-rendered visuals (decal texture key, tint, looping vfx preset). Sim never reads it.
    #[serde(default)]
    pub visuals: Option<SurfaceVisuals>,
}

fn default_lifetime() -> f32 {
    180.0
}
fn default_merge_radius() -> f32 {
    0.25
}
fn default_max_patches() -> usize {
    64
}
fn default_patch_radius() -> f32 {
    0.45
}

/// Standing-payload faction filter, relative to the PAINTER. Maps onto [`crate::assets::HitFilter`]
/// (so `passes_filter` semantics apply exactly): `all` = everyone EXCEPT the painter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StandingFilter {
    Enemies,
    Allies,
    All,
}

impl StandingFilter {
    pub fn to_hit_filter(self) -> crate::assets::HitFilter {
        match self {
            StandingFilter::Enemies => crate::assets::HitFilter::Enemies,
            StandingFilter::Allies => crate::assets::HitFilter::Allies,
            StandingFilter::All => crate::assets::HitFilter::All,
        }
    }
}

/// What happens to entities standing in the surface. One clock (per victim × surface-type,
/// `rehit_interval`) drives BOTH `effect` application/refresh and `tick_skill` execution;
/// `on_enter_only = true` replaces the clock with an enter-edge firing (once per patch × victim
/// visit). NOTE (spec §5.2): the `effect` path applies via `apply_obelisk_effect`, which sources
/// the effect from the VICTIM itself — painter attribution (kill credit, damage-driven scaling)
/// only flows through `tick_skill`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandingPayload {
    #[serde(default = "default_standing_filter")]
    pub filter: StandingFilter,
    /// Obelisk effect id applied/refreshed while standing (statuses — chill, burn).
    #[serde(default)]
    pub effect: Option<String>,
    /// Triggered-only skill executed AT the victim on the clock (periodic damage via the full
    /// combat path — the `firebolt_explosion` pattern).
    #[serde(default)]
    pub tick_skill: Option<String>,
    #[serde(default = "default_rehit_interval")]
    pub rehit_interval: f32,
    #[serde(default)]
    pub on_enter_only: bool,
}

fn default_standing_filter() -> StandingFilter {
    StandingFilter::Enemies
}
fn default_rehit_interval() -> f32 {
    0.5
}

/// A reaction to a hitbox contacting this surface: if the contacting skill's rules `tags`
/// intersect `tags_any`, execute `trigger_skill` at the contact point (attributed to the
/// CONTACTING caster, spec D8), optionally consuming the touched patch.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactReaction {
    pub tags_any: Vec<String>,
    pub trigger_skill: String,
    #[serde(default)]
    pub consume: bool,
}

/// Sim-inert visual hints for the host renderer (decal + looping vfx per patch, spec §6/D10).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceVisuals {
    #[serde(default)]
    pub decal: Option<String>,
    #[serde(default)]
    pub color: Option<[f32; 4]>,
    #[serde(default)]
    pub vfx: Option<String>,
}

/// Registry of loaded surface types, keyed by id. Inserted by
/// `ObeliskConfigExt::add_obelisk_surfaces`; defaulted (empty) by `ObeliskSurfacesPlugin` so
/// surface systems are safe no-ops in apps that never load surface content.
#[derive(Resource, Default)]
pub struct SurfaceRegistry(pub HashMap<String, SurfaceType>);

/// Load every `*.toml` in `dir` into a surface-type map, validating:
/// - unique ids (and non-empty),
/// - numeric sanity (`lifetime > 0`, `merge_radius >= 0`, `max_patches >= 1`,
///   `patch_radius > 0`, `rehit_interval > 0`, non-empty `tags_any`),
/// - `standing.effect` exists in the stat_core effect registry (only checkable when the global
///   registry is initialized — callers should `add_obelisk_effects` first),
/// - `standing.tick_skill` + every `on_skill_contact.trigger_skill` exist in `skills` (only
///   checkable when `Some` — callers should `add_obelisk_skills` first). Timeline HANDLES are
///   deliberately NOT validated here (they load async after skills — same load-order reasoning
///   as `combat::system::partition_conditions`); `execute_skill_timeline` warns at runtime.
pub fn load_surfaces_dir(
    dir: &Path,
    skills: Option<&crate::core::config::SkillRegistry>,
) -> Result<HashMap<String, SurfaceType>, String> {
    let mut map: HashMap<String, SurfaceType> = HashMap::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {dir:?}: {e}"))?;
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    paths.sort(); // deterministic load + error order
    for path in paths {
        let src =
            std::fs::read_to_string(&path).map_err(|e| format!("read {path:?}: {e}"))?;
        let st: SurfaceType =
            toml::from_str(&src).map_err(|e| format!("parse {path:?}: {e}"))?;
        if st.id.is_empty() {
            return Err(format!("{path:?}: surface id must be non-empty"));
        }
        if st.lifetime <= 0.0 {
            return Err(format!("surface '{}': lifetime must be > 0", st.id));
        }
        if st.merge_radius < 0.0 {
            return Err(format!("surface '{}': merge_radius must be >= 0", st.id));
        }
        if st.max_patches == 0 {
            return Err(format!("surface '{}': max_patches must be >= 1", st.id));
        }
        if st.patch_radius <= 0.0 {
            return Err(format!("surface '{}': patch_radius must be > 0", st.id));
        }
        if let Some(standing) = &st.standing {
            if standing.rehit_interval <= 0.0 {
                return Err(format!("surface '{}': rehit_interval must be > 0", st.id));
            }
            if let Some(eff) = &standing.effect {
                if stat_core::config::effect_registry_initialized()
                    && stat_core::effect_registry().get(eff).is_none()
                {
                    return Err(format!(
                        "surface '{}': standing.effect '{eff}' is not a registered effect \
                         (no_such_effect check)",
                        st.id
                    ));
                }
            }
            if let (Some(ts), Some(reg)) = (&standing.tick_skill, skills) {
                if !reg.0.contains_key(ts) {
                    return Err(format!(
                        "surface '{}': standing.tick_skill '{ts}' is not a registered skill",
                        st.id
                    ));
                }
            }
        }
        for r in &st.on_skill_contact {
            if r.tags_any.is_empty() {
                return Err(format!(
                    "surface '{}': on_skill_contact.tags_any must be non-empty",
                    st.id
                ));
            }
            if let Some(reg) = skills {
                if !reg.0.contains_key(&r.trigger_skill) {
                    return Err(format!(
                        "surface '{}': on_skill_contact.trigger_skill '{}' is not a registered \
                         skill",
                        st.id, r.trigger_skill
                    ));
                }
            }
        }
        if map.insert(st.id.clone(), st).is_some() {
            return Err(format!("duplicate surface id in {dir:?}"));
        }
    }
    Ok(map)
}
