//! Apertura: una porta, una finestra o una portafinestra in un muro. È **una
//! sola entità**, la `LWPOLYLINE` chiusa del vano in pianta (larghezza lungo
//! l'asse del muro × spessore del muro, da faccia a faccia) su `AL-APERTURE`,
//! con un record XDATA `ARCHLINE_APERTURA` (spec parte 3 §4.1, R96). Niente
//! ancora di famiglia come per i locali: una copia è un'apertura indipendente.
//!
//! Sequenza XDATA, in quest'ordine: `Integer16(1)` (versione), `"tipo"`,
//! `"porta"|"finestra"|"portafinestra"`, `"stato"`,
//! `"esistente"|"demolita"|"nuova"`, `"asse"` + due `Position3D`, `"spessore"` +
//! `Distance`, `"soglia"` + `Integer16(0|1)`, `"architrave"` + `Integer16(0|1)`,
//! poi le chiavi facoltative presenti: `"altezza"` + `Real`, `"davanzale"` +
//! `Real`, `"appoggio"` + `Real`. La lettura va per chiave e salta le
//! sconosciute.
//!
//! Asse e spessore sono posizione (1011) e distanza (1041): sono i tipi che il
//! codec porta dietro all'entità in MOVE, COPY, ROTATE, SCALE e MIRROR. Altezza,
//! davanzale e appoggio restano `Real`: sono dati dichiarati da chi disegna,
//! non misure del disegno, e non cambiano con la scala in pianta. Un'altezza
//! che manca resta `None` (`n.d.`): **mai un valore di ripiego** (spec §2).
//!
//! Qui non c'è undo: lo apre il comando che crea o cambia le aperture.

use codec::entities::{EntityCommon, Line, LwPolyline, LwVertex};
use codec::tables::{AppId, Layer};
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{Color, Document, EntityType, Handle, Vector2, Vector3};

use rustc_hash::FxHashMap;

use super::project::FIRST_SYNTHETIC_HANDLE;
use super::rooms::Phase;
use super::walls::{rect_from_axis_thickness, sources_of, WallRect};
use super::{ACI_GIALLO, ACI_ROSSO, APP_APERTURA, LAYER_APERTURE};
use crate::scene::BoundarySource;

/// Il layer delle linee di soglia sintetiche: non compaiono mai nel disegno.
const THRESHOLD_LAYER: &str = "0";

/// Versione del record XDATA.
const VERSION: i16 = 1;
/// Colore ACI del layer delle aperture.
const ACI_BIANCO: i16 = 7;

const KEY_KIND: &str = "tipo";
const KEY_STATE: &str = "stato";
const KEY_AXIS: &str = "asse";
const KEY_THICKNESS: &str = "spessore";
const KEY_THRESHOLD: &str = "soglia";
const KEY_LINTEL: &str = "architrave";
const KEY_HEIGHT: &str = "altezza";
const KEY_SILL: &str = "davanzale";
const KEY_BEARING: &str = "appoggio";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApertureKind {
    Porta,
    Finestra,
    Portafinestra,
}

impl ApertureKind {
    /// Testo nel record XDATA.
    pub fn key(self) -> &'static str {
        match self {
            Self::Porta => "porta",
            Self::Finestra => "finestra",
            Self::Portafinestra => "portafinestra",
        }
    }

    pub fn from_key(text: &str) -> Option<Self> {
        match text {
            "porta" => Some(Self::Porta),
            "finestra" => Some(Self::Finestra),
            "portafinestra" => Some(Self::Portafinestra),
            _ => None,
        }
    }

    /// Un vano a terra: la soglia ha senso e il davanzale no.
    pub fn is_door(self) -> bool {
        !matches!(self, Self::Finestra)
    }

    /// Prefisso del codice nell'abaco: `P1`, `F1`, `PF1`.
    pub fn code_prefix(self) -> &'static str {
        match self {
            Self::Porta => "P",
            Self::Finestra => "F",
            Self::Portafinestra => "PF",
        }
    }
}

/// Lo stato dell'apertura rispetto ai lavori.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApertureState {
    Esistente,
    Demolita,
    Nuova,
}

impl ApertureState {
    /// Testo nel record XDATA.
    pub fn key(self) -> &'static str {
        match self {
            Self::Esistente => "esistente",
            Self::Demolita => "demolita",
            Self::Nuova => "nuova",
        }
    }

    pub fn from_key(text: &str) -> Option<Self> {
        match text {
            "esistente" => Some(Self::Esistente),
            "demolita" => Some(Self::Demolita),
            "nuova" => Some(Self::Nuova),
            _ => None,
        }
    }

    /// Indice ACI del contorno: giallo se demolita, rosso se nuova; l'esistente
    /// segue il layer.
    pub fn aci(self) -> Option<i16> {
        match self {
            Self::Esistente => None,
            Self::Demolita => Some(ACI_GIALLO),
            Self::Nuova => Some(ACI_ROSSO),
        }
    }
}

/// Ciò che il record XDATA dice di un'apertura.
#[derive(Clone, Debug, PartialEq)]
pub struct Aperture {
    pub kind: ApertureKind,
    pub state: ApertureState,
    /// I due punti sull'asse del muro alle spalle del vano: la distanza è la
    /// larghezza (`dal disegno`).
    pub axis: ([f64; 2], [f64; 2]),
    /// Spessore del muro, in unità del disegno.
    pub thickness: f64,
    /// Altezza del vano, inserita; `None` = `n.d.`.
    pub height: Option<f64>,
    /// Altezza del davanzale da terra (finestre), inserita.
    pub sill: Option<f64>,
    /// La soglia chiude il vano nel calcolo dei locali e si computa.
    pub threshold: bool,
    /// L'architrave si computa.
    pub lintel: bool,
    /// Appoggio dell'architrave per lato, inserito.
    pub bearing: Option<f64>,
}

impl Aperture {
    /// Larghezza del vano: la distanza tra i due punti dell'asse.
    pub fn width(&self) -> f64 {
        let (a, b) = self.axis;
        (b[0] - a[0]).hypot(b[1] - a[1])
    }
}

/// Un'apertura letta dal documento, con il rettangolo del vano.
#[derive(Clone, Debug, PartialEq)]
pub struct ApertureGeom {
    pub handle: Handle,
    pub aperture: Aperture,
    /// I quattro angoli del vano, senza il punto finale ripetuto.
    pub ring: Vec<[f64; 2]>,
}

/// Un cambio di `AL_APERTURA_DATI` su un'apertura (spec parte 3 §5.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ApertureChange {
    Stato(ApertureState),
    /// `None` toglie il valore: l'altezza torna `n.d.`, mai un ripiego.
    Altezza(Option<f64>),
    Davanzale(Option<f64>),
    Appoggio(Option<f64>),
    Soglia(bool),
    Architrave(bool),
}

impl ApertureChange {
    /// L'apertura con il cambio, o il motivo per cui non si applica: il
    /// davanzale è solo delle finestre, la soglia solo di porte e
    /// portefinestre.
    pub fn apply(self, a: &Aperture) -> Result<Aperture, &'static str> {
        let mut new = a.clone();
        match self {
            Self::Stato(state) => new.state = state,
            Self::Altezza(value) => new.height = value,
            Self::Appoggio(value) => new.bearing = value,
            Self::Architrave(on) => new.lintel = on,
            Self::Davanzale(value) => {
                if a.kind != ApertureKind::Finestra {
                    return Err("il davanzale è solo delle finestre");
                }
                new.sill = value;
            }
            Self::Soglia(on) => {
                if !a.kind.is_door() {
                    return Err("la soglia è solo di porte e portefinestre");
                }
                new.threshold = on;
            }
        }
        Ok(new)
    }
}

fn key(text: &str) -> XDataValue {
    XDataValue::String(text.to_string())
}

fn point(p: [f64; 2]) -> XDataValue {
    XDataValue::Position3D(Vector3::new(p[0], p[1], 0.0))
}

fn read_point(value: Option<&XDataValue>) -> Option<[f64; 2]> {
    match value? {
        XDataValue::Position3D(p) | XDataValue::Point3D(p) => Some([p.x, p.y]),
        _ => None,
    }
}

/// La sequenza dei valori del record `ARCHLINE_APERTURA`.
pub fn encode(a: &Aperture) -> Vec<XDataValue> {
    let mut values = vec![
        XDataValue::Integer16(VERSION),
        key(KEY_KIND),
        key(a.kind.key()),
        key(KEY_STATE),
        key(a.state.key()),
        key(KEY_AXIS),
        point(a.axis.0),
        point(a.axis.1),
        key(KEY_THICKNESS),
        XDataValue::Distance(a.thickness),
        key(KEY_THRESHOLD),
        XDataValue::Integer16(i16::from(a.threshold)),
        key(KEY_LINTEL),
        XDataValue::Integer16(i16::from(a.lintel)),
    ];
    for (name, value) in [
        (KEY_HEIGHT, a.height),
        (KEY_SILL, a.sill),
        (KEY_BEARING, a.bearing),
    ] {
        if let Some(value) = value {
            values.extend([key(name), XDataValue::Real(value)]);
        }
    }
    values
}

/// Legge il record per chiave, saltando ciò che non conosce. `None` senza un
/// tipo, uno stato, un asse e uno spessore leggibili (e positivo).
pub fn decode(rec: &ExtendedDataRecord) -> Option<Aperture> {
    let (mut kind, mut state, mut axis, mut thickness) = (None, None, None, None);
    let (mut height, mut sill, mut bearing) = (None, None, None);
    let (mut threshold, mut lintel) = (false, false);
    let values = &rec.values;
    let mut i = 0;
    while i < values.len() {
        let XDataValue::String(name) = &values[i] else {
            i += 1;
            continue;
        };
        let next = values.get(i + 1);
        let used = match (name.as_str(), next) {
            (KEY_KIND, Some(XDataValue::String(t))) => {
                kind = ApertureKind::from_key(t);
                2
            }
            (KEY_STATE, Some(XDataValue::String(t))) => {
                state = ApertureState::from_key(t);
                2
            }
            (KEY_AXIS, Some(first)) => match (read_point(Some(first)), read_point(values.get(i + 2))) {
                (Some(a), Some(b)) => {
                    axis = Some((a, b));
                    3
                }
                _ => 1,
            },
            (KEY_THICKNESS, Some(XDataValue::Distance(t))) => {
                thickness = Some(*t);
                2
            }
            (KEY_THRESHOLD, Some(XDataValue::Integer16(v))) => {
                threshold = *v != 0;
                2
            }
            (KEY_LINTEL, Some(XDataValue::Integer16(v))) => {
                lintel = *v != 0;
                2
            }
            (KEY_HEIGHT, Some(XDataValue::Real(v))) => {
                height = Some(*v);
                2
            }
            (KEY_SILL, Some(XDataValue::Real(v))) => {
                sill = Some(*v);
                2
            }
            (KEY_BEARING, Some(XDataValue::Real(v))) => {
                bearing = Some(*v);
                2
            }
            _ => 1,
        };
        i += used;
    }
    let thickness = thickness.filter(|t| t.is_finite() && *t > 0.0)?;
    Some(Aperture {
        kind: kind?,
        state: state?,
        axis: axis?,
        thickness,
        height,
        sill,
        threshold,
        lintel,
        bearing,
    })
}

fn record(a: &Aperture) -> ExtendedDataRecord {
    let mut rec = ExtendedDataRecord::new(APP_APERTURA);
    for value in encode(a) {
        rec.add_value(value);
    }
    rec
}

/// Layer, colore e record XDATA secondo lo stato; gli XDATA di altre
/// applicazioni restano.
fn style(common: &mut EntityCommon, a: &Aperture) {
    common.layer = LAYER_APERTURE.to_string();
    common.color = a.state.aci().map_or(Color::ByLayer, Color::from_index);
    common.extended_data.upsert_record(record(a));
}

/// Il vano come `LWPOLYLINE` chiusa sul rettangolo dell'asse e dello spessore
/// (`walls::rect_from_axis_thickness`), con layer, colore e record. `None` se
/// l'asse è degenere. L'handle lo assegna il documento all'inserimento.
pub fn aperture_polyline(a: &Aperture) -> Option<EntityType> {
    let rect = rect_from_axis_thickness(a.axis.0, a.axis.1, a.thickness)?;
    let mut polyline = LwPolyline::new();
    polyline.vertices = rect
        .corners
        .iter()
        .map(|p| LwVertex::new(Vector2::new(p[0], p[1])))
        .collect();
    polyline.is_closed = true;
    style(&mut polyline.common, a);
    Some(EntityType::LwPolyline(polyline))
}

/// Porta layer, colore e record di `entity` allo stato di `a`. Non tocca
/// `raw_dwg_eed`: su un'apertura letta da un DWG il blocco grezzo prevale sul
/// record nuovo al salvataggio, quindi chi cambia un'apertura già nel
/// documento deve scartarlo (`set_entity_xdata`).
pub fn restyle(entity: &mut EntityType, a: &Aperture) {
    style(entity.common_mut(), a);
}

/// L'apertura descritta da `entity`, se è una `LWPOLYLINE` chiusa con un record
/// `ARCHLINE_APERTURA` leggibile.
pub fn read_aperture(handle: Handle, entity: &EntityType) -> Option<ApertureGeom> {
    let EntityType::LwPolyline(polyline) = entity else {
        return None;
    };
    let aperture = decode(polyline.common.extended_data.get_record(APP_APERTURA)?)?;
    if polyline.vertices.len() < 3 {
        return None;
    }
    let ring = polyline
        .vertices
        .iter()
        .map(|v| [v.location.x, v.location.y])
        .collect();
    Some(ApertureGeom {
        handle,
        aperture,
        ring,
    })
}

/// Le aperture dello spazio modello (non quelle in carta o dentro i blocchi).
pub fn apertures_in(doc: &Document) -> Vec<ApertureGeom> {
    doc.block_records
        .iter()
        .filter(|record| record.is_model_space())
        .flat_map(|record| record.entity_handles.iter())
        .filter_map(|&handle| read_aperture(handle, doc.get_entity(handle)?))
        .collect()
}

/// I due lati lunghi del vano di ogni porta o portafinestra con la soglia, per
/// la fase: nel calcolo dei locali chiudono il vano come le spallette di un
/// varco (spec parte 3 §4.2, R97). Una `finestra` non chiude nulla (il muro è
/// continuo nel rilievo). Lo stato di fatto vede le aperture esistenti e
/// quelle da demolire; quello di progetto le esistenti e le nuove.
pub fn threshold_segments(doc: &Document, phase: Phase) -> Vec<([f64; 2], [f64; 2])> {
    closing_rects(doc, phase)
        .into_iter()
        .flat_map(|rect| rect.long_sides())
        .collect()
}

/// Gli anelli dei vani che chiudono i locali nella fase (gli stessi rettangoli
/// di [`threshold_segments`]): l'area di un vano porta non è di nessun locale,
/// quindi la faccia che ne nasce non è mai un locale «(senza nome)».
pub fn vano_rings(doc: &Document, phase: Phase) -> Vec<Vec<[f64; 2]>> {
    closing_rects(doc, phase)
        .into_iter()
        .map(|rect| rect.ring())
        .collect()
}

fn closing_rects(doc: &Document, phase: Phase) -> Vec<WallRect> {
    apertures_in(doc)
        .into_iter()
        .filter(|geom| geom.aperture.threshold && geom.aperture.kind.is_door())
        .filter(|geom| match (phase, geom.aperture.state) {
            (_, ApertureState::Esistente) => true,
            (Phase::Fatto, ApertureState::Demolita) => true,
            (Phase::Progetto, ApertureState::Nuova) => true,
            _ => false,
        })
        .filter_map(|geom| {
            let a = geom.aperture;
            rect_from_axis_thickness(a.axis.0, a.axis.1, a.thickness)
        })
        .collect()
}

/// Aggiunge a `sources` (i muri del rilievo) le soglie delle aperture dello
/// stato di fatto come linee sintetiche, con handle sopra quelli dei muri. Il
/// disegno non si tocca: servono solo alle facce dei locali.
pub fn add_fatto_thresholds(
    doc: &Document,
    sources: &mut FxHashMap<Handle, BoundarySource>,
    tol: f64,
) {
    let first = sources
        .keys()
        .map(|handle| handle.value().saturating_add(1))
        .fold(FIRST_SYNTHETIC_HANDLE, u64::max);
    let lines: Vec<(Handle, EntityType)> = (0u64..)
        .zip(threshold_segments(doc, Phase::Fatto))
        .map(|(offset, (a, b))| {
            let handle = Handle::new(first.saturating_add(offset));
            let mut line = Line::from_points(
                Vector3::new(a[0], a[1], 0.0),
                Vector3::new(b[0], b[1], 0.0),
            );
            line.common.layer = THRESHOLD_LAYER.to_string();
            line.common.handle = handle;
            (handle, EntityType::Line(line))
        })
        .collect();
    sources.extend(sources_of(&lines, tol).0);
}

/// Manca il layer o l'APPID delle aperture?
pub fn missing_layer(doc: &Document) -> bool {
    !doc.layers.contains(LAYER_APERTURE) || !doc.app_ids.contains(APP_APERTURA)
}

/// Crea il layer `AL-APERTURE` e l'APPID che mancano, ognuno con il suo handle
/// (senza, si perderebbero al salvataggio DWG). Niente undo: lo apre il
/// chiamante. Separato da `zone::ensure_layers` (R98).
pub fn ensure_layer(doc: &mut Document) {
    if !doc.layers.contains(LAYER_APERTURE) {
        let mut layer = Layer::new(LAYER_APERTURE);
        layer.handle = doc.allocate_handle();
        layer.color = Color::from_index(ACI_BIANCO);
        let _ = doc.layers.add(layer);
    }
    if !doc.app_ids.contains(APP_APERTURA) {
        let mut app_id = AppId::new(APP_APERTURA);
        app_id.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// La porta di 0,90 m nel tramezzo del caso A (spessore 0,10).
    fn door() -> Aperture {
        Aperture {
            kind: ApertureKind::Porta,
            state: ApertureState::Esistente,
            axis: ([4.05, 1.0], [4.05, 1.9]),
            thickness: 0.10,
            height: Some(2.1),
            sill: None,
            threshold: true,
            lintel: false,
            bearing: None,
        }
    }

    fn doc_with(apertures: &[Aperture]) -> (Document, Vec<Handle>) {
        let mut doc = Document::new();
        ensure_layer(&mut doc);
        let handles = apertures
            .iter()
            .map(|a| doc.add_entity(aperture_polyline(a).unwrap()).unwrap())
            .collect();
        (doc, handles)
    }

    #[test]
    fn xdata_sequence_is_the_documented_one() {
        let a = Aperture {
            kind: ApertureKind::Finestra,
            state: ApertureState::Nuova,
            sill: Some(0.9),
            bearing: Some(0.2),
            lintel: true,
            threshold: false,
            ..door()
        };
        let want = vec![
            XDataValue::Integer16(1),
            XDataValue::String("tipo".into()),
            XDataValue::String("finestra".into()),
            XDataValue::String("stato".into()),
            XDataValue::String("nuova".into()),
            XDataValue::String("asse".into()),
            XDataValue::Position3D(Vector3::new(4.05, 1.0, 0.0)),
            XDataValue::Position3D(Vector3::new(4.05, 1.9, 0.0)),
            XDataValue::String("spessore".into()),
            XDataValue::Distance(0.10),
            XDataValue::String("soglia".into()),
            XDataValue::Integer16(0),
            XDataValue::String("architrave".into()),
            XDataValue::Integer16(1),
            XDataValue::String("altezza".into()),
            XDataValue::Real(2.1),
            XDataValue::String("davanzale".into()),
            XDataValue::Real(0.9),
            XDataValue::String("appoggio".into()),
            XDataValue::Real(0.2),
        ];
        assert_eq!(encode(&a), want);
        let mut rec = ExtendedDataRecord::new(APP_APERTURA);
        for v in want {
            rec.add_value(v);
        }
        assert_eq!(decode(&rec), Some(a));
    }

    /// Un'altezza che manca resta `None`: nessun valore di ripiego.
    #[test]
    fn missing_height_stays_missing() {
        let a = Aperture {
            height: None,
            ..door()
        };
        let mut rec = ExtendedDataRecord::new(APP_APERTURA);
        for v in encode(&a) {
            rec.add_value(v);
        }
        let back = decode(&rec).unwrap();
        assert_eq!(back.height, None);
        assert_eq!(back.sill, None);
        assert_eq!(back.bearing, None);
    }

    #[test]
    fn decode_skips_unknown_keys_and_rejects_broken_records() {
        let mut rec = ExtendedDataRecord::new(APP_APERTURA);
        for v in encode(&door()) {
            rec.add_value(v);
        }
        rec.add_value(XDataValue::String("futuro".into()));
        rec.add_value(XDataValue::Real(1.0));
        assert_eq!(decode(&rec), Some(door()));

        let without_axis = ExtendedDataRecord::new(APP_APERTURA);
        assert_eq!(decode(&without_axis), None);

        let zero = Aperture {
            thickness: 0.0,
            ..door()
        };
        let mut rec = ExtendedDataRecord::new(APP_APERTURA);
        for v in encode(&zero) {
            rec.add_value(v);
        }
        assert_eq!(decode(&rec), None, "spessore nullo");
    }

    #[test]
    fn width_is_the_distance_between_the_axis_points() {
        assert!((door().width() - 0.9).abs() < 1e-12);
    }

    #[test]
    fn polyline_is_the_closed_rectangle_on_its_layer_with_state_colour() {
        for (state, aci) in [
            (ApertureState::Esistente, None),
            (ApertureState::Demolita, Some(2)),
            (ApertureState::Nuova, Some(1)),
        ] {
            let entity = aperture_polyline(&Aperture { state, ..door() }).unwrap();
            let EntityType::LwPolyline(p) = &entity else {
                panic!("{entity:?}")
            };
            assert!(p.is_closed);
            assert_eq!(p.vertices.len(), 4);
            assert_eq!(p.common.layer, LAYER_APERTURE);
            let want = aci.map_or(Color::ByLayer, Color::from_index);
            assert_eq!(p.common.color, want, "{state:?}");
            for x in [4.0, 4.1] {
                for y in [1.0, 1.9] {
                    assert!(
                        p.vertices.iter().any(|v| (v.location.x - x).abs() < 1e-9
                            && (v.location.y - y).abs() < 1e-9),
                        "{x} {y}"
                    );
                }
            }
        }
        assert!(
            aperture_polyline(&Aperture {
                axis: ([1.0, 1.0], [1.0, 1.0]),
                ..door()
            })
            .is_none(),
            "asse degenere"
        );
    }

    #[test]
    fn apertures_in_reads_the_model_space_and_ignores_other_polylines() {
        let (mut doc, handles) = doc_with(&[door()]);
        let mut plain = LwPolyline::new();
        plain.vertices = vec![
            LwVertex::new(Vector2::new(0.0, 0.0)),
            LwVertex::new(Vector2::new(1.0, 0.0)),
            LwVertex::new(Vector2::new(1.0, 1.0)),
        ];
        plain.is_closed = true;
        doc.add_entity(EntityType::LwPolyline(plain)).unwrap();
        let found = apertures_in(&doc);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].handle, handles[0]);
        assert_eq!(found[0].aperture, door());
        assert_eq!(found[0].ring.len(), 4);
    }

    #[test]
    fn ensure_layer_creates_once_with_handles() {
        let mut doc = Document::new();
        assert!(missing_layer(&doc));
        ensure_layer(&mut doc);
        assert!(!missing_layer(&doc));
        let layer = doc.layers.get(LAYER_APERTURE).unwrap();
        assert!(!layer.handle.is_null());
        let count = doc.layers.iter().count();
        ensure_layer(&mut doc);
        assert_eq!(doc.layers.iter().count(), count, "una sola volta");
    }

    #[test]
    fn restyle_switches_state_keeps_other_applications() {
        let (mut doc, handles) = doc_with(&[door()]);
        let mut other = ExtendedDataRecord::new("ALTRA_APP");
        other.add_value(XDataValue::Integer16(7));
        doc.get_entity_mut(handles[0])
            .unwrap()
            .common_mut()
            .extended_data
            .add_record(other);
        let nuova = Aperture {
            state: ApertureState::Nuova,
            ..door()
        };
        restyle(doc.get_entity_mut(handles[0]).unwrap(), &nuova);
        let entity = doc.get_entity(handles[0]).unwrap();
        assert_eq!(entity.common().color, Color::from_index(1));
        assert!(entity.common().extended_data.get_record("ALTRA_APP").is_some());
        assert_eq!(apertures_in(&doc)[0].aperture.state, ApertureState::Nuova);
    }

    /// Asse e spessore seguono l'apertura in MOVE e SCALE (1011 e 1041);
    /// l'altezza dichiarata no.
    #[test]
    fn axis_and_thickness_follow_the_aperture_through_transforms() {
        use crate::command::EntityTransform as T;
        let apply = |entity: &mut EntityType, t: T| {
            crate::entities::traits::EntityTypeOps::apply_transform(entity, &t)
        };
        let read = |entity: &EntityType| {
            decode(entity.common().extended_data.get_record(APP_APERTURA).unwrap()).unwrap()
        };

        let mut scaled = aperture_polyline(&door()).unwrap();
        apply(
            &mut scaled,
            T::Scale {
                center: glam::DVec3::ZERO,
                factor: 2.0,
            },
        );
        let back = read(&scaled);
        assert_eq!(back.axis, ([8.1, 2.0], [8.1, 3.8]));
        assert!((back.thickness - 0.2).abs() < 1e-12, "{}", back.thickness);
        assert_eq!(back.height, Some(2.1), "l'altezza dichiarata non si scala");

        let mut moved = aperture_polyline(&door()).unwrap();
        apply(&mut moved, T::Translate(glam::DVec3::new(10.0, 0.0, 0.0)));
        let back = read(&moved);
        assert_eq!(back.axis, ([14.05, 1.0], [14.05, 1.9]));
        assert_eq!(back.thickness, 0.10);
    }

    #[test]
    fn apertures_survive_a_dxf_save_and_reload() {
        let a = Aperture {
            kind: ApertureKind::Finestra,
            state: ApertureState::Nuova,
            sill: Some(0.9),
            ..door()
        };
        let (doc, _) = doc_with(&[a.clone()]);
        let bytes = codec::DxfWriter::new(&doc).write_to_vec().unwrap();
        let read = codec::DxfReader::from_reader(Cursor::new(bytes))
            .unwrap()
            .read()
            .unwrap();
        let found = apertures_in(&read);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].aperture, a);
        assert!(!missing_layer(&read));
    }

    #[test]
    fn apertures_survive_a_dwg_save_and_reload() {
        let (doc, _) = doc_with(&[door()]);
        let bytes = codec::DwgWriter::write_to_vec(&doc).unwrap();
        let read = codec::DwgReader::from_stream(Cursor::new(bytes))
            .read()
            .unwrap();
        let found = apertures_in(&read);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].aperture, door());
        assert!(
            !missing_layer(&read),
            "layer e APPID devono sopravvivere al DWG"
        );
    }

    /// Un vano porta senza linea di soglia unisce A e B nel rilievo; con la
    /// soglia dell'apertura i due locali restano due (12,00 e 11,70 m²) e
    /// l'area del vano non è di nessuno. Una finestra non chiude nulla.
    #[test]
    fn a_door_closes_the_survey_vano_for_the_rooms_of_the_fatto() {
        use crate::bim::fixtures::{case_a_with_door, scene_with, Form};
        use crate::bim::units::DrawingUnit;
        use crate::bim::walls::{faces, net_area, region_at, wall_set_without_rooms};
        const TOL: f64 = 1e-6;
        let area_at = |sources: &FxHashMap<Handle, BoundarySource>, p: [f64; 2]| {
            let faces = faces(sources, TOL);
            region_at(&faces, p, TOL).map(|region| net_area(&region, sources, TOL).0)
        };
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let mut scene = scene_with(
                case_a_with_door(form, DrawingUnit::Metri),
                DrawingUnit::Metri.insunits(),
            );
            ensure_layer(&mut scene.document);
            let sources = wall_set_without_rooms(&scene.document, TOL).sources;
            let open = area_at(&sources, [2.0, 1.5]).expect("regione aperta");
            // Aperto: A e B sono una regione sola (nella forma a linee anche con la
            // massa dei muri, che il vano senza spallette lascia aperta).
            assert!(open > 23.0, "{form:?}: A+B+vano = {open}");

            for (kind, closes) in [(ApertureKind::Porta, true), (ApertureKind::Finestra, false)] {
                let a = Aperture {
                    kind,
                    threshold: kind.is_door(),
                    ..door()
                };
                let handle = scene.add_entity(aperture_polyline(&a).unwrap());
                let mut closed = sources.clone();
                add_fatto_thresholds(&scene.document, &mut closed, TOL);
                let area = area_at(&closed, [2.0, 1.5]).expect("regione");
                if closes {
                    assert!((area - 12.0).abs() < 0.01, "{form:?}: A = {area}");
                    let b = area_at(&closed, [6.0, 1.5]).expect("regione");
                    assert!((b - 11.7).abs() < 0.01, "{form:?}: B = {b}");
                } else {
                    assert!((area - open).abs() < 0.01, "{form:?}: finestra = {area}");
                }
                scene.erase_entities(&[handle]);
            }
        }
    }

    /// Le fasi: nel fatto contano le aperture esistenti e quelle da demolire,
    /// nel progetto le esistenti e le nuove.
    #[test]
    fn thresholds_follow_the_phase() {
        let count = |state: ApertureState, phase: Phase| {
            let (doc, _) = doc_with(&[Aperture { state, ..door() }]);
            threshold_segments(&doc, phase).len()
        };
        assert_eq!(count(ApertureState::Esistente, Phase::Fatto), 2);
        assert_eq!(count(ApertureState::Esistente, Phase::Progetto), 2);
        assert_eq!(count(ApertureState::Demolita, Phase::Fatto), 2);
        assert_eq!(count(ApertureState::Demolita, Phase::Progetto), 0);
        assert_eq!(count(ApertureState::Nuova, Phase::Fatto), 0);
        assert_eq!(count(ApertureState::Nuova, Phase::Progetto), 2);
        let (doc, _) = doc_with(&[Aperture {
            threshold: false,
            ..door()
        }]);
        assert!(threshold_segments(&doc, Phase::Fatto).is_empty(), "senza soglia");
    }

    /// I cambi di `AL_APERTURA_DATI`: ognuno tocca solo il suo campo; il
    /// davanzale è delle finestre e la soglia delle porte.
    #[test]
    fn changes_touch_only_their_field_and_respect_the_kind() {
        let a = door();
        let c = |change: ApertureChange| change.apply(&a).unwrap();
        assert_eq!(c(ApertureChange::Stato(ApertureState::Nuova)).state, ApertureState::Nuova);
        assert_eq!(c(ApertureChange::Altezza(Some(2.2))).height, Some(2.2));
        assert_eq!(c(ApertureChange::Altezza(None)).height, None, "toglie, non ripiega");
        assert_eq!(c(ApertureChange::Appoggio(Some(0.2))).bearing, Some(0.2));
        assert!(c(ApertureChange::Architrave(true)).lintel);
        assert!(!c(ApertureChange::Soglia(false)).threshold);
        let after = c(ApertureChange::Altezza(Some(2.2)));
        assert_eq!(Aperture { height: a.height, ..after }, a, "il resto non cambia");

        assert!(ApertureChange::Davanzale(Some(0.9)).apply(&a).is_err(), "una porta");
        let window = Aperture { kind: ApertureKind::Finestra, threshold: false, ..door() };
        assert_eq!(ApertureChange::Davanzale(Some(0.9)).apply(&window).unwrap().sill, Some(0.9));
        assert!(ApertureChange::Soglia(true).apply(&window).is_err(), "una finestra");
    }

    /// `AL_ESTRAI` (spec §5.4): il fatto ha le aperture esistenti e da
    /// demolire, la comparativa tutte, il progetto le esistenti e le nuove;
    /// colore dello stato (giallo, rosso, o ByLayer), nessun XDATA né APPID
    /// nostro nei tre DWG.
    #[test]
    fn extraction_keeps_the_apertures_of_each_phase() {
        use crate::bim::extract::{extract, Elaborato, ExtractOptions};
        let at = |y: f64, state| Aperture {
            axis: ([4.05, y], [4.05, y + 0.9]),
            state,
            ..door()
        };
        let (doc, _) = doc_with(&[
            at(0.0, ApertureState::Esistente),
            at(1.0, ApertureState::Demolita),
            at(2.0, ApertureState::Nuova),
        ]);
        let all = [
            Elaborato::StatoDiFatto,
            Elaborato::DemolizioniCostruzioni,
            Elaborato::StatoDiProgetto,
        ];
        let ex = extract(
            &doc,
            &ExtractOptions {
                elaborati: all.to_vec(),
                new_walls_layer: "0".into(),
                unit: crate::bim::units::DrawingUnit::Metri,
                tol: 1e-6,
                room_tol: 1e-6,
            },
        );
        let colours = |elaborato: Elaborato| -> Vec<Color> {
            let (_, out) = ex.docs.iter().find(|(e, _)| *e == elaborato).unwrap();
            assert!(!out.app_ids.iter().any(|a| a.name.starts_with("ARCHLINE")));
            let mut colours: Vec<Color> = out
                .entities()
                .filter(|e| e.common().layer == LAYER_APERTURE)
                .inspect(|e| assert!(e.common().extended_data.records().is_empty()))
                .map(|e| e.common().color.clone())
                .collect();
            colours.sort_by_key(|c| format!("{c:?}"));
            colours
        };
        let (by_layer, yellow, red) = (
            Color::ByLayer,
            Color::from_index(ACI_GIALLO),
            Color::from_index(ACI_ROSSO),
        );
        let mut fatto = vec![by_layer.clone(), yellow.clone()];
        fatto.sort_by_key(|c| format!("{c:?}"));
        let mut comparativa = vec![by_layer.clone(), yellow.clone(), red.clone()];
        comparativa.sort_by_key(|c| format!("{c:?}"));
        let mut progetto = vec![by_layer, red];
        progetto.sort_by_key(|c| format!("{c:?}"));
        assert_eq!(colours(Elaborato::StatoDiFatto), fatto);
        assert_eq!(colours(Elaborato::DemolizioniCostruzioni), comparativa);
        assert_eq!(colours(Elaborato::StatoDiProgetto), progetto);

    }

    #[test]
    fn migration_leaves_apertures_alone() {
        let (mut doc, _) = doc_with(&[door()]);
        assert_eq!(super::super::migrate::migrate_document(&mut doc), 0);
        assert_eq!(apertures_in(&doc)[0].aperture, door());
    }
}
