//! Assegnazione di una stratigrafia a un elemento (spec parte 6 §3.2, §3.3).
//!
//! Un record XDATA a sé, APPID `ARCHLINE_STRATIGRAFIA` (`Integer16` versione,
//! `String` nome), sull'entità che porta i dati dell'elemento:
//!
//! - **parete**: la campitura di una zona nuova (`ARCHLINE_ZONA`, stato nuovo);
//! - **solaio**: il seme (`MTEXT` su `AL-LOCALI-PROGETTO`) di un locale di
//!   progetto.
//!
//! Il record non tocca `ARCHLINE_ZONA` né `ARCHLINE_LOCALE`: `AL_STATO`,
//! `AL_ALTEZZA` e la migrazione restano come sono, e una copia porta con sé
//! l'assegnazione. Logica pura.

use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{Document, EntityType, Handle};

use super::rooms::{self, Phase, RoomPart};
use super::strata::Kind;
use super::zone::{self, ZoneState};
use super::APP_STRATIGRAFIA;

const VERSION: i16 = 1;

/// Che cosa porta una stratigrafia.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    /// La campitura di una zona nuova: una parete.
    NewWall,
    /// Il seme di un locale di progetto: un solaio.
    ProjectRoom,
}

impl Element {
    /// Il tipo di stratigrafia che l'elemento accetta.
    pub fn kind(self) -> Kind {
        match self {
            Element::NewWall => Kind::Parete,
            Element::ProjectRoom => Kind::Solaio,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Element::NewWall => "muratura nuova",
            Element::ProjectRoom => "locale di progetto",
        }
    }
}

/// I valori del record.
pub fn encode(name: &str) -> Vec<XDataValue> {
    vec![
        XDataValue::Integer16(VERSION),
        XDataValue::String(name.to_string()),
    ]
}

/// Il record XDATA dell'assegnazione a `name`.
pub fn record(name: &str) -> ExtendedDataRecord {
    let mut rec = ExtendedDataRecord::new(APP_STRATIGRAFIA);
    for value in encode(name) {
        rec.add_value(value);
    }
    rec
}

/// Il nome nel record, se è leggibile.
pub fn decode(rec: &ExtendedDataRecord) -> Option<String> {
    rec.values.iter().find_map(|value| match value {
        XDataValue::String(name) if !name.is_empty() => Some(name.clone()),
        _ => None,
    })
}

/// Il nome della stratigrafia assegnata a `entity`, se c'è.
pub fn assignment_of(entity: &EntityType) -> Option<String> {
    decode(entity.common().extended_data.get_record(APP_STRATIGRAFIA)?)
}

/// Che elemento è `entity` (`None`: non può avere una stratigrafia).
pub fn element_of(handle: Handle, entity: &EntityType) -> Option<Element> {
    if let Some(found) = zone::read_zone(handle, entity) {
        return (found.zone.state == ZoneState::Nuovo).then_some(Element::NewWall);
    }
    let rec = entity.common().extended_data.get_record(super::APP_LOCALE)?;
    let (room, part) = rooms::decode(rec)?;
    (room.phase == Phase::Progetto && part == RoomPart::Seme).then_some(Element::ProjectRoom)
}

/// Le entità dello spazio modello con un'assegnazione: handle, elemento (se
/// l'entità lo è ancora) e nome.
pub fn assignments_in(doc: &Document) -> Vec<(Handle, Option<Element>, String)> {
    doc.block_records
        .iter()
        .filter(|record| record.is_model_space())
        .flat_map(|record| record.entity_handles.iter().copied())
        .filter_map(|handle| {
            let entity = doc.get_entity(handle)?;
            let name = assignment_of(entity)?;
            Some((handle, element_of(handle, entity), name))
        })
        .collect()
}

/// `p` dentro l'anello (pari/dispari), bordo escluso.
fn inside(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut odd = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                odd = !odd;
            }
        }
    }
    odd
}

/// L'elemento sotto `p`: prima la zona nuova che lo contiene (buchi esclusi),
/// poi il locale di progetto il cui contorno lo contiene. `rooms` sono i
/// contorni misurati dei locali di progetto con il loro seme (vedi
/// [`project_room_outlines`]).
pub fn target_at(
    doc: &Document,
    p: [f64; 2],
    rooms: &[(Handle, Vec<[f64; 2]>)],
) -> Option<(Handle, Element)> {
    for found in zone::zones_in(doc) {
        if found.zone.state != ZoneState::Nuovo || found.rings.is_empty() {
            continue;
        }
        let in_outer = inside(&found.rings[0], p);
        let in_hole = found.rings[1..].iter().any(|hole| inside(hole, p));
        if in_outer && !in_hole {
            return Some((found.handle, Element::NewWall));
        }
    }
    rooms
        .iter()
        .find(|(_, outline)| outline.len() >= 3 && inside(outline, p))
        .map(|(handle, _)| (*handle, Element::ProjectRoom))
}

/// I semi dei locali di progetto con il contorno della riga che ha il loro
/// seme (`rows` da `quantities_rooms::project_room_rows`).
pub fn project_room_outlines(
    doc: &Document,
    rows: &[super::quantities_rooms::ProjectRoomRow],
) -> Vec<(Handle, Vec<[f64; 2]>)> {
    rooms::rooms_in(doc)
        .into_iter()
        .filter(|(_, room, part)| room.phase == Phase::Progetto && *part == RoomPart::Seme)
        .filter_map(|(handle, room, _)| {
            let row = rows.iter().find(|r| r.row.seed == room.seed)?;
            Some((handle, row.row.outline.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bim::fixtures::{case_a, scene_with, Form};
    use crate::bim::rooms::{seed_entity, Room};
    use crate::bim::units::DrawingUnit;
    use crate::bim::walls::{rect_from_axis_thickness, wall_set_without_rooms};
    use crate::bim::zone::{ensure_layers, zone_hatch, Zone, ZoneOrigin};

    const M: DrawingUnit = DrawingUnit::Metri;

    fn zone(state: ZoneState) -> EntityType {
        let rect = rect_from_axis_thickness([2.0, 0.0], [2.0, 3.0], 0.12).unwrap();
        zone_hatch(
            &Zone {
                state,
                origin: ZoneOrigin::MuroDuePunti,
                axis: Some(rect.axis),
                thickness: Some(0.12),
                height: None,
                varco: false,
            },
            &[rect.ring()],
        )
    }

    #[test]
    fn record_roundtrip() {
        assert_eq!(decode(&record("T12")).as_deref(), Some("T12"));
        let mut empty = ExtendedDataRecord::new(APP_STRATIGRAFIA);
        empty.add_value(XDataValue::Integer16(1));
        assert_eq!(decode(&empty), None);
    }

    #[test]
    fn only_new_zones_and_project_seeds_are_elements() {
        let mut scene = scene_with(case_a(Form::Continua, M), M.insunits());
        ensure_layers(&mut scene.document);
        let new = scene.add_entity(zone(ZoneState::Nuovo));
        let old = scene.add_entity(zone(ZoneState::Demolito));
        let seed = |phase| {
            seed_entity(
                &Room {
                    phase,
                    name: "A".into(),
                    number: String::new(),
                    seed: [1.0, 1.5],
                    height: None,
                },
                0.25,
            )
        };
        let project = scene.add_entity(seed(Phase::Progetto));
        let doc = &scene.document;
        let of = |h: Handle| element_of(h, doc.get_entity(h).unwrap());
        assert_eq!(of(new), Some(Element::NewWall));
        assert_eq!(of(old), None);
        assert_eq!(of(project), Some(Element::ProjectRoom));
        let wall = wall_set_without_rooms(doc, 1e-6).entities[0].0;
        assert_eq!(of(wall), None);
    }

    #[test]
    fn assignments_and_targets() {
        let mut scene = scene_with(case_a(Form::Continua, M), M.insunits());
        ensure_layers(&mut scene.document);
        let new = scene.add_entity(zone(ZoneState::Nuovo));
        if let Some(entity) = scene.document.get_entity_mut(new) {
            entity.common_mut().extended_data.upsert_record(record("T12"));
        }
        let found = assignments_in(&scene.document);
        assert_eq!(found, vec![(new, Some(Element::NewWall), "T12".to_string())]);
        // Il punto sull'asse della zona nuova la trova; un punto nel locale B
        // trova il contorno dato.
        let outline = vec![[4.2, 0.0], [8.0, 0.0], [8.0, 3.0], [4.2, 3.0]];
        let rooms = vec![(Handle::new(999), outline)];
        assert_eq!(
            target_at(&scene.document, [2.0, 1.5], &rooms),
            Some((new, Element::NewWall))
        );
        assert_eq!(
            target_at(&scene.document, [6.0, 1.5], &rooms),
            Some((Handle::new(999), Element::ProjectRoom))
        );
        assert_eq!(target_at(&scene.document, [-5.0, -5.0], &rooms), None);
    }
}
