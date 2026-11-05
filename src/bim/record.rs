//! Record `ARCHLINE_PROGETTO`: XRecord nel dizionario radice del disegno con
//! le impostazioni di ArchLine (unità scelta, layer dei muri nuovi).
//!
//! Voci: 90 (Int32) versione; 70 (Int16) codice INSUNITS dell'unità, 0 =
//! nessuna; 8 (String) layer dei muri nuovi, "" = nessuno; 40 (Double)
//! altezza di default dei locali in unità del disegno, scritta solo se c'è.
//! La lettura va per codice e salta ciò che non conosce.
//!
//! Creazione e aggiornamento seguono `HostSession::create_xrecord` /
//! `update_xrecord` (`src/app/plugin_host.rs`). Niente undo qui: lo apre il
//! comando che chiama `write`.

use codec::objects::{ObjectType, XRecord, XRecordEntry, XRecordValue};
use codec::{Document, Handle};

use super::units::DrawingUnit;
use super::RECORD_PROGETTO;

const VERSION: i32 = 1;
const CODE_VERSION: i32 = 90;
const CODE_UNIT: i32 = 70;
const CODE_NEW_WALLS_LAYER: i32 = 8;
const CODE_DEFAULT_HEIGHT: i32 = 40;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectRecord {
    /// Unità scelta quando `INSUNITS` vale 0.
    pub unit: Option<DrawingUnit>,
    /// Layer in cui vanno i muri nuovi.
    pub new_walls_layer: Option<String>,
    /// Altezza di default dei locali, in unità del disegno (parte 2).
    pub default_height: Option<f64>,
}

/// Il record del disegno; senza record (o illeggibile) i valori sono vuoti.
pub fn read(doc: &Document) -> ProjectRecord {
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
            (CODE_DEFAULT_HEIGHT, XRecordValue::Double(height)) => {
                rec.default_height = Some(*height).filter(|h| h.is_finite() && *h > 0.0);
            }
            _ => {}
        }
    }
    rec
}

/// Crea il record o ne sostituisce le voci. Senza dizionario radice non fa
/// nulla (un disegno aperto dal codec lo ha sempre).
pub fn write(doc: &mut Document, rec: &ProjectRecord) {
    let mut entries = vec![
        XRecordEntry::int32(CODE_VERSION, VERSION),
        XRecordEntry::int16(CODE_UNIT, rec.unit.map_or(0, DrawingUnit::insunits)),
        XRecordEntry::string(
            CODE_NEW_WALLS_LAYER,
            rec.new_walls_layer.clone().unwrap_or_default(),
        ),
    ];
    if let Some(height) = rec.default_height {
        entries.push(XRecordEntry::double(CODE_DEFAULT_HEIGHT, height));
    }

    write_named_xrecord(doc, RECORD_PROGETTO, entries);
}

/// Le voci dell'XRecord `name` del dizionario radice, se c'è (parte 6: anche
/// `ARCHLINE_STRATIGRAFIE`).
pub(super) fn named_xrecord_entries<'a>(doc: &'a Document, name: &str) -> Option<&'a [XRecordEntry]> {
    match doc.objects.get(&named_xrecord_handle(doc, name)?) {
        Some(ObjectType::XRecord(xrecord)) => Some(&xrecord.entries),
        _ => None,
    }
}

/// Crea l'XRecord `name` nel dizionario radice o ne sostituisce le voci. Senza
/// dizionario radice non fa nulla (un disegno aperto dal codec lo ha sempre).
pub(super) fn write_named_xrecord(doc: &mut Document, name: &str, entries: Vec<XRecordEntry>) {
    let root = doc.header.named_objects_dict_handle;
    if !matches!(doc.objects.get(&root), Some(ObjectType::Dictionary(_))) {
        return;
    }
    if let Some(handle) = named_xrecord_handle(doc, name) {
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
    let mut xrecord = XRecord::named(name);
    xrecord.handle = handle;
    xrecord.owner = root;
    xrecord.entries = entries;
    xrecord.synchronize_object_references();
    doc.objects.insert(handle, ObjectType::XRecord(xrecord));
    if let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&root) {
        // Una voce rimasta senza XRecord (file danneggiato) non deve restare doppia.
        dictionary
            .entries
            .retain(|(key, _)| !key.eq_ignore_ascii_case(name));
        dictionary.add_entry(name, handle);
    }
}

fn record_handle(doc: &Document) -> Option<Handle> {
    named_xrecord_handle(doc, RECORD_PROGETTO)
}

/// Handle dell'XRecord `name` nel dizionario radice, se esiste ed è un XRecord.
fn named_xrecord_handle(doc: &Document, name: &str) -> Option<Handle> {
    let ObjectType::Dictionary(root) = doc.objects.get(&doc.header.named_objects_dict_handle)?
    else {
        return None;
    };
    let handle = root.get(name)?;
    matches!(doc.objects.get(&handle), Some(ObjectType::XRecord(_))).then_some(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_roundtrip_and_update() {
        let mut doc = codec::Document::new();
        assert_eq!(read(&doc), ProjectRecord::default());
        write(
            &mut doc,
            &ProjectRecord {
                unit: Some(DrawingUnit::Metri),
                new_walls_layer: Some("MURI".into()),
                default_height: None,
            },
        );
        write(
            &mut doc,
            &ProjectRecord {
                unit: Some(DrawingUnit::Centimetri),
                new_walls_layer: Some("MURI".into()),
                default_height: None,
            },
        );
        assert_eq!(
            read(&doc),
            ProjectRecord {
                unit: Some(DrawingUnit::Centimetri),
                new_walls_layer: Some("MURI".into()),
                default_height: None,
            }
        );
    }

    #[test]
    fn record_roundtrips_default_height() {
        let mut doc = codec::Document::new();
        let rec = ProjectRecord {
            unit: Some(DrawingUnit::Metri),
            new_walls_layer: None,
            default_height: Some(2.7),
        };
        write(&mut doc, &rec);
        assert_eq!(read(&doc), rec);
        // Un record riscritto senza altezza la perde: si scrive solo ciò che c'è.
        write(
            &mut doc,
            &ProjectRecord {
                default_height: None,
                ..rec
            },
        );
        assert_eq!(read(&doc).default_height, None);
    }

    #[test]
    fn record_without_height_entry_still_reads() {
        let mut doc = codec::Document::new();
        let root = doc.header.named_objects_dict_handle;
        let handle = doc.allocate_handle();
        let mut xrecord = XRecord::named(RECORD_PROGETTO);
        xrecord.handle = handle;
        xrecord.owner = root;
        xrecord.entries = vec![
            XRecordEntry::int32(CODE_VERSION, VERSION),
            XRecordEntry::int16(CODE_UNIT, DrawingUnit::Centimetri.insunits()),
            XRecordEntry::string(CODE_NEW_WALLS_LAYER, "MURI".to_string()),
        ];
        doc.objects.insert(handle, ObjectType::XRecord(xrecord));
        if let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&root) {
            dictionary.add_entry(RECORD_PROGETTO, handle);
        }
        assert_eq!(
            read(&doc),
            ProjectRecord {
                unit: Some(DrawingUnit::Centimetri),
                new_walls_layer: Some("MURI".into()),
                default_height: None,
            }
        );
    }
}
