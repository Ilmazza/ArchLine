//! Migrazione dei record BIM scritti prima del 2026-11-02.
//!
//! I vecchi record di zona e di locale salvavano seme, asse e spessore come
//! `Point3D` (1010) e `Real` (1040), che il codec non porta dietro all'entità
//! quando la si trasforma (MOVE, COPY, ROTATE, SCALE, MIRROR). Leggerli bastava
//! ai comandi BIM, ma non a impedire il difetto: una copia di un locale
//! restava collegata all'originale e una zona scalata teneva asse e spessore di
//! prima. Quindi i record si portano al formato corrente **all'apertura del
//! disegno**, prima che qualunque trasformazione possa guastarli, e non alla
//! prima riscrittura per caso.
//!
//! Si lavora in memoria e senza segnare il disegno come modificato: chi lo
//! salva scrive il formato nuovo, chi lo chiude senza salvare lo ritrova com'era
//! e viene migrato di nuovo alla prossima apertura. Si toccano solo le entità
//! dello spazio modello (le stesse che leggono `rooms_in` e `zones_in`) e solo
//! i record `ARCHLINE_LOCALE` e `ARCHLINE_ZONA`; l'XDATA di altre applicazioni
//! resta com'è. È idempotente.
//!
//! Limite: un'entità con un vecchio record che arriva in un disegno già aperto
//! per altra via (incolla da un altro disegno, inserimento di un file) non
//! passa da qui.

use codec::Document;

use super::{rooms, zone, APP_LOCALE, APP_ZONA};
use crate::scene::view::dispatch::set_entity_xdata;

/// Porta al formato corrente i record BIM di `doc`; dice quante entità ha
/// riscritto.
pub fn migrate_document(doc: &mut Document) -> usize {
    let handles: Vec<_> = doc
        .block_records
        .iter()
        .filter(|record| record.is_model_space())
        .flat_map(|record| record.entity_handles.iter().copied())
        .collect();
    let mut rewritten = 0;
    for handle in handles {
        let Some(entity) = doc.get_entity(handle) else {
            continue;
        };
        let data = &entity.common().extended_data;
        let rewrites = [
            (APP_LOCALE, data.get_record(APP_LOCALE).and_then(rooms::migrated_values)),
            (APP_ZONA, data.get_record(APP_ZONA).and_then(zone::migrated_values)),
        ];
        let mut touched = false;
        for (app, values) in rewrites {
            if let Some(values) = values {
                set_entity_xdata(doc, handle, app, Some(values));
                touched = true;
            }
        }
        rewritten += usize::from(touched);
    }
    rewritten
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::Line;
    use codec::xdata::{ExtendedDataRecord, XDataValue};
    use codec::{EntityType, Vector3};

    use crate::bim::rooms::{decode, RoomPart};
    use crate::bim::zone::{self as zone_mod, ZoneOrigin, ZoneState};

    fn text(value: &str) -> XDataValue {
        XDataValue::String(value.to_string())
    }

    /// Un record di locale come lo scriveva la versione precedente.
    fn old_room_record(seed: [f64; 2], part: &str) -> ExtendedDataRecord {
        let mut rec = ExtendedDataRecord::new(APP_LOCALE);
        for value in [
            XDataValue::Integer16(1),
            text("fase"),
            text("fatto"),
            text("parte"),
            text(part),
            text("nome"),
            text("Camera"),
            text("numero"),
            text(""),
            text("punto"),
            XDataValue::Point3D(Vector3::new(seed[0], seed[1], 0.0)),
            text("origine"),
            text("punto-interno"),
        ] {
            rec.add_value(value);
        }
        rec
    }

    fn old_zone_record() -> ExtendedDataRecord {
        let mut rec = ExtendedDataRecord::new(APP_ZONA);
        for value in [
            XDataValue::Integer16(1),
            text("stato"),
            text("nuovo"),
            text("origine"),
            text("linea-spessore"),
            text("varco"),
            XDataValue::Integer16(0),
            text("asse"),
            XDataValue::Point3D(Vector3::new(0.0, 0.05, 0.0)),
            XDataValue::Point3D(Vector3::new(2.0, 0.05, 0.0)),
            text("spessore"),
            XDataValue::Real(0.1),
            text("altezza"),
            XDataValue::Real(3.0),
        ] {
            rec.add_value(value);
        }
        rec
    }

    fn line_with(records: Vec<ExtendedDataRecord>) -> EntityType {
        let mut line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(2.0, 0.0, 0.0));
        for rec in records {
            line.common.extended_data.add_record(rec);
        }
        EntityType::Line(line)
    }

    #[test]
    fn old_records_are_brought_to_the_current_format_once() {
        let mut doc = Document::new();
        let mut other = ExtendedDataRecord::new("ALTRA_APP");
        other.add_value(XDataValue::Point3D(Vector3::new(1.0, 1.0, 0.0)));
        let room = doc
            .add_entity(line_with(vec![old_room_record([2.0, 1.5], "contorno"), other.clone()]))
            .unwrap();
        let zone = doc.add_entity(line_with(vec![old_zone_record()])).unwrap();

        assert_eq!(migrate_document(&mut doc), 2);
        assert_eq!(migrate_document(&mut doc), 0, "idempotente");

        let data = &doc.get_entity(room).unwrap().common().extended_data;
        let rec = data.get_record(APP_LOCALE).unwrap();
        assert!(rec.values.contains(&XDataValue::Position3D(Vector3::new(2.0, 1.5, 0.0))));
        assert!(rec.values.contains(&XDataValue::Point3D(Vector3::new(2.0, 1.5, 0.0))), "l'ancora");
        let (back, part) = decode(rec).unwrap();
        assert_eq!((back.seed, part), ([2.0, 1.5], RoomPart::Contorno));
        assert_eq!(data.get_record("ALTRA_APP"), Some(&other), "l'altra applicazione non si tocca");

        let data = &doc.get_entity(zone).unwrap().common().extended_data;
        let rec = data.get_record(APP_ZONA).unwrap();
        assert!(rec.values.contains(&XDataValue::Position3D(Vector3::new(0.0, 0.05, 0.0))));
        assert!(rec.values.contains(&XDataValue::Distance(0.1)));
        assert!(rec.values.contains(&XDataValue::Real(3.0)), "l'altezza resta un reale");
        let z = zone_mod::decode(rec).unwrap();
        assert_eq!((z.state, z.origin), (ZoneState::Nuovo, ZoneOrigin::LineaSpessore));
        assert_eq!(z.axis, Some(([0.0, 0.05], [2.0, 0.05])));
        assert_eq!(z.thickness, Some(0.1));
    }

    #[test]
    fn a_record_already_current_is_left_alone() {
        let mut doc = Document::new();
        let entity = doc.add_entity(line_with(vec![old_room_record([2.0, 1.5], "testo")])).unwrap();
        assert_eq!(migrate_document(&mut doc), 1);
        let before = doc.get_entity(entity).unwrap().common().extended_data.clone();
        assert_eq!(migrate_document(&mut doc), 0);
        assert_eq!(doc.get_entity(entity).unwrap().common().extended_data, before);
    }

    #[test]
    fn entities_without_bim_records_are_not_counted() {
        let mut doc = Document::new();
        doc.add_entity(line_with(vec![])).unwrap();
        assert_eq!(migrate_document(&mut doc), 0);
    }
}
