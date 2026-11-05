//! Modello 3D, quarto passo (spec parte 5 §4.5): le facce di
//! [`super::model3d`] come `3DFACE` nel disegno. Logica pura: dà ai modelli i
//! layer, i colori e il record `ARCHLINE_MODELLO3D` (fase), li affianca a partire
//! da un angolo e ritrova quelli già posati.
//!
//! - ogni `3DFACE` sta su `AL-3D-FATTO` o `AL-3D-PROGETTO` con il record
//!   `Integer16(1)`, `"fase"` + `"fatto"|"progetto"`; massa grigio scuro (ACI 8),
//!   pavimenti grigio chiaro (ACI 9). L'XDATA lo tolgono `AL_ESTRAI` e le copie
//!   fuori dal disegno di lavoro (prefisso `ARCHLINE_`);
//! - i due modelli stanno affiancati lungo X a partire da `anchor` (l'angolo in
//!   basso a sinistra del primo): il fatto prima, il progetto dopo, staccati di
//!   [`GAP_RATIO`] volte la larghezza del primo (almeno [`MIN_GAP_M`] m); le
//!   quote Z non cambiano;
//! - quelli già posati si ritrovano dal record ([`models_in`]); l'angolo
//!   in basso a sinistra del loro insieme è l'angolo della posa successiva:
//!   rigenerare lascia i modelli dove sono (come le sezioni, R117).

use codec::entities::Face3D;
use codec::tables::{AppId, Layer};
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{Color, Document, EntityType, Handle, Vector3};

use super::mass3d_prism::{Face3, Tone};
use super::model3d::Model3d;
use super::APP_MODELLO3D;

/// Layer dei modelli.
pub const LAYER_FATTO: &str = "AL-3D-FATTO";
pub const LAYER_PROGETTO: &str = "AL-3D-PROGETTO";

const VERSION: i16 = 1;
const KEY_PHASE: &str = "fase";
const ACI_MASS: i16 = 8;
const ACI_FLOOR: i16 = 9;
const ACI_LAYER: i16 = 7;
/// Distanza fra i due modelli, in frazione della larghezza del primo.
pub const GAP_RATIO: f64 = 0.25;
/// E non meno di questo, in metri.
pub const MIN_GAP_M: f64 = 2.0;

/// Quale dei due modelli.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelPhase {
    Fatto,
    Progetto,
}

impl ModelPhase {
    pub fn key(self) -> &'static str {
        match self {
            Self::Fatto => "fatto",
            Self::Progetto => "progetto",
        }
    }

    pub fn from_key(text: &str) -> Option<Self> {
        match text {
            "fatto" => Some(Self::Fatto),
            "progetto" => Some(Self::Progetto),
            _ => None,
        }
    }

    pub fn layer(self) -> &'static str {
        match self {
            Self::Fatto => LAYER_FATTO,
            Self::Progetto => LAYER_PROGETTO,
        }
    }
}

fn record(phase: ModelPhase) -> ExtendedDataRecord {
    let mut rec = ExtendedDataRecord::new(APP_MODELLO3D);
    for value in [
        XDataValue::Integer16(VERSION),
        XDataValue::String(KEY_PHASE.into()),
        XDataValue::String(phase.key().into()),
    ] {
        rec.add_value(value);
    }
    rec
}

/// La fase di un record `ARCHLINE_MODELLO3D`.
pub fn decode(rec: &ExtendedDataRecord) -> Option<ModelPhase> {
    let values = &rec.values;
    (0..values.len().saturating_sub(1)).find_map(|i| match (&values[i], &values[i + 1]) {
        (XDataValue::String(k), XDataValue::String(v)) if k == KEY_PHASE => ModelPhase::from_key(v),
        _ => None,
    })
}

/// L'angolo in basso a sinistra (X, Y) e la larghezza in X delle facce.
pub fn extent(faces: &[Face3]) -> Option<([f64; 2], f64)> {
    let mut xs = faces.iter().flat_map(|f| f.quad.iter().map(|p| p[0]));
    let first = xs.next()?;
    let (lo, hi) = xs.fold((first, first), |(lo, hi), x| (lo.min(x), hi.max(x)));
    let ymin = faces
        .iter()
        .flat_map(|f| f.quad.iter().map(|p| p[1]))
        .fold(f64::MAX, f64::min);
    Some(([lo, ymin], hi - lo))
}

fn entity(face: &Face3, phase: ModelPhase, shift: [f64; 2]) -> EntityType {
    let v = |p: [f64; 3]| Vector3::new(p[0] + shift[0], p[1] + shift[1], p[2]);
    let mut e = Face3D::new(v(face.quad[0]), v(face.quad[1]), v(face.quad[2]), v(face.quad[3]));
    e.common.layer = phase.layer().to_string();
    e.common.color = Color::from_index(match face.tone {
        Tone::Mass => ACI_MASS,
        Tone::Floor => ACI_FLOOR,
    });
    e.common.extended_data.upsert_record(record(phase));
    EntityType::Face3D(e)
}

/// Le `3DFACE` dei due modelli, il fatto con il suo angolo in basso a sinistra
/// in `anchor` e il progetto a destra del fatto (`per_metre` per lo stacco).
pub fn place(
    fatto: &Model3d,
    progetto: &Model3d,
    anchor: [f64; 2],
    per_metre: f64,
) -> Vec<EntityType> {
    let mut out = Vec::new();
    let mut next_x = anchor[0];
    for (model, phase) in [(fatto, ModelPhase::Fatto), (progetto, ModelPhase::Progetto)] {
        let Some((lo, width)) = extent(&model.faces) else {
            continue;
        };
        let shift = [next_x - lo[0], anchor[1] - lo[1]];
        out.extend(model.faces.iter().map(|f| entity(f, phase, shift)));
        next_x += width + (width * GAP_RATIO).max(MIN_GAP_M * per_metre);
    }
    out
}

/// Le `3DFACE` dei modelli già posate (handle e fase), nello spazio modello.
pub fn models_in(doc: &Document) -> Vec<(Handle, ModelPhase)> {
    doc.block_records
        .iter()
        .filter(|record| record.is_model_space())
        .flat_map(|record| record.entity_handles.iter().copied())
        .filter_map(|handle| {
            let EntityType::Face3D(face) = doc.get_entity(handle)? else {
                return None;
            };
            Some((handle, decode(face.common.extended_data.get_record(APP_MODELLO3D)?)?))
        })
        .collect()
}

/// L'angolo in basso a sinistra dell'insieme dei modelli già posati.
pub fn existing_anchor(doc: &Document) -> Option<[f64; 2]> {
    let mut points: Vec<[f64; 2]> = Vec::new();
    for (handle, _) in models_in(doc) {
        if let Some(EntityType::Face3D(face)) = doc.get_entity(handle) {
            for c in [
                face.first_corner,
                face.second_corner,
                face.third_corner,
                face.fourth_corner,
            ] {
                points.push([c.x, c.y]);
            }
        }
    }
    let first = *points.first()?;
    Some(points.iter().fold(first, |lo, p| [lo[0].min(p[0]), lo[1].min(p[1])]))
}

/// Mancano i layer o l'APPID dei modelli?
pub fn missing_layers(doc: &Document) -> bool {
    !doc.layers.contains(LAYER_FATTO)
        || !doc.layers.contains(LAYER_PROGETTO)
        || !doc.app_ids.contains(APP_MODELLO3D)
}

/// Crea i layer e l'APPID che mancano, con il loro handle. Niente undo: lo apre
/// il chiamante.
pub fn ensure_layers(doc: &mut Document) {
    for name in [LAYER_FATTO, LAYER_PROGETTO] {
        if !doc.layers.contains(name) {
            let mut layer = Layer::new(name);
            layer.handle = doc.allocate_handle();
            layer.color = Color::from_index(ACI_LAYER);
            let _ = doc.layers.add(layer);
        }
    }
    if !doc.app_ids.contains(APP_MODELLO3D) {
        let mut app_id = AppId::new(APP_MODELLO3D);
        app_id.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn quad(points: [[f64; 3]; 4], tone: Tone) -> Face3 {
        Face3 { quad: points, tone }
    }

    /// Due modelli minimi: un coperchio 2 × 1 a quota 2,7 e un pavimento.
    fn model(x0: f64) -> Model3d {
        Model3d {
            faces: vec![
                quad(
                    [[x0, 5.0, 2.7], [x0 + 2.0, 5.0, 2.7], [x0 + 2.0, 6.0, 2.7], [x0, 6.0, 2.7]],
                    Tone::Mass,
                ),
                quad(
                    [[x0, 5.0, 0.0], [x0 + 2.0, 5.0, 0.0], [x0 + 2.0, 6.0, 0.0], [x0, 6.0, 0.0]],
                    Tone::Floor,
                ),
            ],
            notices: Vec::new(),
            prisms: 1,
            floors: 1,
        }
    }

    /// Il fatto parte dall'ancora, il progetto sta a destra staccato di
    /// 25% della larghezza o 2 m; le quote Z non cambiano; layer, colori e record.
    #[test]
    fn models_are_placed_side_by_side_from_the_anchor() {
        let entities = place(&model(10.0), &model(10.0), [100.0, 50.0], 1.0);
        assert_eq!(entities.len(), 4);
        let xs = |layer: &str| -> (f64, f64) {
            let all: Vec<f64> = entities
                .iter()
                .filter_map(|e| match e {
                    EntityType::Face3D(f) if f.common.layer == layer => Some([
                        f.first_corner.x,
                        f.second_corner.x,
                        f.third_corner.x,
                        f.fourth_corner.x,
                    ]),
                    _ => None,
                })
                .flatten()
                .collect();
            (
                all.iter().cloned().fold(f64::MAX, f64::min),
                all.iter().cloned().fold(f64::MIN, f64::max),
            )
        };
        assert_eq!(xs(LAYER_FATTO), (100.0, 102.0));
        // 102 + max(25% di 2, 2 m) = 104
        assert_eq!(xs(LAYER_PROGETTO), (104.0, 106.0));
        for e in &entities {
            let EntityType::Face3D(f) = e else { panic!() };
            assert!((f.first_corner.y - 50.0).abs() < 1e-9 || (f.first_corner.y - 51.0).abs() < 1e-9);
            assert!(f.common.extended_data.get_record(APP_MODELLO3D).is_some());
            let want = if (f.first_corner.z - 2.7).abs() < 1e-9 { 8 } else { 9 };
            assert_eq!(f.common.color, Color::from_index(want));
        }
    }

    #[test]
    fn placed_models_are_found_again_with_their_phase_and_anchor() {
        let mut doc = Document::new();
        ensure_layers(&mut doc);
        assert!(!missing_layers(&doc));
        assert_eq!(existing_anchor(&doc), None);
        for e in place(&model(0.0), &model(0.0), [30.0, 20.0], 1.0) {
            doc.add_entity(e).unwrap();
        }
        let found = models_in(&doc);
        assert_eq!(found.iter().filter(|(_, p)| *p == ModelPhase::Fatto).count(), 2);
        assert_eq!(found.iter().filter(|(_, p)| *p == ModelPhase::Progetto).count(), 2);
        assert_eq!(existing_anchor(&doc), Some([30.0, 20.0]));
    }

    /// Le `3DFACE` sopravvivono al salvataggio in DXF e in DWG con layer, fase
    /// e coordinate.
    #[test]
    fn faces_survive_a_dxf_and_a_dwg_save() {
        let mut doc = Document::new();
        ensure_layers(&mut doc);
        for e in place(&model(0.0), &model(0.0), [0.0, 0.0], 1.0) {
            doc.add_entity(e).unwrap();
        }
        let bytes = codec::DxfWriter::new(&doc).write_to_vec().unwrap();
        let read = codec::DxfReader::from_reader(Cursor::new(bytes)).unwrap().read().unwrap();
        let found = models_in(&read);
        assert_eq!(found.len(), 4, "DXF");
        assert!(!missing_layers(&read));
        assert_eq!(existing_anchor(&read), Some([0.0, 0.0]));

        let bytes = codec::DwgWriter::write_to_vec(&doc).unwrap();
        let read = codec::DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
        assert_eq!(models_in(&read).len(), 4, "DWG");
        assert!(!missing_layers(&read), "layer e APPID devono sopravvivere al DWG");
    }

    #[test]
    fn an_empty_model_gives_nothing_and_the_record_roundtrips() {
        assert!(place(&Model3d::default(), &Model3d::default(), [0.0, 0.0], 1.0).is_empty());
        assert_eq!(decode(&record(ModelPhase::Progetto)), Some(ModelPhase::Progetto));
        assert_eq!(ModelPhase::from_key("altro"), None);
    }
}
