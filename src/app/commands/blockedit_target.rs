//! Which block definition BEDIT and REFEDIT open for a block reference.
//!
//! The reference of a dynamic block points at an evaluated anonymous copy
//! (`*U1`), a snapshot made for that one instance; the definition the user
//! means to change is the block the copy was made from.

use codec::entities::Insert;
use codec::Document;

/// The name of the block to edit for `insert`: its dynamic definition when it
/// refers to an evaluated copy, otherwise the block it names.
pub(crate) fn definition_name(document: &Document, insert: &Insert) -> String {
    if !insert.block_name.starts_with("*U") {
        return insert.block_name.clone();
    }
    document
        .dynamic_definition_for_insert(insert.common.handle)
        .and_then(|definition| document.block_records.iter().find(|br| br.handle == definition))
        .map(|br| br.name.clone())
        .filter(|name| !name.starts_with('*'))
        .unwrap_or_else(|| insert.block_name.clone())
}
