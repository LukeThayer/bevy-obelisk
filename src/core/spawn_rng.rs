use bevy::prelude::*;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// Dedicated RNG stream for emitter jitter sampling (Task 11, spec §3.2). A live hitbox's
/// emitter tick (`crate::timeline::advance::tick_emitters`) draws xz-disc jitter offsets from
/// THIS resource — never from [`crate::core::config::CombatRng`]. Keeping emission on its own
/// stream means casting (or not casting) an emitter skill can never perturb combat's RNG draw
/// sequence (crit rolls, damage variance, status-apply chance, ...) and vice versa — pinned by
/// `tests/emitters.rs::spawn_rng_does_not_perturb_combat_rng`.
#[derive(Resource)]
pub struct SpawnRng(pub ChaCha8Rng);

impl Default for SpawnRng {
    /// Deterministic seed-0 default, mirroring `CombatRng`'s — a consumer who forgets
    /// `App::seed_combat_rng` gets graceful (if not intentionally-seeded) behavior instead of a
    /// panic on a missing `Res`. Real content should always go through `seed_combat_rng`, which
    /// seeds this resource from `seed ^ 0x5EED_5EED` alongside `CombatRng`'s `seed`.
    fn default() -> Self {
        SpawnRng(ChaCha8Rng::seed_from_u64(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;

    #[test]
    fn default_is_deterministic_seed_zero() {
        let mut a = SpawnRng::default();
        let mut b = SpawnRng::default();
        // `r#gen` (final review, item 4): `gen` is reserved in edition 2024 / trips
        // rust-analyzer today — same rename as `timeline/advance.rs`'s emitter jitter draws.
        assert_eq!(a.0.r#gen::<u64>(), b.0.r#gen::<u64>());
    }
}
