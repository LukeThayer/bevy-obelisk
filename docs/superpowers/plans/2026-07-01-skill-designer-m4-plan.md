# Skill Designer M4 — Rules Authoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The skill designer authors the obelisk RULES files (`config/skills/<id>.toml` = `stat_core::Skill`, `config/effects/<id>.toml` = `stat_core::config::EffectConfig`) in-editor, with Save hot-reloading both into the running "Play the real skill" preview.

**Architecture:** Three staged landings on `arena_editor` (obelisk-arena repo), each independently verified: **Stage 1** = core Skill form on a new Rules tab + in-process `SkillRegistry` reload on Save + skill switcher/New-Skill seeds; **Stage 2** = trigger-cascade authoring (`SkillCondition` rows + the 34-variant `TriggerCondition` picker + referential-integrity validation); **Stage 3** = full effect-body authoring (`EffectConfig` form + searchable `StatType` picker) enabled by a small obelisk-core **registry-swap API** (the only obelisk change). TOML writes are **full-rewrite** via `toml::to_string` (user decision — verbose output accepted for v1; unedited struct fields round-trip because we serialize the whole loaded struct).

**Tech Stack:** Rust, Bevy 0.18, egui (`bevy_egui`), `toml` 0.8, `stat_core`/`loot_core` (obelisk), `arena_editor` (standalone workspace).

## Global Constraints

- **User decisions (2026-07-01, locked):** v1 scope = full effect authoring (Skill rules + trigger cascades + effect bodies); effect hot-reload via a stat_core registry-swap API; TOML saves = full-rewrite `toml::to_string`; build in 3 verified stages.
- **Explicitly deferred (NOT v1):** `global_conditionals` / `conditional_modifiers` (passive/aura surface) on both `Skill` and `EffectConfig` — preserved on load/save (full-rewrite serializes the loaded values), not editable. Also deferred: a `toml_edit` comment-preserving writer.
- **Build env (hard-won — violating these wastes hours):** nix is UNUSABLE on this mac — plain `cargo` only. `arena_editor` is its OWN standalone workspace: build/test via `cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo build/test`, NEVER `-p arena_editor` from the obelisk-arena root. Do NOT run `cargo update` there (pinned bevy-0.18 / bevy_egui `81904da` lock; adding new deps is fine — cargo extends the lock minimally). The full editor app CANNOT advance a frame headlessly — test logic on minimal `App`s or pure functions; never `build_editor_app().update()`. Cold arena_editor builds (~4 min) exceed the subagent 600 s watchdog — pre-warm `cargo build && cargo test --no-run` in the MAIN loop before dispatching task agents.
- **Hard gates (must stay green):** obelisk-bevy golden suite byte-identical (`cd /Users/luke/src/obelisk-bevy && cargo test --features test-support --test golden`, NO `UPDATE_GOLDEN`); arena_game net-test PASS (`cd /Users/luke/src/obelisk-arena && pkill -f arena-server; pkill -f arena-client; sleep 1; bash crates/arena_game/tools/net-test/run_session.sh`, flaky — retry ≤3×, one PASS = green); `arena_editor` suite green; obelisk (`/Users/luke/src/obelisk`) `cargo test` green after Task 9.
- **Determinism is sacred:** the editor authors DATA only — never resolves combat, rolls RNG, or applies effects. The swap API must not change any sim behavior (goldens prove it).
- **Git:** never push (all repos local-only). Branches: obelisk-arena `feat/skill-designer-m4`; obelisk `feat/effect-registry-swap` (Task 9 only). obelisk-bevy is untouched by M4. The 12 pre-existing `stat_core` dead-code warnings are ALLOWED.
- **Repo layout:** `arena_editor` sources at `/Users/luke/src/obelisk-arena/crates/arena_editor/src/`; obelisk at `/Users/luke/src/obelisk`. `editor_root()` (io.rs) resolves the obelisk-arena workspace root — the SAME `config/skills` + `config/effects` the game and the preview load.

---

## Verified API facts (read this before any task — saves re-derivation)

- `stat_core::Skill` derives `Serialize+Deserialize+PartialEq+Clone` (skill.rs:463); every field except `id`/`name` is `#[serde(default)]`. `Skill::default()` is a weapon-scaling ATTACK (`damage.weapon_effectiveness = 1.0` — skill.rs:566); a spell needs `weapon_effectiveness = 0.0` + flat `base_damages`.
- `toml::to_string(&skill)` emits the bare-top-level shape `load_skills_dir` accepts (it tries `[[skills]]` array first, falls back to single top-level `Skill` — config/skills.rs:51-58). Round-tripping was empirically validated on 5 real skills + 6 effects (research doc §1); toml 0.8 auto-reorders scalars before tables.
- `load_skills_dir` ERRORS if any `SkillCondition.trigger_skill` is not a known skill id (config/skills.rs:77-89) — the editor must validate refs before Save and reload defensively.
- Re-exports (exact paths used in code below): `stat_core::{Skill, Targeting, Delivery, DamageConfig, BaseDamage, ConsumeEffect, SkillCondition, TriggerCondition, EffectCondition, EffectTrigger}`; `stat_core::skill::{EffectApplication, ApplicationTarget, ApplicationScaling, ApplyChance}`; `stat_core::config::{EffectConfig, EffectModConfig, ChargeConfig, EffectRegistry, StatusApplication, load_effect_configs, load_skills_dir}`; `stat_core::config::effects::EffectDuration` (NOT re-exported from `config`); `stat_core::types::{StackingBehavior, ChargeConsumption}`; `loot_core::types::{EnumVariants, SkillTag}`; `loot_core::{DamageType, StatType}`; `obelisk_bevy::prelude::SkillRegistry` (a `Resource` newtype over `HashMap<String, Skill>`, field `.0` pub).
- `EnumVariants` (`all_variants() -> &'static [Self]` + `variant_name() -> &'static str`) is impl'd on: `SkillTag`, `DamageType`, `Targeting`, `Delivery`, `ApplicationTarget`, `StackingBehavior`, `ChargeConsumption`, `EffectTrigger`, `StatusApplication`, `CritSource`. NOT on `TriggerCondition`/`ApplicationScaling`/`ApplyChance`/`StatType`.
- `StatType::all_variants()` (loot_core types.rs:578) returns all ~80 non-parameterized variants; `StatType::all_variants_with_effects(&[String])` adds the parameterized expansions per effect id + all `ConvertDamage` combos. `StatType::to_serde_string()` gives the snake_case TOML vocabulary; `StatType` serializes as that string. **The stat picker is a searchable combo over `all_variants_with_effects` — no hand-built vocabulary.**
- `TriggerCondition` (loot_core types.rs:1270): 34 variants, `#[serde(tag="type", rename_all="snake_case")]`, has `Display` (human labels) + `Default` (= `Always`). Payloads are only: `threshold: f64`, `id: String`, `min_stacks: u32`, `n: u32`, `damage_type: DamageType`.
- `SkillCondition` (stat_core damage/triggers.rs): `{ trigger_skill: String, additional: bool (default false), #[serde(flatten)] condition: TriggerCondition }`, has `Default`. `EffectCondition`: `{ trigger_skill, #[serde(flatten)] trigger: EffectTrigger }`, has `Default`. `EffectTrigger`: `OnMaxStacks{consume: bool = true}`, `OnExpire`, `OnConsume`, `OnApply`.
- `EffectConfig` (stat_core config/effects.rs:170): NO `Default` derive — construct minimal instances via `toml::from_str::<EffectConfig>("id = \"x\"\nname = \"X\"")` (every other field is `#[serde(default)]`). `EffectModConfig` and `ChargeConfig` DO have `Default`.
- The effect registry global TODAY: `static EFFECT_REGISTRY: OnceLock<EffectRegistry>` (effects.rs:19) with `init_effect_registry(dir)` / `effect_registry() -> &'static EffectRegistry` / `effect_registry_initialized()` / `ensure_effect_registry_initialized()`. ~16 call sites, all through `effect_registry()`, several binding `let reg = effect_registry();` — **the swap API must keep the `-> &'static EffectRegistry` signature** (Task 9 does, via leak-on-swap).
- `PreviewSimConfigPlugin` (arena_editor sim_config.rs) loads constants + `config/effects` + `config/skills` + seeds RNG at plugin-build time; the preview casts through `SkillRegistry`, so replacing that resource re-rules the next cast. Effects resolve through the global registry, so swapping it re-effects the next cast.

---

# STAGE 1 — Skill rules authoring (Rules tab + Save→reload + skill switcher)

### Task 1: Branch, deps, and the `EditedRules` model

**Files:**
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/Cargo.toml`
- Create: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/rules_model.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/lib.rs` (add `pub mod rules_model;`)

**Interfaces:**
- Produces: `EditedRules { skill: stat_core::Skill, path: PathBuf, dirty: bool }` (Resource) with `EditedRules::from_skill(skill, path)`; `blank_attack_skill(id: &str, name: &str) -> Skill`; `blank_spell_skill(id: &str, name: &str) -> Skill`. Tasks 2–8 consume these.

- [ ] **Step 1: Create the branch (obelisk-arena repo — arena_editor lives inside it)**

```bash
cd /Users/luke/src/obelisk-arena && git checkout master && git checkout -b feat/skill-designer-m4
```

- [ ] **Step 2: Add the deps** — in `crates/arena_editor/Cargo.toml` `[dependencies]`, after the `ron = "0.8"` line add:

```toml
stat_core = { path = "../../../obelisk/stat_core" }
loot_core = { path = "../../../obelisk/loot_core" }
toml = "0.8"
```

(These unify with obelisk-bevy's transitive path deps — same canonical paths. Do NOT `cargo update`; a plain build extends the pinned lock minimally.)

- [ ] **Step 3: Write the failing test** — create `src/rules_model.rs`:

```rust
//! The rules-authoring model: the `EditedRules` resource (the in-flight `stat_core::Skill` the
//! designer is editing — the obelisk RULES side of the skill triad) plus the pure seeds for new
//! skills. `Skill::default()` is a weapon-scaling attack (skill.rs:566), so the attack seed is a
//! thin rename over it; the spell seed zeroes weapon scaling and carries flat base damage.

use bevy::prelude::*;
use loot_core::types::SkillTag;
use loot_core::DamageType;
use stat_core::{BaseDamage, DamageConfig, Delivery, Skill, Targeting};
use std::path::PathBuf;

/// The obelisk rules currently open in the designer: the `Skill`, the `config/skills/<id>.toml`
/// path it saves to, and whether it has unsaved edits. Edited alongside [`crate::model::EditedSkill`]
/// (the timeline) and [`crate::model::EditedSkillFx`] (cosmetics); Save writes all three files.
#[derive(Resource)]
pub struct EditedRules {
    pub skill: Skill,
    pub path: PathBuf,
    pub dirty: bool,
}

impl EditedRules {
    /// Open `skill` for editing, saving to `path`, with no unsaved edits.
    pub fn from_skill(skill: Skill, path: PathBuf) -> Self {
        Self { skill, path, dirty: false }
    }
}

/// A fresh weapon-scaling melee attack (the `Skill::default()` shape, renamed).
pub fn blank_attack_skill(id: &str, name: &str) -> Skill {
    Skill { id: id.into(), name: name.into(), ..Skill::default() }
}

/// A fresh flat-damage projectile spell: `weapon_effectiveness = 0.0` (the default overrides it to
/// 1.0 for attacks) + one fire base-damage row + spell tag + a small mana cost.
pub fn blank_spell_skill(id: &str, name: &str) -> Skill {
    Skill {
        id: id.into(),
        name: name.into(),
        tags: vec![SkillTag::Spell],
        targeting: Targeting::SingleEnemy,
        delivery: Delivery::Projectile,
        mana_cost: 5.0,
        damage: DamageConfig {
            weapon_effectiveness: 0.0,
            base_damages: vec![BaseDamage { damage_type: DamageType::Fire, min: 10.0, max: 15.0 }],
            ..DamageConfig::default()
        },
        ..Skill::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_attack_skill_is_a_weapon_scaling_attack() {
        let s = blank_attack_skill("slam", "Slam");
        assert_eq!(s.id, "slam");
        assert!((s.damage.weapon_effectiveness - 1.0).abs() < f64::EPSILON);
        assert!(s.tags.contains(&SkillTag::Attack));
    }

    #[test]
    fn blank_spell_skill_is_flat_damage_with_no_weapon_scaling() {
        let s = blank_spell_skill("icebolt", "Icebolt");
        assert!((s.damage.weapon_effectiveness - 0.0).abs() < f64::EPSILON);
        assert_eq!(s.damage.base_damages.len(), 1);
        assert!(s.tags.contains(&SkillTag::Spell));
        assert!((s.mana_cost - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn edited_rules_from_skill_starts_clean() {
        let r = EditedRules::from_skill(blank_attack_skill("x", "X"), PathBuf::from("/tmp/x.toml"));
        assert!(!r.dirty);
        assert_eq!(r.skill.id, "x");
    }
}
```

Add `pub mod rules_model;` to `src/lib.rs` alongside the existing `pub mod model;`.

- [ ] **Step 4: Run the tests**

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo test rules_model
```
Expected: 3 PASS (first run recompiles with the new deps — slow once).

- [ ] **Step 5: Commit**

```bash
cd /Users/luke/src/obelisk-arena && git add crates/arena_editor/Cargo.toml crates/arena_editor/Cargo.lock crates/arena_editor/src/rules_model.rs crates/arena_editor/src/lib.rs && git commit -m "feat(editor): EditedRules model + attack/spell skill seeds (M4 stage 1)"
```

---

### Task 2: Rules TOML io + in-process SkillRegistry reload

**Files:**
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/io.rs`

**Interfaces:**
- Consumes: `editor_root()` (existing).
- Produces: `default_rules_path(skill_id: &str) -> PathBuf`; `save_skill_rules(&Skill, &Path) -> std::io::Result<()>`; `load_skill_rules(&Path) -> Result<Skill, String>`; `list_skill_ids() -> Vec<String>`; `reload_skill_registry(&mut SkillRegistry) -> Result<usize, String>`. Tasks 5–8 consume these.

- [ ] **Step 1: Write the failing tests** — append to `src/io.rs` (create the `#[cfg(test)]` module; io.rs has none yet):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Full-rewrite round-trip against the REAL firebolt rules file (under cargo,
    /// `editor_root()` resolves the obelisk-arena workspace root where config/skills lives).
    #[test]
    fn skill_rules_round_trip_the_real_firebolt() {
        let loaded = load_skill_rules(&default_rules_path("firebolt")).expect("firebolt.toml parses");
        assert_eq!(loaded.id, "firebolt");
        let tmp = std::env::temp_dir().join("m4_io_test_firebolt.toml");
        save_skill_rules(&loaded, &tmp).expect("save");
        let reloaded = load_skill_rules(&tmp).expect("reparse");
        assert_eq!(loaded, reloaded, "full-rewrite serialize must round-trip losslessly");
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn list_skill_ids_contains_firebolt() {
        assert!(list_skill_ids().iter().any(|s| s == "firebolt"));
    }

    #[test]
    fn reload_skill_registry_loads_the_real_dir() {
        let mut reg = obelisk_bevy::prelude::SkillRegistry::default();
        let n = reload_skill_registry(&mut reg).expect("reload");
        assert!(n >= 1);
        assert!(reg.0.contains_key("firebolt"));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo test io::
```
Expected: FAIL — `load_skill_rules` etc. not found.

- [ ] **Step 3: Implement** — append to `src/io.rs` (add `use stat_core::Skill;` and `use obelisk_bevy::prelude::SkillRegistry;` to the imports):

```rust
/// The canonical obelisk rules path for a skill id, under the workspace `config/skills/`.
pub fn default_rules_path(skill_id: &str) -> PathBuf {
    editor_root().join(format!("config/skills/{skill_id}.toml"))
}

/// Serialize a `Skill` to `path` as TOML (full-rewrite: every default field is written — a
/// verbosity cost accepted for v1). Emits the bare top-level shape `load_skills_dir` accepts.
pub fn save_skill_rules(skill: &Skill, path: &Path) -> std::io::Result<()> {
    let s = toml::to_string(skill)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, s)
}

/// Parse a `Skill` from a rules TOML file, returning a human-readable error string on failure
/// (so callers can fall back to a blank seed).
pub fn load_skill_rules(path: &Path) -> Result<Skill, String> {
    let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str::<Skill>(&s).map_err(|e| e.to_string())
}

/// The skill ids on disk: `config/skills/*.toml` file stems, sorted. (Arena convention is one
/// bare top-level skill per file; a `[[skills]]` array file would list under its filename stem.)
pub fn list_skill_ids() -> Vec<String> {
    let dir = editor_root().join("config/skills");
    let mut ids: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "toml"))
                .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
}

/// Re-load `config/skills` into the live `SkillRegistry` resource so the "Play the real skill"
/// preview casts with the just-saved rules. Returns the number of skills loaded. On error the
/// registry is left UNCHANGED (the loader validates trigger_skill refs and can reject the dir).
pub fn reload_skill_registry(reg: &mut SkillRegistry) -> Result<usize, String> {
    let map = stat_core::config::load_skills_dir(&editor_root().join("config/skills"))
        .map_err(|e| e.to_string())?;
    let n = map.len();
    reg.0 = map;
    Ok(n)
}
```

- [ ] **Step 4: Run the tests**

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo test io::
```
Expected: 3 PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/luke/src/obelisk-arena && git add crates/arena_editor/src/io.rs && git commit -m "feat(editor): rules TOML io + SkillRegistry in-process reload (M4 stage 1)"
```

---

### Task 3: Panel tab strip (Timeline | Rules | Effects) + shell restructure

**Files:**
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/panel.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/skill_designer.rs`

**Interfaces:**
- Produces: `PanelTab` (Resource enum: `Timeline` default | `Rules` | `Effects`); `RulesStatus(pub String)` (Resource, panel status line); `draw_skill_panel` gains `ResMut<PanelTab>` + `ResMut<RulesStatus>` params. Tasks 5/8/12 add per-tab bodies.
- Consumes: existing `draw_skill_panel` body (becomes the Timeline tab).

- [ ] **Step 1: Restructure `panel.rs`.** Add at the top (after the existing consts):

```rust
/// Which authoring surface the bottom dock shows. Timeline = the M2/M3 phase strip + cosmetic
/// lanes; Rules = the obelisk Skill form (M4); Effects = the EffectConfig form (M4 stage 3).
#[derive(Resource, Default, PartialEq, Eq, Clone, Copy)]
pub enum PanelTab {
    #[default]
    Timeline,
    Rules,
    Effects,
}

/// One-line save/reload status shown in the panel header (e.g. "saved; 3 skills reloaded",
/// "rules save blocked: unknown trigger_skill 'x'").
#[derive(Resource, Default)]
pub struct RulesStatus(pub String);
```

Change the `draw_skill_panel` signature to:

```rust
pub fn draw_skill_panel(
    mut contexts: EguiContexts,
    mut edited: ResMut<EditedSkill>,
    mut edited_fx: ResMut<EditedSkillFx>,
    mut tab: ResMut<PanelTab>,
    status: Res<RulesStatus>,
    playhead: Res<Playhead>,
) {
```

Inside the `TopBottomPanel::bottom(...).show(ctx, |ui| { ... })` closure, FIRST draw the tab strip + shared header, then dispatch:

```rust
ui.horizontal(|ui| {
    ui.label(egui::RichText::new(&edited.timeline.skill_id).strong());
    ui.selectable_value(&mut *tab, PanelTab::Timeline, "Timeline");
    ui.selectable_value(&mut *tab, PanelTab::Rules, "Rules");
    ui.selectable_value(&mut *tab, PanelTab::Effects, "Effects");
    if ui.button("Save").clicked() {
        save_clicked = true;
    }
    if !status.0.is_empty() {
        ui.label(egui::RichText::new(&status.0).small().weak());
    }
});
match *tab {
    PanelTab::Timeline => {
        let (c, fc) = draw_timeline_tab(ui, &mut edited, &mut edited_fx, &playhead);
        changed |= c;
        fx_changed |= fc;
    }
    PanelTab::Rules => {
        ui.label("Rules authoring lands in Task 5.");
    }
    PanelTab::Effects => {
        ui.label("Effect authoring lands in Stage 3.");
    }
}
```

Move the ENTIRE existing panel body (the old header's targeting/delivery combos + phase DragValues + painted strip + hit-windows list + cosmetic lanes — everything except the skill-id label and Save button, which moved to the shared header) into a new private function in the same file, preserving the code verbatim:

```rust
/// The M2/M3 timeline surface: targeting/delivery combos, phase DragValues, the painted
/// phase/window strip + playhead, the hit-windows list, and the cosmetic lanes.
/// Returns (timeline_changed, fx_changed).
fn draw_timeline_tab(
    ui: &mut egui::Ui,
    edited: &mut EditedSkill,
    edited_fx: &mut EditedSkillFx,
    playhead: &Playhead,
) -> (bool, bool) {
    let mut changed = false;
    let mut fx_changed = false;
    // ... existing body, verbatim, with `ui` already provided (no TopBottomPanel here) ...
    (changed, fx_changed)
}
```

The trailing save block stays in `draw_skill_panel` unchanged for now (Task 5 extends it).

- [ ] **Step 2: Register the new resources** — in `skill_designer.rs` `SkillDesignerPlugin::build`, after `register_skill_mode(app);` add:

```rust
app.init_resource::<crate::panel::PanelTab>();
app.init_resource::<crate::panel::RulesStatus>();
```

- [ ] **Step 3: Compile-gate + suite**

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo build && cargo test
```
Expected: build clean; all existing tests PASS (the boot/registration tests don't render the panel — egui systems are only exercised windowed).

- [ ] **Step 4: Commit**

```bash
cd /Users/luke/src/obelisk-arena && git add crates/arena_editor/src/panel.rs crates/arena_editor/src/skill_designer.rs && git commit -m "feat(editor): tab strip (Timeline|Rules|Effects) + panel shell restructure (M4 stage 1)"
```

---

### Task 4: Rules form — pure row-edit helpers

**Files:**
- Create: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/rules_edits.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/lib.rs` (add `pub mod rules_edits;`)

**Interfaces:**
- Produces: `toggle_tag(&mut Skill, SkillTag)`; `add_base_damage(&mut Skill)`; `remove_base_damage(&mut Skill, usize)`; `add_effect_application(&mut Skill)`; `remove_effect_application(&mut Skill, usize)`; `set_opt_text(&mut Option<String>, String)`. Task 5's egui form consumes these (keeps the panel an egui shell over tested helpers, the M2 idiom).

- [ ] **Step 1: Write the failing tests** — create `src/rules_edits.rs`:

```rust
//! Pure row-edit helpers for the Rules form (the egui panel stays a thin shell, the M2 idiom).

use loot_core::types::SkillTag;
use loot_core::DamageType;
use stat_core::skill::{ApplicationScaling, ApplicationTarget, ApplyChance, EffectApplication};
use stat_core::{BaseDamage, Skill};

/// Add `tag` if absent, remove it if present.
pub fn toggle_tag(skill: &mut Skill, tag: SkillTag) {
    if skill.tags.contains(&tag) {
        skill.tags.retain(|t| *t != tag);
    } else {
        skill.tags.push(tag);
    }
}

/// Append a default physical base-damage row.
pub fn add_base_damage(skill: &mut Skill) {
    skill.damage.base_damages.push(BaseDamage {
        damage_type: DamageType::Physical,
        min: 1.0,
        max: 2.0,
    });
}

pub fn remove_base_damage(skill: &mut Skill, idx: usize) {
    if idx < skill.damage.base_damages.len() {
        skill.damage.base_damages.remove(idx);
    }
}

/// Append a blank effect application (target-directed, direct scaling, always applies).
pub fn add_effect_application(skill: &mut Skill) {
    skill.effect_applications.push(EffectApplication {
        effect_id: String::new(),
        target: ApplicationTarget::Target,
        scaling: ApplicationScaling::Direct,
        apply_chance: ApplyChance::Always,
    });
}

pub fn remove_effect_application(skill: &mut Skill, idx: usize) {
    if idx < skill.effect_applications.len() {
        skill.effect_applications.remove(idx);
    }
}

/// Empty text ⇒ `None` (the Option<String> fields: use_message / hint / hint_effect).
pub fn set_opt_text(slot: &mut Option<String>, text: String) {
    *slot = if text.is_empty() { None } else { Some(text) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules_model::blank_attack_skill;

    #[test]
    fn toggle_tag_adds_then_removes() {
        let mut s = blank_attack_skill("x", "X");
        assert!(!s.tags.contains(&SkillTag::Fire));
        toggle_tag(&mut s, SkillTag::Fire);
        assert!(s.tags.contains(&SkillTag::Fire));
        toggle_tag(&mut s, SkillTag::Fire);
        assert!(!s.tags.contains(&SkillTag::Fire));
    }

    #[test]
    fn base_damage_rows_add_and_remove() {
        let mut s = blank_attack_skill("x", "X");
        add_base_damage(&mut s);
        add_base_damage(&mut s);
        assert_eq!(s.damage.base_damages.len(), 2);
        remove_base_damage(&mut s, 0);
        assert_eq!(s.damage.base_damages.len(), 1);
        remove_base_damage(&mut s, 5); // out of range is a no-op
        assert_eq!(s.damage.base_damages.len(), 1);
    }

    #[test]
    fn effect_application_rows_add_and_remove() {
        let mut s = blank_attack_skill("x", "X");
        add_effect_application(&mut s);
        assert_eq!(s.effect_applications.len(), 1);
        assert!(matches!(s.effect_applications[0].apply_chance, ApplyChance::Always));
        remove_effect_application(&mut s, 0);
        assert!(s.effect_applications.is_empty());
    }

    #[test]
    fn set_opt_text_maps_empty_to_none() {
        let mut slot = Some("hi".to_string());
        set_opt_text(&mut slot, String::new());
        assert!(slot.is_none());
        set_opt_text(&mut slot, "msg".into());
        assert_eq!(slot.as_deref(), Some("msg"));
    }
}
```

- [ ] **Step 2: Run to verify they fail, then pass** (the module IS the implementation — the failure mode is "module not in lib.rs"; add `pub mod rules_edits;` and re-run):

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo test rules_edits
```
Expected: 4 PASS.

- [ ] **Step 3: Commit**

```bash
cd /Users/luke/src/obelisk-arena && git add crates/arena_editor/src/rules_edits.rs crates/arena_editor/src/lib.rs && git commit -m "feat(editor): pure rules-form row helpers (M4 stage 1)"
```

---

### Task 5: Rules tab egui form + Save→write TOML→reload SkillRegistry

**Files:**
- Create: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/rules_panel.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/panel.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/skill_designer.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/lib.rs` (add `pub mod rules_panel;`)

**Interfaces:**
- Consumes: `EditedRules` (Task 1), io fns (Task 2), row helpers (Task 4), `PanelTab`/`RulesStatus` (Task 3).
- Produces: `rules_panel::draw_rules_tab(ui: &mut egui::Ui, skill: &mut Skill, effect_ids: &[String]) -> bool` (true = changed). Task 8 extends this file with the conditions section.

- [ ] **Step 1: Implement the form** — create `src/rules_panel.rs`:

```rust
//! The Rules tab: a hand-built serde-driven egui form over `stat_core::Skill` (stat_core has no
//! Reflect — by design; see the M4 research doc §1). An egui shell over `rules_edits`; compile
//! gate + windowed use, like `panel.rs`.

use bevy_egui::egui;
use loot_core::types::{EnumVariants, SkillTag};
use loot_core::DamageType;
use stat_core::skill::{ApplicationScaling, ApplicationTarget, ApplyChance};
use stat_core::{Delivery, Skill, Targeting};
use std::collections::HashMap;

use crate::rules_edits::{
    add_base_damage, add_effect_application, remove_base_damage, remove_effect_application,
    set_opt_text, toggle_tag,
};

/// Draw the Rules form. `effect_ids` populates the effect-application picker (from the live
/// obelisk effect registry). Returns true if any field changed.
pub fn draw_rules_tab(ui: &mut egui::Ui, skill: &mut Skill, effect_ids: &[String]) -> bool {
    let mut changed = false;
    egui::ScrollArea::vertical().show(ui, |ui| {
        // === Identity ===
        ui.horizontal(|ui| {
            ui.label("name");
            changed |= ui.text_edit_singleline(&mut skill.name).changed();
            ui.label("description");
            changed |= ui.text_edit_singleline(&mut skill.description).changed();
        });

        // === Tags ===
        ui.horizontal_wrapped(|ui| {
            ui.label("tags");
            for tag in SkillTag::all_variants() {
                let mut has = skill.tags.contains(tag);
                if ui.checkbox(&mut has, tag.variant_name()).changed() {
                    toggle_tag(skill, *tag);
                    changed = true;
                }
            }
        });

        // === Targeting / delivery / cost ===
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("rules_targeting")
                .selected_text(skill.targeting.variant_name())
                .show_ui(ui, |ui| {
                    for v in Targeting::all_variants() {
                        if ui.selectable_value(&mut skill.targeting, *v, v.variant_name()).clicked() {
                            changed = true;
                        }
                    }
                });
            egui::ComboBox::from_id_salt("rules_delivery")
                .selected_text(skill.delivery.variant_name())
                .show_ui(ui, |ui| {
                    for v in Delivery::all_variants() {
                        if ui.selectable_value(&mut skill.delivery, *v, v.variant_name()).clicked() {
                            changed = true;
                        }
                    }
                });
            for (lab, val) in [
                ("mana", &mut skill.mana_cost),
                ("cooldown", &mut skill.cooldown),
                ("speed×", &mut skill.attack_speed_modifier),
            ] {
                ui.label(lab);
                changed |= ui
                    .add(egui::DragValue::new(val).speed(0.1).range(0.0..=1000.0))
                    .changed();
            }
            ui.label("elude");
            changed |= ui
                .add(egui::DragValue::new(&mut skill.grants_elude_stacks).range(0..=20))
                .changed();
        });

        // === UI-hint strings (empty ⇒ None) ===
        ui.horizontal(|ui| {
            for (lab, slot) in [
                ("use_message", &mut skill.use_message),
                ("hint", &mut skill.hint),
                ("hint_effect", &mut skill.hint_effect),
            ] {
                ui.label(lab);
                let mut text = slot.clone().unwrap_or_default();
                if ui.add(egui::TextEdit::singleline(&mut text).desired_width(110.0)).changed() {
                    set_opt_text(slot, text);
                    changed = true;
                }
            }
        });

        ui.separator();

        // === Damage ===
        ui.label(egui::RichText::new("Damage").strong());
        ui.horizontal(|ui| {
            for (lab, val) in [
                ("weapon eff", &mut skill.damage.weapon_effectiveness),
                ("damage eff", &mut skill.damage.damage_effectiveness),
                ("crit %", &mut skill.damage.base_crit_chance),
                ("crit multi+", &mut skill.damage.crit_multiplier_bonus),
            ] {
                ui.label(lab);
                changed |= ui.add(egui::DragValue::new(val).speed(0.05)).changed();
            }
            changed |= ui.checkbox(&mut skill.damage.guaranteed_crit, "guaranteed crit").changed();
            ui.label("hits");
            changed |= ui
                .add(egui::DragValue::new(&mut skill.damage.hits_per_attack).range(1..=20))
                .changed();
        });
        ui.horizontal(|ui| {
            ui.label("base damages");
            if ui.button("+ add").clicked() {
                add_base_damage(skill);
                changed = true;
            }
        });
        let mut remove_bd: Option<usize> = None;
        for (i, bd) in skill.damage.base_damages.iter_mut().enumerate() {
            ui.push_id(("bd", i), |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("bd_type")
                        .selected_text(format!("{:?}", bd.damage_type))
                        .show_ui(ui, |ui| {
                            for v in DamageType::all_variants() {
                                if ui
                                    .selectable_value(&mut bd.damage_type, *v, format!("{v:?}"))
                                    .clicked()
                                {
                                    changed = true;
                                }
                            }
                        });
                    ui.label("min");
                    changed |= ui.add(egui::DragValue::new(&mut bd.min).speed(0.5)).changed();
                    ui.label("max");
                    changed |= ui.add(egui::DragValue::new(&mut bd.max).speed(0.5)).changed();
                    if ui.button("✕").clicked() {
                        remove_bd = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove_bd {
            remove_base_damage(skill, i);
            changed = true;
        }

        ui.separator();

        // === Effect applications ===
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Applies Effects").strong());
            if ui.button("+ add").clicked() {
                add_effect_application(skill);
                changed = true;
            }
        });
        let mut remove_ea: Option<usize> = None;
        for (i, ea) in skill.effect_applications.iter_mut().enumerate() {
            ui.push_id(("ea", i), |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("ea_effect")
                        .selected_text(if ea.effect_id.is_empty() {
                            "(pick effect)".to_string()
                        } else {
                            ea.effect_id.clone()
                        })
                        .show_ui(ui, |ui| {
                            for id in effect_ids {
                                if ui.selectable_value(&mut ea.effect_id, id.clone(), id).clicked() {
                                    changed = true;
                                }
                            }
                        });
                    egui::ComboBox::from_id_salt("ea_target")
                        .selected_text(ea.target.variant_name())
                        .show_ui(ui, |ui| {
                            for v in ApplicationTarget::all_variants() {
                                if ui.selectable_value(&mut ea.target, *v, v.variant_name()).clicked()
                                {
                                    changed = true;
                                }
                            }
                        });
                    let mut driven = matches!(ea.scaling, ApplicationScaling::DamageDriven { .. });
                    if ui.checkbox(&mut driven, "damage-driven").changed() {
                        ea.scaling = if driven {
                            ApplicationScaling::DamageDriven {
                                conversions: HashMap::from([(DamageType::Fire, 0.5)]),
                            }
                        } else {
                            ApplicationScaling::Direct
                        };
                        changed = true;
                    }
                    let mut scaled = matches!(ea.apply_chance, ApplyChance::DamageScaled { .. });
                    if ui.checkbox(&mut scaled, "damage-scaled chance").changed() {
                        ea.apply_chance = if scaled {
                            ApplyChance::DamageScaled { bonus: 0.0 }
                        } else {
                            ApplyChance::Always
                        };
                        changed = true;
                    }
                    if let ApplyChance::DamageScaled { bonus } = &mut ea.apply_chance {
                        ui.label("bonus");
                        changed |= ui.add(egui::DragValue::new(bonus).speed(0.05)).changed();
                    }
                    if ui.button("✕").clicked() {
                        remove_ea = Some(i);
                    }
                });
                if let ApplicationScaling::DamageDriven { conversions } = &mut ea.scaling {
                    ui.horizontal(|ui| {
                        ui.label("   conversions");
                        for dt in DamageType::all_variants() {
                            let mut v = conversions.get(dt).copied().unwrap_or(0.0);
                            ui.label(format!("{dt:?}"));
                            if ui
                                .add(egui::DragValue::new(&mut v).speed(0.05).range(0.0..=1.0))
                                .changed()
                            {
                                if v > 0.0 {
                                    conversions.insert(*dt, v);
                                } else {
                                    conversions.remove(dt);
                                }
                                changed = true;
                            }
                        }
                    });
                }
            });
        }
        if let Some(i) = remove_ea {
            remove_effect_application(skill, i);
            changed = true;
        }
    });
    changed
}
```

- [ ] **Step 2: Wire the tab + Save.** In `panel.rs`, extend the `draw_skill_panel` params:

```rust
    mut rules: ResMut<EditedRules>,
    registry: Option<ResMut<SkillRegistry>>,
    mut status: ResMut<RulesStatus>,   // was Res — Save writes it now
```

with imports `use crate::model::…` extended by `use crate::rules_model::EditedRules; use obelisk_bevy::prelude::SkillRegistry;`. Replace the Rules placeholder arm:

```rust
    PanelTab::Rules => {
        let ids = effect_id_list();
        if crate::rules_panel::draw_rules_tab(ui, &mut rules.skill, &ids) {
            rules.dirty = true;
        }
    }
```

with a module-level helper in `panel.rs`:

```rust
/// Effect ids for the pickers, from the live obelisk registry (empty if uninitialized —
/// minimal test apps don't init it; the windowed editor does via PreviewSimConfigPlugin).
fn effect_id_list() -> Vec<String> {
    if stat_core::config::effect_registry_initialized() {
        let mut ids: Vec<String> = stat_core::config::effect_registry()
            .all_ids()
            .into_iter()
            .map(str::to_owned)
            .collect();
        ids.sort();
        ids
    } else {
        Vec::new()
    }
}
```

Extend the save block (after the existing `.cast.ron` + `.skillfx.ron` writes):

```rust
    if save_clicked {
        // ... existing cast + skillfx writes ...
        match crate::io::save_skill_rules(&rules.skill, &rules.path) {
            Ok(()) => {
                rules.dirty = false;
                if let Some(mut reg) = registry {
                    match crate::io::reload_skill_registry(&mut reg) {
                        Ok(n) => status.0 = format!("saved; {n} skills reloaded"),
                        Err(e) => status.0 = format!("saved, but skill reload failed: {e}"),
                    }
                } else {
                    status.0 = "saved (no SkillRegistry to reload)".into();
                }
            }
            Err(e) => status.0 = format!("rules save failed: {e}"),
        }
    }
```

- [ ] **Step 3: Seed `EditedRules` at startup.** In `skill_designer.rs` `SkillDesignerPlugin::build`, after the `EditedSkillFx` insertion:

```rust
        // Load firebolt's obelisk rules if they parse, else a blank attack seed at the canonical
        // path (load-or-blank) — the rules side of the skill triad.
        let rules_path = crate::io::default_rules_path("firebolt");
        let skill = crate::io::load_skill_rules(&rules_path)
            .unwrap_or_else(|_| crate::rules_model::blank_attack_skill("firebolt", "Firebolt"));
        app.insert_resource(crate::rules_model::EditedRules::from_skill(skill, rules_path));
```

Add a test to `skill_designer.rs`'s test module pinning the seed:

```rust
    #[test]
    fn plugin_seeds_edited_rules_with_the_real_firebolt() {
        let skill = crate::io::load_skill_rules(&crate::io::default_rules_path("firebolt"))
            .expect("real firebolt.toml parses");
        assert_eq!(skill.id, "firebolt");
        assert!((skill.mana_cost - 5.0).abs() < f64::EPSILON);
    }
```

- [ ] **Step 4: Build + suite**

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo build && cargo test
```
Expected: clean build, all tests PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/luke/src/obelisk-arena && git add crates/arena_editor/src/rules_panel.rs crates/arena_editor/src/panel.rs crates/arena_editor/src/skill_designer.rs crates/arena_editor/src/lib.rs && git commit -m "feat(editor): Rules tab form + Save writes rules TOML + hot-reloads SkillRegistry (M4 stage 1)"
```

---

### Task 6: Skill switcher + New Skill (attack/spell seeds)

**Files:**
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/rules_model.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/panel.rs`

**Interfaces:**
- Produces: `rules_model::open_skill(id: &str) -> (EditedSkill, EditedSkillFx, EditedRules)` — load-or-blank for all three files of the skill triad. The panel header gains a skill picker + New-Attack/New-Spell buttons.
- Consumes: io load/blank fns (Tasks 1–2, existing model fns).

- [ ] **Step 1: Write the failing test** — append to `rules_model.rs` tests:

```rust
    #[test]
    fn open_skill_loads_the_real_firebolt_triple() {
        let (cast, fx, rules) = open_skill("firebolt");
        assert_eq!(cast.timeline.skill_id, "firebolt");
        assert_eq!(fx.fx.skill_id, "firebolt");
        assert_eq!(rules.skill.id, "firebolt");
        assert!(!cast.dirty && !fx.dirty && !rules.dirty);
    }

    #[test]
    fn open_skill_falls_back_to_blanks_for_an_unknown_id() {
        let (cast, _fx, rules) = open_skill("no_such_skill_zzz");
        assert_eq!(cast.timeline.skill_id, "no_such_skill_zzz");
        assert_eq!(rules.skill.id, "no_such_skill_zzz");
    }
```

- [ ] **Step 2: Run to verify FAIL** (`open_skill` undefined), then implement in `rules_model.rs`:

```rust
use crate::io::{
    default_cast_path, default_rules_path, default_skillfx_path, load_cast_timeline,
    load_skill_rules, load_skillfx,
};
use crate::model::{blank_cast_timeline, blank_skillfx, EditedSkill, EditedSkillFx};

/// Open all three files of a skill's authoring triad (timeline / cosmetics / rules),
/// falling back to blank seeds for any that are missing or unparsable (load-or-blank,
/// the same policy the plugin uses at startup).
pub fn open_skill(id: &str) -> (EditedSkill, EditedSkillFx, EditedRules) {
    let cast_path = default_cast_path(id);
    let timeline = load_cast_timeline(&cast_path).unwrap_or_else(|_| blank_cast_timeline(id));
    let fx_path = default_skillfx_path(id);
    let fx = load_skillfx(&fx_path).unwrap_or_else(|_| blank_skillfx(id));
    let rules_path = default_rules_path(id);
    let skill = load_skill_rules(&rules_path).unwrap_or_else(|_| blank_attack_skill(id, id));
    (
        EditedSkill::from_timeline(timeline, cast_path),
        EditedSkillFx::from_fx(fx, fx_path),
        EditedRules::from_skill(skill, rules_path),
    )
}
```

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo test rules_model
```
Expected: PASS.

- [ ] **Step 3: Panel header switcher.** In `draw_skill_panel`, add a `mut new_id: Local<String>` param, and extend the shared header row (after the skill-id label):

```rust
    let mut switch_to: Option<(EditedSkill, EditedSkillFx, EditedRules)> = None;
    egui::ComboBox::from_id_salt("skill_picker")
        .selected_text("open…")
        .show_ui(ui, |ui| {
            for id in crate::io::list_skill_ids() {
                if ui.selectable_label(edited.timeline.skill_id == id, &id).clicked() {
                    switch_to = Some(crate::rules_model::open_skill(&id));
                }
            }
        });
    ui.add(egui::TextEdit::singleline(&mut *new_id).hint_text("new id").desired_width(80.0));
    for (lab, seed) in [
        ("+Attack", crate::rules_model::blank_attack_skill as fn(&str, &str) -> stat_core::Skill),
        ("+Spell", crate::rules_model::blank_spell_skill as fn(&str, &str) -> stat_core::Skill),
    ] {
        if ui.button(lab).clicked() && !new_id.is_empty() {
            let id = new_id.clone();
            let (mut c, mut f, mut r) = crate::rules_model::open_skill(&id);
            r.skill = seed(&id, &id);
            c.dirty = true;
            f.dirty = true;
            r.dirty = true;
            switch_to = Some((c, f, r));
            new_id.clear();
        }
    }
```

and after the `.show(ctx, …)` call (still before the save block), apply the switch:

```rust
    if let Some((c, f, r)) = switch_to {
        *edited = c;
        *edited_fx = f;
        *rules = r;
        status.0 = format!("opened {}", edited.timeline.skill_id);
    }
```

- [ ] **Step 4: Build + suite + STAGE 1 GATE**

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo build && cargo test
cd /Users/luke/src/obelisk-bevy && cargo test --features test-support --test golden
cd /Users/luke/src/obelisk-arena && pkill -f arena-server; pkill -f arena-client; sleep 1; bash crates/arena_game/tools/net-test/run_session.sh
```
Expected: arena_editor green; goldens byte-identical (nothing in obelisk-land was touched — this is the cheap insurance run); net-test `session PASS` (retry ≤3×).

**Manual QA (windowed, main loop):** `cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo run --bin arena-editor`, press `K`, click the Rules tab: firebolt's real rules populate the form. Change `mana_cost`, Save, press F4 (Play): the preview cast uses the new cost (status line shows "saved; N skills reloaded").

- [ ] **Step 5: Commit**

```bash
cd /Users/luke/src/obelisk-arena && git add crates/arena_editor/src/rules_model.rs crates/arena_editor/src/panel.rs && git commit -m "feat(editor): skill switcher + New Attack/Spell seeds (M4 stage 1 complete)"
```

---

# STAGE 2 — Trigger cascades (SkillCondition + TriggerCondition picker)

### Task 7: The 34-variant TriggerCondition catalog (pure)

**Files:**
- Create: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/trigger_ui.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/lib.rs` (add `pub mod trigger_ui;`)

**Interfaces:**
- Produces: `trigger_prototypes() -> Vec<(&'static str, TriggerCondition)>` (group label + a default-payload prototype for every variant, in pipeline-phase order); `trigger_index(&TriggerCondition) -> usize` (discriminant match into that list); `invalid_trigger_refs(&Skill, &HashSet<String>) -> Vec<String>`. Task 8's picker consumes all three.

- [ ] **Step 1: Write the failing tests** — create `src/trigger_ui.rs` with tests first:

```rust
//! The TriggerCondition catalog: one default-payload prototype per variant, grouped by pipeline
//! phase, so the Rules panel can drive a single ComboBox over all 34 variants (`TriggerCondition`
//! has no EnumVariants impl — it carries payloads). Labels come from the enum's own `Display`.
//! Also referential-integrity validation for `SkillCondition.trigger_skill` — the obelisk loader
//! ERRORS on unknown refs (config/skills.rs:77), so Save must block them.

use loot_core::DamageType;
use stat_core::{Skill, TriggerCondition};
use std::collections::HashSet;

/// Every `TriggerCondition` variant as (pipeline-group label, default-payload prototype), in
/// pipeline order. The single source of truth for the condition picker.
pub fn trigger_prototypes() -> Vec<(&'static str, TriggerCondition)> {
    use TriggerCondition::*;
    vec![
        ("Unconditional", Always),
        ("Pre-calculation", PlayerFullLife),
        ("Pre-calculation", PlayerLowLife { threshold: 0.35 }),
        ("Pre-calculation", TargetFullLife),
        ("Pre-calculation", TargetLowLife { threshold: 0.35 }),
        ("Pre-calculation", TargetHasEffect { id: String::new() }),
        ("Pre-calculation", TargetEffectStacks { id: String::new(), min_stacks: 1 }),
        ("Pre-calculation", SelfHasEffect { id: String::new() }),
        ("Pre-calculation", EveryNthHit { n: 3 }),
        ("Pre-calculation", PlayerLowMana { threshold: 0.35 }),
        ("Pre-calculation", PlayerFullMana),
        ("Pre-calculation", PlayerHasBarrier),
        ("Pre-calculation", PlayerNoBarrier),
        ("Pre-calculation", TargetHasBarrier),
        ("Pre-calculation", TargetNoBarrier),
        ("Pre-calculation", SelfEffectStacks { id: String::new(), min_stacks: 1 }),
        ("Pre-calculation", TargetNoEffect { id: String::new() }),
        ("Post-calculation", OnCrit),
        ("Post-calculation", DamageTypeDealt { damage_type: DamageType::Fire }),
        ("Post-calculation", OnNonCrit),
        ("Post-calculation", DamageOverThreshold { threshold: 0.0 }),
        ("Post-calculation", MultipleDamageTypes),
        ("Post-resolution", OnKill),
        ("Post-resolution", OnBarrierBroken),
        ("Post-resolution", OnOverkill { threshold: 0.0 }),
        ("Defensive", OnDamageTaken),
        ("Defensive", OnDamageTakenOfType { damage_type: DamageType::Fire }),
        ("Defensive", OnEffectConsumed { id: String::new() }),
        ("Defensive", OnEffectChargeUsed { id: String::new() }),
        ("Defensive", OnDodge),
        ("Defensive", OnEvasionCap),
        ("Defensive", OnHitTaken),
        ("Defensive", OnBarrierDepleted),
        ("Defensive", OnLowLifeReached { threshold: 0.35 }),
    ]
}

/// Index of `c`'s VARIANT in `trigger_prototypes()` (payload-insensitive, via discriminant).
pub fn trigger_index(c: &TriggerCondition) -> usize {
    trigger_prototypes()
        .iter()
        .position(|(_, p)| std::mem::discriminant(p) == std::mem::discriminant(c))
        .unwrap_or(0)
}

/// The `conditions[].trigger_skill` ids that are NOT in `known_ids` (and not the skill itself —
/// self-reference is valid: the loader validates against the post-insert map). Non-empty ⇒ the
/// obelisk loader would reject the whole skills dir; Save must refuse to write.
pub fn invalid_trigger_refs(skill: &Skill, known_ids: &HashSet<String>) -> Vec<String> {
    skill
        .conditions
        .iter()
        .map(|c| c.trigger_skill.clone())
        .filter(|id| id != &skill.id && !known_ids.contains(id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules_model::blank_attack_skill;
    use stat_core::SkillCondition;

    #[test]
    fn catalog_covers_all_34_variants_uniquely() {
        let protos = trigger_prototypes();
        assert_eq!(protos.len(), 34);
        for (i, (_, p)) in protos.iter().enumerate() {
            assert_eq!(trigger_index(p), i, "prototype {i} must index back to itself");
        }
    }

    #[test]
    fn trigger_index_is_payload_insensitive() {
        let c = TriggerCondition::PlayerLowLife { threshold: 0.1 };
        let proto = TriggerCondition::PlayerLowLife { threshold: 0.35 };
        assert_eq!(trigger_index(&c), trigger_index(&proto));
        assert_eq!(trigger_index(&TriggerCondition::Always), 0);
    }

    #[test]
    fn invalid_trigger_refs_flags_unknown_and_allows_known_and_self() {
        let mut s = blank_attack_skill("zap", "Zap");
        s.conditions.push(SkillCondition {
            trigger_skill: "discharge".into(),
            additional: true,
            condition: TriggerCondition::OnCrit,
        });
        s.conditions.push(SkillCondition {
            trigger_skill: "zap".into(), // self-reference: valid
            additional: false,
            condition: TriggerCondition::OnKill,
        });
        s.conditions.push(SkillCondition {
            trigger_skill: "ghost".into(),
            additional: true,
            condition: TriggerCondition::OnCrit,
        });
        let known: HashSet<String> = ["discharge".to_string()].into();
        assert_eq!(invalid_trigger_refs(&s, &known), vec!["ghost".to_string()]);
    }
}
```

- [ ] **Step 2: Add the module + run**

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo test trigger_ui
```
Expected: 3 PASS. (If the compiler flags a variant-count mismatch here, the loot_core enum changed — reconcile against `loot_core/src/types.rs:1270` rather than deleting entries.)

- [ ] **Step 3: Commit**

```bash
cd /Users/luke/src/obelisk-arena && git add crates/arena_editor/src/trigger_ui.rs crates/arena_editor/src/lib.rs && git commit -m "feat(editor): TriggerCondition catalog + trigger-ref validation (M4 stage 2)"
```

---

### Task 8: Conditions UI (trigger cascade + use_conditions) + Save gating

**Files:**
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/rules_panel.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/panel.rs`

**Interfaces:**
- Consumes: `trigger_prototypes`/`trigger_index`/`invalid_trigger_refs` (Task 7).
- Produces: `draw_rules_tab` gains a `known_skill_ids: &[String]` param and renders `skill.conditions` (SkillCondition rows) + `skill.use_conditions` (TriggerCondition rows). Save refuses to write rules with invalid trigger refs.

- [ ] **Step 1: Add the shared picker helpers to `rules_panel.rs`** (module-private):

```rust
use crate::trigger_ui::{trigger_index, trigger_prototypes};
use stat_core::{SkillCondition, TriggerCondition};

/// One ComboBox over all 34 TriggerCondition variants, grouped by pipeline phase.
/// Selecting a variant resets its payload to the prototype default. Returns true on change.
fn trigger_variant_combo(ui: &mut egui::Ui, salt: &str, cond: &mut TriggerCondition) -> bool {
    let mut changed = false;
    let protos = trigger_prototypes();
    let mut idx = trigger_index(cond);
    egui::ComboBox::from_id_salt(salt)
        .selected_text(cond.to_string())
        .show_ui(ui, |ui| {
            let mut last_group = "";
            for (i, (group, proto)) in protos.iter().enumerate() {
                if *group != last_group {
                    ui.label(egui::RichText::new(*group).small().weak());
                    last_group = group;
                }
                if ui.selectable_value(&mut idx, i, proto.to_string()).clicked() {
                    *cond = protos[i].1.clone();
                    changed = true;
                }
            }
        });
    changed
}

/// Payload editors for the 5 payload shapes (threshold / id / id+stacks / n / damage type).
fn trigger_params(ui: &mut egui::Ui, cond: &mut TriggerCondition) -> bool {
    use TriggerCondition::*;
    let mut changed = false;
    match cond {
        PlayerLowLife { threshold }
        | TargetLowLife { threshold }
        | PlayerLowMana { threshold }
        | DamageOverThreshold { threshold }
        | OnOverkill { threshold }
        | OnLowLifeReached { threshold } => {
            ui.label("threshold");
            changed |= ui
                .add(egui::DragValue::new(threshold).speed(0.01).range(0.0..=10_000.0))
                .changed();
        }
        TargetHasEffect { id }
        | SelfHasEffect { id }
        | TargetNoEffect { id }
        | OnEffectConsumed { id }
        | OnEffectChargeUsed { id } => {
            ui.label("effect");
            changed |= ui.add(egui::TextEdit::singleline(id).desired_width(80.0)).changed();
        }
        TargetEffectStacks { id, min_stacks } | SelfEffectStacks { id, min_stacks } => {
            ui.label("effect");
            changed |= ui.add(egui::TextEdit::singleline(id).desired_width(80.0)).changed();
            ui.label("stacks ≥");
            changed |= ui.add(egui::DragValue::new(min_stacks).range(1..=100)).changed();
        }
        EveryNthHit { n } => {
            ui.label("every n");
            changed |= ui.add(egui::DragValue::new(n).range(1..=100)).changed();
        }
        DamageTypeDealt { damage_type } | OnDamageTakenOfType { damage_type } => {
            egui::ComboBox::from_id_salt("tp_dt")
                .selected_text(format!("{damage_type:?}"))
                .show_ui(ui, |ui| {
                    for v in DamageType::all_variants() {
                        if ui.selectable_value(damage_type, *v, format!("{v:?}")).clicked() {
                            changed = true;
                        }
                    }
                });
        }
        _ => {}
    }
    changed
}
```

- [ ] **Step 2: Render the two condition lists.** Change the signature to `pub fn draw_rules_tab(ui: &mut egui::Ui, skill: &mut Skill, effect_ids: &[String], known_skill_ids: &[String]) -> bool` and append inside the ScrollArea (after effect applications):

```rust
        ui.separator();

        // === Trigger cascade (SkillCondition) ===
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Triggers (cascade)").strong());
            if ui.button("+ add").clicked() {
                skill.conditions.push(SkillCondition::default());
                changed = true;
            }
        });
        let mut remove_sc: Option<usize> = None;
        for (i, sc) in skill.conditions.iter_mut().enumerate() {
            ui.push_id(("sc", i), |ui| {
                ui.horizontal(|ui| {
                    changed |= trigger_variant_combo(ui, "sc_cond", &mut sc.condition);
                    changed |= trigger_params(ui, &mut sc.condition);
                    ui.label("→ cast");
                    egui::ComboBox::from_id_salt("sc_skill")
                        .selected_text(if sc.trigger_skill.is_empty() {
                            "(pick skill)".to_string()
                        } else {
                            sc.trigger_skill.clone()
                        })
                        .show_ui(ui, |ui| {
                            for id in known_skill_ids {
                                if ui
                                    .selectable_value(&mut sc.trigger_skill, id.clone(), id)
                                    .clicked()
                                {
                                    changed = true;
                                }
                            }
                        });
                    changed |= ui
                        .checkbox(&mut sc.additional, "additional")
                        .on_hover_text("checked: fires in ADDITION to the primary; unchecked: REPLACES it")
                        .changed();
                    if ui.button("✕").clicked() {
                        remove_sc = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove_sc {
            skill.conditions.remove(i);
            changed = true;
        }

        // === Use conditions (usability gate) ===
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Use conditions").strong());
            if ui.button("+ add").clicked() {
                skill.use_conditions.push(TriggerCondition::default());
                changed = true;
            }
        });
        let mut remove_uc: Option<usize> = None;
        for (i, uc) in skill.use_conditions.iter_mut().enumerate() {
            ui.push_id(("uc", i), |ui| {
                ui.horizontal(|ui| {
                    changed |= trigger_variant_combo(ui, "uc_cond", uc);
                    changed |= trigger_params(ui, uc);
                    if ui.button("✕").clicked() {
                        remove_uc = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove_uc {
            skill.use_conditions.remove(i);
            changed = true;
        }
```

(Note: use_conditions are documented as PreCalculation-only (skill.rs:503); the full picker is offered for v1 simplicity — obelisk itself doesn't validate the phase here.)

- [ ] **Step 3: Update the call site + gate Save.** In `panel.rs`, the Rules arm becomes:

```rust
    PanelTab::Rules => {
        let ids = effect_id_list();
        let known = crate::io::list_skill_ids();
        if crate::rules_panel::draw_rules_tab(ui, &mut rules.skill, &ids, &known) {
            rules.dirty = true;
        }
    }
```

and the rules-save block gains the referential-integrity gate (wrap the existing `match save_skill_rules…`):

```rust
        let known: std::collections::HashSet<String> =
            crate::io::list_skill_ids().into_iter().collect();
        let bad = crate::trigger_ui::invalid_trigger_refs(&rules.skill, &known);
        if !bad.is_empty() {
            status.0 = format!("rules save blocked: unknown trigger_skill {bad:?}");
        } else {
            match crate::io::save_skill_rules(&rules.skill, &rules.path) {
                // ... existing Ok/Err arms unchanged ...
            }
        }
```

- [ ] **Step 4: Build + suite + STAGE 2 GATE**

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo build && cargo test
```
Expected: clean + green. **Manual QA (windowed):** open firebolt → Rules; add a cascade row `OnKill → firebolt (additional)`; Save; status shows reload; F4-play a kill → the obelisk `TriggerFired` shows in the preview (the cascade actually fires through the real sim). Then set trigger_skill to a bogus id → Save is blocked with the status message and the file on disk is untouched.

- [ ] **Step 5: Commit**

```bash
cd /Users/luke/src/obelisk-arena && git add crates/arena_editor/src/rules_panel.rs crates/arena_editor/src/panel.rs && git commit -m "feat(editor): trigger-cascade + use-condition authoring w/ ref-validated Save (M4 stage 2 complete)"
```

---

# STAGE 3 — Effect authoring (registry swap + StatType picker + EffectConfig form)

### Task 9: obelisk-core effect-registry swap API

**Files:**
- Modify: `/Users/luke/src/obelisk/stat_core/src/config/effects.rs` (lines 14, 19, 427–451 + tests)
- Modify: `/Users/luke/src/obelisk/stat_core/src/config/mod.rs` (re-export)
- Modify: `/Users/luke/src/obelisk/stat_core/src/lib.rs` (re-export)

**Interfaces:**
- Produces: `stat_core::config::swap_effect_registry(EffectRegistry)` — replaces the global registry at runtime. **`effect_registry()` keeps its exact `-> &'static EffectRegistry` signature** (leak-on-swap), so ALL ~16 existing call sites compile unchanged and sim behavior is identical (goldens prove it).
- Design: the `OnceLock` becomes `RwLock<Option<&'static EffectRegistry>>`; every install leaks a `Box<EffectRegistry>` (init leaks once as before-equivalent; each swap leaks one more — bounded by editor Save count, negligible size, and old references stay valid forever, which is what makes `&'static` sound).

- [ ] **Step 1: Branch**

```bash
cd /Users/luke/src/obelisk && git checkout master && git checkout -b feat/effect-registry-swap
```

- [ ] **Step 2: Write the failing test** — append to the existing `#[cfg(test)] mod tests` in `stat_core/src/config/effects.rs`:

```rust
    /// swap_effect_registry replaces the global registry in-process (the editor hot-reload path).
    /// stat_core tests share one process: the swap PRESERVES all currently-registered effects
    /// (superset semantics) so concurrently-running tests are unaffected, and only ADDS a marker.
    #[test]
    fn swap_effect_registry_replaces_the_global_registry() {
        ensure_effect_registry_initialized();
        let mut next = EffectRegistry::new();
        for id in effect_registry().all_ids() {
            next.register(effect_registry().get(id).unwrap().clone());
        }
        let marker: EffectConfig =
            toml::from_str("id = \"__swap_marker\"\nname = \"Swap Marker\"").unwrap();
        next.register(marker);
        swap_effect_registry(next);
        assert!(effect_registry().get("__swap_marker").is_some());
        assert!(effect_registry_initialized());
    }
```

Run: `cd /Users/luke/src/obelisk && cargo test -p stat_core swap_effect_registry`
Expected: FAIL — `swap_effect_registry` not found.

- [ ] **Step 3: Implement.** In `effects.rs`, change line 14 `use std::sync::OnceLock;` → `use std::sync::RwLock;`, line 19 to:

```rust
/// Global effect registry slot. Historically a `OnceLock`; now a swappable slot so the skill
/// designer can hot-reload edited effects. Each install leaks its registry (`Box::leak`) — that
/// is what keeps `effect_registry()`'s `&'static` return sound across swaps (old references
/// remain valid forever). Leaks are bounded by swap count (editor Saves) and registries are tiny.
static EFFECT_REGISTRY: RwLock<Option<&'static EffectRegistry>> = RwLock::new(None);
```

and replace the four accessor fns (427–451) with:

```rust
/// Initialize the global effect registry from a directory of TOML files.
/// First call wins; later calls are silently ignored (the historical OnceLock semantics —
/// obelisk-bevy's guarded `add_obelisk_effects` depends on second-init being a no-op).
pub fn init_effect_registry(dir: &Path) -> Result<(), ConfigError> {
    let registry = load_effect_configs(dir)?;
    let mut slot = EFFECT_REGISTRY.write().unwrap();
    if slot.is_none() {
        *slot = Some(Box::leak(Box::new(registry)));
    }
    Ok(())
}

/// Get a reference to the global effect registry
/// Panics if not initialized - call init_effect_registry first
pub fn effect_registry() -> &'static EffectRegistry {
    EFFECT_REGISTRY
        .read()
        .unwrap()
        .expect("Effect registry not initialized. Call init_effect_registry() first.")
}

/// Check if the effect registry has been initialized
pub fn effect_registry_initialized() -> bool {
    EFFECT_REGISTRY.read().unwrap().is_some()
}

/// Ensure the effect registry is initialized (for tests)
/// Uses an empty registry if not already initialized
pub fn ensure_effect_registry_initialized() {
    let mut slot = EFFECT_REGISTRY.write().unwrap();
    if slot.is_none() {
        *slot = Some(Box::leak(Box::new(EffectRegistry::new())));
    }
}

/// Replace the global effect registry (editor hot-reload). Unlike `init_effect_registry`, this
/// ALWAYS installs — readers that already hold `&'static` references keep seeing the old
/// registry (leaked, never freed); new `effect_registry()` calls see the new one.
pub fn swap_effect_registry(registry: EffectRegistry) {
    *EFFECT_REGISTRY.write().unwrap() = Some(Box::leak(Box::new(registry)));
}
```

Re-export: in `config/mod.rs` add `swap_effect_registry` to the `pub use effects::{…}` list; in `lib.rs` line 59 extend to `pub use config::{effect_registry, init_effect_registry, ensure_effect_registry_initialized, swap_effect_registry};`.

- [ ] **Step 4: Run the full obelisk suite**

```bash
cd /Users/luke/src/obelisk && cargo test
```
Expected: all PASS including the new swap test.

- [ ] **Step 5: Run the downstream gates (this task touches the sim's dependency — all three):**

```bash
cd /Users/luke/src/obelisk-bevy && cargo test --features test-support --test golden
cd /Users/luke/src/obelisk-bevy && cargo test --features test-support --lib --tests
cd /Users/luke/src/obelisk-arena && pkill -f arena-server; pkill -f arena-client; sleep 1; bash crates/arena_game/tools/net-test/run_session.sh
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo build
```
Expected: goldens byte-identical (NO `UPDATE_GOLDEN`); obelisk-bevy suite green; net-test `session PASS` (≤3 tries); arena_editor builds.

- [ ] **Step 6: Commit (obelisk repo)**

```bash
cd /Users/luke/src/obelisk && git add stat_core/src/config/effects.rs stat_core/src/config/mod.rs stat_core/src/lib.rs && git commit -m "feat(stat_core): swappable effect registry (editor hot-reload; leak-on-swap keeps effect_registry() &'static)"
```

---

### Task 10: Searchable StatType picker (pure)

**Files:**
- Create: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/stat_ui.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/lib.rs` (add `pub mod stat_ui;`)

**Interfaces:**
- Produces: `stat_choices(effect_ids: &[String]) -> Vec<(String, StatType)>` (serde-string label + value, from `StatType::all_variants_with_effects`); `filter_stats<'a>(&'a [(String, StatType)], query: &str) -> Vec<&'a (String, StatType)>`. Task 12's picker widget consumes these.

- [ ] **Step 1: Write the failing tests** — create `src/stat_ui.rs`:

```rust
//! The StatType picker's data layer. `StatType` has ~80 non-parameterized variants plus
//! per-effect parameterized expansions; `StatType::all_variants_with_effects` enumerates them and
//! `to_serde_string()` yields the exact snake_case vocabulary the TOML round-trips — so the picker
//! is a searchable combo over real values, never a hand-maintained list.

use loot_core::StatType;

/// All pickable stats: (serde-string label, value). Parameterized variants expand per effect id.
pub fn stat_choices(effect_ids: &[String]) -> Vec<(String, StatType)> {
    StatType::all_variants_with_effects(effect_ids)
        .into_iter()
        .map(|s| (s.to_serde_string(), s))
        .collect()
}

/// Case-insensitive substring filter over the labels. Empty query returns everything.
pub fn filter_stats<'a>(
    choices: &'a [(String, StatType)],
    query: &str,
) -> Vec<&'a (String, StatType)> {
    let q = query.to_lowercase();
    choices
        .iter()
        .filter(|(name, _)| name.to_lowercase().contains(&q))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_cover_the_base_variants_and_round_trip_serde_strings() {
        let choices = stat_choices(&[]);
        assert!(choices.len() >= 80, "expected the full base catalog, got {}", choices.len());
        for (name, stat) in &choices {
            let parsed = StatType::from_serde_str(name).expect("label must parse back");
            assert_eq!(&parsed, stat, "serde-string label must round-trip");
        }
    }

    #[test]
    fn effect_ids_expand_parameterized_variants() {
        let with = stat_choices(&["burn".to_string()]);
        let without = stat_choices(&[]);
        assert!(with.len() > without.len());
        assert!(with.iter().any(|(_, s)| matches!(s, StatType::EffectMagnitude(id) if id == "burn")));
    }

    #[test]
    fn filter_is_case_insensitive_substring() {
        let choices = stat_choices(&[]);
        let fire = filter_stats(&choices, "FIRE");
        assert!(!fire.is_empty());
        assert!(fire.iter().all(|(n, _)| n.to_lowercase().contains("fire")));
        assert_eq!(filter_stats(&choices, "").len(), choices.len());
    }
}
```

- [ ] **Step 2: Add module + run**

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo test stat_ui
```
Expected: 3 PASS. (If `from_serde_str` errors on any label, that's a loot_core vocabulary bug worth surfacing — do not weaken the assertion; report it.)

- [ ] **Step 3: Commit**

```bash
cd /Users/luke/src/obelisk-arena && git add crates/arena_editor/src/stat_ui.rs crates/arena_editor/src/lib.rs && git commit -m "feat(editor): searchable StatType choice catalog (M4 stage 3)"
```

---

### Task 11: Effect model + io + hot-reload plumbing

**Files:**
- Create: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/effect_model.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/io.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/lib.rs` (add `pub mod effect_model;`)

**Interfaces:**
- Consumes: `swap_effect_registry` (Task 9), `editor_root()`.
- Produces: `EditedEffect { config: EffectConfig, path: PathBuf, dirty: bool }` (Resource) + `EditedEffect::from_config`; `blank_effect(id: &str) -> EffectConfig`; io fns `default_effect_path(&str) -> PathBuf`, `save_effect_config(&EffectConfig, &Path) -> std::io::Result<()>`, `load_effect_config(&Path) -> Result<EffectConfig, String>`, `list_effect_ids_on_disk() -> Vec<String>`, `reload_effect_registry() -> Result<usize, String>`. Task 12 consumes all.

- [ ] **Step 1: Write the failing tests.** Create `src/effect_model.rs`:

```rust
//! The effect-authoring model: the `EditedEffect` resource (the in-flight `EffectConfig` open in
//! the Effects tab). `EffectConfig` has no `Default` derive — `blank_effect` builds a minimal
//! instance through serde (every field except id/name is `#[serde(default)]`).

use bevy::prelude::*;
use stat_core::config::EffectConfig;
use std::path::PathBuf;

/// The effect body currently open in the designer: its `EffectConfig`, the
/// `config/effects/<id>.toml` path it saves to, and whether it has unsaved edits.
#[derive(Resource)]
pub struct EditedEffect {
    pub config: EffectConfig,
    pub path: PathBuf,
    pub dirty: bool,
}

impl EditedEffect {
    /// Open `config` for editing, saving to `path`, with no unsaved edits.
    pub fn from_config(config: EffectConfig, path: PathBuf) -> Self {
        Self { config, path, dirty: false }
    }
}

/// A minimal fresh effect: 5s buff-shaped defaults, no modifiers/conditions yet.
pub fn blank_effect(id: &str) -> EffectConfig {
    let mut c: EffectConfig =
        toml::from_str(&format!("id = \"{id}\"\nname = \"{id}\"")).expect("minimal EffectConfig");
    c.duration = stat_core::config::effects::EffectDuration::Finite(5.0);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_effect_is_a_minimal_finite_buff() {
        let e = blank_effect("haste");
        assert_eq!(e.id, "haste");
        assert!(!e.is_debuff);
        assert!(e.modifiers.is_empty());
        assert!((e.duration.as_seconds() - 5.0).abs() < f64::EPSILON);
        assert_eq!(e.max_stacks, 1);
    }
}
```

Append to `io.rs` tests:

```rust
    #[test]
    fn effect_config_round_trips_the_real_burn() {
        let loaded = load_effect_config(&default_effect_path("burn")).expect("burn.toml parses");
        assert_eq!(loaded.id, "burn");
        let tmp = std::env::temp_dir().join("m4_io_test_burn.toml");
        save_effect_config(&loaded, &tmp).expect("save");
        let reloaded = load_effect_config(&tmp).expect("reparse");
        assert_eq!(loaded, reloaded);
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn list_effect_ids_on_disk_contains_burn() {
        assert!(list_effect_ids_on_disk().iter().any(|s| s == "burn"));
    }

    #[test]
    fn reload_effect_registry_swaps_in_the_real_dir() {
        let n = reload_effect_registry().expect("swap");
        assert!(n >= 1);
        assert!(stat_core::config::effect_registry().get("burn").is_some());
    }
```

- [ ] **Step 2: Run to verify FAIL, then implement** — append to `io.rs` (add `use stat_core::config::EffectConfig;`):

```rust
/// The canonical effect-body path for an effect id, under the workspace `config/effects/`.
pub fn default_effect_path(effect_id: &str) -> PathBuf {
    editor_root().join(format!("config/effects/{effect_id}.toml"))
}

/// Serialize an `EffectConfig` to `path` as TOML (full-rewrite, like skill rules).
pub fn save_effect_config(config: &EffectConfig, path: &Path) -> std::io::Result<()> {
    let s = toml::to_string(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, s)
}

/// Parse an `EffectConfig` from a TOML file, human-readable error on failure.
pub fn load_effect_config(path: &Path) -> Result<EffectConfig, String> {
    let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str::<EffectConfig>(&s).map_err(|e| e.to_string())
}

/// The effect ids on disk: `config/effects/*.toml` stems, sorted.
pub fn list_effect_ids_on_disk() -> Vec<String> {
    let dir = editor_root().join("config/effects");
    let mut ids: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "toml"))
                .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
}

/// Re-load `config/effects` and SWAP it into the process-global obelisk registry so the preview
/// resolves the just-saved effect bodies (Task 9's stat_core API). Returns the effect count.
/// On load error the registry is left unchanged.
pub fn reload_effect_registry() -> Result<usize, String> {
    let reg = stat_core::config::load_effect_configs(&editor_root().join("config/effects"))
        .map_err(|e| e.to_string())?;
    let n = reg.all_ids().len();
    stat_core::config::swap_effect_registry(reg);
    Ok(n)
}
```

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo test io:: effect_model
```
Expected: all PASS.

- [ ] **Step 3: Commit**

```bash
cd /Users/luke/src/obelisk-arena && git add crates/arena_editor/src/effect_model.rs crates/arena_editor/src/io.rs crates/arena_editor/src/lib.rs && git commit -m "feat(editor): EditedEffect model + effect TOML io + registry hot-swap plumbing (M4 stage 3)"
```

---

### Task 12: Effects tab egui form

**Files:**
- Create: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/effects_panel.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/panel.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/skill_designer.rs`
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/lib.rs` (add `pub mod effects_panel;`)

**Interfaces:**
- Consumes: `EditedEffect`/`blank_effect` (Task 11), `stat_choices`/`filter_stats` (Task 10), effect io (Task 11), `trigger` picker style (Task 8's discriminant idiom, applied to the 4-variant `EffectTrigger`).
- Produces: `effects_panel::draw_effects_tab(ui, edited: &mut EditedEffect, known_skill_ids: &[String], stat_query: &mut String) -> (bool /*changed*/, Option<String> /*open request*/)`.

- [ ] **Step 1: Implement** — create `src/effects_panel.rs`:

```rust
//! The Effects tab: a serde-driven egui form over `stat_core::config::EffectConfig` (buff /
//! ailment bodies). Egui shell over the tested catalogs (`stat_ui`) — compile gate + windowed use.
//! Deferred (v1): `global_conditionals` / `conditional_modifiers` — preserved on save, not shown.

use bevy_egui::egui;
use loot_core::types::EnumVariants;
use loot_core::DamageType;
use stat_core::config::effects::EffectDuration;
use stat_core::config::{ChargeConfig, EffectModConfig, StatusApplication};
use stat_core::types::{ChargeConsumption, StackingBehavior};
use stat_core::{EffectCondition, EffectTrigger};

use crate::effect_model::EditedEffect;
use crate::io::list_effect_ids_on_disk;
use crate::stat_ui::{filter_stats, stat_choices};

/// EffectTrigger prototypes for the 4-variant picker (discriminant idiom, like trigger_ui).
fn effect_trigger_prototypes() -> Vec<EffectTrigger> {
    vec![
        EffectTrigger::OnMaxStacks { consume: true },
        EffectTrigger::OnExpire,
        EffectTrigger::OnConsume,
        EffectTrigger::OnApply,
    ]
}

fn effect_trigger_label(t: &EffectTrigger) -> &'static str {
    match t {
        EffectTrigger::OnMaxStacks { .. } => "on max stacks",
        EffectTrigger::OnExpire => "on expire",
        EffectTrigger::OnConsume => "on consume",
        EffectTrigger::OnApply => "on apply",
    }
}

/// Draw the Effects form. Returns (changed, open-request): `open` is `Some(id)` when the user
/// picked a different effect (or typed a new id) — the caller swaps the `EditedEffect` resource.
pub fn draw_effects_tab(
    ui: &mut egui::Ui,
    edited: &mut EditedEffect,
    known_skill_ids: &[String],
    stat_query: &mut String,
) -> (bool, Option<String>) {
    let mut changed = false;
    let mut open: Option<String> = None;
    let e = &mut edited.config;

    // === Selector row ===
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&e.id).strong());
        egui::ComboBox::from_id_salt("fx_effect_picker")
            .selected_text("open…")
            .show_ui(ui, |ui| {
                for id in list_effect_ids_on_disk() {
                    if ui.selectable_label(e.id == id, &id).clicked() {
                        open = Some(id);
                    }
                }
            });
        if ui.button("+ New").clicked() {
            open = Some(String::new()); // caller treats empty id as "new blank"
        }
    });

    egui::ScrollArea::vertical().id_salt("fx_scroll").show(ui, |ui| {
        // === Identity / duration / stacking ===
        ui.horizontal(|ui| {
            ui.label("name");
            changed |= ui.text_edit_singleline(&mut e.name).changed();
            changed |= ui.checkbox(&mut e.is_debuff, "debuff").changed();
            let mut infinite = e.duration.is_infinite();
            if ui.checkbox(&mut infinite, "infinite").changed() {
                e.duration = if infinite {
                    EffectDuration::Infinite
                } else {
                    EffectDuration::Finite(5.0)
                };
                changed = true;
            }
            if let EffectDuration::Finite(secs) = &mut e.duration {
                changed |= ui
                    .add(egui::DragValue::new(secs).speed(0.1).range(0.0..=600.0).suffix(" s"))
                    .changed();
            }
            egui::ComboBox::from_id_salt("fx_stacking")
                .selected_text(e.stacking.variant_name())
                .show_ui(ui, |ui| {
                    for v in StackingBehavior::all_variants() {
                        if ui.selectable_value(&mut e.stacking, v.clone(), v.variant_name()).clicked()
                        {
                            changed = true;
                        }
                    }
                });
            ui.label("max stacks");
            changed |= ui.add(egui::DragValue::new(&mut e.max_stacks).range(1..=99)).changed();
        });

        // === Ailment fields ===
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("fx_dot_type")
                .selected_text(match e.damage_type {
                    Some(dt) => format!("DoT: {dt:?}"),
                    None => "DoT: none".to_string(),
                })
                .show_ui(ui, |ui| {
                    if ui.selectable_label(e.damage_type.is_none(), "none").clicked() {
                        e.damage_type = None;
                        changed = true;
                    }
                    for v in DamageType::all_variants() {
                        if ui.selectable_label(e.damage_type == Some(*v), format!("{v:?}")).clicked()
                        {
                            e.damage_type = Some(*v);
                            changed = true;
                        }
                    }
                });
            ui.label("dmg %");
            changed |= ui
                .add(egui::DragValue::new(&mut e.base_damage_percent).speed(0.01).range(0.0..=10.0))
                .changed();
            ui.label("tick");
            changed |= ui
                .add(egui::DragValue::new(&mut e.tick_rate).speed(0.05).range(0.0..=10.0).suffix(" s"))
                .changed();
            // application: Chance | Buildup{threshold} (discriminant picker — payload variant)
            let is_buildup = matches!(e.application, StatusApplication::Buildup { .. });
            let mut buildup = is_buildup;
            if ui.checkbox(&mut buildup, "buildup").changed() {
                e.application = if buildup {
                    StatusApplication::Buildup { threshold: 100.0 }
                } else {
                    StatusApplication::Chance
                };
                changed = true;
            }
            if let StatusApplication::Buildup { threshold } = &mut e.application {
                changed |= ui.add(egui::DragValue::new(threshold).range(1.0..=10_000.0)).changed();
            }
        });

        // === Charges ===
        ui.horizontal(|ui| {
            let mut has = e.charges.is_some();
            if ui.checkbox(&mut has, "charges").changed() {
                e.charges = if has {
                    Some(ChargeConfig { count: 3, consumption: ChargeConsumption::AllSkills })
                } else {
                    None
                };
                changed = true;
            }
            if let Some(c) = &mut e.charges {
                changed |= ui.add(egui::DragValue::new(&mut c.count).range(1..=99)).changed();
                egui::ComboBox::from_id_salt("fx_consumption")
                    .selected_text(c.consumption.variant_name())
                    .show_ui(ui, |ui| {
                        for v in ChargeConsumption::all_variants() {
                            let selected = c.consumption.variant_name() == v.variant_name();
                            if ui.selectable_label(selected, v.variant_name()).clicked() {
                                c.consumption = v.clone();
                                changed = true;
                            }
                        }
                    });
            }
        });

        ui.separator();

        // === Stat modifiers (the StatType picker) ===
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Modifiers").strong());
            if ui.button("+ add").clicked() {
                e.modifiers.push(EffectModConfig::default());
                changed = true;
            }
        });
        let effect_ids: Vec<String> =
            list_effect_ids_on_disk().into_iter().collect();
        let choices = stat_choices(&effect_ids);
        let mut remove_m: Option<usize> = None;
        for (i, m) in e.modifiers.iter_mut().enumerate() {
            ui.push_id(("fxmod", i), |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("fx_stat")
                        .selected_text(m.stat.to_serde_string())
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(stat_query).hint_text("search stats…"),
                            );
                            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                                for (name, s) in filter_stats(&choices, stat_query) {
                                    if ui.selectable_label(&m.stat == s, name).clicked() {
                                        m.stat = s.clone();
                                        changed = true;
                                    }
                                }
                            });
                        });
                    ui.label("value");
                    changed |= ui.add(egui::DragValue::new(&mut m.value).speed(0.5)).changed();
                    changed |= ui
                        .checkbox(&mut m.is_more, "more")
                        .on_hover_text("checked: MORE multiplier; unchecked: increased")
                        .changed();
                    if ui.button("✕").clicked() {
                        remove_m = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove_m {
            e.modifiers.remove(i);
            changed = true;
        }

        ui.separator();

        // === Effect triggers (the Static-at-3-stacks → discharge cascade) ===
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Triggers").strong());
            if ui.button("+ add").clicked() {
                e.conditions.push(EffectCondition::default());
                changed = true;
            }
        });
        let mut remove_c: Option<usize> = None;
        for (i, ec) in e.conditions.iter_mut().enumerate() {
            ui.push_id(("fxcond", i), |ui| {
                ui.horizontal(|ui| {
                    let protos = effect_trigger_prototypes();
                    let mut idx = protos
                        .iter()
                        .position(|p| std::mem::discriminant(p) == std::mem::discriminant(&ec.trigger))
                        .unwrap_or(0);
                    egui::ComboBox::from_id_salt("fx_trig")
                        .selected_text(effect_trigger_label(&ec.trigger))
                        .show_ui(ui, |ui| {
                            for (j, p) in protos.iter().enumerate() {
                                if ui.selectable_value(&mut idx, j, effect_trigger_label(p)).clicked()
                                {
                                    ec.trigger = protos[j].clone();
                                    changed = true;
                                }
                            }
                        });
                    if let EffectTrigger::OnMaxStacks { consume } = &mut ec.trigger {
                        changed |= ui.checkbox(consume, "consume").changed();
                    }
                    ui.label("→ cast");
                    egui::ComboBox::from_id_salt("fx_trig_skill")
                        .selected_text(if ec.trigger_skill.is_empty() {
                            "(pick skill)".to_string()
                        } else {
                            ec.trigger_skill.clone()
                        })
                        .show_ui(ui, |ui| {
                            for id in known_skill_ids {
                                if ui
                                    .selectable_value(&mut ec.trigger_skill, id.clone(), id)
                                    .clicked()
                                {
                                    changed = true;
                                }
                            }
                        });
                    if ui.button("✕").clicked() {
                        remove_c = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove_c {
            e.conditions.remove(i);
            changed = true;
        }
    });
    (changed, open)
}
```

- [ ] **Step 2: Wire the tab.** In `panel.rs`, add params `mut effect: ResMut<EditedEffect>` and `mut stat_query: Local<String>` (and `mut new_effect_id: Local<String>` if you route "+ New" through a text field — v1: `+ New` opens a blank with a placeholder id `new_effect`). Replace the Effects placeholder arm:

```rust
    PanelTab::Effects => {
        let known = crate::io::list_skill_ids();
        let (c, open) =
            crate::effects_panel::draw_effects_tab(ui, &mut effect, &known, &mut stat_query);
        if c {
            effect.dirty = true;
        }
        if let Some(id) = open {
            let id = if id.is_empty() { "new_effect".to_string() } else { id };
            let path = crate::io::default_effect_path(&id);
            let cfg = crate::io::load_effect_config(&path)
                .unwrap_or_else(|_| crate::effect_model::blank_effect(&id));
            *effect = crate::effect_model::EditedEffect::from_config(cfg, path);
        }
    }
```

- [ ] **Step 3: Seed `EditedEffect` at startup.** In `skill_designer.rs` `build`, after the `EditedRules` insertion:

```rust
        // Seed the Effects tab with the first effect on disk (burn today), else a blank.
        let first = crate::io::list_effect_ids_on_disk().into_iter().next()
            .unwrap_or_else(|| "new_effect".to_string());
        let effect_path = crate::io::default_effect_path(&first);
        let cfg = crate::io::load_effect_config(&effect_path)
            .unwrap_or_else(|_| crate::effect_model::blank_effect(&first));
        app.insert_resource(crate::effect_model::EditedEffect::from_config(cfg, effect_path));
```

- [ ] **Step 4: Build + suite**

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo build && cargo test
```
Expected: clean + green.

- [ ] **Step 5: Commit**

```bash
cd /Users/luke/src/obelisk-arena && git add crates/arena_editor/src/effects_panel.rs crates/arena_editor/src/panel.rs crates/arena_editor/src/skill_designer.rs crates/arena_editor/src/lib.rs && git commit -m "feat(editor): Effects tab — EffectConfig form w/ searchable StatType picker (M4 stage 3)"
```

---

### Task 13: Effect Save→swap hot-reload + full M4 gate

**Files:**
- Modify: `/Users/luke/src/obelisk-arena/crates/arena_editor/src/panel.rs`

**Interfaces:**
- Consumes: `save_effect_config`/`reload_effect_registry` (Task 11).
- Produces: the unified Save now writes all FOUR files (cast.ron, skillfx.ron, skill toml, effect toml) and hot-reloads both registries.

- [ ] **Step 1: Extend the save block** — after the rules save/reload in `draw_skill_panel`:

```rust
        if effect.dirty {
            match crate::io::save_effect_config(&effect.config, &effect.path) {
                Ok(()) => {
                    effect.dirty = false;
                    match crate::io::reload_effect_registry() {
                        Ok(n) => {
                            status.0 = format!("{} | {n} effects swapped", status.0);
                        }
                        Err(e2) => status.0 = format!("{} | effect swap failed: {e2}", status.0),
                    }
                }
                Err(e2) => status.0 = format!("{} | effect save failed: {e2}", status.0),
            }
        }
```

- [ ] **Step 2: Build + suite**

```bash
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo build && cargo test
```
Expected: clean + green.

- [ ] **Step 3: FULL M4 GATE (all four surfaces):**

```bash
cd /Users/luke/src/obelisk && cargo test
cd /Users/luke/src/obelisk-bevy && cargo test --features test-support --test golden
cd /Users/luke/src/obelisk-bevy && cargo test --features test-support --lib --tests
cd /Users/luke/src/obelisk-arena && pkill -f arena-server; pkill -f arena-client; sleep 1; bash crates/arena_game/tools/net-test/run_session.sh
cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo test
```
Expected: everything green; goldens byte-identical; net-test PASS ≤3 tries.

- [ ] **Step 4: Windowed end-to-end QA (main loop, the /verify pass):**
`cd /Users/luke/src/obelisk-arena/crates/arena_editor && cargo run --bin arena-editor`, press `K`:
1. Rules tab: bump firebolt's fire max damage 20→40, Save, F4-play → the preview `DamageResolved` shows bigger hits (the HUD/log or the dummy's death timing).
2. Effects tab: open `burn`, change `base_damage_percent` 0.5→2.0, Save (status shows "N effects swapped"), F4-play → burn DoT ticks visibly harder, WITHOUT restarting the editor (this is the Task 9 payoff).
3. New Spell seed: type an id, `+Spell`, author a base damage row, Save, open it from the picker again → round-trips.
Revert the QA edits afterwards (`git -C /Users/luke/src/obelisk-arena checkout -- config/ assets/` if the experiments shouldn't persist).

- [ ] **Step 5: Commit + merge both repos to their mainlines (after user sign-off per finishing-a-development-branch)**

```bash
cd /Users/luke/src/obelisk-arena && git add crates/arena_editor/src/panel.rs && git commit -m "feat(editor): effect Save hot-swaps the obelisk registry (M4 stage 3 complete)"
```

- [ ] **Step 6: Update the project memory + handoff doc** — record: M4 done (or how far it got), the swap-API design (leak-on-swap keeps `&'static`), and that `StatType::all_variants_with_effects` made the stat picker trivial (the research doc's "cannot be enumerated" was wrong).

---

## Self-review notes (already applied)

- **Spec coverage:** all four user decisions are implemented (full effect authoring = Tasks 11–13; swap API = Task 9; full-rewrite = Tasks 2/11; stages = the three gates). Deferred items are declared in Global Constraints, not silently dropped.
- **Research-doc correction:** `StatType` IS enumerable (`all_variants[_with_effects]`, loot_core types.rs:578/689) — Task 10 leans on it; no hand-built vocabulary exists in this plan.
- **Type consistency:** `EditedRules.skill: stat_core::Skill`, `EditedEffect.config: stat_core::config::EffectConfig`, `draw_rules_tab(ui, &mut Skill, effect_ids, known_skill_ids) -> bool` (4-arg form after Task 8), `draw_effects_tab(...) -> (bool, Option<String>)` — checked against every call site shown.
- **The one risky compile point:** Task 12 assumes `ChargeConsumption: Clone` and compares via `variant_name()` (PartialEq unverified); `StackingBehavior`/`StatusApplication` PartialEq ARE verified. If `StatType` lacks `Clone`… it derives Clone (used in `all_variants_with_effects`). If `egui::ComboBox::width` or `ScrollArea::id_salt` differ in bevy_egui 0.39's egui, adapt to the local egui version's API — cosmetic, not structural.
