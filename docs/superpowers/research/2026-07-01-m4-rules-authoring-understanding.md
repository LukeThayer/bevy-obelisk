## M4 — Full Rules Authoring: Understanding and Design Doc

### 0. What M4 is
The Skill Designer (M1–M3) authors a skill timeline + cosmetics (`assets/skills/<id>.cast.ron` = `CastTimeline`; `assets/skills/<id>.skillfx.ron` = `SkillFx`). M4 adds authoring of the obelisk RULES — the pure-Rust `stat_core::Skill` (and, if in scope, the `EffectConfig` it references) — which today live in hand-edited `config/skills/<id>.toml` and `config/effects/<id>.toml`. A castable skill is TWO files keyed by the same id. Authority split (obelisk-bevy/CLAUDE.md): the `.cast.ron` owns WHEN a hit fires (deterministic fixed-timestep timing); obelisk `Skill` owns WHAT it does (damage / effects / triggers / mana / cooldown / tags). obelisk has no timing fields; the timeline has no damage fields.

### 1. Serialization: TESTED, fully round-trippable, ZERO obelisk-core changes
This was the #1 open question in all four reader reports (serialize was never exercised in-tree — only `from_str`). I built a throwaway crate under the scratchpad that path-deps `stat_core`+`loot_core` and ran real round-trips (`toml::from_str::<Skill>` then `toml::to_string` then `toml::from_str`, asserting `==`):
- 5 real skill files round-trip with equality: `firebolt` (effect_applications + DamageDriven HashMap), `critzap` (a `[[conditions]]` block using `#[serde(flatten)]` + internally-tagged `TriggerCondition::OnCrit`), `basic_attack`, `discharge_strike` (consumes_self_effect), `static_discharge`.
- A maximally-populated constructed `Skill` round-trips: two flatten+tagged conditions (`OnCrit`, `TargetHasEffect{id}`), a `DamageDriven` HashMap, `ApplyChance::DamageScaled`, `ConsumeEffect{stacks:Some}`, `base_damages`, populated `damage.es_from_damage`/`amplifies_status` HashMaps.
- 6 real `EffectConfig` files round-trip with equality, including `charged`/`on_apply_proc` whose `[[conditions]]` blocks flatten the tagged `EffectTrigger` enum.

The `toml = "0.8"` serializer AUTO-REORDERS output so scalar values precede tables. Proof: in the emitted `[damage]` section, all scalars (`weapon_effectiveness`, ...) print first, THEN `[[damage.base_damages]]`, even though `base_damages: Vec<BaseDamage>` is declared FIRST in the struct (skill.rs:348). So the feared "values must be emitted before tables" error does NOT occur with toml 0.8. The flatten + internally-tagged enum concern is also unfounded here: `[[conditions]]` emits `trigger_skill`, `additional`, then `type = "on_crit"` at the same level, and re-parses cleanly.

The ONE real downside: NO type carries `#[serde(skip_serializing_if)]`, so every default field is written. `firebolt.toml` is 16 hand-authored lines but 63 lines when re-serialized (every DamageConfig scalar, `description = ""`, empty tables like `[damage.damage_conversions]`, and a full `[damage]` block even for a damage-less skill because `Skill.damage: DamageConfig` is not Optional). Effects are milder (8 to 18 lines). This is a cleanliness/diff-noise problem, not a correctness one — see design forks.

Confirmed absences: `grep -rc Reflect` over stat_core/src and loot_core/src returns nothing; `stat_core/Cargo.toml` deps are exactly serde, serde_json, toml, rand, thiserror, loot_core (NO bevy, NO bevy_reflect). So Reflect is absent, and adding it would pull bevy into a pure-Rust crate. The panel must be a hand-built serde-driven egui form (like M2's `draw_skill_panel`), NOT a Reflect inspector.

### 2. The `Skill` schema (the rules file) — `stat_core/src/skill.rs:463`
`#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] pub struct Skill` (no Eq — f64 fields; no Reflect). PartialEq gives a free dirty-check vs loaded state. Every field except `id`/`name` is `#[serde(default)]`, so minimal TOML is just id+name. Fields:
- Identity: `id`, `name`, `description`.
- Classification: `tags: Vec<SkillTag>`, `targeting: Targeting`, `delivery: Delivery`.
- Cost/timing: `mana_cost`, `cooldown` (f64), `attack_speed_modifier` (f64, default 1.0 via `default_speed_modifier`).
- Effects: `effect_applications: Vec<EffectApplication>`, `consumes_self_effect: Vec<ConsumeEffect>`, `consumes_target_effect: Vec<String>`.
- Conditions: `use_conditions: Vec<TriggerCondition>` (usability gate), `conditions: Vec<SkillCondition>` (trigger cascade).
- Passive/aura: `global_conditionals: Vec<GlobalConditional>`, `conditional_modifiers: Vec<ConditionalModifier>`.
- Misc: `grants_elude_stacks: u32`, `damage: DamageConfig`, `use_message`/`hint`/`hint_effect: Option<String>`.

`impl Default for Skill` (skill.rs:545) yields a basic_attack-shaped melee attack: `tags=[Attack]`, `targeting=SingleEnemy`, `delivery=Melee`, and crucially `damage.weapon_effectiveness = 1.0` (skill.rs:566) — this OVERRIDES `DamageConfig::default()` which sets 0.0 (skill.rs:424). So a fresh `Skill::default()` is a weapon-scaling attack; a spell needs weapon_effectiveness back to 0.0 with flat `base_damages`.

Cast type is NOT a single field. It is derived from tags x targeting x delivery:
- `SkillTag` (loot_core/types.rs:1201, snake_case, Copy): Attack, Spell, Physical, Fire, Cold, Lightning, Chaos, Elemental, Melee, Ranged, Projectile, Aoe. `is_attack()`/`is_spell()` test tag membership.
- `Targeting` (skill.rs:287, snake_case, Copy, Eq, Default=SingleEnemy, alias "self" -> SelfCast): SelfCast, SingleEnemy, None (passive).
- `Delivery` (skill.rs:301, snake_case, Copy, Eq, Default=Melee): Melee, Projectile, Instant.

### 3. Damage — `DamageConfig` (`stat_core/src/skill.rs:345`)
`base_damages: Vec<BaseDamage>` carries the damage. `BaseDamage` (damage/generator.rs:628): `{ #[serde(rename="type")] damage_type: DamageType, min: f64, max: f64 }`. `DamageType` (loot_core/types.rs:98, snake_case, Default=Physical): Physical, Fire, Cold, Lightning, Chaos. Other notable DamageConfig fields (all `#[serde(default)]`): `weapon_effectiveness`, `damage_effectiveness` (default 1.0), `base_crit_chance`, `crit_multiplier_bonus`, `crit_source: CritSource`, `guaranteed_crit`, `hits_per_attack` (default 1), chain/pierce fields, `status_only`, plus HashMap fields `es_from_damage`/`amplifies_status`/`bonus_damage_from_dot`. `damage_conversions: DamageConversions` (damage/generator.rs:254) derives only Debug/Clone/Default/PartialEq but has a MANUAL Serialize/Deserialize emitting flat `{from}_to_{to}` keys — round-trippable.

### 4. Effect application (Skill -> Effect linkage) — `stat_core/src/skill.rs:132`
`EffectApplication { effect_id: String, target: ApplicationTarget, scaling: ApplicationScaling, apply_chance: ApplyChance }` (all inner fields `#[serde(default)]`; the effect body is a SEPARATE file, referenced only by `effect_id` string):
- `ApplicationTarget` (skill.rs:62, snake_case, Copy+Eq, Default=Target, alias "self" -> Caster): Caster, Target.
- `ApplicationScaling` (skill.rs:91, snake_case, Default=Direct): Direct, or `DamageDriven { conversions: HashMap<DamageType, f64> }`.
- `ApplyChance` (skill.rs:112, snake_case, Default=Always): Always, or `DamageScaled { bonus: f64 }`.
`ConsumeEffect` (skill.rs:27): `{ id: String, stacks: Option<u32> }` (None = remove entirely).

### 5. The TWO trigger systems (the hard part)
There are two distinct condition layers M4 must expose:

(A) SKILL-level triggers -> `SkillCondition` (damage/triggers.rs:16): `{ trigger_skill: String, additional: bool (default false), #[serde(flatten)] condition: TriggerCondition }`. `additional=false` REPLACES the primary packet; true fires the trigger IN ADDITION. `TriggerCondition` (loot_core/types.rs:1268, `#[serde(tag="type", rename_all="snake_case")]`) is the big enum — 34 variants, grouped by pipeline phase:
- Unconditional: Always.
- PreCalculation: PlayerFullLife, PlayerLowLife{threshold=0.35}, TargetFullLife, TargetLowLife{threshold}, TargetHasEffect{id}, TargetEffectStacks{id,min_stacks}, SelfHasEffect{id}, EveryNthHit{n}, PlayerLowMana{threshold}, PlayerFullMana, Player/Target Has/No Barrier, SelfEffectStacks{id,min_stacks}, TargetNoEffect{id}.
- PostCalculation: OnCrit, DamageTypeDealt{damage_type}, OnNonCrit, DamageOverThreshold{threshold=0.0}, MultipleDamageTypes.
- PostResolution (attacker): OnKill, OnBarrierBroken, OnOverkill{threshold}.
- DefensiveResolution (defender): OnDamageTaken, OnDamageTakenOfType{damage_type}, OnEffectConsumed{id}, OnEffectChargeUsed{id}, OnDodge, OnEvasionCap, OnHitTaken, OnBarrierDepleted, OnLowLifeReached{threshold}.
Each variant has a `Display` impl (human strings like "On critical strike") and `phase()` classification (ConditionPhase, loot_core/types.rs:1369, NOT serde — a computed grouping, never authored). The loader ERRORS if a `SkillCondition.trigger_skill` is not a known skill id (config/skills.rs:77) — cross-file referential integrity the editor must validate.

(B) EFFECT-level triggers -> `EffectCondition` (types.rs:229): `{ trigger_skill: String, #[serde(flatten)] trigger: EffectTrigger }`. `EffectTrigger` (types.rs:248, tag="type", snake_case) has only 4 variants: `OnMaxStacks { consume: bool = true }`, OnExpire, OnConsume, OnApply. These live on the EFFECT, not the Skill (the Static-at-3-stacks -> discharge cascade). At runtime they produce transient `TriggeredEffect`s (types.rs:266, NOT serde — runtime only) that the sim resolves into damage packets; those runtime types are OUT of authoring scope.

### 6. Passive/aura modifiers (loot_core/src/types.rs)
`GlobalConditional` (types.rs:1568): `{ source_id, filter: SkillFilter, trigger_skill, additional (default true), condition: Option<TriggerCondition> }`. `ConditionalModifier` (types.rs:1623): `{ source_id, filter: SkillFilter, condition: TriggerCondition, stat: StatType, value: f64, is_more: bool, scaling: ScalingMode }`. `SkillFilter` (types.rs:1524, tag="type", Default=All): All, HasAnyTag{tags}, HasAllTags{tags}, SkillId{ids}. `ScalingMode` (types.rs:1599): Fixed, PerStack{effect_id}, PerDebuff. These reference `StatType` (see 8) and are the complex passive-skill surface — recommend deferring past v1.

### 7. Effects are a SEPARATE authoring surface — `EffectConfig` (`stat_core/src/config/effects.rs:169`)
Skills only carry `effect_id` strings; the buff/ailment/DoT bodies live in `config/effects/<id>.toml` as `EffectConfig` (NOT the runtime `Effect` at types.rs:26, whose `timers`/`stacks`/`stored_damage` are live combat state). `EffectConfig` fields: `id`, `name`, `duration: EffectDuration` (alias base_duration), `is_debuff`, `modifiers: Vec<EffectModConfig>`, `stacking: StackingBehavior`, `charges: Option<ChargeConfig>`, `damage_type: Option<DamageType>`, `base_damage_percent`, `tick_rate`, `max_stacks` (default 1), `application: StatusApplication`, `conditions: Vec<EffectCondition>`, `global_conditionals`, `conditional_modifiers`, `icon`/`banner`.
- `EffectDuration` (effects.rs:26): manual Serialize/Deserialize — `duration = 5.0` (Finite) or `duration = "infinite"` (Infinite).
- `StackingBehavior` (types.rs:201, snake_case, Default=Refresh): Refresh, StrongestOnly, Unlimited, Limited (Limited is the mode that fires OnMaxStacks).
- `EffectModConfig` (effects.rs:121): `{ stat: StatType, value: f64, is_more: bool }`.
- `StatusApplication` (effects.rs:94, tag="type", Default=Chance): Chance, Buildup{threshold}.
- `ChargeConfig` (effects.rs:143): `{ count: u32, consumption: ChargeConsumption }`.
- Loaded per-file by `load_effect_configs` (effects.rs:454) into a process-global `static EFFECT_REGISTRY: OnceLock<EffectRegistry>` (effects.rs:19) via `init_effect_registry` (effects.rs:428). See 10 for the reload consequence.

### 8. Enum pickers: what the editor gets for free vs must hand-build
`EnumVariants` trait (loot_core/types.rs:6: `all_variants()` + `variant_name()`) is impl'd on the UNIT-like enums M4 needs, giving ready-made dropdown lists — the same idiom M2 uses:
- SkillTag (loot_core:1222), DamageType (loot_core:109), CritSource (generator.rs:20).
- ApplicationTarget (skill.rs:78), Targeting (skill.rs:313), Delivery (skill.rs:326).
- StackingBehavior (types.rs:713), ChargeConsumption (types.rs:733), EffectTrigger (types.rs:746), StatusApplication (effects.rs:104).
NO EnumVariants (must hand-build index-to-variant pickers, like the existing `enum_ui.rs` idiom for f32-payload enums): `TriggerCondition` (34 variants, many carry threshold f64 / id String / min_stacks u32 / damage_type — the hardest picker), `ApplicationScaling`, `ApplyChance`, `ScalingMode`, `SkillFilter`.
`StatType` (loot_core/types.rs:454) is the worst: ~80 variants, some PARAMETERIZED (e.g. carrying String or DamageType pairs), and it derives NO serde — it has a MANUAL Serialize/Deserialize via `to_serde_string`/`from_serde_str` (a snake_case string vocabulary). It cannot be enumerated trivially. Needed only for effect `modifiers` and passive `conditional_modifiers`, so its cost is concentrated in the Effects/passives surface, not the core Skill surface.

### 9. Loaders and on-disk shape — `stat_core/src/config/skills.rs`
- `SkillsFileNew { #[serde(rename="skills")] skills: Vec<Skill> }` (skills.rs:15) = the `[[skills]]` array shape.
- `load_skills_dir(dir)` (skills.rs:37): per `.toml`, tries `SkillsFileNew` (array) first, ELSE parses the whole file as a SINGLE top-level `Skill` (skills.rs:51-58). This is why the real arena files (firebolt/basic_attack/pummel) are bare top-level skills. `toml::to_string(&skill)` emits exactly this bare-top-level shape, which the fallback parses. Recommend the editor write ONE bare top-level `Skill` per file.
- `parse_skills(content)` (skills.rs:67) wants a `[[skills]]` array. Public API (obelisk-bevy footgun): use `stat_core::config::{load_skills, load_skills_dir, parse_skills}` (the `skills` submodule is private). Do NOT use `default_skills()`/`load_skill_configs()` (legacy `DamagePacketGenerator`, still present at skills.rs:96 but not the target).
- Rules on disk are TOML (loaded via `toml::from_str`). To be consumed by the game/preview WITHOUT an obelisk change, M4 MUST emit TOML (RON is not an option for the rules file — the loader is TOML-only). This is the opposite of `.cast.ron`/`.skillfx.ron`, which use RON.

### 10. How M4 maps onto arena_editor, and the runtime load path
The arena game AND the editor preview load rules from the SAME dirs: `config/skills` + `config/effects` under the workspace root. Confirmed at `crates/arena_editor/src/sim_config.rs:21-22` (`add_obelisk_effects(root.join("config/effects"))`, `add_obelisk_skills(SkillSource::Dir(root.join("config/skills")))`), matching `arena_game/src/bin/server.rs:48-49` and `client/app_windowed.rs:67-68`. `io.rs:12 editor_root()` resolves that root; its doc comment (io.rs:4) explicitly names "config/ skill + effect rules". So M4 save paths are `editor_root()/config/skills/<id>.toml` and `.../config/effects/<id>.toml` — the exact files the preview reads. The current io.rs has `default_cast_path` (assets/skills/<id>.cast.ron) as the analogue; M4 adds `default_skill_rules_path`/`default_effect_path` under config/.

The load/reload asymmetry (a real M4 constraint): `add_obelisk_skills` inserts a normal Bevy `SkillRegistry(HashMap<String,Skill>)` Resource (obelisk-bevy core/config.rs:66) — re-injectable via a fresh `insert_resource`/`load_skills_dir`, so editing SKILL rules and re-previewing is feasible in-process. But EFFECTS live in the process-global `EFFECT_REGISTRY: OnceLock` (effects.rs:19); obelisk-bevy guards `init_effect_registry` behind `effect_registry_initialized()` (core/config.rs:56) precisely because a second init PANICS. So editing an EFFECT and re-previewing in the SAME process is BLOCKED by obelisk today — it needs an editor restart, an obelisk-core registry-swap API, or the editor bypassing the global. Skills reload; effects do not.

The existing Skill-mode panel (`panel.rs:56 draw_skill_panel`) is a single `egui::TopBottomPanel::bottom` with a header row (skill id + targeting/delivery ComboBoxes + Save), phase DragValues, a painted phase/window strip, and a hit-windows list; dispatched by `dispatch_custom_panel` while in Skill mode (EditorMode::Custom("skill")). Every edit flips `dirty`; Save writes the `.cast.ron` via RON. M4 extends this with a Rules surface (see ui_sketch).

### 11. Determinism constraint (unchanged by M4)
The editor authors DATA only; it must never resolve combat, roll RNG, or apply effects. Combat resolves inside obelisk's server-only `ObeliskSet::ResolveHits` on a seeded `CombatRng` (sim_config.rs seeds it). M4 verification is by the existing "Play the real skill" preview: re-load the edited `Skill` into `SkillRegistry`, cast it, and watch the real `DamageResolved`/`EffectApplied`/`TriggerFired` events — what you author is what the game plays.