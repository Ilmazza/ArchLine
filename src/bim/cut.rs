//! Taglio di sezione (spec parte 4 §4.1): una `LINE` sul layer `AL-TAGLI` con
//! un record XDATA `ARCHLINE_TAGLIO`. Il tracciato è la linea stessa (si sposta,
//! ruota e scala con le trasformazioni del CAD); il record dice il **nome**
//! («A-A») e il **punto di vista**, un punto dal lato in cui si guarda.
//!
//! Sequenza XDATA, in quest'ordine: `Integer16(1)` (versione), `"nome"` +
//! stringa, `"vista"` + `Position3D`. La lettura va per chiave e salta le
//! sconosciute. Il punto di vista è una posizione (1011) perché segue MOVE,
//! COPY, ROTATE, SCALE e MIRROR come l'asse delle aperture.
//!
//! Orientamento (R114): chi guarda sta con le spalle al lato opposto al punto di
//! vista e guarda verso di esso; il disegno della sezione va da sinistra a
//! destra come lo vede lui. Se il punto di vista è a **sinistra** del verso
//! `from → to` il verso del taglio è già da sinistra a destra, altrimenti si
//! legge da `to` a `from` ([`Cut::oriented`]).
//!
//! Il primo giro non ha profondità di vista (R114): la sezione mostra solo il
//! piano di taglio, non le masse oltre.
//!
//! Qui non c'è undo: lo apre il comando che crea il taglio.

use codec::entities::{EntityCommon, Line};
use codec::tables::{AppId, Layer};
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{Color, Document, EntityType, Handle, Vector3};

use super::{APP_TAGLIO, LAYER_TAGLI};

/// Versione del record XDATA.
const VERSION: i16 = 1;
/// Colore ACI del layer dei tagli (bianco, come i locali).
const ACI_BIANCO: i16 = 7;

const KEY_NAME: &str = "nome";
const KEY_VIEW: &str = "vista";

/// Un taglio di sezione.
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    pub name: String,
    pub from: [f64; 2],
    pub to: [f64; 2],
    /// Un punto dal lato in cui si guarda.
    pub view: [f64; 2],
}

impl Cut {
    /// Il taglio da sinistra a destra come lo vede chi guarda: `(from, to)` se
    /// il punto di vista sta a sinistra del verso del tracciato, scambiati se
    /// sta a destra (un punto sul tracciato non ha lato: resta com'è).
    pub fn oriented(&self) -> ([f64; 2], [f64; 2]) {
        let d = [self.to[0] - self.from[0], self.to[1] - self.from[1]];
        let v = [self.view[0] - self.from[0], self.view[1] - self.from[1]];
        let side = d[0] * v[1] - d[1] * v[0];
        if side < 0.0 {
            (self.to, self.from)
        } else {
            (self.from, self.to)
        }
    }

    /// Lunghezza del tracciato, in unità del disegno.
    pub fn length(&self) -> f64 {
        (self.to[0] - self.from[0]).hypot(self.to[1] - self.from[1])
    }
}

/// Un taglio letto dal documento.
#[derive(Clone, Debug, PartialEq)]
pub struct CutGeom {
    pub handle: Handle,
    pub cut: Cut,
}

fn key(text: &str) -> XDataValue {
    XDataValue::String(text.to_string())
}

/// La sequenza dei valori del record `ARCHLINE_TAGLIO`.
pub fn encode(cut: &Cut) -> Vec<XDataValue> {
    vec![
        XDataValue::Integer16(VERSION),
        key(KEY_NAME),
        key(&cut.name),
        key(KEY_VIEW),
        XDataValue::Position3D(Vector3::new(cut.view[0], cut.view[1], 0.0)),
    ]
}

/// Legge nome e punto di vista; `None` senza un nome non vuoto e un punto di
/// vista leggibili.
pub fn decode(rec: &ExtendedDataRecord) -> Option<(String, [f64; 2])> {
    let (mut name, mut view) = (None, None);
    let values = &rec.values;
    let mut i = 0;
    while i < values.len() {
        let XDataValue::String(label) = &values[i] else {
            i += 1;
            continue;
        };
        match (label.as_str(), values.get(i + 1)) {
            (KEY_NAME, Some(XDataValue::String(text))) => {
                name = Some(text.clone());
                i += 2;
            }
            (KEY_VIEW, Some(XDataValue::Position3D(p) | XDataValue::Point3D(p))) => {
                view = Some([p.x, p.y]);
                i += 2;
            }
            _ => i += 1,
        }
    }
    Some((name.filter(|n| !n.trim().is_empty())?, view?))
}

fn style(common: &mut EntityCommon, cut: &Cut) {
    common.layer = LAYER_TAGLI.to_string();
    common.color = Color::ByLayer;
    let mut rec = ExtendedDataRecord::new(APP_TAGLIO);
    for value in encode(cut) {
        rec.add_value(value);
    }
    common.extended_data.upsert_record(rec);
}

/// Il taglio come `LINE` con il suo record. `None` se il tracciato è nullo.
/// L'handle lo assegna il documento all'inserimento.
pub fn cut_line(cut: &Cut) -> Option<EntityType> {
    if !(cut.length() > 0.0) || !cut.length().is_finite() {
        return None;
    }
    let mut line = Line::from_points(
        Vector3::new(cut.from[0], cut.from[1], 0.0),
        Vector3::new(cut.to[0], cut.to[1], 0.0),
    );
    style(&mut line.common, cut);
    Some(EntityType::Line(line))
}

/// Il taglio descritto da `entity`, se è una `LINE` con un record leggibile. Il
/// tracciato è quello della linea (può essere stato trasformato).
pub fn read_cut(handle: Handle, entity: &EntityType) -> Option<CutGeom> {
    let EntityType::Line(line) = entity else {
        return None;
    };
    let (name, view) = decode(line.common.extended_data.get_record(APP_TAGLIO)?)?;
    Some(CutGeom {
        handle,
        cut: Cut {
            name,
            from: [line.start.x, line.start.y],
            to: [line.end.x, line.end.y],
            view,
        },
    })
}

/// I tagli dello spazio modello, in ordine di disegno.
pub fn cuts_in(doc: &Document) -> Vec<CutGeom> {
    doc.block_records
        .iter()
        .filter(|record| record.is_model_space())
        .flat_map(|record| record.entity_handles.iter())
        .filter_map(|&handle| read_cut(handle, doc.get_entity(handle)?))
        .collect()
}

/// Il nome proposto per un nuovo taglio: «A-A», «B-B»… la prima lettera non
/// ancora usata (senza badare alle maiuscole); dopo la «Z» niente proposta.
pub fn suggested_name(existing: &[String]) -> Option<String> {
    ('A'..='Z')
        .map(|letter| format!("{letter}-{letter}"))
        .find(|name| !existing.iter().any(|used| used.eq_ignore_ascii_case(name)))
}

/// Manca il layer o l'APPID dei tagli?
pub fn missing_layer(doc: &Document) -> bool {
    !doc.layers.contains(LAYER_TAGLI) || !doc.app_ids.contains(APP_TAGLIO)
}

/// Crea il layer `AL-TAGLI` e l'APPID che mancano, con il loro handle. Niente
/// undo: lo apre il chiamante.
pub fn ensure_layer(doc: &mut Document) {
    if !doc.layers.contains(LAYER_TAGLI) {
        let mut layer = Layer::new(LAYER_TAGLI);
        layer.handle = doc.allocate_handle();
        layer.color = Color::from_index(ACI_BIANCO);
        let _ = doc.layers.add(layer);
    }
    if !doc.app_ids.contains(APP_TAGLIO) {
        let mut app_id = AppId::new(APP_TAGLIO);
        app_id.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn cut() -> Cut {
        Cut {
            name: "A-A".into(),
            from: [2.0, -1.0],
            to: [2.0, 4.0],
            view: [3.0, 1.5],
        }
    }

    fn doc_with(cuts: &[Cut]) -> (Document, Vec<Handle>) {
        let mut doc = Document::new();
        ensure_layer(&mut doc);
        let handles = cuts
            .iter()
            .map(|c| doc.add_entity(cut_line(c).unwrap()).unwrap())
            .collect();
        (doc, handles)
    }

    #[test]
    fn xdata_sequence_is_the_documented_one() {
        assert_eq!(
            encode(&cut()),
            vec![
                XDataValue::Integer16(1),
                XDataValue::String("nome".into()),
                XDataValue::String("A-A".into()),
                XDataValue::String("vista".into()),
                XDataValue::Position3D(Vector3::new(3.0, 1.5, 0.0)),
            ]
        );
        let mut rec = ExtendedDataRecord::new(APP_TAGLIO);
        for v in encode(&cut()) {
            rec.add_value(v);
        }
        assert_eq!(decode(&rec), Some(("A-A".to_string(), [3.0, 1.5])));
        rec.add_value(XDataValue::String("futuro".into()));
        rec.add_value(XDataValue::Real(1.0));
        assert_eq!(decode(&rec), Some(("A-A".to_string(), [3.0, 1.5])), "sconosciute saltate");
        assert_eq!(decode(&ExtendedDataRecord::new(APP_TAGLIO)), None);
    }

    #[test]
    fn the_cut_is_a_line_on_its_layer_and_reads_back() {
        let (doc, handles) = doc_with(&[cut()]);
        let entity = doc.get_entity(handles[0]).unwrap();
        assert!(matches!(entity, EntityType::Line(_)));
        assert_eq!(entity.common().layer, LAYER_TAGLI);
        let found = cuts_in(&doc);
        assert_eq!(found, vec![CutGeom { handle: handles[0], cut: cut() }]);
        assert!(cut_line(&Cut { to: [2.0, -1.0], ..cut() }).is_none(), "tracciato nullo");
    }

    /// Il punto di vista a sinistra del verso lascia il taglio com'è; a destra
    /// lo scambia, così il disegno va sempre da sinistra a destra per chi guarda.
    #[test]
    fn orientation_follows_the_side_of_the_view_point() {
        // Verso nord (0,5): a sinistra c'è ovest (x minore).
        let looking_west = Cut { view: [1.0, 1.5], ..cut() };
        assert_eq!(looking_west.oriented(), ([2.0, -1.0], [2.0, 4.0]));
        let looking_east = Cut { view: [3.0, 1.5], ..cut() };
        assert_eq!(looking_east.oriented(), ([2.0, 4.0], [2.0, -1.0]));
        let on_the_line = Cut { view: [2.0, 1.5], ..cut() };
        assert_eq!(on_the_line.oriented(), ([2.0, -1.0], [2.0, 4.0]));
    }

    #[test]
    fn next_name_is_the_first_unused_letter() {
        assert_eq!(suggested_name(&[]), Some("A-A".into()));
        assert_eq!(suggested_name(&["A-A".into(), "c-c".into()]), Some("B-B".into()));
        let all: Vec<String> = ('A'..='Z').map(|l| format!("{l}-{l}")).collect();
        assert_eq!(suggested_name(&all), None);
    }

    /// Tracciato e punto di vista seguono le trasformazioni; il nome no.
    #[test]
    fn the_cut_moves_with_the_drawing() {
        use crate::command::EntityTransform as T;
        let mut entity = cut_line(&cut()).unwrap();
        crate::entities::traits::EntityTypeOps::apply_transform(
            &mut entity,
            &T::Translate(glam::DVec3::new(10.0, 0.0, 0.0)),
        );
        let found = read_cut(Handle::NULL, &entity).unwrap().cut;
        assert_eq!(found.from, [12.0, -1.0]);
        assert_eq!(found.to, [12.0, 4.0]);
        assert_eq!(found.view, [13.0, 1.5]);
        assert_eq!(found.name, "A-A");
    }

    #[test]
    fn cuts_survive_a_dxf_and_a_dwg_save() {
        let (doc, _) = doc_with(&[cut()]);
        let bytes = codec::DxfWriter::new(&doc).write_to_vec().unwrap();
        let read = codec::DxfReader::from_reader(Cursor::new(bytes)).unwrap().read().unwrap();
        assert_eq!(cuts_in(&read).iter().map(|c| c.cut.clone()).collect::<Vec<_>>(), vec![cut()]);
        assert!(!missing_layer(&read));

        let bytes = codec::DwgWriter::write_to_vec(&doc).unwrap();
        let read = codec::DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
        assert_eq!(cuts_in(&read).iter().map(|c| c.cut.clone()).collect::<Vec<_>>(), vec![cut()]);
        assert!(!missing_layer(&read), "layer e APPID devono sopravvivere al DWG");
    }

    #[test]
    fn ensure_layer_creates_once() {
        let mut doc = Document::new();
        assert!(missing_layer(&doc));
        ensure_layer(&mut doc);
        let count = doc.layers.iter().count();
        ensure_layer(&mut doc);
        assert_eq!(doc.layers.iter().count(), count);
        assert!(!doc.layers.get(LAYER_TAGLI).unwrap().handle.is_null());
    }
}
