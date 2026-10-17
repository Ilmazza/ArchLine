//! Block name and attribute columns of DATAEXTRACTION.
//!
//! A schedule (abaco) of doors, windows or fixtures is a table of block
//! references: one column for the block, one for each attribute tag. The
//! tags differ from drawing to drawing, so they are properties discovered from
//! the chosen source (key `attr:TAG`), next to the fixed `block_name`.

use std::collections::{BTreeMap, BTreeSet};

use codec::types::Handle;
use codec::{CadDocument, EntityType};

use crate::ui::window::annotation_data::ExtractionProperty;

pub(super) const BLOCK_NAME_KEY: &str = "block_name";
pub(super) const ATTRIBUTE_PREFIX: &str = "attr:";
pub(super) const ATTRIBUTE_CATEGORY: &str = "Attributes";

/// The block a reference stands for: its dynamic definition rather than the
/// anonymous `*U` copy, so references of one block group together.
pub(super) fn block_name(doc: &CadDocument, entity: &EntityType) -> String {
    match entity {
        EntityType::Insert(insert) => crate::app::commands::blockedit_target::definition_name(doc, insert),
        _ => String::new(),
    }
}

/// Attribute values of a reference by tag. A tag repeated in one reference
/// keeps its first value.
pub(super) fn attribute_values(entity: &EntityType) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    if let EntityType::Insert(insert) = entity {
        for attribute in &insert.attributes {
            values.entry(attribute.tag.clone()).or_insert_with(|| attribute.value.clone());
        }
    }
    values
}

/// Adds the attribute tags of the references in `doc` (all of them, or only
/// `allowed_handles`) to `tags`.
pub(super) fn collect_attribute_tags(
    doc: &CadDocument,
    allowed_handles: Option<&BTreeSet<Handle>>,
    tags: &mut BTreeSet<String>,
) {
    for entity in drawing_entities(doc) {
        if allowed_handles.is_some_and(|handles| !handles.contains(&entity.common().handle)) {
            continue;
        }
        tags.extend(attribute_values(entity).into_keys());
    }
}

/// The objects of the drawing proper: model space and the layouts, not the
/// geometry stored inside block definitions.
pub(crate) fn drawing_entities(doc: &CadDocument) -> impl Iterator<Item = &EntityType> + '_ {
    doc.block_records
        .iter()
        .filter(|record| record.is_model_space() || record.is_paper_space())
        .flat_map(|record| record.entity_handles.iter())
        .filter_map(|handle| doc.get_entity(*handle))
        .filter(|entity| !matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_)))
}

/// What makes two block references "the same block" for SELECTSIMILAR: the
/// definition name, ignoring case; empty for anything that is not a reference.
pub(crate) fn block_identity(doc: &CadDocument, entity: &EntityType) -> String {
    block_name(doc, entity).to_uppercase()
}

fn attribute_property(tag: &str, checked: bool) -> ExtractionProperty {
    ExtractionProperty {
        key: format!("{ATTRIBUTE_PREFIX}{tag}"),
        name: tag.to_string(),
        category: ATTRIBUTE_CATEGORY.to_string(),
        checked,
    }
}

/// Brings the attribute properties in line with the tags found in the current
/// source: new tags are added unchecked, ticked ones are kept even when the
/// source no longer has them (a saved setting still asks for the column), the
/// rest are dropped.
pub(super) fn merge_attribute_properties(properties: &mut Vec<ExtractionProperty>, tags: &BTreeSet<String>) {
    properties.retain(|item| {
        item.category != ATTRIBUTE_CATEGORY
            || item.checked
            || item.key.strip_prefix(ATTRIBUTE_PREFIX).is_some_and(|tag| tags.contains(tag))
    });
    for tag in tags {
        let key = format!("{ATTRIBUTE_PREFIX}{tag}");
        if !properties.iter().any(|item| item.key == key) {
            properties.push(attribute_property(tag, false));
        }
    }
}

/// Re-creates, ticked, the attribute properties that a saved setting lists.
pub(super) fn restore_attribute_properties(properties: &mut Vec<ExtractionProperty>, saved_keys: &BTreeSet<&str>) {
    for key in saved_keys {
        let Some(tag) = key.strip_prefix(ATTRIBUTE_PREFIX) else { continue };
        if !properties.iter().any(|item| item.key == *key) {
            properties.push(attribute_property(tag, true));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::scene::Scene;
    use codec::entities::{Insert, Line};
    use codec::types::{Transform, Vector3};

    fn add_insert(scene: &mut Scene, block: &str, x: f64) -> codec::Handle {
        scene.add_entity(EntityType::Insert(Insert::new(block, Vector3::new(x, 0.0, 0.0))))
    }

    fn add_line(scene: &mut Scene) -> codec::Handle {
        scene.add_entity(EntityType::Line(Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0))))
    }

    fn define_block(scene: &mut Scene, name: &str, members: &[codec::Handle]) {
        let identity = Transform::identity();
        scene.create_block_from_entities(members, name, &identity, &identity).expect("block");
    }

    #[test]
    fn drawing_entities_leave_out_the_geometry_of_block_definitions() {
        let mut scene = Scene::new();
        let inside = add_line(&mut scene);
        add_line(&mut scene);
        define_block(&mut scene, "Door", &[inside]);
        let lines = |doc: &CadDocument| drawing_entities(doc).filter(|e| matches!(e, EntityType::Line(_))).count();
        assert_eq!(lines(&scene.document), 1);
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

    fn tags(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn keys(properties: &[ExtractionProperty]) -> Vec<(&str, bool)> {
        properties.iter().map(|item| (item.key.as_str(), item.checked)).collect()
    }

    #[test]
    fn new_tags_are_added_unchecked_and_stale_unchecked_ones_dropped() {
        let mut properties = vec![attribute_property("VECCHIO", false), attribute_property("TIPO", true)];
        merge_attribute_properties(&mut properties, &tags(&["LARGHEZZA", "TIPO"]));
        assert_eq!(keys(&properties), vec![("attr:TIPO", true), ("attr:LARGHEZZA", false)]);
    }

    #[test]
    fn a_ticked_tag_survives_a_source_that_lacks_it() {
        let mut properties = vec![attribute_property("TIPO", true)];
        merge_attribute_properties(&mut properties, &tags(&[]));
        assert_eq!(keys(&properties), vec![("attr:TIPO", true)]);
    }

    #[test]
    fn saved_attribute_keys_come_back_ticked_once() {
        let mut properties = vec![attribute_property("TIPO", false)];
        let saved = ["attr:TIPO", "attr:LARGHEZZA", "layer"].into_iter().collect();
        restore_attribute_properties(&mut properties, &saved);
        assert_eq!(keys(&properties), vec![("attr:TIPO", false), ("attr:LARGHEZZA", true)]);
    }
}
