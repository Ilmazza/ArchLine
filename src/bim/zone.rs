//! Zona: una `HATCH` solida con un record XDATA `ARCHLINE_ZONA`. Gialla su
//! `AL-DEMOLIZIONI` se demolita, rossa su `AL-NUOVE` se nuova (indici ACI, così
//! in stampa un CTB può rimapparli). In AutoCAD è un tratteggio comune.
//!
//! Sequenza XDATA, in quest'ordine: `Integer16(1)` (versione), `"stato"`,
//! `"demolito"|"nuovo"`, `"origine"`, `<origine>`, `"varco"`, `Integer16(0|1)`,
//! poi le chiavi facoltative presenti: `"asse"` + due `Point3D`, `"spessore"` +
//! `Real`, `"altezza"` + `Real`. La lettura va per chiave e salta le sconosciute.
//!
//! Qui non c'è undo: lo apre il comando che crea o cambia le zone.

use codec::entities::hatch::BoundaryPathFlags;
use codec::entities::{BoundaryEdge, BoundaryPath, EntityCommon, Hatch, PolylineEdge};
use codec::tables::{AppId, Layer};
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{CadDocument, Color, EntityType, Handle, Vector2, Vector3};

use super::{
    ACI_GIALLO, ACI_ROSSO, APP_LOCALE, APP_ZONA, LAYER_DEMOLIZIONI, LAYER_LOCALI,
    LAYER_LOCALI_PROGETTO, LAYER_NUOVE,
};

/// Versione del record XDATA.
const VERSION: i16 = 1;
/// Colore ACI dei layer dei locali.
const ACI_BIANCO: i16 = 7;

/// Distanza sotto cui l'ultimo punto di un anello coincide col primo.
const CLOSE_EPS: f64 = 1.0e-9;

const KEY_STATE: &str = "stato";
const KEY_ORIGIN: &str = "origine";
const KEY_VARCO: &str = "varco";
const KEY_AXIS: &str = "asse";
const KEY_THICKNESS: &str = "spessore";
const KEY_HEIGHT: &str = "altezza";

/// I layer `AL-*` con il loro colore ACI.
const LAYERS: [(&str, i16); 4] = [
    (LAYER_DEMOLIZIONI, ACI_GIALLO),
    (LAYER_NUOVE, ACI_ROSSO),
    (LAYER_LOCALI, ACI_BIANCO),
    (LAYER_LOCALI_PROGETTO, ACI_BIANCO),
];
/// Gli APPID di ArchLine che l'XDATA richiede nella tabella.
const APPS: [&str; 2] = [APP_ZONA, APP_LOCALE];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneState {
    Demolito,
    Nuovo,
}

impl ZoneState {
    /// Testo nel record XDATA.
    pub fn key(self) -> &'static str {
        match self {
            Self::Demolito => "demolito",
            Self::Nuovo => "nuovo",
        }
    }

    pub fn from_key(text: &str) -> Option<Self> {
        match text {
            "demolito" => Some(Self::Demolito),
            "nuovo" => Some(Self::Nuovo),
            _ => None,
        }
    }

    pub fn layer(self) -> &'static str {
        match self {
            Self::Demolito => LAYER_DEMOLIZIONI,
            Self::Nuovo => LAYER_NUOVE,
        }
    }

    pub fn aci(self) -> i16 {
        match self {
            Self::Demolito => ACI_GIALLO,
            Self::Nuovo => ACI_ROSSO,
        }
    }
}

/// Come è nata la zona (tracciabilità e fonte del dato per la parte 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneOrigin {
    MuroDuePunti,
    LineaSpessore,
    PuntoInterno,
    Rettangolo,
    Poligono,
    PolilineaEsistente,
}

impl ZoneOrigin {
    /// Testo nel record XDATA.
    pub fn key(self) -> &'static str {
        match self {
            Self::MuroDuePunti => "muro-due-punti",
            Self::LineaSpessore => "linea-spessore",
            Self::PuntoInterno => "punto-interno",
            Self::Rettangolo => "rettangolo",
            Self::Poligono => "poligono",
            Self::PolilineaEsistente => "polilinea-esistente",
        }
    }

    pub fn from_key(text: &str) -> Option<Self> {
        match text {
            "muro-due-punti" => Some(Self::MuroDuePunti),
            "linea-spessore" => Some(Self::LineaSpessore),
            "punto-interno" => Some(Self::PuntoInterno),
            "rettangolo" => Some(Self::Rettangolo),
            "poligono" => Some(Self::Poligono),
            "polilinea-esistente" => Some(Self::PolilineaEsistente),
            _ => None,
        }
    }
}

/// Ciò che il record XDATA dice di una zona.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub state: ZoneState,
    pub origin: ZoneOrigin,
    /// I due punti sull'asse del muro, quando la zona nasce da «muro tra due punti».
    pub axis: Option<([f64; 2], [f64; 2])>,
    /// In unità del disegno.
    pub thickness: Option<f64>,
    /// Mai riempita da noi.
    pub height: Option<f64>,
    /// Solo per le zone demolite (spec §5.3).
    pub varco: bool,
}

/// Una zona letta dal disegno, con la sua geometria.
#[derive(Clone, Debug)]
pub struct ZoneGeom {
    pub handle: Handle,
    pub zone: Zone,
    /// `rings[0]` è il contorno esterno, i successivi sono buchi. Anelli chiusi
    /// senza ripetere il primo punto in fondo; i lati curvi sono approssimati.
    pub rings: Vec<Vec<[f64; 2]>>,
}

fn key(text: &str) -> XDataValue {
    XDataValue::String(text.to_string())
}

fn point(p: [f64; 2]) -> XDataValue {
    XDataValue::Point3D(Vector3::new(p[0], p[1], 0.0))
}

/// La sequenza dei valori del record `ARCHLINE_ZONA`.
pub fn encode(zone: &Zone) -> Vec<XDataValue> {
    let mut values = vec![
        XDataValue::Integer16(VERSION),
        key(KEY_STATE),
        key(zone.state.key()),
        key(KEY_ORIGIN),
        key(zone.origin.key()),
        key(KEY_VARCO),
        XDataValue::Integer16(i16::from(zone.varco)),
    ];
    if let Some((a, b)) = zone.axis {
        values.extend([key(KEY_AXIS), point(a), point(b)]);
    }
    if let Some(t) = zone.thickness {
        values.extend([key(KEY_THICKNESS), XDataValue::Real(t)]);
    }
    if let Some(h) = zone.height {
        values.extend([key(KEY_HEIGHT), XDataValue::Real(h)]);
    }
    values
}

/// Legge il record per chiave, saltando ciò che non conosce. `None` senza uno
/// stato e un'origine leggibili; senza `varco` vale no.
pub fn decode(rec: &ExtendedDataRecord) -> Option<Zone> {
    let (mut state, mut origin) = (None, None);
    let (mut axis, mut thickness, mut height, mut varco) = (None, None, None, false);
    let values = &rec.values;
    let mut i = 0;
    while i < values.len() {
        let XDataValue::String(name) = &values[i] else {
            i += 1;
            continue;
        };
        let next = values.get(i + 1);
        let used = match (name.as_str(), next) {
            (KEY_STATE, Some(XDataValue::String(t))) => {
                state = ZoneState::from_key(t);
                2
            }
            (KEY_ORIGIN, Some(XDataValue::String(t))) => {
                origin = ZoneOrigin::from_key(t);
                2
            }
            (KEY_VARCO, Some(XDataValue::Integer16(v))) => {
                varco = *v != 0;
                2
            }
            (KEY_AXIS, Some(XDataValue::Point3D(a))) => match values.get(i + 2) {
                Some(XDataValue::Point3D(b)) => {
                    axis = Some(([a.x, a.y], [b.x, b.y]));
                    3
                }
                _ => 1,
            },
            (KEY_THICKNESS, Some(XDataValue::Real(t))) => {
                thickness = Some(*t);
                2
            }
            (KEY_HEIGHT, Some(XDataValue::Real(h))) => {
                height = Some(*h);
                2
            }
            _ => 1,
        };
        i += used;
    }
    Some(Zone {
        state: state?,
        origin: origin?,
        axis,
        thickness,
        height,
        varco,
    })
}

fn zone_record(zone: &Zone) -> ExtendedDataRecord {
    let mut rec = ExtendedDataRecord::new(APP_ZONA);
    for value in encode(zone) {
        rec.add_value(value);
    }
    rec
}

/// Layer, colore ACI e record XDATA secondo lo stato; gli XDATA di altre
/// applicazioni restano.
fn style(common: &mut EntityCommon, zone: &Zone) {
    common.layer = zone.state.layer().to_string();
    common.color = Color::from_index(zone.state.aci());
    common.extended_data.upsert_record(zone_record(zone));
}

/// La zona come campitura solida non associativa con i percorsi dati. L'handle
/// lo assegna il documento all'inserimento.
pub fn zone_hatch_from_paths(zone: &Zone, paths: Vec<BoundaryPath>) -> EntityType {
    let mut hatch = Hatch::new();
    hatch.paths = paths;
    style(&mut hatch.common, zone);
    EntityType::Hatch(Box::new(hatch))
}

/// La zona sugli anelli dati: il primo è il contorno esterno, gli altri buchi.
pub fn zone_hatch(zone: &Zone, rings: &[Vec<[f64; 2]>]) -> EntityType {
    let paths = rings
        .iter()
        .enumerate()
        .map(|(index, ring)| {
            let flags = if index == 0 {
                BoundaryPathFlags::from_bits(
                    BoundaryPathFlags::OUTERMOST.bits() | BoundaryPathFlags::EXTERNAL.bits(),
                )
            } else {
                BoundaryPathFlags::DEFAULT
            };
            let vertices = ring.iter().map(|p| Vector2::new(p[0], p[1])).collect();
            let mut path = BoundaryPath::with_flags(flags);
            path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(vertices, true)));
            path
        })
        .collect();
    zone_hatch_from_paths(zone, paths)
}

/// L'anello senza il punto finale uguale al primo: la tassellazione dei
/// percorsi lo ripete.
fn open_ring(mut ring: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    if let (true, Some(first), Some(last)) = (ring.len() > 3, ring.first(), ring.last()) {
        if (first[0] - last[0]).abs() <= CLOSE_EPS && (first[1] - last[1]).abs() <= CLOSE_EPS {
            ring.pop();
        }
    }
    ring
}

/// La zona descritta da `entity`, se è una `HATCH` con un record
/// `ARCHLINE_ZONA` leggibile e almeno un contorno.
pub fn read_zone(handle: Handle, entity: &EntityType) -> Option<ZoneGeom> {
    let EntityType::Hatch(hatch) = entity else {
        return None;
    };
    let zone = decode(hatch.common.extended_data.get_record(APP_ZONA)?)?;
    let rings: Vec<_> = crate::scene::hatch_boundary_rings(hatch)
        .into_iter()
        .map(open_ring)
        .collect();
    if rings.is_empty() {
        return None;
    }
    Some(ZoneGeom {
        handle,
        zone,
        rings,
    })
}

/// Le zone dello spazio modello (non quelle in carta o dentro i blocchi).
pub fn zones_in(doc: &CadDocument) -> Vec<ZoneGeom> {
    doc.block_records
        .iter()
        .filter(|record| record.is_model_space())
        .flat_map(|record| record.entity_handles.iter())
        .filter_map(|&handle| read_zone(handle, doc.get_entity(handle)?))
        .collect()
}

/// Porta layer, colore e record di `entity` allo stato di `zone`. Gli XDATA di
/// altre applicazioni restano.
///
/// Non tocca `raw_dwg_eed`: su una zona letta da un DWG il blocco grezzo di
/// `ARCHLINE_ZONA` prevale sul record nuovo al salvataggio, quindi chi cambia una
/// zona già nel documento deve scartarlo (voce con l'handle dell'APPID
/// `ARCHLINE_ZONA`, come fa `set_entity_xdata`). Qui l'handle non c'è.
pub fn restyle(entity: &mut EntityType, zone: &Zone) {
    style(entity.common_mut(), zone);
}

/// Manca uno dei quattro layer `AL-*` o uno dei due APPID?
pub fn missing_layers(doc: &CadDocument) -> bool {
    LAYERS.iter().any(|(name, _)| !doc.layers.contains(name))
        || APPS.iter().any(|app| !doc.app_ids.contains(app))
}

/// Crea i layer `AL-*` e gli APPID che mancano, ognuno con il suo handle
/// (senza, si perderebbero al salvataggio DWG). Quelli già presenti non si
/// toccano. Niente undo: lo apre il chiamante.
pub fn ensure_layers(doc: &mut CadDocument) {
    for (name, aci) in LAYERS {
        if doc.layers.contains(name) {
            continue;
        }
        let mut layer = Layer::new(name);
        layer.handle = doc.allocate_handle();
        layer.color = Color::from_index(aci);
        let _ = doc.layers.add(layer);
    }
    for app in APPS {
        if doc.app_ids.contains(app) {
            continue;
        }
        let mut app_id = AppId::new(app);
        app_id.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn rect() -> Vec<[f64; 2]> {
        vec![[4.0, 0.0], [4.1, 0.0], [4.1, 3.0], [4.0, 3.0]]
    }

    fn plain(state: ZoneState, origin: ZoneOrigin) -> Zone {
        Zone {
            state,
            origin,
            axis: None,
            thickness: None,
            height: None,
            varco: false,
        }
    }

    fn record_of(zone: &Zone) -> ExtendedDataRecord {
        let mut rec = ExtendedDataRecord::new(APP_ZONA);
        for v in encode(zone) {
            rec.add_value(v);
        }
        rec
    }

    #[test]
    fn xdata_roundtrip_all_fields() {
        let z = Zone {
            state: ZoneState::Demolito,
            origin: ZoneOrigin::MuroDuePunti,
            axis: Some(([4.05, 0.0], [4.05, 3.0])),
            thickness: Some(0.10),
            height: Some(2.70),
            varco: true,
        };
        assert_eq!(decode(&record_of(&z)), Some(z));
    }

    #[test]
    fn xdata_sequence_is_the_documented_one() {
        let z = Zone {
            state: ZoneState::Nuovo,
            origin: ZoneOrigin::LineaSpessore,
            axis: Some(([1.0, 2.0], [3.0, 4.0])),
            thickness: Some(0.3),
            height: Some(2.7),
            varco: false,
        };
        let v = encode(&z);
        assert_eq!(v.len(), 14);
        let s = |t: &str| XDataValue::String(t.to_string());
        assert_eq!(v[0], XDataValue::Integer16(1));
        assert_eq!(
            v[1..7],
            [
                s("stato"),
                s("nuovo"),
                s("origine"),
                s("linea-spessore"),
                s("varco"),
                XDataValue::Integer16(0)
            ]
        );
        assert_eq!(v[7], s("asse"));
        assert!(matches!(v[8], XDataValue::Point3D(p) if p.x == 1.0 && p.y == 2.0 && p.z == 0.0));
        assert!(matches!(v[9], XDataValue::Point3D(p) if p.x == 3.0 && p.y == 4.0 && p.z == 0.0));
        assert_eq!(
            v[10..],
            [
                s("spessore"),
                XDataValue::Real(0.3),
                s("altezza"),
                XDataValue::Real(2.7)
            ]
        );
        // Senza campi facoltativi la sequenza finisce al varco.
        assert_eq!(
            encode(&plain(ZoneState::Nuovo, ZoneOrigin::Poligono)).len(),
            7
        );
    }

    #[test]
    fn origin_texts() {
        let all = [
            (ZoneOrigin::MuroDuePunti, "muro-due-punti"),
            (ZoneOrigin::LineaSpessore, "linea-spessore"),
            (ZoneOrigin::PuntoInterno, "punto-interno"),
            (ZoneOrigin::Rettangolo, "rettangolo"),
            (ZoneOrigin::Poligono, "poligono"),
            (ZoneOrigin::PolilineaEsistente, "polilinea-esistente"),
        ];
        for (origin, text) in all {
            assert_eq!(origin.key(), text);
            assert_eq!(ZoneOrigin::from_key(text), Some(origin));
        }
        assert_eq!(ZoneOrigin::from_key("altro"), None);
    }

    #[test]
    fn decode_reads_by_key_and_skips_unknown_keys() {
        let mut rec = ExtendedDataRecord::new(APP_ZONA);
        let s = |t: &str| XDataValue::String(t.to_string());
        for v in [
            XDataValue::Integer16(1),
            s("origine"),
            s("rettangolo"),
            s("nota"),
            XDataValue::Real(9.0),
            s("spessore"),
            XDataValue::Real(0.25),
            s("stato"),
            s("nuovo"),
        ] {
            rec.add_value(v);
        }
        let z = decode(&rec).unwrap();
        assert_eq!(z.state, ZoneState::Nuovo);
        assert_eq!(z.origin, ZoneOrigin::Rettangolo);
        assert_eq!(z.thickness, Some(0.25));
        assert!(!z.varco);
        assert_eq!((z.axis, z.height), (None, None));
    }

    #[test]
    fn decode_rejects_records_without_a_readable_state_or_origin() {
        let s = |t: &str| XDataValue::String(t.to_string());
        let build = |values: Vec<XDataValue>| {
            let mut rec = ExtendedDataRecord::new(APP_ZONA);
            for v in values {
                rec.add_value(v);
            }
            rec
        };
        assert_eq!(decode(&build(vec![])), None);
        assert_eq!(
            decode(&build(vec![
                XDataValue::Integer16(1),
                s("origine"),
                s("poligono")
            ])),
            None
        );
        assert_eq!(
            decode(&build(vec![
                XDataValue::Integer16(1),
                s("stato"),
                s("nuovo")
            ])),
            None
        );
        assert_eq!(
            decode(&build(vec![
                XDataValue::Integer16(1),
                s("stato"),
                s("sconosciuto"),
                s("origine"),
                s("poligono"),
            ])),
            None
        );
    }

    #[test]
    fn hatch_is_solid_on_state_layer_with_aci() {
        let ring = rect();
        let z = plain(ZoneState::Demolito, ZoneOrigin::Rettangolo);
        let EntityType::Hatch(h) = zone_hatch(&z, &[ring.clone()]) else {
            panic!()
        };
        assert!(h.is_solid);
        assert!(!h.is_associative);
        assert_eq!(h.common.layer, LAYER_DEMOLIZIONI);
        assert_eq!(h.common.color, codec::Color::from_index(ACI_GIALLO));
        assert!((crate::entities::hatch::boundary_area(&h) - 0.30).abs() < 1e-9);
        let n = Zone {
            state: ZoneState::Nuovo,
            ..z
        };
        let EntityType::Hatch(h) = zone_hatch(&n, &[ring]) else {
            panic!()
        };
        assert_eq!(
            (h.common.layer.as_str(), h.common.color),
            (LAYER_NUOVE, codec::Color::from_index(ACI_ROSSO))
        );
    }

    #[test]
    fn hatch_carries_the_zone_record() {
        let z = Zone {
            axis: Some(([4.05, 0.0], [4.05, 3.0])),
            thickness: Some(0.1),
            varco: true,
            ..plain(ZoneState::Demolito, ZoneOrigin::MuroDuePunti)
        };
        let entity = zone_hatch(&z, &[rect()]);
        let zg = read_zone(codec::Handle::new(0x99), &entity).unwrap();
        assert_eq!(zg.zone, z);
        assert_eq!(zg.handle, codec::Handle::new(0x99));
    }

    #[test]
    fn hatch_rings_keep_outer_first_and_holes_after() {
        let outer = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]];
        let hole = vec![[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]];
        let z = plain(ZoneState::Nuovo, ZoneOrigin::PuntoInterno);
        let entity = zone_hatch(&z, &[outer, hole]);
        let EntityType::Hatch(h) = &entity else {
            panic!()
        };
        assert_eq!(h.paths.len(), 2);
        assert!(h.paths[0].flags.is_external());
        assert!(h.paths[0].flags.bits() & 16 != 0, "OUTERMOST");
        assert_eq!(
            h.paths[1].flags.bits() & (1 | 16),
            0,
            "il buco non è esterno"
        );
        assert!((crate::entities::hatch::boundary_area(h) - 11.0).abs() < 1e-9);
        let zg = read_zone(codec::Handle::new(1), &entity).unwrap();
        assert_eq!(zg.rings.len(), 2);
        assert_eq!(zg.rings[0].len(), 4);
        assert_eq!(zg.rings[1].len(), 4);
    }

    #[test]
    fn zone_hatch_from_paths_keeps_the_given_paths() {
        let z = plain(ZoneState::Nuovo, ZoneOrigin::PolilineaEsistente);
        let EntityType::Hatch(source) = zone_hatch(&z, &[rect()]) else {
            panic!()
        };
        let EntityType::Hatch(h) = zone_hatch_from_paths(&z, source.paths.clone()) else {
            panic!()
        };
        assert_eq!(h.paths, source.paths);
        assert_eq!(h.common.layer, LAYER_NUOVE);
        assert!(h.is_solid);
    }

    #[test]
    fn read_zone_ignores_what_is_not_a_zone() {
        let h = codec::Handle::new(5);
        let line = EntityType::Line(codec::entities::Line::from_points(
            codec::Vector3::ZERO,
            codec::Vector3::UNIT_X,
        ));
        assert!(read_zone(h, &line).is_none());
        // Un tratteggio qualunque, senza record.
        let EntityType::Hatch(mut hatch) =
            zone_hatch(&plain(ZoneState::Nuovo, ZoneOrigin::Poligono), &[rect()])
        else {
            panic!()
        };
        hatch.common.extended_data = codec::xdata::ExtendedData::new();
        assert!(read_zone(h, &EntityType::Hatch(hatch.clone())).is_none());
        // Record di un'altra applicazione.
        let mut other = ExtendedDataRecord::new("ALTRA_APP");
        other.add_value(XDataValue::String("stato".into()));
        hatch.common.extended_data.add_record(other);
        assert!(read_zone(h, &EntityType::Hatch(hatch.clone())).is_none());
        // Record nostro ma illeggibile.
        let mut bad = ExtendedDataRecord::new(APP_ZONA);
        bad.add_value(XDataValue::Integer16(1));
        hatch.common.extended_data.add_record(bad);
        assert!(read_zone(h, &EntityType::Hatch(hatch)).is_none());
    }

    #[test]
    fn ensure_layers_creates_once_with_colors() {
        let mut doc = CadDocument::new();
        assert!(missing_layers(&doc));
        ensure_layers(&mut doc);
        assert!(!missing_layers(&doc));
        assert_eq!(
            doc.layers.get(LAYER_DEMOLIZIONI).unwrap().color,
            codec::Color::from_index(2)
        );
        assert_eq!(
            doc.layers.get(LAYER_NUOVE).unwrap().color,
            codec::Color::from_index(1)
        );
        assert_eq!(
            doc.layers.get(LAYER_LOCALI).unwrap().color,
            codec::Color::from_index(7)
        );
        assert_eq!(
            doc.layers.get(LAYER_LOCALI_PROGETTO).unwrap().color,
            codec::Color::from_index(7)
        );
        for name in [
            LAYER_DEMOLIZIONI,
            LAYER_NUOVE,
            LAYER_LOCALI,
            LAYER_LOCALI_PROGETTO,
        ] {
            assert!(
                !doc.layers.get(name).unwrap().handle.is_null(),
                "{name} senza handle"
            );
        }
        for app in [APP_ZONA, APP_LOCALE] {
            assert!(
                !doc.app_ids.get(app).expect(app).handle.is_null(),
                "{app} senza handle"
            );
        }
        let (layers, apps) = (doc.layers.iter().count(), doc.app_ids.iter().count());
        ensure_layers(&mut doc);
        assert_eq!(doc.layers.iter().count(), layers);
        assert_eq!(doc.app_ids.iter().count(), apps);
    }

    #[test]
    fn missing_layers_notices_each_piece() {
        let mut doc = CadDocument::new();
        ensure_layers(&mut doc);
        assert!(!missing_layers(&doc));
        // Manca un solo APPID.
        let mut no_app = doc.clone();
        no_app.app_ids.remove(APP_LOCALE);
        assert!(missing_layers(&no_app));
        // Manca un solo layer, anche quello dei locali.
        let mut no_layer = doc.clone();
        no_layer.layers.remove(LAYER_LOCALI_PROGETTO);
        assert!(missing_layers(&no_layer));
        // Un layer già presente (di Mauro) non si tocca.
        let mut mine = CadDocument::new();
        let mut layer = codec::Layer::new(LAYER_NUOVE);
        layer.handle = mine.allocate_handle();
        layer.color = codec::Color::from_index(30);
        mine.layers.add(layer).unwrap();
        ensure_layers(&mut mine);
        assert_eq!(
            mine.layers.get(LAYER_NUOVE).unwrap().color,
            codec::Color::from_index(30)
        );
        assert!(!missing_layers(&mine));
    }

    fn doc_with(zones: &[(Zone, Vec<Vec<[f64; 2]>>)]) -> (CadDocument, Vec<codec::Handle>) {
        let mut doc = CadDocument::new();
        ensure_layers(&mut doc);
        let handles = zones
            .iter()
            .map(|(z, rings)| doc.add_entity(zone_hatch(z, rings)).unwrap())
            .collect();
        (doc, handles)
    }

    #[test]
    fn read_zone_roundtrip_through_document() {
        let z = Zone {
            axis: Some(([4.05, 0.0], [4.05, 3.0])),
            thickness: Some(0.10),
            ..plain(ZoneState::Demolito, ZoneOrigin::MuroDuePunti)
        };
        let (doc, handles) = doc_with(&[(z.clone(), vec![rect()])]);
        let found = zones_in(&doc);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].handle, handles[0]);
        assert_eq!(found[0].zone, z);
        assert_eq!(found[0].rings.len(), 1);
        assert_eq!(found[0].rings[0].len(), 4);
        for (got, want) in found[0].rings[0].iter().zip(rect()) {
            assert!((got[0] - want[0]).abs() < 1e-12 && (got[1] - want[1]).abs() < 1e-12);
        }
    }

    #[test]
    fn zones_in_scans_model_space_only_and_skips_other_hatches() {
        let a = plain(ZoneState::Demolito, ZoneOrigin::Rettangolo);
        let b = plain(ZoneState::Nuovo, ZoneOrigin::Poligono);
        let (mut doc, handles) = doc_with(&[(a, vec![rect()]), (b, vec![rect()])]);
        // Una zona in carta non conta.
        let mut in_paper = zone_hatch(&plain(ZoneState::Nuovo, ZoneOrigin::Rettangolo), &[rect()]);
        assert!(!doc.header.paper_space_block_handle.is_null());
        in_paper.common_mut().owner_handle = doc.header.paper_space_block_handle;
        doc.add_entity(in_paper).unwrap();
        // Una linea e un tratteggio senza record non sono zone.
        doc.add_entity(EntityType::Line(codec::entities::Line::from_points(
            codec::Vector3::ZERO,
            codec::Vector3::UNIT_X,
        )))
        .unwrap();
        let EntityType::Hatch(mut bare) =
            zone_hatch(&plain(ZoneState::Nuovo, ZoneOrigin::Rettangolo), &[rect()])
        else {
            panic!()
        };
        bare.common.extended_data = codec::xdata::ExtendedData::new();
        doc.add_entity(EntityType::Hatch(bare)).unwrap();

        let found = zones_in(&doc);
        assert_eq!(found.iter().map(|g| g.handle).collect::<Vec<_>>(), handles);
    }

    #[test]
    fn restyle_switches_layer_colour_and_record_and_keeps_other_apps() {
        let z = plain(ZoneState::Demolito, ZoneOrigin::Rettangolo);
        let mut entity = zone_hatch(&z, &[rect()]);
        let mut other = ExtendedDataRecord::new("ALTRA_APP");
        other.add_value(XDataValue::Integer16(7));
        entity.common_mut().extended_data.add_record(other.clone());

        let new = Zone {
            thickness: Some(0.2),
            ..plain(ZoneState::Nuovo, ZoneOrigin::Rettangolo)
        };
        restyle(&mut entity, &new);

        let common = entity.common();
        assert_eq!(common.layer, LAYER_NUOVE);
        assert_eq!(common.color, codec::Color::from_index(ACI_ROSSO));
        assert_eq!(common.extended_data.get_record("ALTRA_APP"), Some(&other));
        let zone_records = common
            .extended_data
            .records()
            .iter()
            .filter(|r| r.application_name == APP_ZONA)
            .count();
        assert_eq!(zone_records, 1);
        assert_eq!(read_zone(codec::Handle::new(1), &entity).unwrap().zone, new);

        restyle(&mut entity, &z);
        assert_eq!(entity.common().layer, LAYER_DEMOLIZIONI);
        assert_eq!(entity.common().color, codec::Color::from_index(ACI_GIALLO));
        assert_eq!(read_zone(codec::Handle::new(1), &entity).unwrap().zone, z);
    }

    #[test]
    fn zones_survive_a_dxf_save_and_reload() {
        let z = Zone {
            axis: Some(([4.05, 0.0], [4.05, 3.0])),
            thickness: Some(0.10),
            height: Some(2.7),
            varco: true,
            ..plain(ZoneState::Demolito, ZoneOrigin::MuroDuePunti)
        };
        let (doc, _) = doc_with(&[(z.clone(), vec![rect()])]);
        let bytes = codec::DxfWriter::new(&doc).write_to_vec().unwrap();
        let read = codec::DxfReader::from_reader(Cursor::new(bytes))
            .unwrap()
            .read()
            .unwrap();
        let found = zones_in(&read);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].zone, z);
        assert_eq!(found[0].rings[0].len(), 4);
        assert!(!missing_layers(&read));
    }

    #[test]
    fn zones_survive_a_dwg_save_and_reload() {
        let z = Zone {
            axis: Some(([4.05, 0.0], [4.05, 3.0])),
            thickness: Some(0.10),
            ..plain(ZoneState::Nuovo, ZoneOrigin::LineaSpessore)
        };
        let (doc, _) = doc_with(&[(z.clone(), vec![rect()])]);
        let bytes = codec::DwgWriter::write_to_vec(&doc).unwrap();
        let read = codec::DwgReader::from_stream(Cursor::new(bytes))
            .read()
            .unwrap();
        let found = zones_in(&read);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].zone, z);
        assert_eq!(found[0].rings[0].len(), 4);
        assert!(
            !missing_layers(&read),
            "layer e APPID devono sopravvivere al DWG"
        );
        assert_eq!(
            read.layers.get(LAYER_NUOVE).unwrap().color,
            codec::Color::from_index(ACI_ROSSO)
        );
    }
}
