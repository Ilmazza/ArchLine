//! Block identity for SELECTSIMILAR.
//!
//! DATAEXTRACTION's block and attribute columns are upstream's since v2026.43;
//! what stays here is how two block references are told apart.

use codec::{Document, EntityType};

/// The block a reference stands for: its dynamic definition rather than the
/// anonymous `*U` copy, so references of one block group together.
pub(super) fn block_name(doc: &Document, entity: &EntityType) -> String {
    match entity {
        EntityType::Insert(insert) => crate::app::commands::blockedit_target::definition_name(doc, insert),
        _ => String::new(),
    }
}

/// What makes two block references "the same block" for SELECTSIMILAR: the
/// definition name, ignoring case; empty for anything that is not a reference.
pub(crate) fn block_identity(doc: &Document, entity: &EntityType) -> String {
    block_name(doc, entity).to_uppercase()
}

#[cfg(test)]
mod tests {
    use crate::scene::Scene;
    use codec::entities::Insert;
    use codec::types::Vector3;
    use codec::EntityType;

    fn add_insert(scene: &mut Scene, block: &str, x: f64) -> codec::Handle {
        scene.add_entity(EntityType::Insert(Insert::new(block, Vector3::new(x, 0.0, 0.0))))
    }

    #[test]
    fn select_similar_keeps_blocks_apart() {
        let mut scene = Scene::new();
        let first = add_insert(&mut scene, "Door", 0.0);
        add_insert(&mut scene, "Door", 5.0);
        add_insert(&mut scene, "Window", 10.0);
        scene.select_entity(first, true);
        assert_eq!(scene.select_similar(), 1);
    }
}
