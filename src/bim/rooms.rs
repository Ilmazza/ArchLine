//! Locali (spec §4.2, §5.2, §5.3, §5.4): superficie netta tra le facce dei
//! muri, nello stato di fatto e in quello di progetto. Logica pura: i comandi
//! (`AL_LOCALE`, `AL_AGGIORNA`) e l'estrazione preparano le istantanee della
//! geometria e scrivono le entità.
//!
//! **Nel disegno.** Un locale di fatto è una `LWPOLYLINE` chiusa su
//! `AL-LOCALI` (il contorno) e un `MTEXT` con nome e superficie (il testo); di
//! un locale di progetto si salva solo il seme, un `MTEXT` col nome su
//! `AL-LOCALI-PROGETTO`: forma e superficie si ricalcolano ogni volta. Ognuno
//! porta un record XDATA `ARCHLINE_LOCALE`, in quest'ordine: `Integer16(1)`
//! (versione), `"fase"`, `"fatto"|"progetto"`, `"parte"`,
//! `"contorno"|"testo"|"seme"`, `"nome"`, `<nome>`, `"numero"`, `<numero>`
//! (anche vuoto), `"punto"`, `Point3D(seme)`, `"origine"`, `"punto-interno"`.
//! La lettura va per chiave e salta ciò che non conosce. Le entità nascono con
//! handle `NULL`: chi le inserisce chiama prima `zone::ensure_layers`, che crea
//! i layer e l'APPID.
//!
//! **Calcolo** ([`compute_at`]). Il locale attorno al seme è la faccia più
//! piccola che lo contiene, meno le isole (`walls::region_at`,
//! `walls::net_area`: archi veri dove si può, mai un'area sbagliata dichiarata
//! esatta). In più vale la **regola del gruppo staccato**: se dentro la regione
//! trovata c'è un gruppo di segmenti dei muri, collegati tra loro, che non
//! tocca (entro `tol`) né il contorno né i buchi della regione, il locale vero
//! è aperto e le sue linee galleggiano dentro una faccia più grande. Succede con
//! un vano senza soglia o con un vuoto d'angolo di 2 mm: senza la regola il
//! punto cadrebbe nel contorno esterno dell'edificio e darebbe la sua area.
//! Allora il risultato è `None`, «non chiuso». I monconi attaccati al contorno
//! (linee che sporgono oltre un angolo) non contano. Il costo, dichiarato: anche
//! un arredo aperto (una linea, una polilinea non chiusa) lasciato acceso sul
//! layer dei muri dentro un locale lo rende «non chiuso». È un messaggio, mai
//! un numero sbagliato; si spegne quel layer, come in AutoCAD (spec §5.0).
//! Infine (R47) un locale chiuso solo grazie a una tolleranza larga
//! (`HPGAPTOL` sopra la tolleranza di base) non è mai esatto: se alla
//! tolleranza di base non è chiuso, o misura un'altra area, il numero resta
//! quello alla tolleranza larga con `exact = false`.
//!
//! **Locali di progetto** ([`project_rooms`], spec §5.4). Tutto si calcola
//! sulla geometria di progetto (`project::ProjectGeometry::room_entities`, con
//! le soglie dei varchi):
//!
//! 1. Per ogni seme, prima quelli di fatto e poi quelli di progetto, nel loro
//!    ordine: un seme di progetto dentro una zona `nuovo` dà
//!    [`Notice::SemeInMuroNuovo`] e non nomina nessun locale; un seme senza
//!    locale chiuso attorno dà [`Notice::LocaleNonChiuso`]; un seme la cui
//!    faccia sta tutta dentro una zona non nomina niente (quelle facce non sono
//!    mai locali): un seme di progetto nella faccia di un varco dà
//!    [`Notice::SemeNelVarco`]; un seme di fatto dentro un muro nuovo non ha un
//!    avviso suo, ma le parti del locale escono «(senza nome)», ognuna con il
//!    suo. Gli altri si raggruppano per faccia.
//! 2. Un gruppo con almeno un seme di progetto prende il nome e il numero del
//!    primo, senza «da confermare»; più semi di progetto nella stessa faccia
//!    danno anche un [`Notice::LocaleFuso`] con i loro nomi (scelta oltre il
//!    piano: un nome di Mauro non sparisce in silenzio). Un gruppo di soli semi
//!    di fatto prende i nomi uniti da « + », nell'ordine dei semi, e i numeri
//!    non vuoti uniti allo stesso modo; con più di un seme è «da confermare» e
//!    dà un [`Notice::LocaleFuso`]. L'etichetta sta sul primo seme del gruppo
//!    che dà il nome.
//! 3. Ogni faccia che non è un locale né un buco di un locale: se sta tutta
//!    dentro una zona non è un locale, e dentro un varco dà [`Notice::Varco`]
//!    con la sua area, che non appartiene a nessun locale. Altrimenti, se il
//!    suo punto interno cade dentro il contorno esterno di una regione di
//!    fatto, è un locale «(senza nome)» con [`Notice::LocaleSenzaNome`]
//!    (un'isola di un locale senza nome è dell'isola, non un locale); se
//!    contiene un gruppo di muri staccato, [`Notice::LocaleNonChiuso`] con il
//!    nome «(senza nome)». Il punto
//!    interno si prende sulla faccia come regione, cioè meno i suoi figli
//!    diretti (`walls::face_regions`, la regola di `region_at`): il
//!    baricentro del triangolo più grande di `kernel::geom2d::triangulate`, o
//!    del primo, in ordine di area, che cada davvero dentro (a più di `tol`
//!    dagli anelli); se `triangulate` non ne dà, quelli di
//!    `triangulate_rings`. Sull'anello nudo del contorno esterno dell'edificio
//!    il punto cadrebbe dentro un locale.
//! 4. Per ogni locale: isole → [`Notice::Isole`], area non esatta →
//!    [`Notice::ContornoApprossimato`].

use codec::entities::{AttachmentPoint, EntityCommon, LwPolyline, LwVertex, MText};
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{CadDocument, EntityType, Handle, Vector2, Vector3};
use kernel::geom2d::{
    contains, distance_to, segment_crossing, signed_area, triangulate, triangulate_rings, Curve,
    Line, SegmentCrossing, Tolerance,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::command::WorkingPlane;
use crate::scene::{boundary_polyline_entities, BoundarySource};

use super::report::Notice;
use super::units::{area_m2, format_m2, DrawingUnit};
use super::walls::{face_regions, faces as wall_faces, net_area, region_at, Region};
use super::zone::{ZoneGeom, ZoneState};
use super::{APP_LOCALE, LAYER_LOCALI, LAYER_LOCALI_PROGETTO};

/// Versione del record XDATA.
const VERSION: i16 = 1;

const KEY_PHASE: &str = "fase";
const KEY_PART: &str = "parte";
const KEY_NAME: &str = "nome";
const KEY_NUMBER: &str = "numero";
const KEY_POINT: &str = "punto";
const KEY_ORIGIN: &str = "origine";
/// L'unica origine di un locale nel primo giro.
const ORIGIN_POINT: &str = "punto-interno";

/// Il nome di un locale di progetto senza seme.
pub const UNNAMED: &str = "(senza nome)";
/// L'unione meccanica dei nomi (e dei numeri) di fatto.
const JOIN: &str = " + ";
/// Dopo il nome di un locale fuso.
const TO_CONFIRM: &str = " (da confermare)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Fatto,
    Progetto,
}

impl Phase {
    /// Testo nel record XDATA.
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
}

/// Quale entità di un locale: il contorno e il testo di un locale calcolato,
/// o il seme di un locale di progetto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomPart {
    Contorno,
    Testo,
    Seme,
}

impl RoomPart {
    /// Testo nel record XDATA.
    pub fn key(self) -> &'static str {
        match self {
            Self::Contorno => "contorno",
            Self::Testo => "testo",
            Self::Seme => "seme",
        }
    }

    pub fn from_key(text: &str) -> Option<Self> {
        match text {
            "contorno" => Some(Self::Contorno),
            "testo" => Some(Self::Testo),
            "seme" => Some(Self::Seme),
            _ => None,
        }
    }
}

/// Ciò che il record XDATA dice di un locale.
#[derive(Clone, Debug, PartialEq)]
pub struct Room {
    pub phase: Phase,
    pub name: String,
    /// Vuoto se Mauro non l'ha dato.
    pub number: String,
    /// Il punto interno da cui è nato, in unità del disegno.
    pub seed: [f64; 2],
}

fn key(text: &str) -> XDataValue {
    XDataValue::String(text.to_string())
}

/// La sequenza dei valori del record `ARCHLINE_LOCALE`.
pub fn encode(room: &Room, part: RoomPart) -> Vec<XDataValue> {
    vec![
        XDataValue::Integer16(VERSION),
        key(KEY_PHASE),
        key(room.phase.key()),
        key(KEY_PART),
        key(part.key()),
        key(KEY_NAME),
        key(&room.name),
        key(KEY_NUMBER),
        key(&room.number),
        key(KEY_POINT),
        XDataValue::Point3D(Vector3::new(room.seed[0], room.seed[1], 0.0)),
        key(KEY_ORIGIN),
        key(ORIGIN_POINT),
    ]
}

/// Legge il record per chiave, saltando ciò che non conosce. `None` senza una
/// fase, una parte e un punto leggibili; nome e numero assenti valgono vuoti.
pub fn decode(rec: &ExtendedDataRecord) -> Option<(Room, RoomPart)> {
    let (mut phase, mut part, mut seed) = (None, None, None);
    let (mut name, mut number) = (String::new(), String::new());
    let values = &rec.values;
    let mut i = 0;
    while i < values.len() {
        let XDataValue::String(field) = &values[i] else {
            i += 1;
            continue;
        };
        let used = match (field.as_str(), values.get(i + 1)) {
            (KEY_PHASE, Some(XDataValue::String(text))) => {
                phase = Phase::from_key(text);
                2
            }
            (KEY_PART, Some(XDataValue::String(text))) => {
                part = RoomPart::from_key(text);
                2
            }
            (KEY_NAME, Some(XDataValue::String(text))) => {
                name = text.clone();
                2
            }
            (KEY_NUMBER, Some(XDataValue::String(text))) => {
                number = text.clone();
                2
            }
            (KEY_POINT, Some(XDataValue::Point3D(p))) => {
                seed = Some([p.x, p.y]);
                2
            }
            _ => 1,
        };
        i += used;
    }
    Some((
        Room {
            phase: phase?,
            name,
            number,
            seed: seed?,
        },
        part?,
    ))
}

/// Le entità di locale dello spazio modello (non quelle in carta o dentro i
/// blocchi), in ordine di disegno.
pub fn rooms_in(doc: &CadDocument) -> Vec<(Handle, Room, RoomPart)> {
    doc.block_records
        .iter()
        .filter(|record| record.is_model_space())
        .flat_map(|record| record.entity_handles.iter())
        .filter_map(|&handle| {
            let entity = doc.get_entity(handle)?;
            let (room, part) = decode(entity.common().extended_data.get_record(APP_LOCALE)?)?;
            Some((handle, room, part))
        })
        .collect()
}

/// Il testo di un locale: `A\PS = 12,00 m²` (`\P` è l'a capo dell'MTEXT),
/// con ` (da confermare)` dopo il nome quando `confirm`. Nel nome barre e
/// graffe hanno la barra davanti, così restano testo.
pub fn label(name: &str, area_m2: f64, confirm: bool) -> String {
    let mark = if confirm { TO_CONFIRM } else { "" };
    format!("{}{mark}\\PS = {}", mtext_escape(name), format_m2(area_m2))
}

/// `\`, `{` e `}` sono codici di formato dell'MTEXT.
fn mtext_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '\\' | '{' | '}') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Un locale calcolato, pronto per [`room_entities`].
#[derive(Clone, Debug, PartialEq)]
pub struct Computed {
    pub name: String,
    pub number: String,
    pub area_m2: f64,
    pub region: Region,
    /// Nome composto da semi di fatto fusi: «da confermare».
    pub confirm: bool,
    /// Area dalle curve vere dei muri (`walls::net_area`).
    pub exact: bool,
    pub islands: usize,
    /// Dove va il testo, in unità del disegno.
    pub seed: [f64; 2],
}

/// Il locale attorno a `seed`: la regione, l'area netta in m², se l'area è
/// esatta e quante isole sono state sottratte. `None` se nessuna faccia
/// contiene il punto o se la regione contiene un gruppo di muri staccato (il
/// locale vero è aperto: vedi la documentazione del modulo). `faces` vengono da
/// `walls::faces(sources, tol)`, con la stessa `tol` in unità del disegno.
///
/// Un locale chiuso solo grazie a una tolleranza larga (`HPGAPTOL` sopra
/// `unit.tolerance()`) non è esatto (R47): si rifà la misura sulle facce alla
/// tolleranza di base e, se lì il locale non è chiuso o l'area cambia più di
/// [`GAP_SLACK`] tolleranze di base per il perimetro, il risultato resta
/// quello alla tolleranza larga ma con `exact = false` (il chiamante dà
/// [`Notice::ContornoApprossimato`]). Le facce alla tolleranza di base si
/// calcolano qui; chi misura molti locali le passa una volta sola con
/// [`compute_at_with_base`].
pub fn compute_at(
    faces: &[Vec<[f64; 2]>],
    sources: &FxHashMap<Handle, BoundarySource>,
    seed: [f64; 2],
    unit: DrawingUnit,
    tol: f64,
) -> Option<(Region, f64, bool, usize)> {
    let base_faces = gap_tolerance(unit, tol).then(|| wall_faces(sources, unit.tolerance()));
    compute_checked(faces, base_faces.as_deref(), sources, seed, unit, tol)
}

/// [`compute_at`] con le facce alla tolleranza di base già pronte:
/// `base_faces = walls::faces(sources, unit.tolerance())`. Con `tol` uguale
/// alla tolleranza di base non si usano.
pub fn compute_at_with_base(
    faces: &[Vec<[f64; 2]>],
    base_faces: &[Vec<[f64; 2]>],
    sources: &FxHashMap<Handle, BoundarySource>,
    seed: [f64; 2],
    unit: DrawingUnit,
    tol: f64,
) -> Option<(Region, f64, bool, usize)> {
    compute_checked(faces, Some(base_faces), sources, seed, unit, tol)
}

/// Quante tolleranze di base, per unità di perimetro, può cambiare l'area tra
/// la misura alla tolleranza larga e quella alla tolleranza di base (lo
/// spostamento ammesso di un lato, come la saldatura delle facce).
pub const GAP_SLACK: f64 = 4.0;

/// `tol` supera la tolleranza di base oltre l'arrotondamento?
fn gap_tolerance(unit: DrawingUnit, tol: f64) -> bool {
    tol > unit.tolerance() * (1.0 + 1.0e-9)
}

/// R47: con la tolleranza larga il locale vale esatto solo se alla
/// tolleranza di base (`base_faces`) è chiuso e misura lo stesso. Senza
/// `base_faces` con una tolleranza larga, non esatto.
fn compute_checked(
    faces: &[Vec<[f64; 2]>],
    base_faces: Option<&[Vec<[f64; 2]>]>,
    sources: &FxHashMap<Handle, BoundarySource>,
    seed: [f64; 2],
    unit: DrawingUnit,
    tol: f64,
) -> Option<(Region, f64, bool, usize)> {
    let (region, area, exact, islands) = measure(faces, sources, seed, unit, tol)?;
    let exact = exact
        && (!gap_tolerance(unit, tol)
            || base_faces.is_some_and(|base_faces| {
                let base = unit.tolerance();
                let perimeter: f64 = std::iter::once(&region.outer)
                    .chain(&region.holes)
                    .flat_map(|ring| ring_lines(ring))
                    .map(|line| length(&line))
                    .sum();
                let slack = area_m2(GAP_SLACK * base * perimeter, unit);
                measure(base_faces, sources, seed, unit, base)
                    .is_some_and(|(_, base_area, _, _)| (base_area - area).abs() <= slack)
            }));
    Some((region, area, exact, islands))
}

/// La misura alla tolleranza `tol`, senza il controllo di R47.
fn measure(
    faces: &[Vec<[f64; 2]>],
    sources: &FxHashMap<Handle, BoundarySource>,
    seed: [f64; 2],
    unit: DrawingUnit,
    tol: f64,
) -> Option<(Region, f64, bool, usize)> {
    let region = region_at(faces, seed, tol)?;
    if has_floating_group(&region, sources, tol) {
        return None;
    }
    let (area, exact) = net_area(&region, sources, tol);
    let islands = region.holes.len();
    Some((region, area_m2(area, unit), exact, islands))
}

/// Il contorno e il testo del locale, entrambi con il record della `phase`
/// data, sul layer `AL-LOCALI`. Il contorno segue l'anello esterno: con gli
/// archi veri (`boundary_polyline_entities`) se l'area è esatta, altrimenti
/// l'anello delle facce, lo stesso che ha dato il numero. Il testo
/// ([`label`]) sta sul seme, centrato, senza a capo automatico.
pub fn room_entities(
    c: &Computed,
    phase: Phase,
    sources: &FxHashMap<Handle, BoundarySource>,
    text_height: f64,
    tol: f64,
) -> Vec<EntityType> {
    let room = Room {
        phase,
        name: c.name.clone(),
        number: c.number.clone(),
        seed: c.seed,
    };
    let traced = if c.exact {
        boundary_polyline_entities(
            &[vec![c.region.outer.clone()]],
            WorkingPlane::default(),
            sources,
            tol,
        )
        .into_iter()
        .next()
    } else {
        None
    };
    let mut outline = traced.unwrap_or_else(|| ring_polyline(&c.region.outer));
    tag(
        outline.common_mut(),
        LAYER_LOCALI,
        &room,
        RoomPart::Contorno,
    );
    let mut text = mtext_at(c.seed, label(&c.name, c.area_m2, c.confirm), text_height);
    tag(text.common_mut(), LAYER_LOCALI, &room, RoomPart::Testo);
    vec![outline, text]
}

/// Il seme di un locale di progetto: un `MTEXT` con il solo nome sul punto,
/// layer `AL-LOCALI-PROGETTO`.
pub fn seed_entity(room: &Room, text_height: f64) -> EntityType {
    let mut text = mtext_at(room.seed, mtext_escape(&room.name), text_height);
    tag(
        text.common_mut(),
        LAYER_LOCALI_PROGETTO,
        room,
        RoomPart::Seme,
    );
    text
}

fn ring_polyline(ring: &[[f64; 2]]) -> EntityType {
    let mut polyline = LwPolyline::new();
    polyline.vertices = ring
        .iter()
        .map(|p| LwVertex::new(Vector2::new(p[0], p[1])))
        .collect();
    polyline.is_closed = true;
    EntityType::LwPolyline(polyline)
}

/// Un `MTEXT` centrato sul punto, senza a capo automatico. Un'altezza non
/// positiva (o non numerica) lascia quella predefinita dell'MTEXT.
fn mtext_at(at: [f64; 2], value: String, height: f64) -> EntityType {
    let mut text = MText::with_value(value, Vector3::new(at[0], at[1], 0.0));
    if height.is_finite() && height > 0.0 {
        text.height = height;
    }
    text.attachment_point = AttachmentPoint::MiddleCenter;
    text.rectangle_width = 0.0;
    EntityType::MText(Box::new(text))
}

fn tag(common: &mut EntityCommon, layer: &str, room: &Room, part: RoomPart) {
    common.layer = layer.to_string();
    let mut rec = ExtendedDataRecord::new(APP_LOCALE);
    for value in encode(room, part) {
        rec.add_value(value);
    }
    common.extended_data.upsert_record(rec);
}

/// I locali di progetto sulla geometria di progetto (`pg_faces` da
/// `walls::faces(pg_sources, tol)`, `pg_sources` da `walls::sources_of` di
/// `ProjectGeometry::room_entities`), con i loro avvisi. Regole nella
/// documentazione del modulo. `fatto_regions` sono le regioni dei locali di
/// fatto sul rilievo; `zones` quelle del disegno (`zone::zones_in`). Ordine:
/// i gruppi nell'ordine del loro primo seme, poi i locali senza nome.
#[allow(clippy::too_many_arguments)]
pub fn project_rooms(
    pg_faces: &[Vec<[f64; 2]>],
    pg_sources: &FxHashMap<Handle, BoundarySource>,
    fatto: &[Room],
    fatto_regions: &[Region],
    progetto: &[Room],
    zones: &[ZoneGeom],
    unit: DrawingUnit,
    tol: f64,
) -> (Vec<Computed>, Vec<Notice>) {
    let tolerance = Tolerance::new(tol);
    let zones: Vec<ZoneShape> = zones.iter().filter_map(ZoneShape::new).collect();
    let mut notices = Vec::new();
    // R47: le facce alla tolleranza di base, una volta sola.
    let base_faces = gap_tolerance(unit, tol).then(|| wall_faces(pg_sources, unit.tolerance()));
    let compute_at = |seed: [f64; 2]| {
        compute_checked(pg_faces, base_faces.as_deref(), pg_sources, seed, unit, tol)
    };

    // 1. I semi, raggruppati per faccia.
    let mut groups: Vec<Group> = Vec::new();
    let seeds = fatto
        .iter()
        .map(|room| (room, false))
        .chain(progetto.iter().map(|room| (room, true)));
    for (room, of_project) in seeds {
        if of_project
            && zones
                .iter()
                .any(|zone| zone.state == ZoneState::Nuovo && zone.holds(room.seed, tolerance))
        {
            notices.push(Notice::SemeInMuroNuovo {
                name: room.name.clone(),
            });
            continue;
        }
        let Some(found) = compute_at(room.seed) else {
            notices.push(Notice::LocaleNonChiuso {
                name: room.name.clone(),
            });
            continue;
        };
        // La faccia sta tutta in una zona? Decide il suo punto interno, come
        // per le facce senza seme: il seme può stare sul bordo della zona.
        // Un seme di progetto nella faccia di un varco lo dice (R48).
        if !zones.is_empty() {
            let probe = interior_point(&found.0, tol).unwrap_or(room.seed);
            if let Some(zone) = zones
                .iter()
                .find(|zone| zone.covers(&found.0.outer, probe, tolerance))
            {
                if of_project && zone.state == ZoneState::Demolito && zone.varco {
                    notices.push(Notice::SemeNelVarco {
                        name: room.name.clone(),
                    });
                }
                continue;
            }
        }
        match groups
            .iter_mut()
            .find(|group| group.found.0.outer == found.0.outer)
        {
            Some(group) => group.add(room, of_project),
            None => {
                let mut group = Group {
                    found,
                    fatto: Vec::new(),
                    progetto: Vec::new(),
                };
                group.add(room, of_project);
                groups.push(group);
            }
        }
    }
    // 2. Nomi.
    let mut rooms: Vec<Computed> = groups
        .into_iter()
        .map(|group| group.into_room(&mut notices))
        .collect();

    // 3. Le altre facce: varchi e locali senza nome.
    let fatto_outers: Vec<(Bounds, Vec<Curve>)> = fatto_regions
        .iter()
        .filter(|region| region.outer.len() >= 3)
        .map(|region| (bounds(&region.outer), ring_curves(&region.outer)))
        .collect();
    let mut unnamed: Vec<Computed> = Vec::new();
    for face in face_regions(pg_faces, tol) {
        if rooms
            .iter()
            .any(|room| room.region.outer == face.outer || room.region.holes.contains(&face.outer))
        {
            continue;
        }
        let face_box = bounds(&face.outer);
        let near_zone = zones
            .iter()
            .any(|zone| box_within(&face_box, &zone.bounds, tol));
        let near_fatto = fatto_outers
            .iter()
            .any(|(fatto_box, _)| boxes_overlap(&face_box, fatto_box, tol));
        if !near_zone && !near_fatto {
            continue;
        }
        let Some(inner) = interior_point(&face, tol) else {
            continue;
        };
        if let Some(zone) = zones
            .iter()
            .find(|zone| zone.covers(&face.outer, inner, tolerance))
        {
            if zone.state == ZoneState::Demolito && zone.varco {
                let (area, _) = net_area(&face, pg_sources, tol);
                notices.push(Notice::Varco {
                    area_m2: area_m2(area, unit),
                });
            }
            continue;
        }
        if !fatto_outers
            .iter()
            .any(|(_, outer)| contains(outer, inner, tolerance))
        {
            continue;
        }
        match compute_at(inner) {
            Some((region, area_m2, exact, islands)) if region.outer == face.outer => {
                unnamed.push(Computed {
                    name: UNNAMED.to_string(),
                    number: String::new(),
                    area_m2,
                    region,
                    confirm: false,
                    exact,
                    islands,
                    seed: inner,
                })
            }
            // Una faccia chiusa con dentro un gruppo di muri staccato.
            _ => notices.push(Notice::LocaleNonChiuso {
                name: UNNAMED.to_string(),
            }),
        }
    }
    let unnamed_holes: Vec<Vec<[f64; 2]>> = unnamed
        .iter()
        .flat_map(|room| room.region.holes.iter().cloned())
        .collect();
    for room in unnamed {
        if unnamed_holes.contains(&room.region.outer) {
            continue;
        }
        notices.push(Notice::LocaleSenzaNome {
            area_m2: room.area_m2,
        });
        push_quality(&mut notices, &room.name, room.exact, room.islands);
        rooms.push(room);
    }
    (rooms, notices)
}

/// I semi che cadono nella stessa faccia di progetto.
struct Group<'a> {
    found: (Region, f64, bool, usize),
    fatto: Vec<&'a Room>,
    progetto: Vec<&'a Room>,
}

impl<'a> Group<'a> {
    fn add(&mut self, room: &'a Room, of_project: bool) {
        if of_project {
            self.progetto.push(room);
        } else {
            self.fatto.push(room);
        }
    }

    /// Il locale col suo nome meccanico, e i suoi avvisi.
    fn into_room(self, notices: &mut Vec<Notice>) -> Computed {
        let (region, area_m2, exact, islands) = self.found;
        let names = |rooms: &[&Room]| -> Vec<String> {
            rooms.iter().map(|room| room.name.clone()).collect()
        };
        let (name, number, seed, confirm) = match self.progetto.first() {
            Some(first) => {
                if self.progetto.len() > 1 {
                    notices.push(Notice::LocaleFuso {
                        names: names(&self.progetto),
                        area_m2,
                    });
                }
                (first.name.clone(), first.number.clone(), first.seed, false)
            }
            None => {
                let merged = self.fatto.len() > 1;
                if merged {
                    notices.push(Notice::LocaleFuso {
                        names: names(&self.fatto),
                        area_m2,
                    });
                }
                let numbers: Vec<&str> = self
                    .fatto
                    .iter()
                    .map(|room| room.number.as_str())
                    .filter(|number| !number.is_empty())
                    .collect();
                (
                    names(&self.fatto).join(JOIN),
                    numbers.join(JOIN),
                    self.fatto[0].seed,
                    merged,
                )
            }
        };
        push_quality(notices, &name, exact, islands);
        Computed {
            name,
            number,
            area_m2,
            region,
            confirm,
            exact,
            islands,
            seed,
        }
    }
}

fn push_quality(notices: &mut Vec<Notice>, name: &str, exact: bool, islands: usize) {
    if islands > 0 {
        notices.push(Notice::Isole {
            name: name.to_string(),
            count: islands,
        });
    }
    if !exact {
        notices.push(Notice::ContornoApprossimato {
            name: name.to_string(),
        });
    }
}

/// Una zona pronta per i controlli dei locali.
struct ZoneShape {
    state: ZoneState,
    varco: bool,
    outer: Vec<Curve>,
    holes: Vec<Vec<Curve>>,
    bounds: Bounds,
}

impl ZoneShape {
    fn new(geom: &ZoneGeom) -> Option<Self> {
        let (outer, holes) = geom.rings.split_first()?;
        if outer.len() < 3 {
            return None;
        }
        Some(Self {
            state: geom.zone.state,
            varco: geom.zone.varco,
            outer: ring_curves(outer),
            holes: holes
                .iter()
                .filter(|hole| hole.len() >= 3)
                .map(|hole| ring_curves(hole))
                .collect(),
            bounds: bounds(outer),
        })
    }

    /// `p` sta nella zona, bordo compreso.
    fn holds(&self, p: [f64; 2], tolerance: Tolerance) -> bool {
        contains(&self.outer, p, tolerance)
            && !self
                .holes
                .iter()
                .any(|hole| strictly_in(hole, p, tolerance))
    }

    /// La faccia `ring`, con il punto interno `inner`, sta tutta nella zona:
    /// il punto strettamente dentro, i vertici dentro o sul bordo esterno.
    fn covers(&self, ring: &[[f64; 2]], inner: [f64; 2], tolerance: Tolerance) -> bool {
        box_within(&bounds(ring), &self.bounds, tolerance.linear())
            && strictly_in(&self.outer, inner, tolerance)
            && !self
                .holes
                .iter()
                .any(|hole| contains(hole, inner, tolerance))
            && ring
                .iter()
                .all(|&vertex| contains(&self.outer, vertex, tolerance))
    }
}

/// Un punto della regione a più di `tol` da ogni suo anello (vedi la
/// documentazione del modulo, regola 3). `None` se la triangolazione non ne
/// dà nessuno.
fn interior_point(region: &Region, tol: f64) -> Option<[f64; 2]> {
    let tolerance = Tolerance::new(tol);
    let outer = ring_curves(&region.outer);
    let holes: Vec<Vec<Curve>> = region.holes.iter().map(|hole| ring_curves(hole)).collect();
    let inside = |p: [f64; 2]| {
        strictly_in(&outer, p, tolerance)
            && holes.iter().all(|hole| {
                hole.iter()
                    .all(|edge| distance_to(edge, p) > tolerance.linear())
                    && !contains(hole, p, tolerance)
            })
    };
    // `triangulate` (orecchie, buchi uniti con ponti) non dà nessun triangolo
    // quando i ponti corrono lungo lati allineati, come per il contorno
    // esterno con i locali dentro; `triangulate_rings` controlla l'area e
    // ripiega su fasce pari-dispari, anche con buchi che si toccano.
    let first = triangulate(&region.outer, &region.holes);
    centres_by_area(first)
        .into_iter()
        .find(|&p| inside(p))
        .or_else(|| {
            let rings: Vec<Vec<[f64; 2]>> = std::iter::once(&region.outer)
                .chain(&region.holes)
                .cloned()
                .collect();
            centres_by_area(triangulate_rings(&rings))
                .into_iter()
                .find(|&p| inside(p))
        })
}

/// I baricentri dei triangoli, dal più grande; a pari area vale l'ordine
/// della triangolazione.
fn centres_by_area((points, triangles): (Vec<[f64; 2]>, Vec<[usize; 3]>)) -> Vec<[f64; 2]> {
    let mut candidates: Vec<([f64; 2], f64)> = triangles
        .iter()
        .filter(|triangle| triangle.iter().all(|&index| index < points.len()))
        .map(|triangle| {
            let [a, b, c] = triangle.map(|index| points[index]);
            let centre = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
            (centre, signed_area(&[a, b, c]).abs())
        })
        .collect();
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    candidates.into_iter().map(|(centre, _)| centre).collect()
}

/// Regola del gruppo staccato (documentazione del modulo). I lati degli anelli
/// della regione sono un solo gruppo; i segmenti dei muri che toccano il
/// riquadro della regione vi si uniscono quando si toccano entro `tol`
/// ([`touch`]). Un gruppo che non arriva agli anelli non li attraversa, quindi
/// sta tutto da una parte: decide il punto medio di un suo segmento. I
/// segmenti lunghi `tol` o meno si saltano, come fa la ricerca delle facce.
fn has_floating_group(
    region: &Region,
    sources: &FxHashMap<Handle, BoundarySource>,
    tol: f64,
) -> bool {
    let mut lines: Vec<Line> = std::iter::once(&region.outer)
        .chain(&region.holes)
        .flat_map(|ring| ring_lines(ring))
        .collect();
    let border = lines.len();
    if border == 0 {
        return false;
    }
    let reach = bounds(&region.outer);
    lines.extend(
        sources
            .values()
            .flat_map(|source| source.segments.iter().copied())
            .filter(|segment| {
                length(segment) > tol
                    && boxes_overlap(&bounds(&[segment.start, segment.end]), &reach, tol)
            }),
    );
    if lines.len() == border {
        return false;
    }

    let boxes: Vec<Bounds> = lines
        .iter()
        .map(|line| bounds(&[line.start, line.end]))
        .collect();
    let mut groups = UnionFind::new(lines.len());
    for index in 1..border {
        groups.union(0, index);
    }
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by(|&a, &b| boxes[a].0[0].total_cmp(&boxes[b].0[0]));
    let mut active: Vec<usize> = Vec::new();
    for index in order {
        let front = boxes[index].0[0];
        active.retain(|&other| boxes[other].1[0] + tol >= front);
        for &other in &active {
            if (index < border && other < border)
                || !boxes_overlap(&boxes[index], &boxes[other], tol)
                || groups.find(index) == groups.find(other)
            {
                continue;
            }
            if touch(lines[index], lines[other], tol) {
                groups.union(index, other);
            }
        }
        active.push(index);
    }

    let tolerance = Tolerance::new(tol);
    let outer = ring_curves(&region.outer);
    let holes: Vec<Vec<Curve>> = region.holes.iter().map(|hole| ring_curves(hole)).collect();
    let border_group = groups.find(0);
    let mut seen = FxHashSet::default();
    (border..lines.len()).any(|index| {
        let group = groups.find(index);
        if group == border_group || !seen.insert(group) {
            return false;
        }
        let line = lines[index];
        let middle = [
            (line.start[0] + line.end[0]) / 2.0,
            (line.start[1] + line.end[1]) / 2.0,
        ];
        contains(&outer, middle, tolerance)
            && !holes.iter().any(|hole| contains(hole, middle, tolerance))
    })
}

/// Due segmenti si toccano entro `tol`: si incrociano (o si sovrappongono)
/// secondo `segment_crossing`, oppure un estremo dell'uno dista dall'altro non
/// più di `tol` (anche quasi paralleli, dove l'incrocio delle rette cade
/// lontano).
fn touch(a: Line, b: Line, tol: f64) -> bool {
    if !matches!(
        segment_crossing(a, b, Tolerance::new(tol)),
        SegmentCrossing::None
    ) {
        return true;
    }
    let (along_a, along_b) = (Curve::Line(a), Curve::Line(b));
    [a.start, a.end]
        .iter()
        .any(|&p| distance_to(&along_b, p) <= tol)
        || [b.start, b.end]
            .iter()
            .any(|&p| distance_to(&along_a, p) <= tol)
}

/// Insiemi disgiunti con compressione dei cammini.
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
        }
    }

    fn find(&mut self, mut index: usize) -> usize {
        while self.parent[index] != index {
            self.parent[index] = self.parent[self.parent[index]];
            index = self.parent[index];
        }
        index
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.parent[b] = a;
        }
    }
}

/// Un punto strettamente dentro: a più di `tol` dal bordo, e dentro.
fn strictly_in(curves: &[Curve], p: [f64; 2], tolerance: Tolerance) -> bool {
    curves
        .iter()
        .all(|edge| distance_to(edge, p) > tolerance.linear())
        && contains(curves, p, tolerance)
}

fn ring_lines(ring: &[[f64; 2]]) -> Vec<Line> {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(&start, &end)| Line { start, end })
        .filter(|line| length(line) > 0.0)
        .collect()
}

fn ring_curves(ring: &[[f64; 2]]) -> Vec<Curve> {
    ring_lines(ring).into_iter().map(Curve::Line).collect()
}

fn length(line: &Line) -> f64 {
    (line.end[0] - line.start[0]).hypot(line.end[1] - line.start[1])
}

type Bounds = ([f64; 2], [f64; 2]);

fn bounds(points: &[[f64; 2]]) -> Bounds {
    points.iter().fold(
        ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]),
        |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        },
    )
}

/// I riquadri si toccano, allargati di `tol`?
fn boxes_overlap(a: &Bounds, b: &Bounds, tol: f64) -> bool {
    (0..2).all(|axis| a.0[axis] <= b.1[axis] + tol && b.0[axis] <= a.1[axis] + tol)
}

/// Il riquadro `inner` sta in `outer`, allargato di `tol`?
fn box_within(inner: &Bounds, outer: &Bounds, tol: f64) -> bool {
    (0..2).all(|axis| inner.0[axis] >= outer.0[axis] - tol && inner.1[axis] <= outer.1[axis] + tol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI};
    use std::io::Cursor;

    use codec::entities::{Arc, AttachmentPoint, Line as LineEntity, LwPolyline, LwVertex, MText};
    use codec::{Vector2, Vector3};
    use kernel::geom2d::{signed_area, triangulate};

    use crate::bim::fixtures::{case_a, pilastro, scene_with, Form, LAYER_MURI};
    use crate::bim::project::project_geometry;
    use crate::bim::units::area_m2;
    use crate::bim::walls::{
        faces, net_area, rect_from_axis, rect_from_axis_thickness, region_at, sources_of, wall_set,
        WallRect,
    };
    use crate::bim::zone::{ensure_layers, read_zone, zone_hatch, Zone, ZoneOrigin, ZoneState};
    use crate::bim::{APP_LOCALE, LAYER_LOCALI, LAYER_LOCALI_PROGETTO, MAX_WALL_THICKNESS_M};

    const M: DrawingUnit = DrawingUnit::Metri;
    const TOL: f64 = 1.0e-6;

    // -----------------------------------------------------------------------
    // Aiuti
    // -----------------------------------------------------------------------

    /// Aree in m² confrontate entro 1e-9 m².
    #[track_caller]
    fn assert_m2(got: f64, want: f64, what: impl std::fmt::Debug) {
        assert!(
            (got - want).abs() < 1e-9,
            "{what:?}: {got} m² invece di {want} m²"
        );
    }

    fn room(phase: Phase, name: &str, seed: [f64; 2]) -> Room {
        Room {
            phase,
            name: name.into(),
            number: String::new(),
            seed,
        }
    }

    fn fatto(name: &str, seed: [f64; 2]) -> Room {
        room(Phase::Fatto, name, seed)
    }

    fn progetto(name: &str, seed: [f64; 2]) -> Room {
        room(Phase::Progetto, name, seed)
    }

    fn text(value: &str) -> XDataValue {
        XDataValue::String(value.into())
    }

    fn record(values: Vec<XDataValue>) -> ExtendedDataRecord {
        let mut rec = ExtendedDataRecord::new(APP_LOCALE);
        for value in values {
            rec.add_value(value);
        }
        rec
    }

    fn wall_line(a: [f64; 2], b: [f64; 2]) -> EntityType {
        let mut line =
            LineEntity::from_points(Vector3::new(a[0], a[1], 0.0), Vector3::new(b[0], b[1], 0.0));
        line.common.layer = LAYER_MURI.to_string();
        EntityType::Line(line)
    }

    fn wall_polyline(vertices: &[([f64; 2], f64)], closed: bool) -> EntityType {
        let mut poly = LwPolyline::new();
        for &(p, bulge) in vertices {
            poly.add_vertex(LwVertex::with_bulge(Vector2::new(p[0], p[1]), bulge));
        }
        poly.is_closed = closed;
        poly.common.layer = LAYER_MURI.to_string();
        EntityType::LwPolyline(poly)
    }

    fn is_line(entity: &EntityType, a: [f64; 2], b: [f64; 2]) -> bool {
        let EntityType::Line(line) = entity else {
            return false;
        };
        let near = |p: Vector3, q: [f64; 2]| (p.x - q[0]).abs() < 1e-9 && (p.y - q[1]).abs() < 1e-9;
        (near(line.start, a) && near(line.end, b)) || (near(line.start, b) && near(line.end, a))
    }

    /// Facce e sorgenti dei muri di `entities`, alla tolleranza `tol`.
    fn survey(
        entities: Vec<EntityType>,
        tol: f64,
    ) -> (Vec<Vec<[f64; 2]>>, FxHashMap<Handle, BoundarySource>) {
        let scene = scene_with(entities, 6);
        let walls = wall_set(&scene.document, tol);
        assert_eq!(walls.off_plane, 0);
        (faces(&walls.sources, tol), walls.sources)
    }

    /// Il locale attorno a `seed` sul rilievo, già misurato.
    fn computed(
        faces: &[Vec<[f64; 2]>],
        sources: &FxHashMap<Handle, BoundarySource>,
        name: &str,
        seed: [f64; 2],
    ) -> Computed {
        let (region, area_m2, exact, islands) =
            compute_at(faces, sources, seed, M, TOL).expect("locale chiuso");
        Computed {
            name: name.into(),
            number: String::new(),
            area_m2,
            region,
            confirm: false,
            exact,
            islands,
            seed,
        }
    }

    /// La zona come la legge il disegno: campitura scritta da `zone_hatch` e
    /// riletta da `read_zone`.
    fn zone_geom(zone: Zone, ring: Vec<[f64; 2]>) -> ZoneGeom {
        let hatch = zone_hatch(&zone, &[ring]);
        read_zone(Handle::new(0xA000), &hatch).expect("zona leggibile")
    }

    fn axis_zone_with(
        state: ZoneState,
        origin: ZoneOrigin,
        rect: WallRect,
        varco: bool,
    ) -> ZoneGeom {
        zone_geom(
            Zone {
                state,
                origin,
                axis: Some(rect.axis),
                thickness: Some(rect.thickness),
                height: None,
                varco,
            },
            rect.ring(),
        )
    }

    /// Zona demolita «muro tra due punti» sulle facce del caso A (il
    /// tramezzo è lo stesso nelle due forme).
    fn axis_zone(p1: [f64; 2], p2: [f64; 2], varco: bool) -> ZoneGeom {
        let scene = scene_with(case_a(Form::Continua, M), 6);
        let sources = wall_set(&scene.document, TOL).sources;
        let rect = rect_from_axis(&sources, p1, p2, MAX_WALL_THICKNESS_M, TOL).expect("muro");
        axis_zone_with(ZoneState::Demolito, ZoneOrigin::MuroDuePunti, rect, varco)
    }

    /// Zona nuova «linea + spessore».
    fn new_zone(p1: [f64; 2], p2: [f64; 2], thickness: f64) -> ZoneGeom {
        let rect = rect_from_axis_thickness(p1, p2, thickness).expect("rettangolo");
        axis_zone_with(ZoneState::Nuovo, ZoneOrigin::LineaSpessore, rect, false)
    }

    /// `project_rooms` sulla geometria di progetto di `entities` (metri,
    /// tolleranza 1e-6), con le regioni di fatto misurate sul rilievo.
    fn rooms_of(
        entities: Vec<EntityType>,
        zones: &[ZoneGeom],
        fatto: &[Room],
        progetto: &[Room],
    ) -> (Vec<Computed>, Vec<Notice>) {
        let scene = scene_with(entities, 6);
        let walls = wall_set(&scene.document, TOL);
        let survey_faces = faces(&walls.sources, TOL);
        let fatto_regions: Vec<Region> = fatto
            .iter()
            .map(|room| {
                compute_at(&survey_faces, &walls.sources, room.seed, M, TOL)
                    .expect("locale di fatto chiuso")
                    .0
            })
            .collect();
        let pg = project_geometry(&scene.document, &walls, zones, LAYER_MURI, TOL);
        let (pg_sources, off_plane) = sources_of(&pg.room_entities, TOL);
        assert_eq!(off_plane, 0);
        let pg_faces = faces(&pg_sources, TOL);
        project_rooms(
            &pg_faces,
            &pg_sources,
            fatto,
            &fatto_regions,
            progetto,
            zones,
            M,
            TOL,
        )
    }

    fn names(rooms: &[Computed]) -> Vec<&str> {
        rooms.iter().map(|room| room.name.as_str()).collect()
    }

    fn named<'a>(rooms: &'a [Computed], name: &str) -> &'a Computed {
        rooms
            .iter()
            .find(|room| room.name == name)
            .unwrap_or_else(|| panic!("manca {name}; locali: {:?}", names(rooms)))
    }

    fn fatto_ab() -> [Room; 2] {
        [fatto("A", [2.0, 1.5]), fatto("B", [6.0, 1.5])]
    }

    fn total_demolition() -> Vec<ZoneGeom> {
        vec![axis_zone([4.05, 0.0], [4.05, 3.0], false)]
    }

    fn inside_box(p: [f64; 2], lo: [f64; 2], hi: [f64; 2]) -> bool {
        p[0] > lo[0] && p[0] < hi[0] && p[1] > lo[1] && p[1] < hi[1]
    }

    /// Dentro A o B del caso A.
    fn in_a_or_b(p: [f64; 2]) -> bool {
        inside_box(p, [0.0, 0.0], [4.0, 3.0]) || inside_box(p, [4.1, 0.0], [8.0, 3.0])
    }

    // -----------------------------------------------------------------------
    // XDATA
    // -----------------------------------------------------------------------

    #[test]
    fn xdata_sequence_and_roundtrip() {
        let a = Room {
            phase: Phase::Fatto,
            name: "A".into(),
            number: "1".into(),
            seed: [2.0, 1.5],
        };
        assert_eq!(
            encode(&a, RoomPart::Contorno),
            vec![
                XDataValue::Integer16(1),
                text("fase"),
                text("fatto"),
                text("parte"),
                text("contorno"),
                text("nome"),
                text("A"),
                text("numero"),
                text("1"),
                text("punto"),
                XDataValue::Point3D(Vector3::new(2.0, 1.5, 0.0)),
                text("origine"),
                text("punto-interno"),
            ]
        );
        let keys = |room: &Room, part| match &encode(room, part)[..] {
            [_, _, XDataValue::String(phase), _, XDataValue::String(part), ..] => {
                (phase.clone(), part.clone())
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(
            keys(&progetto("B", [0.0, 0.0]), RoomPart::Seme),
            ("progetto".to_string(), "seme".to_string())
        );
        assert_eq!(keys(&a, RoomPart::Testo).1, "testo");
        for phase in [Phase::Fatto, Phase::Progetto] {
            for part in [RoomPart::Contorno, RoomPart::Testo, RoomPart::Seme] {
                for (name, number) in [("A + B", ""), ("WC\\Doccia {2}", "12")] {
                    let room = Room {
                        phase,
                        name: name.into(),
                        number: number.into(),
                        seed: [-1250.5, 4_650_000.25],
                    };
                    assert_eq!(
                        decode(&record(encode(&room, part))),
                        Some((room.clone(), part)),
                        "{room:?} {part:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn decode_reads_by_key() {
        let full = || {
            vec![
                text("sconosciuta"),
                XDataValue::Real(3.0),
                text("punto"),
                XDataValue::Point3D(Vector3::new(1.0, 2.0, 0.0)),
                text("parte"),
                text("testo"),
                XDataValue::Integer16(9),
                text("fase"),
                text("progetto"),
                text("nome"),
                text("Bagno"),
            ]
        };
        assert_eq!(
            decode(&record(full())),
            Some((progetto("Bagno", [1.0, 2.0]), RoomPart::Testo)),
            "chiavi in un altro ordine, sconosciute saltate, numero assente = vuoto"
        );
        // Senza fase, parte o punto (o con valori sconosciuti) non è un locale.
        for missing in ["fase", "parte", "punto"] {
            let mut values = full();
            let at = values
                .iter()
                .position(|value| *value == text(missing))
                .unwrap();
            values.drain(at..at + 2);
            assert_eq!(decode(&record(values)), None, "senza {missing}");
        }
        for (key, wrong) in [("fase", "rilievo"), ("parte", "etichetta")] {
            let mut values = full();
            let at = values.iter().position(|value| *value == text(key)).unwrap();
            values[at + 1] = text(wrong);
            assert_eq!(decode(&record(values)), None, "{key} = {wrong}");
        }
    }

    #[test]
    fn label_text() {
        assert_eq!(label("A", 12.0, false), "A\\PS = 12,00 m²");
        assert_eq!(
            label("A + B", 24.0, true),
            "A + B (da confermare)\\PS = 24,00 m²"
        );
        assert_eq!(label(UNNAMED, 5.82, false), "(senza nome)\\PS = 5,82 m²");
        // I caratteri di formato dell'MTEXT nel nome restano testo.
        assert_eq!(
            label("WC\\Doccia {2}", 4.0, false),
            "WC\\\\Doccia \\{2\\}\\PS = 4,00 m²"
        );
    }

    // -----------------------------------------------------------------------
    // Calcolo attorno a un punto
    // -----------------------------------------------------------------------

    #[test]
    fn compute_at_gives_net_area_and_islands() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let (faces, sources) = survey(case_a(form, M), TOL);
            let (region, area, exact, islands) =
                compute_at(&faces, &sources, [2.0, 1.5], M, TOL).expect("A");
            assert_m2(area, 12.0, form);
            assert!(exact, "{form:?}");
            assert_eq!(islands, 0);
            assert!(region.holes.is_empty());
            let (_, area, exact, _) = compute_at(&faces, &sources, [6.0, 1.5], M, TOL).expect("B");
            assert_m2(area, 11.70, form);
            assert!(exact);

            // Caso D: il pilastro è un'isola, sottratta.
            let mut e = case_a(form, M);
            e.push(pilastro(M));
            let (faces, sources) = survey(e, TOL);
            let (region, area, exact, islands) =
                compute_at(&faces, &sources, [2.0, 2.5], M, TOL).expect("A con pilastro");
            assert_m2(area, 11.91, form);
            assert!(exact);
            assert_eq!(islands, 1);
            assert_eq!(region.holes.len(), 1);
        }
        // Caso C: in centimetri, le stesse aree in m².
        let cm = DrawingUnit::Centimetri;
        let (faces, sources) = survey(case_a(Form::Continua, cm), cm.tolerance());
        let (_, area, exact, _) =
            compute_at(&faces, &sources, [200.0, 150.0], cm, cm.tolerance()).expect("A in cm");
        assert_m2(area, 12.0, "cm");
        assert!(exact);
        // Fuori da ogni faccia.
        let (faces, sources) = survey(case_a(Form::Continua, M), TOL);
        assert_eq!(compute_at(&faces, &sources, [20.0, 20.0], M, TOL), None);
    }

    /// Il caso A con l'angolo (0; 0) di A aperto di `gap`, sulla faccia ovest
    /// (il lato (0; 3)-(0; 0) si ferma a (0; gap)) o su quella sud (il lato
    /// (0; 0)-(4; 0) parte da (gap; 0)).
    fn open_corner(gap: f64, on_west: bool) -> Vec<EntityType> {
        let mut e = case_a(Form::Continua, M);
        let (side, open) = if on_west {
            (([0.0, 3.0], [0.0, 0.0]), wall_line([0.0, 3.0], [0.0, gap]))
        } else {
            (([0.0, 0.0], [4.0, 0.0]), wall_line([gap, 0.0], [4.0, 0.0]))
        };
        let wall = e
            .iter_mut()
            .find(|entity| is_line(entity, side.0, side.1))
            .expect("lato di A");
        *wall = open;
        e
    }

    // Review Focus 3
    #[test]
    fn corner_gap_needs_tolerance() {
        let tol = 1.0e-6;
        let hpgaptol = 0.005;
        for on_west in [true, false] {
            let (faces, sources) = survey(open_corner(0.002, on_west), tol);
            assert_eq!(compute_at(&faces, &sources, [2.0, 1.5], M, tol), None);
            // Premessa: senza la regola del gruppo staccato il punto cadrebbe
            // nel contorno esterno, meno B: 30,96 − 11,70 = 19,26 m².
            let outer = region_at(&faces, [2.0, 1.5], tol).expect("contorno esterno");
            assert_eq!(outer.holes.len(), 1);
            assert_m2(
                area_m2(net_area(&outer, &sources, tol).0, M),
                19.26,
                "premessa",
            );
        }

        // Con HPGAPTOL 5 mm il vuoto si chiude e il locale misura giusto, ma
        // si chiude solo grazie alla tolleranza: non è esatto (R47).
        let (faces, sources) = survey(open_corner(0.002, true), hpgaptol);
        let (_, area, exact, islands) =
            compute_at(&faces, &sources, [2.0, 1.5], M, hpgaptol).expect("chiuso con HPGAPTOL");
        assert!((area - 12.0).abs() < 1e-3, "{area}");
        assert!(!exact, "chiuso solo dalla tolleranza");
        assert_eq!(islands, 0);

        // Con il vuoto sulla faccia sud la saldatura entro HPGAPTOL fonde i
        // due estremi in uno, e `refined_boundary_ring` (upstream) porta
        // l'angolo sulla curva del lato uscente (antiorario: da sud): può
        // restare a (0,002; 0), e il triangolo (0; 0)-(0,002; 0)-(0; 3) manca
        // (0,003 m² al più). Anche qui: non esatto.
        let (faces, sources) = survey(open_corner(0.002, false), hpgaptol);
        let (_, area, exact, _) =
            compute_at(&faces, &sources, [2.0, 1.5], M, hpgaptol).expect("chiuso con HPGAPTOL");
        assert!((area - 12.0).abs() < 1e-2, "{area}");
        assert!(!exact, "chiuso solo dalla tolleranza");
        assert_eq!(label("A", area, false), "A\\PS = 12,00 m²");
        // Le facce alla tolleranza di base date una volta sola: lo stesso.
        let base_faces = crate::bim::walls::faces(&sources, M.tolerance());
        assert_eq!(
            compute_at_with_base(&faces, &base_faces, &sources, [2.0, 1.5], M, hpgaptol),
            compute_at(&faces, &sources, [2.0, 1.5], M, hpgaptol)
        );

        // Senza vuoti, alla stessa tolleranza larga: esatto, al nanometro
        // quadro.
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let (faces, sources) = survey(case_a(form, M), hpgaptol);
            let (_, area, exact, islands) =
                compute_at(&faces, &sources, [2.0, 1.5], M, hpgaptol).expect("A");
            assert_m2(area, 12.0, form);
            assert!(exact, "{form:?}");
            assert_eq!(islands, 0);
            let mut e = case_a(form, M);
            e.push(pilastro(M));
            let (faces, sources) = survey(e, hpgaptol);
            let (_, area, exact, islands) =
                compute_at(&faces, &sources, [2.0, 2.5], M, hpgaptol).expect("A con pilastro");
            assert_m2(area, 11.91, form);
            assert!(exact, "{form:?}");
            assert_eq!(islands, 1);
        }
    }

    // R47 anche nei locali di progetto: il contorno chiuso solo dalla
    // tolleranza esce come approssimato.
    #[test]
    fn project_room_closed_by_gap_tolerance_is_approximate() {
        let hpgaptol = 0.005;
        let scene = scene_with(open_corner(0.002, false), 6);
        let walls = wall_set(&scene.document, hpgaptol);
        let survey_faces = faces(&walls.sources, hpgaptol);
        let fatto_a = fatto("A", [2.0, 1.5]);
        let (region, ..) =
            compute_at(&survey_faces, &walls.sources, fatto_a.seed, M, hpgaptol).expect("A");
        let pg = project_geometry(&scene.document, &walls, &[], LAYER_MURI, hpgaptol);
        let (pg_sources, _) = sources_of(&pg.room_entities, hpgaptol);
        let pg_faces = faces(&pg_sources, hpgaptol);
        let (rooms, notices) = project_rooms(
            &pg_faces,
            &pg_sources,
            &[fatto_a],
            &[region],
            &[],
            &[],
            M,
            hpgaptol,
        );
        assert_eq!(names(&rooms), ["A"]);
        assert!(!rooms[0].exact);
        assert!((rooms[0].area_m2 - 12.0).abs() < 1e-2);
        assert_eq!(notices, [Notice::ContornoApprossimato { name: "A".into() }]);
    }

    // R48
    #[test]
    fn project_seed_in_the_varco_is_reported() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let zones = vec![axis_zone([4.05, 1.0], [4.05, 1.9], true)];
            let (rooms, notices) = rooms_of(
                case_a(form, M),
                &zones,
                &fatto_ab(),
                &[progetto("Disimpegno", [4.05, 1.45])],
            );
            assert_eq!(names(&rooms), ["A", "B"], "{form:?}");
            assert!(
                notices.contains(&Notice::SemeNelVarco {
                    name: "Disimpegno".into()
                }),
                "{form:?}: {notices:?}"
            );
            assert_eq!(notices.len(), 2, "{form:?}: {notices:?}");
            assert!(notices
                .iter()
                .any(|notice| matches!(notice, Notice::Varco { .. })));
        }
    }

    #[test]
    fn floating_wall_lines_make_the_room_open() {
        // Senza il lato (0; 0)-(4; 0) i tre lati di A galleggiano nel
        // contorno esterno (Task 14, `aggiorna_reports_open_room`).
        let mut e = case_a(Form::Continua, M);
        e.retain(|entity| !is_line(entity, [0.0, 0.0], [4.0, 0.0]));
        let (faces, sources) = survey(e, TOL);
        assert_eq!(compute_at(&faces, &sources, [2.0, 1.5], M, TOL), None);
        let (_, area, _, _) = compute_at(&faces, &sources, [6.0, 1.5], M, TOL).expect("B");
        assert_m2(area, 11.70, "B resta");

        // Un moncone attaccato al contorno (la faccia del tramezzo che
        // sporge dentro A) non conta.
        let mut e = case_a(Form::Continua, M);
        e.push(wall_line([4.0, 1.5], [3.0, 1.5]));
        let (faces, sources) = survey(e, TOL);
        let (_, area, _, islands) =
            compute_at(&faces, &sources, [2.0, 2.5], M, TOL).expect("moncone");
        assert_m2(area, 12.0, "moncone");
        assert_eq!(islands, 0);

        // Il costo della regola: una linea o una polilinea aperta staccate
        // dentro A (un arredo lasciato acceso sul layer dei muri) danno «non
        // chiuso», mai un numero sbagliato.
        for stray in [
            wall_line([1.0, 1.0], [3.0, 1.0]),
            wall_polyline(
                &[([1.0, 1.0], 0.0), ([2.0, 1.0], 0.0), ([2.0, 2.0], 0.0)],
                false,
            ),
        ] {
            let mut e = case_a(Form::Continua, M);
            e.push(stray);
            let (faces, sources) = survey(e, TOL);
            assert_eq!(compute_at(&faces, &sources, [2.0, 2.5], M, TOL), None);
        }
        // Chiusa, la stessa polilinea è un'isola.
        let mut e = case_a(Form::Continua, M);
        e.push(wall_polyline(
            &[
                ([1.0, 1.0], 0.0),
                ([2.0, 1.0], 0.0),
                ([2.0, 2.0], 0.0),
                ([1.0, 2.0], 0.0),
            ],
            true,
        ));
        let (faces, sources) = survey(e, TOL);
        let (_, area, _, islands) =
            compute_at(&faces, &sources, [3.0, 2.5], M, TOL).expect("isola");
        assert_m2(area, 11.0, "isola");
        assert_eq!(islands, 1);
    }

    // -----------------------------------------------------------------------
    // Entità
    // -----------------------------------------------------------------------

    #[test]
    fn room_entities_outline_and_label() {
        let (faces, sources) = survey(case_a(Form::Continua, M), TOL);
        let c = Computed {
            number: "1".into(),
            ..computed(&faces, &sources, "A", [2.0, 1.5])
        };
        let entities = room_entities(&c, Phase::Fatto, &sources, 0.25, TOL);
        assert_eq!(entities.len(), 2);
        let want = Room {
            phase: Phase::Fatto,
            name: "A".into(),
            number: "1".into(),
            seed: [2.0, 1.5],
        };

        let EntityType::LwPolyline(outline) = &entities[0] else {
            panic!("{:?}", entities[0]);
        };
        assert!(outline.is_closed);
        assert_eq!(outline.common.layer, LAYER_LOCALI);
        assert!(outline.common.handle.is_null());
        assert_eq!(outline.vertices.len(), 4);
        for corner in [[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]] {
            assert!(
                outline
                    .vertices
                    .iter()
                    .any(|v| (v.location.x - corner[0]).abs() < 1e-9
                        && (v.location.y - corner[1]).abs() < 1e-9),
                "{corner:?}"
            );
        }
        assert!(outline.vertices.iter().all(|v| v.bulge == 0.0));
        assert_eq!(
            decode(outline.common.extended_data.get_record(APP_LOCALE).unwrap()),
            Some((want.clone(), RoomPart::Contorno))
        );

        let EntityType::MText(label_text) = &entities[1] else {
            panic!("{:?}", entities[1]);
        };
        assert_eq!(label_text.value, "A\\PS = 12,00 m²");
        assert_eq!(label_text.insertion_point, Vector3::new(2.0, 1.5, 0.0));
        assert_eq!(label_text.height, 0.25);
        assert_eq!(label_text.attachment_point, AttachmentPoint::MiddleCenter);
        assert_eq!(label_text.rectangle_width, 0.0, "nessun a capo");
        assert_eq!(label_text.common.layer, LAYER_LOCALI);
        assert!(label_text.common.handle.is_null());
        assert_eq!(
            decode(
                label_text
                    .common
                    .extended_data
                    .get_record(APP_LOCALE)
                    .unwrap()
            ),
            Some((want, RoomPart::Testo))
        );

        // Di progetto, con un nome da confermare.
        let merged = Computed {
            name: "A + B".into(),
            confirm: true,
            ..c
        };
        let entities = room_entities(&merged, Phase::Progetto, &sources, 0.25, TOL);
        let EntityType::MText(label_text) = &entities[1] else {
            panic!();
        };
        assert_eq!(label_text.value, "A + B (da confermare)\\PS = 12,00 m²");
        let (room, part) = decode(
            label_text
                .common
                .extended_data
                .get_record(APP_LOCALE)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            (room.phase, room.name.as_str(), part),
            (Phase::Progetto, "A + B", RoomPart::Testo)
        );
    }

    #[test]
    fn outline_keeps_true_arcs() {
        // Mezzo disco di raggio 1 (corda + arco): π/2 esatti.
        let mut arc = EntityType::Arc(Arc::from_center_radius_angles(Vector3::ZERO, 1.0, 0.0, PI));
        arc.common_mut().layer = LAYER_MURI.to_string();
        let (faces, sources) = survey(vec![wall_line([-1.0, 0.0], [1.0, 0.0]), arc], TOL);
        let c = computed(&faces, &sources, "Abside", [0.0, 0.5]);
        assert!(c.exact);
        assert_m2(c.area_m2, FRAC_PI_2, "mezzo disco");
        let entities = room_entities(&c, Phase::Fatto, &sources, 0.1, TOL);
        let EntityType::LwPolyline(outline) = &entities[0] else {
            panic!();
        };
        assert!(
            outline.vertices.iter().any(|v| v.bulge.abs() > 1e-6),
            "archi veri"
        );
        // Rimisurato, il contorno dà la stessa superficie esatta.
        let (again, off_plane) = sources_of(&[(Handle::new(1), entities[0].clone())], TOL);
        assert_eq!(off_plane, 0);
        let again_faces = crate::bim::walls::faces(&again, TOL);
        let (_, area, exact, _) =
            compute_at(&again_faces, &again, [0.0, 0.5], M, TOL).expect("contorno");
        assert!(exact);
        assert_m2(area, FRAC_PI_2, "contorno rimisurato");
    }

    #[test]
    fn approximate_outline_follows_the_ring() {
        // La «D» (arco e corda nella stessa polilinea): area a corde, non
        // esatta; il contorno è l'anello delle facce, senza archi.
        let d = wall_polyline(&[([1.0, 0.0], 1.0), ([-1.0, 0.0], 0.0)], true);
        let (faces, sources) = survey(vec![d], TOL);
        let c = computed(&faces, &sources, "D", [0.0, 0.5]);
        assert!(!c.exact);
        let entities = room_entities(&c, Phase::Fatto, &sources, 0.1, TOL);
        let EntityType::LwPolyline(outline) = &entities[0] else {
            panic!();
        };
        let ring: Vec<[f64; 2]> = outline
            .vertices
            .iter()
            .map(|v| [v.location.x, v.location.y])
            .collect();
        assert_eq!(ring, c.region.outer);
        assert!(outline.vertices.iter().all(|v| v.bulge == 0.0));
        assert!(outline.is_closed);
        assert_m2(
            signed_area(&ring).abs(),
            c.area_m2,
            "stessa area del numero",
        );
    }

    #[test]
    fn seed_entity_has_only_the_name() {
        let soggiorno = Room {
            number: "3".into(),
            ..progetto("Soggiorno", [3.0, 1.0])
        };
        let EntityType::MText(seed) = seed_entity(&soggiorno, 0.25) else {
            panic!();
        };
        assert_eq!(seed.value, "Soggiorno");
        assert_eq!(seed.common.layer, LAYER_LOCALI_PROGETTO);
        assert_eq!(seed.insertion_point, Vector3::new(3.0, 1.0, 0.0));
        assert_eq!(seed.height, 0.25);
        assert!(seed.common.handle.is_null());
        assert_eq!(
            decode(seed.common.extended_data.get_record(APP_LOCALE).unwrap()),
            Some((soggiorno.clone(), RoomPart::Seme))
        );
        // Nel testo i caratteri di formato hanno la barra, nell'XDATA no.
        let odd = Room {
            name: "WC\\Doccia {2}".into(),
            ..soggiorno
        };
        let EntityType::MText(seed) = seed_entity(&odd, 0.25) else {
            panic!();
        };
        assert_eq!(seed.value, "WC\\\\Doccia \\{2\\}");
        assert_eq!(
            decode(seed.common.extended_data.get_record(APP_LOCALE).unwrap()),
            Some((odd, RoomPart::Seme))
        );
    }

    #[test]
    fn rooms_in_reads_the_model_space_and_survives_dxf_and_dwg() {
        let (faces, sources) = survey(case_a(Form::Continua, M), TOL);
        let a = computed(&faces, &sources, "A", [2.0, 1.5]);
        let mut doc = CadDocument::new();
        ensure_layers(&mut doc);
        let mut handles = Vec::new();
        for entity in room_entities(&a, Phase::Fatto, &sources, 0.25, TOL) {
            handles.push(doc.add_entity(entity).unwrap());
        }
        let soggiorno = Room {
            number: "3".into(),
            ..progetto("Soggiorno", [3.0, 1.0])
        };
        handles.push(doc.add_entity(seed_entity(&soggiorno, 0.25)).unwrap());
        // Un MTEXT senza record e un seme nello spazio carta non contano.
        doc.add_entity(EntityType::MText(Box::new(MText::with_value(
            "A",
            Vector3::ZERO,
        ))))
        .unwrap();
        let mut in_paper = seed_entity(&soggiorno, 0.25);
        assert!(!doc.header.paper_space_block_handle.is_null());
        in_paper.common_mut().owner_handle = doc.header.paper_space_block_handle;
        doc.add_entity(in_paper).unwrap();

        let room_a = fatto("A", [2.0, 1.5]);
        let want = vec![
            (handles[0], room_a.clone(), RoomPart::Contorno),
            (handles[1], room_a, RoomPart::Testo),
            (handles[2], soggiorno, RoomPart::Seme),
        ];
        assert_eq!(rooms_in(&doc), want);

        let strip = |found: Vec<(Handle, Room, RoomPart)>| -> Vec<(Room, RoomPart)> {
            found
                .into_iter()
                .map(|(_, room, part)| (room, part))
                .collect()
        };
        let want = strip(want);
        let bytes = codec::DxfWriter::new(&doc).write_to_vec().unwrap();
        let read = codec::DxfReader::from_reader(Cursor::new(bytes))
            .unwrap()
            .read()
            .unwrap();
        assert_eq!(strip(rooms_in(&read)), want, "DXF (numero vuoto compreso)");
        let bytes = codec::DwgWriter::write_to_vec(&doc).unwrap();
        let read = codec::DwgReader::from_stream(Cursor::new(bytes))
            .read()
            .unwrap();
        assert_eq!(strip(rooms_in(&read)), want, "DWG (numero vuoto compreso)");
    }

    // -----------------------------------------------------------------------
    // Locali di progetto
    // -----------------------------------------------------------------------

    #[test]
    fn case_a_project_merges_into_24() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let (rooms, notices) = rooms_of(case_a(form, M), &total_demolition(), &fatto_ab(), &[]);
            assert_eq!(names(&rooms), ["A + B"], "{form:?}");
            let merged = &rooms[0];
            assert_m2(merged.area_m2, 24.0, form);
            assert!(merged.confirm);
            assert!(merged.exact);
            assert_eq!(merged.islands, 0);
            assert_eq!(merged.seed, [2.0, 1.5], "il primo seme");
            assert_eq!(merged.number, "");
            let [Notice::LocaleFuso { names, area_m2 }] = &notices[..] else {
                panic!("{form:?}: {notices:?}");
            };
            assert_eq!(names, &["A", "B"]);
            assert_m2(*area_m2, 24.0, form);
        }
    }

    #[test]
    fn project_seed_wins_the_name() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let soggiorno = Room {
                number: "4".into(),
                ..progetto("Soggiorno", [3.0, 1.0])
            };
            let (rooms, notices) = rooms_of(
                case_a(form, M),
                &total_demolition(),
                &fatto_ab(),
                &[soggiorno],
            );
            assert_eq!(names(&rooms), ["Soggiorno"], "{form:?}");
            let room = &rooms[0];
            assert!(!room.confirm);
            assert_eq!(room.number, "4");
            assert_eq!(room.seed, [3.0, 1.0]);
            assert_m2(room.area_m2, 24.0, form);
            assert!(notices.is_empty(), "{form:?}: {notices:?}");
        }
    }

    #[test]
    fn two_project_seeds_in_one_room_are_reported() {
        let (rooms, notices) = rooms_of(
            case_a(Form::Continua, M),
            &total_demolition(),
            &fatto_ab(),
            &[
                progetto("Cucina", [2.0, 1.0]),
                progetto("Pranzo", [6.0, 1.0]),
            ],
        );
        assert_eq!(names(&rooms), ["Cucina"]);
        assert!(!rooms[0].confirm);
        let [Notice::LocaleFuso { names, area_m2 }] = &notices[..] else {
            panic!("{notices:?}");
        };
        assert_eq!(names, &["Cucina", "Pranzo"]);
        assert_m2(*area_m2, 24.0, "fusi");
    }

    #[test]
    fn case_b_varco_keeps_two_rooms() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let zones = vec![axis_zone([4.05, 1.0], [4.05, 1.9], true)];
            let (rooms, notices) = rooms_of(case_a(form, M), &zones, &fatto_ab(), &[]);
            assert_eq!(names(&rooms), ["A", "B"], "{form:?}");
            for (name, want) in [("A", 12.0), ("B", 11.70)] {
                let room = named(&rooms, name);
                assert_m2(room.area_m2, want, (form, name));
                assert!(!room.confirm);
                assert!(room.exact);
            }
            let [Notice::Varco { area_m2 }] = &notices[..] else {
                panic!("{form:?}: {notices:?}");
            };
            assert_m2(*area_m2, 0.09, form);
        }
    }

    #[test]
    fn split_room_has_unnamed_half() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let zones = vec![new_zone([2.0, 0.0], [2.0, 3.0], 0.12)];
            let (rooms, notices) =
                rooms_of(case_a(form, M), &zones, &[fatto("A", [1.0, 1.5])], &[]);
            assert_eq!(names(&rooms), ["A", UNNAMED], "{form:?}");
            let a = named(&rooms, "A");
            assert_m2(a.area_m2, 5.82, (form, "A"));
            assert!(!a.confirm);
            let half = named(&rooms, UNNAMED);
            assert_m2(half.area_m2, 5.82, (form, UNNAMED));
            assert!(!half.confirm);
            assert!(half.exact);
            assert_eq!(half.number, "");
            assert!(
                inside_box(half.seed, [2.06, 0.0], [4.0, 3.0]),
                "etichetta nella metà destra: {:?}",
                half.seed
            );
            let [Notice::LocaleSenzaNome { area_m2 }] = &notices[..] else {
                panic!("{form:?}: {notices:?}");
            };
            assert_m2(*area_m2, 5.82, form);
        }
    }

    #[test]
    fn seed_in_new_wall_is_reported() {
        let zones = vec![new_zone([2.0, 0.0], [2.0, 3.0], 0.12)];
        let (rooms, notices) = rooms_of(
            case_a(Form::Continua, M),
            &zones,
            &[fatto("A", [1.0, 1.5])],
            &[progetto("X", [2.0, 1.5])],
        );
        assert_eq!(
            names(&rooms),
            ["A", UNNAMED],
            "X non dà il nome a nessun locale"
        );
        assert!(
            notices.contains(&Notice::SemeInMuroNuovo { name: "X".into() }),
            "{notices:?}"
        );
        assert_eq!(notices.len(), 2, "{notices:?}");
        assert!(notices
            .iter()
            .any(|notice| matches!(notice, Notice::LocaleSenzaNome { .. })));

        // Un seme di fatto dentro il muro nuovo non nomina niente: le due metà
        // escono senza nome, ognuna con il suo avviso.
        let (rooms, notices) = rooms_of(
            case_a(Form::Continua, M),
            &zones,
            &[fatto("A", [2.0, 1.5])],
            &[],
        );
        assert_eq!(names(&rooms), [UNNAMED, UNNAMED]);
        for room in &rooms {
            assert_m2(room.area_m2, 5.82, "metà");
        }
        assert_eq!(
            notices
                .iter()
                .filter(|notice| matches!(notice, Notice::LocaleSenzaNome { .. }))
                .count(),
            2,
            "{notices:?}"
        );
        assert_eq!(notices.len(), 2, "{notices:?}");
    }

    #[test]
    fn island_in_a_project_room() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let mut e = case_a(form, M);
            e.push(pilastro(M));
            let (rooms, notices) = rooms_of(
                e,
                &[],
                &[fatto("A", [2.0, 2.5]), fatto("B", [6.0, 1.5])],
                &[],
            );
            assert_eq!(
                names(&rooms),
                ["A", "B"],
                "il pilastro non è un locale: {form:?}"
            );
            let a = named(&rooms, "A");
            assert_m2(a.area_m2, 11.91, form);
            assert_eq!(a.islands, 1);
            assert_eq!(
                notices,
                [Notice::Isole {
                    name: "A".into(),
                    count: 1
                }],
                "{form:?}"
            );
        }
    }

    #[test]
    fn outer_wall_ring_is_not_an_unnamed_room() {
        // Premessa (R7): sull'anello nudo del contorno esterno il baricentro
        // del triangolo più grande cade dentro A o B.
        let (faces, _) = survey(case_a(Form::Continua, M), TOL);
        let outer = region_at(&faces, [-0.15, 1.5], TOL).expect("muri");
        assert_eq!(outer.holes.len(), 2);
        let (points, triangles) = triangulate(&outer.outer, &[]);
        let corners = |t: &[usize; 3]| t.map(|index| points[index]);
        let biggest = triangles
            .iter()
            .map(corners)
            .max_by(|a, b| signed_area(a).abs().total_cmp(&signed_area(b).abs()))
            .unwrap();
        let centre = [
            (biggest[0][0] + biggest[1][0] + biggest[2][0]) / 3.0,
            (biggest[0][1] + biggest[1][1] + biggest[2][1]) / 3.0,
        ];
        assert!(in_a_or_b(centre), "premessa: {centre:?}");
        // Sulla faccia come regione (meno i figli) il punto sta nel muro.
        let inner = interior_point(&outer, TOL).expect("punto interno");
        assert!(!in_a_or_b(inner), "{inner:?}");
        assert!(inside_box(inner, [-0.3, -0.3], [8.3, 3.3]));

        // Nessun «(senza nome)» dal muro d'ambito, con la demolizione totale e
        // con il varco (buchi che si toccano).
        for zones in [
            total_demolition(),
            vec![axis_zone([4.05, 1.0], [4.05, 1.9], true)],
        ] {
            let (rooms, notices) = rooms_of(case_a(Form::Continua, M), &zones, &fatto_ab(), &[]);
            assert!(!names(&rooms).contains(&UNNAMED), "{:?}", names(&rooms));
            assert!(
                !notices
                    .iter()
                    .any(|notice| matches!(notice, Notice::LocaleSenzaNome { .. })),
                "{notices:?}"
            );
        }
    }

    /// Nessuna faccia esce dai controlli in silenzio perché manca il punto
    /// interno: ogni faccia di progetto dei casi di prova ne ha uno, nella sua
    /// regione e lontano dagli anelli.
    #[test]
    fn every_project_face_has_an_interior_point() {
        let mut with_pillar = case_a(Form::ChiusaPerMuro, M);
        with_pillar.push(pilastro(M));
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            for zones in [
                Vec::new(),
                total_demolition(),
                vec![axis_zone([4.05, 1.0], [4.05, 1.9], true)],
                vec![new_zone([2.0, 0.0], [2.0, 3.0], 0.12)],
            ] {
                for entities in [case_a(form, M), with_pillar.clone()] {
                    let scene = scene_with(entities, 6);
                    let walls = wall_set(&scene.document, TOL);
                    let pg = project_geometry(&scene.document, &walls, &zones, LAYER_MURI, TOL);
                    let (pg_sources, _) = sources_of(&pg.room_entities, TOL);
                    let pg_faces = faces(&pg_sources, TOL);
                    for region in crate::bim::walls::face_regions(&pg_faces, TOL) {
                        let p = interior_point(&region, TOL)
                            .unwrap_or_else(|| panic!("{form:?}: nessun punto in {region:?}"));
                        assert_eq!(
                            region_at(&pg_faces, p, TOL).as_ref(),
                            Some(&region),
                            "{form:?}: {p:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn seed_outside_every_room_is_reported() {
        let (rooms, notices) = rooms_of(
            case_a(Form::Continua, M),
            &[],
            &fatto_ab(),
            &[progetto("Esterno", [20.0, 20.0])],
        );
        assert_eq!(names(&rooms), ["A", "B"]);
        assert_eq!(
            notices,
            [Notice::LocaleNonChiuso {
                name: "Esterno".into()
            }]
        );
    }
}
