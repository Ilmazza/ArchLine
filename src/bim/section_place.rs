//! La posa delle sezioni nel disegno (spec parte 4 §5.1). Logica pura: dà ai
//! disegni di [`super::section_build`] il record XDATA `ARCHLINE_SEZIONE`
//! (taglio e fase, per riconoscerli e sostituirli a ogni lancio), li impila in
//! verticale a partire da un angolo e legge dal documento quelli già posati.
//!
//! - ogni entità posata porta `ARCHLINE_SEZIONE`: `Integer16(1)`, `"taglio"` +
//!   nome, `"fase"` + chiave della fase. L'XDATA lo tolgono `AL_ESTRAI` e
//!   chiunque copi fuori dal disegno di lavoro (prefisso `ARCHLINE_`);
//! - i disegni si impilano dall'alto: il primo ha l'angolo in alto a sinistra
//!   in `anchor`, i successivi sotto il precedente, staccati di
//!   [`GAP_TEXTS`] altezze di testo; la larghezza non conta (stessa X);
//! - quelli già posati si ritrovano dal record (`sections_in`) e l'angolo in
//!   alto a sinistra del loro insieme è l'angolo di una nuova posa: rigenerare
//!   lascia le sezioni dove sono (R117).

use codec::tables::{AppId, Layer};
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{Color, Document, EntityType, Handle};
use glam::DVec3;

use super::section_build::{DrawingKind, SectionDrawing};
use super::section_draw::LAYER_SEZIONI;
use super::APP_SEZIONE;

const VERSION: i16 = 1;
const KEY_CUT: &str = "taglio";
const KEY_PHASE: &str = "fase";
const KEY_KIND: &str = "tipo";
const ACI_BIANCO: i16 = 7;

/// Distanza verticale tra due disegni, in altezze di testo.
pub const GAP_TEXTS: f64 = 4.0;
/// Quanto sotto il punto d'inserimento dei limiti (il più basso) arriva il loro
/// testo, in altezze di testo: tre righe al più (R117).
const LIMITS_DEPTH_TEXTS: f64 = 3.0;

fn record(kind: DrawingKind, cut: &str, phase: &str) -> ExtendedDataRecord {
    let mut rec = ExtendedDataRecord::new(APP_SEZIONE);
    for value in [
        XDataValue::Integer16(VERSION),
        XDataValue::String(KEY_KIND.into()),
        XDataValue::String(kind.key().into()),
        XDataValue::String(KEY_CUT.into()),
        XDataValue::String(cut.into()),
        XDataValue::String(KEY_PHASE.into()),
        XDataValue::String(phase.into()),
    ] {
        rec.add_value(value);
    }
    rec
}

/// Taglio (o prospetto) e fase di un record `ARCHLINE_SEZIONE`.
pub fn decode(rec: &ExtendedDataRecord) -> Option<(String, String)> {
    decode_with_kind(rec).map(|(_, cut, phase)| (cut, phase))
}

/// Tipo, nome e fase; senza `tipo` (record di prima) è una sezione.
pub fn decode_with_kind(rec: &ExtendedDataRecord) -> Option<(DrawingKind, String, String)> {
    let (mut kind, mut cut, mut phase) = (DrawingKind::Sezione, None, None);
    let values = &rec.values;
    let mut i = 0;
    while i + 1 < values.len() {
        match (&values[i], &values[i + 1]) {
            (XDataValue::String(k), XDataValue::String(v)) if k == KEY_KIND => {
                kind = DrawingKind::from_key(v).unwrap_or(kind);
                i += 2;
            }
            (XDataValue::String(k), XDataValue::String(v)) if k == KEY_CUT => {
                cut = Some(v.clone());
                i += 2;
            }
            (XDataValue::String(k), XDataValue::String(v)) if k == KEY_PHASE => {
                phase = Some(v.clone());
                i += 2;
            }
            _ => i += 1,
        }
    }
    Some((kind, cut?, phase?))
}

/// L'angolo in basso a sinistra e in alto a destra delle entità (linee,
/// polilinee e punti d'inserimento dei testi), in unità del disegno.
pub fn bbox(entities: &[EntityType]) -> Option<([f64; 2], [f64; 2])> {
    let mut points: Vec<[f64; 2]> = Vec::new();
    for entity in entities {
        match entity {
            EntityType::Line(l) => {
                points.push([l.start.x, l.start.y]);
                points.push([l.end.x, l.end.y]);
            }
            EntityType::LwPolyline(p) => {
                points.extend(p.vertices.iter().map(|v| [v.location.x, v.location.y]));
            }
            EntityType::MText(t) => points.push([t.insertion_point.x, t.insertion_point.y]),
            _ => {}
        }
    }
    let first = *points.first()?;
    Some(points.iter().fold((first, first), |(lo, hi), p| {
        (
            [lo[0].min(p[0]), lo[1].min(p[1])],
            [hi[0].max(p[0]), hi[1].max(p[1])],
        )
    }))
}

/// Le entità dei disegni di `drawings` (a partire da `(0; 0)`) traslate e
/// impilate dall'alto da `anchor` (l'angolo in alto a sinistra del primo
/// disegno), con il record `ARCHLINE_SEZIONE` di ognuna. `text_height` è
/// l'altezza dei testi, per i margini.
pub fn place(drawings: Vec<SectionDrawing>, anchor: [f64; 2], text_height: f64) -> Vec<EntityType> {
    let mut out = Vec::new();
    let mut top = anchor[1];
    for drawing in drawings {
        let Some((lo, hi)) = bbox(&drawing.entities) else {
            continue;
        };
        // In alto il disegno arriva a `hi.y`; in basso il punto più basso è
        // l'inserimento dei limiti, e il loro testo scende ancora di qualche riga.
        let bottom = lo[1] - LIMITS_DEPTH_TEXTS * text_height;
        let shift = DVec3::new(anchor[0] - lo[0], top - hi[1], 0.0);
        let rec = record(drawing.kind, &drawing.cut_name, drawing.phase.key());
        for mut entity in drawing.entities {
            crate::entities::traits::EntityTypeOps::apply_transform(
                &mut entity,
                &crate::command::EntityTransform::Translate(shift),
            );
            entity.common_mut().extended_data.upsert_record(rec.clone());
            out.push(entity);
        }
        top = bottom + shift.y - GAP_TEXTS * text_height;
    }
    out
}

/// Le entità già posate di un tipo (taglio o prospetto e fase di ognuna), nello
/// spazio modello.
pub fn drawings_in(doc: &Document, kind: DrawingKind) -> Vec<(Handle, String, String)> {
    doc.block_records
        .iter()
        .filter(|record| record.is_model_space())
        .flat_map(|record| record.entity_handles.iter().copied())
        .filter_map(|handle| {
            let rec = doc.get_entity(handle)?.common().extended_data.get_record(APP_SEZIONE)?;
            let (found, cut, phase) = decode_with_kind(rec)?;
            (found == kind).then_some((handle, cut, phase))
        })
        .collect()
}

/// Le entità delle sezioni già posate.
pub fn sections_in(doc: &Document) -> Vec<(Handle, String, String)> {
    drawings_in(doc, DrawingKind::Sezione)
}

/// L'angolo in alto a sinistra delle entità già posate di un tipo, e di un
/// solo nome se dato.
pub fn existing_anchor_of(doc: &Document, kind: DrawingKind, name: Option<&str>) -> Option<[f64; 2]> {
    let entities: Vec<EntityType> = drawings_in(doc, kind)
        .into_iter()
        .filter(|(_, cut, _)| name.map_or(true, |n| n == cut))
        .filter_map(|(handle, _, _)| doc.get_entity(handle).cloned())
        .collect();
    bbox(&entities).map(|(lo, hi)| [lo[0], hi[1]])
}

/// L'angolo in alto a sinistra dell'insieme delle sezioni già posate.
pub fn existing_anchor(doc: &Document) -> Option<[f64; 2]> {
    existing_anchor_of(doc, DrawingKind::Sezione, None)
}

/// Manca il layer o l'APPID delle sezioni?
pub fn missing_layer(doc: &Document) -> bool {
    !doc.layers.contains(LAYER_SEZIONI) || !doc.app_ids.contains(APP_SEZIONE)
}

/// Crea il layer `AL-SEZIONI` e l'APPID che mancano, con il loro handle. Niente
/// undo: lo apre il chiamante.
pub fn ensure_layer(doc: &mut Document) {
    if !doc.layers.contains(LAYER_SEZIONI) {
        let mut layer = Layer::new(LAYER_SEZIONI);
        layer.handle = doc.allocate_handle();
        layer.color = Color::from_index(ACI_BIANCO);
        let _ = doc.layers.add(layer);
    }
    if !doc.app_ids.contains(APP_SEZIONE) {
        let mut app_id = AppId::new(APP_SEZIONE);
        app_id.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bim::height::{Height, HeightState};
    use crate::bim::section::{Element, Span, SpanKind};
    use crate::bim::section_build::SectionPhase;
    use crate::bim::section_draw::{draw_section, Tint};

    fn drawing(name: &str, phase: SectionPhase) -> SectionDrawing {
        let height = Height {
            value_m: Some(2.7),
            state: HeightState::Inserita,
            note: None,
        };
        let list = vec![(
            Element::Mass {
                span: Span {
                    from: 1.0,
                    to: 1.3,
                    kind: SpanKind::Mass,
                },
                height,
                holes: Vec::new(),
            },
            Tint::Plain,
        )];
        SectionDrawing {
            kind: DrawingKind::Sezione,
            cut_name: name.into(),
            phase,
            entities: draw_section(&format!("Sezione {name}"), &list, [0.0, 0.0], 1.0, 0.25),
        }
    }

    /// Il primo disegno ha l'angolo in alto a sinistra sull'ancora, i
    /// successivi stanno sotto, staccati di quattro altezze di testo, con la
    /// stessa X.
    #[test]
    fn drawings_are_stacked_from_the_anchor_downwards() {
        let drawings = vec![
            drawing("A-A", SectionPhase::Fatto),
            drawing("A-A", SectionPhase::Progetto),
        ];
        let placed = place(drawings, [100.0, 50.0], 0.25);
        let (lo, hi) = bbox(&placed).unwrap();
        assert!((lo[0] - 100.0).abs() < 1e-9, "{lo:?}");
        assert!((hi[1] - 50.0).abs() < 1e-9, "il primo arriva all'ancora: {hi:?}");
        // Due disegni uguali: i muri (poligoni) sono a due quote, la seconda sotto
        // la prima di altezza + testi + stacco.
        let tops: Vec<f64> = placed
            .iter()
            .filter_map(|e| match e {
                EntityType::LwPolyline(p) => {
                    Some(p.vertices.iter().map(|v| v.location.y).fold(f64::MIN, f64::max))
                }
                _ => None,
            })
            .collect();
        assert_eq!(tops.len(), 2);
        assert!((tops[0] - 50.0).abs() < 1e-9, "{tops:?}");
        // Pavimento a 50 − 2,7; i limiti partono 3 testi sotto (0,75) e
        // scendono di altri 3, poi lo stacco di 4 testi: il secondo arriva in
        // alto a 47,3 − 0,75 − 0,75 − 1,0.
        let floor1 = 50.0 - 2.7;
        let top2 = floor1 - 3.0 * 0.25 - 3.0 * 0.25 - 4.0 * 0.25;
        assert!((tops[1] - top2).abs() < 1e-9, "{tops:?} contro {top2}");
        for e in &placed {
            let rec = e.common().extended_data.get_record(APP_SEZIONE).expect("record");
            assert_eq!(decode(rec).map(|(cut, _)| cut), Some("A-A".to_string()));
        }
    }

    #[test]
    fn the_record_roundtrips_and_the_drawings_are_found_again() {
        let mut doc = Document::new();
        ensure_layer(&mut doc);
        for e in place(vec![drawing("B-B", SectionPhase::Comparativa)], [0.0, 0.0], 0.25) {
            doc.add_entity(e).unwrap();
        }
        let found = sections_in(&doc);
        assert!(!found.is_empty());
        assert!(found.iter().all(|(_, cut, phase)| cut == "B-B" && phase == "demolizioni-costruzioni"));
        assert!(!missing_layer(&doc));
    }

    /// L'angolo in alto a sinistra dell'insieme già posato è quello della
    /// prossima posa.
    #[test]
    fn the_existing_anchor_is_the_top_left_of_what_is_there() {
        let mut doc = Document::new();
        ensure_layer(&mut doc);
        assert_eq!(existing_anchor(&doc), None);
        for e in place(vec![drawing("A-A", SectionPhase::Fatto)], [30.0, 20.0], 0.25) {
            doc.add_entity(e).unwrap();
        }
        let anchor = existing_anchor(&doc).unwrap();
        assert!((anchor[0] - 30.0).abs() < 1e-9 && (anchor[1] - 20.0).abs() < 1e-9, "{anchor:?}");
    }

    #[test]
    fn nothing_to_place_gives_nothing() {
        assert!(place(Vec::new(), [0.0, 0.0], 0.25).is_empty());
    }
}
