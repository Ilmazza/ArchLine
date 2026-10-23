//! Record `ARCHLINE_PROGETTO`: XRecord nel dizionario radice del disegno con
//! le impostazioni di ArchLine (unità scelta, layer dei muri nuovi).
//!
//! Voci: 90 (Int32) versione; 70 (Int16) codice INSUNITS dell'unità, 0 =
//! nessuna; 8 (String) layer dei muri nuovi, "" = nessuno.
//!
//! Creazione e aggiornamento seguono `HostSession::create_xrecord` /
//! `update_xrecord` (`src/app/plugin_host.rs`). Niente undo qui: lo apre il
//! comando che chiama `write`.

use codec::objects::{ObjectType, XRecord, XRecordEntry, XRecordValue};
use codec::{CadDocument, Handle};

use super::units::DrawingUnit;
use super::RECORD_PROGETTO;

const VERSION: i32 = 1;
const CODE_VERSION: i32 = 90;
const CODE_UNIT: i32 = 70;
const CODE_NEW_WALLS_LAYER: i32 = 8;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectRecord {
    /// Unità scelta quando `INSUNITS` vale 0.
    pub unit: Option<DrawingUnit>,
    /// Layer in cui vanno i muri nuovi.
    pub new_walls_layer: Option<String>,
}

/// Il record del disegno; senza record (o illeggibile) i valori sono vuoti.
pub fn read(doc: &CadDocument) -> ProjectRecord {
    let mut rec = ProjectRecord::default();
    let Some(ObjectType::XRecord(xrecord)) = record_handle(doc).and_then(|h| doc.objects.get(&h))
    else {
        return rec;
    };
    for entry in &xrecord.entries {
        match (entry.code, &entry.value) {
            (CODE_UNIT, XRecordValue::Int16(code)) => {
                rec.unit = DrawingUnit::from_insunits(*code);
            }
            (CODE_NEW_WALLS_LAYER, XRecordValue::String(name)) => {
                rec.new_walls_layer = Some(name.clone()).filter(|n| !n.is_empty());
            }
            _ => {}
        }
    }
    rec
}

/// Crea il record o ne sostituisce le voci. Senza dizionario radice non fa
/// nulla (un disegno aperto dal codec lo ha sempre).
pub fn write(doc: &mut CadDocument, rec: &ProjectRecord) {
    let root = doc.header.named_objects_dict_handle;
    if !matches!(doc.objects.get(&root), Some(ObjectType::Dictionary(_))) {
        return;
    }
    let entries = vec![
        XRecordEntry::int32(CODE_VERSION, VERSION),
        XRecordEntry::int16(CODE_UNIT, rec.unit.map_or(0, DrawingUnit::insunits)),
        XRecordEntry::string(
            CODE_NEW_WALLS_LAYER,
            rec.new_walls_layer.clone().unwrap_or_default(),
        ),
    ];

    if let Some(handle) = record_handle(doc) {
        if let Some(ObjectType::XRecord(xrecord)) = doc.objects.get_mut(&handle) {
            xrecord.entries = entries;
            // Come `update_xrecord`: flusso dei riferimenti ricostruito e byte
            // grezzi della lettura scartati, così il salvataggio scrive le voci nuove.
            xrecord.synchronize_object_references();
            xrecord.preserve_object_reference_stream = false;
            xrecord.entries_complete = true;
            xrecord.raw_data.clear();
            xrecord.raw_dwg_data = None;
            xrecord.raw_dwg_handle_bits = 0;
            xrecord.raw_dwg_version = None;
            return;
        }
    }

    let handle = doc.allocate_handle();
    let mut xrecord = XRecord::named(RECORD_PROGETTO);
    xrecord.handle = handle;
    xrecord.owner = root;
    xrecord.entries = entries;
    xrecord.synchronize_object_references();
    doc.objects.insert(handle, ObjectType::XRecord(xrecord));
    if let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&root) {
        // Una voce rimasta senza XRecord (file danneggiato) non deve restare doppia.
        dictionary
            .entries
            .retain(|(key, _)| !key.eq_ignore_ascii_case(RECORD_PROGETTO));
        dictionary.add_entry(RECORD_PROGETTO, handle);
    }
}

/// Handle dell'XRecord `ARCHLINE_PROGETTO` nel dizionario radice, se esiste ed è un XRecord.
fn record_handle(doc: &CadDocument) -> Option<Handle> {
    let ObjectType::Dictionary(root) = doc.objects.get(&doc.header.named_objects_dict_handle)?
    else {
        return None;
    };
    let handle = root.get(RECORD_PROGETTO)?;
    matches!(doc.objects.get(&handle), Some(ObjectType::XRecord(_))).then_some(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_roundtrip_and_update() {
        let mut doc = codec::CadDocument::new();
        assert_eq!(read(&doc), ProjectRecord::default());
        write(
            &mut doc,
            &ProjectRecord {
                unit: Some(DrawingUnit::Metri),
                new_walls_layer: Some("MURI".into()),
            },
        );
        write(
            &mut doc,
            &ProjectRecord {
                unit: Some(DrawingUnit::Centimetri),
                new_walls_layer: Some("MURI".into()),
            },
        );
        assert_eq!(
            read(&doc),
            ProjectRecord {
                unit: Some(DrawingUnit::Centimetri),
                new_walls_layer: Some("MURI".into()),
            }
        );
    }
}
