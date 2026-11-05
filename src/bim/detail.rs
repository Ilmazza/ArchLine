//! Particolari costruttivi (spec parte 6 §4): il disegno 2D di una
//! stratigrafia, in scala 1:1 nello spazio modello. Logica pura.
//!
//! - **parete**: gli strati sono bande verticali affiancate da sinistra
//!   (larghe quanto lo spessore), alte [`BAND_M`];
//! - **solaio**: bande orizzontali impilate dall'alto (il primo strato è la
//!   finitura), lunghe [`BAND_M`];
//! - ogni banda è una `LWPOLYLINE` chiusa; un richiamo (`LINE`) va da un punto
//!   della banda alla colonna dei testi a destra, a passo di
//!   [`STEP_TEXTS`] altezze; il testo è «n. Materiale — sp. X,X cm»;
//! - il titolo sta sopra: «Particolare <nome> — parete|solaio, spessore totale
//!   X,X cm».
//!
//! Nessun retino (spec §2: la scala di un retino non ha una fonte). Colori
//! `ByLayer` su `AL-PARTICOLARI`. Ogni entità posata ha il record
//! `ARCHLINE_PARTICOLARE` (`Integer16(1)`, nome della stratigrafia); i
//! particolari si impilano dall'alto da un angolo (in alto a sinistra), come le
//! sezioni (R117), e quelli già posati danno l'angolo della posa successiva.

use codec::entities::{AttachmentPoint, Line, LwPolyline, MText};
use codec::tables::{AppId, Layer};
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{Color, Document, EntityType, Handle, Vector2, Vector3};
use glam::DVec3;

use super::section_place::bbox;
use super::strata::{cm, Kind, Stratigraphy};
use super::units::DrawingUnit;
use super::{APP_PARTICOLARE, LAYER_PARTICOLARI};

const VERSION: i16 = 1;
const ACI_BIANCO: i16 = 7;

/// Altezza (parete) o lunghezza (solaio) delle bande, in metri (scelta mia).
pub const BAND_M: f64 = 0.50;
/// Altezza dei testi, in metri: 2,5 mm in una stampa 1:10 (scelta mia).
pub const TEXT_M: f64 = 0.025;
/// Altezza del titolo, in metri.
pub const TITLE_M: f64 = 0.035;
/// Passo dei testi dei richiami, in altezze di testo.
pub const STEP_TEXTS: f64 = 1.6;
/// Distanza della colonna dei testi dal disegno, in metri.
pub const LABEL_GAP_M: f64 = 0.15;
/// Spazio fra due particolari impilati, in metri.
pub const GAP_M: f64 = 0.30;

fn line(a: [f64; 2], b: [f64; 2]) -> EntityType {
    let mut line = Line::from_points(Vector3::new(a[0], a[1], 0.0), Vector3::new(b[0], b[1], 0.0));
    line.common.layer = LAYER_PARTICOLARI.to_string();
    EntityType::Line(line)
}

fn band(x0: f64, y0: f64, x1: f64, y1: f64) -> EntityType {
    let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let mut outline =
        LwPolyline::from_points(corners.iter().map(|c| Vector2::new(c[0], c[1])).collect());
    outline.is_closed = true;
    outline.common.layer = LAYER_PARTICOLARI.to_string();
    EntityType::LwPolyline(outline)
}

fn text(at: [f64; 2], value: &str, height: f64, attachment: AttachmentPoint) -> EntityType {
    let mut text = MText::with_value(value, Vector3::new(at[0], at[1], 0.0));
    text.height = height;
    text.attachment_point = attachment;
    text.common.layer = LAYER_PARTICOLARI.to_string();
    EntityType::MText(Box::new(text))
}

/// Il testo del richiamo dello strato `n` (da 1).
pub fn label(n: usize, material: &str, thickness: f64, unit: DrawingUnit) -> String {
    format!("{n}. {material} — sp. {} cm", cm(thickness, unit))
}

/// Il titolo del particolare.
pub fn title(s: &Stratigraphy, unit: DrawingUnit) -> String {
    format!(
        "Particolare {} — {}, spessore totale {} cm",
        s.name,
        s.kind.word(),
        cm(s.total(), unit)
    )
}

/// Le entità del particolare di `s`, con l'angolo in basso a sinistra delle
/// bande in `(0; 0)`, in unità del disegno.
pub fn draw(s: &Stratigraphy, unit: DrawingUnit) -> Vec<EntityType> {
    let pm = unit.per_metre();
    let band_len = BAND_M * pm;
    let th = TEXT_M * pm;
    let step = STEP_TEXTS * th;
    let total = s.total();
    let n = s.layers.len();
    let mut out = Vec::new();
    // Gli strati, i punti dei richiami e il lato destro del disegno.
    let (right, top) = match s.kind {
        Kind::Parete => (total, band_len),
        Kind::Solaio => (band_len, total),
    };
    let column = right + LABEL_GAP_M * pm;
    let mut cumulative = 0.0;
    for (k, layer) in s.layers.iter().enumerate() {
        let (from, to) = (cumulative, cumulative + layer.thickness);
        cumulative = to;
        // La riga del testo k: dall'alto, a passo costante.
        let y_text = top - (k as f64 + 0.5) * step;
        let anchor = match s.kind {
            Kind::Parete => {
                out.push(band(from, 0.0, to, band_len));
                [(from + to) / 2.0, y_text]
            }
            Kind::Solaio => {
                out.push(band(0.0, total - to, band_len, total - from));
                // Punti sfalsati lungo la banda, perché i richiami non si coprano.
                let x = band_len * (0.25 + 0.5 * k as f64 / n.max(1) as f64);
                [x, total - (from + to) / 2.0]
            }
        };
        out.push(line(anchor, [column, y_text]));
        out.push(text(
            [column + th * 0.5, y_text],
            &label(k + 1, &layer.material, layer.thickness, unit),
            th,
            AttachmentPoint::MiddleLeft,
        ));
    }
    let title_y = top.max(0.0) + STEP_TEXTS * th;
    out.push(text(
        [0.0, title_y],
        &title(s, unit),
        TITLE_M * pm,
        AttachmentPoint::BottomLeft,
    ));
    out
}

fn record(name: &str) -> ExtendedDataRecord {
    let mut rec = ExtendedDataRecord::new(APP_PARTICOLARE);
    rec.add_value(XDataValue::Integer16(VERSION));
    rec.add_value(XDataValue::String(name.to_string()));
    rec
}

/// Il nome della stratigrafia nel record, se è leggibile.
pub fn decode(rec: &ExtendedDataRecord) -> Option<String> {
    rec.values.iter().find_map(|value| match value {
        XDataValue::String(name) if !name.is_empty() => Some(name.clone()),
        _ => None,
    })
}

/// I particolari di `all` impilati dall'alto da `anchor` (l'angolo in alto a
/// sinistra del primo), ognuno con il suo record.
pub fn place(all: &[Stratigraphy], unit: DrawingUnit, anchor: [f64; 2]) -> Vec<EntityType> {
    let pm = unit.per_metre();
    let mut out = Vec::new();
    let mut top = anchor[1];
    for s in all {
        let entities = draw(s, unit);
        let Some((lo, hi)) = bbox(&entities) else {
            continue;
        };
        let shift = DVec3::new(anchor[0] - lo[0], top - hi[1], 0.0);
        let rec = record(&s.name);
        for mut entity in entities {
            crate::entities::traits::EntityTypeOps::apply_transform(
                &mut entity,
                &crate::command::EntityTransform::Translate(shift),
            );
            entity.common_mut().extended_data.upsert_record(rec.clone());
            out.push(entity);
        }
        // Il testo più basso scende di mezza riga sotto il suo punto.
        top = lo[1] + shift.y - (TEXT_M + GAP_M) * pm;
    }
    out
}

/// Le entità dei particolari già posati (handle e nome), nello spazio modello.
pub fn details_in(doc: &Document) -> Vec<(Handle, String)> {
    doc.block_records
        .iter()
        .filter(|record| record.is_model_space())
        .flat_map(|record| record.entity_handles.iter().copied())
        .filter_map(|handle| {
            let entity = doc.get_entity(handle)?;
            let name = decode(entity.common().extended_data.get_record(APP_PARTICOLARE)?)?;
            Some((handle, name))
        })
        .collect()
}

/// L'angolo in alto a sinistra dei particolari già posati.
pub fn existing_anchor(doc: &Document) -> Option<[f64; 2]> {
    let entities: Vec<EntityType> = details_in(doc)
        .into_iter()
        .filter_map(|(handle, _)| doc.get_entity(handle).cloned())
        .collect();
    bbox(&entities).map(|(lo, hi)| [lo[0], hi[1]])
}

/// Mancano il layer o l'APPID dei particolari?
pub fn missing_layer(doc: &Document) -> bool {
    !doc.layers.contains(LAYER_PARTICOLARI) || !doc.app_ids.contains(APP_PARTICOLARE)
}

/// Crea layer e APPID che mancano, con il loro handle. Niente undo: lo apre il
/// comando.
pub fn ensure_layer(doc: &mut Document) {
    if !doc.layers.contains(LAYER_PARTICOLARI) {
        let mut layer = Layer::new(LAYER_PARTICOLARI);
        layer.handle = doc.allocate_handle();
        layer.color = Color::from_index(ACI_BIANCO);
        let _ = doc.layers.add(layer);
    }
    if !doc.app_ids.contains(APP_PARTICOLARE) {
        let mut app_id = AppId::new(APP_PARTICOLARE);
        app_id.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bim::strata::parse_layers;

    const M: DrawingUnit = DrawingUnit::Metri;

    fn t12() -> Stratigraphy {
        Stratigraphy {
            name: "T12".into(),
            kind: Kind::Parete,
            layers: parse_layers("Intonaco 0.015; Laterizio forato 0.09; Intonaco 0.015").unwrap(),
        }
    }

    fn p1() -> Stratigraphy {
        Stratigraphy {
            name: "P1".into(),
            kind: Kind::Solaio,
            layers: parse_layers("Ceramica 0.01; Massetto 0.05").unwrap(),
        }
    }

    fn bands(entities: &[EntityType]) -> Vec<([f64; 2], [f64; 2])> {
        entities
            .iter()
            .filter_map(|e| match e {
                EntityType::LwPolyline(p) => bbox(&[EntityType::LwPolyline(p.clone())]),
                _ => None,
            })
            .collect()
    }

    fn texts(entities: &[EntityType]) -> Vec<String> {
        entities
            .iter()
            .filter_map(|e| match e {
                EntityType::MText(t) => Some(t.value.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_wall_is_three_vertical_bands_with_labels_and_title() {
        let entities = draw(&t12(), M);
        let b = bands(&entities);
        assert_eq!(b.len(), 3);
        let widths: Vec<f64> = b.iter().map(|(lo, hi)| hi[0] - lo[0]).collect();
        for (w, expected) in widths.iter().zip([0.015, 0.09, 0.015]) {
            assert!((w - expected).abs() < 1e-12, "{widths:?}");
        }
        assert!(b.iter().all(|(lo, hi)| lo[1] == 0.0 && (hi[1] - 0.5).abs() < 1e-12));
        // Affiancate senza vuoti: da 0 a 0,12.
        assert!((b[2].1[0] - 0.12).abs() < 1e-12 && b[0].0[0] == 0.0);
        let lines = entities.iter().filter(|e| matches!(e, EntityType::Line(_))).count();
        assert_eq!(lines, 3, "un richiamo per strato");
        assert_eq!(
            texts(&entities),
            [
                "1. Intonaco — sp. 1,5 cm",
                "2. Laterizio forato — sp. 9,0 cm",
                "3. Intonaco — sp. 1,5 cm",
                "Particolare T12 — parete, spessore totale 12,0 cm",
            ]
        );
        assert!(entities.iter().all(|e| e.common().layer == LAYER_PARTICOLARI));
    }

    #[test]
    fn a_floor_is_horizontal_bands_from_the_top() {
        let entities = draw(&p1(), M);
        let b = bands(&entities);
        // Ceramica in alto (da 0,05 a 0,06), massetto sotto (da 0 a 0,05).
        assert!((b[0].0[1] - 0.05).abs() < 1e-12 && (b[0].1[1] - 0.06).abs() < 1e-12, "{b:?}");
        assert!(b[1].0[1].abs() < 1e-12 && (b[1].1[1] - 0.05).abs() < 1e-12, "{b:?}");
        assert!(b.iter().all(|(lo, hi)| lo[0] == 0.0 && (hi[0] - 0.5).abs() < 1e-12));
        assert!(texts(&entities).last().unwrap().contains("solaio, spessore totale 6,0 cm"));
    }

    #[test]
    fn units_scale_the_drawing_and_keep_the_numbers() {
        let mut mm = t12();
        for layer in &mut mm.layers {
            layer.thickness *= 1000.0;
        }
        let entities = draw(&mm, DrawingUnit::Millimetri);
        let b = bands(&entities);
        assert!((b[1].1[0] - b[1].0[0] - 90.0).abs() < 1e-9);
        assert!((b[0].1[1] - 500.0).abs() < 1e-9, "bande alte 500 mm");
        assert_eq!(texts(&entities)[1], "2. Laterizio forato — sp. 9,0 cm");
    }

    #[test]
    fn placing_stacks_downwards_and_finds_them_again() {
        let mut doc = Document::new();
        ensure_layer(&mut doc);
        assert!(!missing_layer(&doc));
        for e in place(&[t12(), p1()], M, [20.0, 10.0]) {
            doc.add_entity(e).unwrap();
        }
        let found = details_in(&doc);
        let names: std::collections::BTreeSet<String> =
            found.iter().map(|(_, n)| n.clone()).collect();
        assert_eq!(names.into_iter().collect::<Vec<_>>(), ["P1", "T12"]);
        let anchor = existing_anchor(&doc).unwrap();
        assert!((anchor[0] - 20.0).abs() < 1e-9 && (anchor[1] - 10.0).abs() < 1e-9, "{anchor:?}");
        // Il secondo sta tutto sotto il primo.
        let of = |name: &str| -> Vec<EntityType> {
            found
                .iter()
                .filter(|(_, n)| n == name)
                .map(|(h, _)| doc.get_entity(*h).unwrap().clone())
                .collect()
        };
        let (first_lo, _) = bbox(&of("T12")).unwrap();
        let (_, second_hi) = bbox(&of("P1")).unwrap();
        assert!(second_hi[1] < first_lo[1], "{second_hi:?} {first_lo:?}");
    }
}
