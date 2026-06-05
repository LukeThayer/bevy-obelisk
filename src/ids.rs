use bevy::prelude::*;

/// Component mirroring `StatBlock.id`. Auto-registered into `ObeliskEntityIndex`.
#[derive(Component, Clone, Debug)]
pub struct ObeliskId(pub String);

impl Default for ObeliskId { fn default() -> Self { ObeliskId(String::new()) } }

/// Bidirectional Entity <-> obelisk String id map.
#[derive(Resource, Default)]
pub struct ObeliskEntityIndex {
    to_entity: std::collections::HashMap<String, Entity>,
    to_id: std::collections::HashMap<Entity, String>,
}

impl ObeliskEntityIndex {
    pub fn entity(&self, id: &str) -> Option<Entity> { self.to_entity.get(id).copied() }
    pub fn id(&self, e: Entity) -> Option<&str> { self.to_id.get(&e).map(|s| s.as_str()) }
    fn insert(&mut self, e: Entity, id: String) { self.to_entity.insert(id.clone(), e); self.to_id.insert(e, id); }
    fn remove(&mut self, e: Entity) { if let Some(id) = self.to_id.remove(&e) { self.to_entity.remove(&id); } }
}

pub fn sync_index_added(
    mut index: ResMut<ObeliskEntityIndex>,
    added: Query<(Entity, &ObeliskId), Added<ObeliskId>>,
) {
    for (e, id) in &added { index.insert(e, id.0.clone()); }
}

pub fn sync_index_removed(
    mut index: ResMut<ObeliskEntityIndex>,
    mut removed: RemovedComponents<ObeliskId>,
) {
    for e in removed.read() { index.remove(e); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_syncs_on_spawn_and_despawn() {
        let mut app = App::new();
        app.init_resource::<ObeliskEntityIndex>();
        app.add_systems(Update, (sync_index_added, sync_index_removed));

        let e = app.world_mut().spawn(ObeliskId("goblin".into())).id();
        app.update();
        assert_eq!(app.world().resource::<ObeliskEntityIndex>().entity("goblin"), Some(e));

        app.world_mut().entity_mut(e).despawn();
        app.update();
        assert_eq!(app.world().resource::<ObeliskEntityIndex>().entity("goblin"), None);
    }
}
