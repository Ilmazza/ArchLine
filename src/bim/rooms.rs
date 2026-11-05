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
//! (anche vuoto), `"punto"`, `Position3D(seme)`, `"origine"`, `"punto-interno"`,
//! `"ancora"`, `Point3D(ancora)`. La lettura va per chiave e salta ciò che non
//! conosce.
//!
//! **Identità, punto di calcolo, posizione dell'etichetta.** Il seme è una
//! posizione (1011) perché il codec la porta dietro all'entità in MOVE, COPY,
//! ROTATE, SCALE e MIRROR: con `Point3D` (1010) una copia traslata teneva il
//! seme dell'originale e `same_room` la prendeva per lo stesso locale. Ma il
//! contorno e il testo di un locale di fatto portano ognuno il proprio seme, e
//! spostando il solo testo (o il solo contorno) i due semi si separano. Per
//! questo c'è l'**ancora**: il punto in cui il locale è nato, scritto come
//! 1010 e quindi fermo qualunque cosa si trasformi. [`rooms_in`] riabbina un
//! testo staccato al suo contorno solo dove l'abbinamento è certo, famiglia
//! per famiglia (stessa ancora): un contorno e un testo soli sono la coppia
//! anche se un pezzo è stato spostato, e il testo prende il seme del contorno;
//! con più contorni o più testi (c'è stata una copia) ogni testo deve avere
//! esattamente un contorno dallo stesso seme, altrimenti tutta la famiglia è
//! ambigua ([`ambiguous_parts`]): `AL_ALTEZZA` la rifiuta e le misure la
//! segnalano con `Notice::LocaleAmbiguo`. Un record copiato è identico
//! all'originale, quindi nel disegno non c'è un dato che separi due etichette
//! finite sullo stesso seme, né che dica se un'etichetta rimasta senza
//! contorno per esclusione sia la sua: la scelta silenziosa sarebbe una
//! scommessa. Tutti gli altri (`same_room`, `AL_AGGIORNA`, abaco) leggono da
//! `rooms_in`. I record senza ancora (scritti prima) la prendono dal seme, e
//! all'apertura del disegno [`super::migrate`] li porta al formato corrente. Le entità nascono con
//! handle `NULL`: chi le inserisce chiama prima `zone::ensure_layers`, che crea
//! i layer e l'APPID.
//!
//! **Calcolo** ([`measure_at`], [`compute_at`]). Il locale attorno al seme è
//! la faccia più piccola che lo contiene, meno le isole (`walls::region_at`,
//! `walls::net_area`: archi veri dove si può, mai un'area sbagliata dichiarata
//! esatta). In più vale la **regola dei muri dentro** (R50): i segmenti dei
//! muri strettamente dentro la regione (punto medio nella regione, a più di
//! `tol` dal contorno e da ogni buco: la regione da tutte e due le parti),
//! uniti quando si toccano, non devono formare un gruppo lungo più di
//! [`OPEN_WALLS_M`] (0,50 m). Se lo formano, il locale vero è aperto e le sue
//! linee stanno dentro una faccia più grande: un vuoto d'angolo di 2 mm lascia
//! i lati del locale nel contorno esterno dell'edificio, anche quando toccano un
//! locale vicino chiuso (spallette e linea di soglia di un vano). Allora il
//! risultato è `None`, «non chiuso». I gruppi più corti (sporgenze oltre un
//! angolo, monconi, battute) non contano. Prima della regola si tolgono dagli
//! anelli le punte: un moncone attaccato al contorno entra nell'anello della
//! faccia come andata e ritorno, ma ha la regione da tutte e due le parti, e la
//! regola lo deve vedere; l'area non cambia e il contorno disegnato non lo
//! segue. Il costo, dichiarato: un arredo aperto
//! (una linea, una polilinea non chiusa) lasciato acceso sul layer dei muri, o
//! una parete a una linea sola che entra nel locale per più di 0,50 m, lo
//! rende «non chiuso». È un messaggio, mai un numero sbagliato; si spegne quel
//! layer, come in AutoCAD (spec §5.0). Un vano porta senza soglia non rende il
//! locale «non chiuso» da sé: se di là c'è un altro locale chiuso, i due escono
//! come una faccia sola, e il contorno lo mostra; la linea di soglia li separa
//! (spec §5.2).
//!
//! Infine (R47) un locale chiuso solo grazie a una tolleranza larga
//! (`HPGAPTOL` sopra la tolleranza di base) non è mai esatto: se alla
//! tolleranza di base non è chiuso, o misura un'altra area, il numero resta
//! quello alla tolleranza larga con [`Measured::by_tolerance`] ed
//! `exact = false`; l'avviso è [`Notice::ChiusoPerTolleranza`]
//! ([`quality_notices`]).
//!
//! **Locali di progetto** ([`project_rooms`], spec §5.4). Tutto si calcola
//! sulla geometria di progetto (`project::ProjectGeometry::room_entities`, con
//! le soglie dei varchi):
//!
//! 1. Per ogni seme, prima quelli di fatto e poi quelli di progetto, nel loro
//!    ordine e senza doppioni (stesso nome, seme entro `tol`: il contorno e il
//!    testo dello stesso locale contano una volta): un seme di progetto dentro
//!    una zona `nuovo` dà [`Notice::SemeInMuroNuovo`] e non nomina nessun
//!    locale; un seme senza locale chiuso attorno dà
//!    [`Notice::LocaleNonChiuso`]; un seme la cui faccia sta tutta dentro una
//!    zona non nomina niente (quelle facce non sono mai locali): nella faccia di
//!    un varco un seme di progetto dà [`Notice::SemeNelVarco`], in una zona
//!    demolita (anche un varco, per un seme di fatto) [`Notice::SemeInDemolizione`];
//!    un seme di fatto dentro un muro nuovo non ha un avviso suo, ma le parti
//!    del locale escono «(senza nome)», ognuna con il suo. Gli altri si
//!    raggruppano per faccia.
//! 2. Un gruppo con almeno un seme di progetto prende il nome e il numero del
//!    primo, senza «da confermare»; più semi di progetto nella stessa faccia
//!    danno anche un [`Notice::LocaleFuso`] con i loro nomi (scelta oltre il
//!    piano: un nome di Mauro non sparisce in silenzio). Un gruppo di soli semi
//!    di fatto prende i nomi uniti da « + », nell'ordine dei semi, e i numeri
//!    non vuoti uniti allo stesso modo; con più di un seme è «da confermare» e
//!    dà un [`Notice::LocaleFuso`]. L'etichetta sta sul primo seme del gruppo
//!    che dà il nome.
//! 3. Ogni faccia che non è un locale né sta dentro un'isola di un locale (il
//!    buco stesso o ciò che contiene: un pilastro a doppio contorno, un
//!    cavedio, una vasca a due linee): se sta tutta dentro una zona non è un
//!    locale, e dentro un varco dà [`Notice::Varco`] con la sua area, che non
//!    appartiene a nessun locale. Altrimenti, se il suo punto interno cade
//!    dentro il contorno esterno di una regione di fatto, è un locale «(senza
//!    nome)» con [`Notice::LocaleSenzaNome`] (ciò che sta dentro un'isola di un
//!    locale senza nome è dell'isola, non un locale); se nella faccia restano
//!    muri (R50), [`Notice::LocaleNonChiuso`] con il nome «(senza nome)». Il punto
//!    interno si prende sulla faccia come regione, cioè meno i suoi figli
//!    diretti (`walls::face_regions`, la regola di `region_at`): il
//!    baricentro del triangolo più grande di `kernel::geom2d::triangulate`, o
//!    del primo, in ordine di area, che cada davvero dentro (a più di `tol`
//!    dagli anelli); se `triangulate` non ne dà, quelli di
//!    `triangulate_rings`. Sull'anello nudo del contorno esterno dell'edificio
//!    il punto cadrebbe dentro un locale.
//! 4. Per ogni locale ([`quality_notices`]): isole → [`Notice::Isole`];
//!    chiuso solo dalla tolleranza → [`Notice::ChiusoPerTolleranza`];
//!    altrimenti area non esatta → [`Notice::ContornoApprossimato`].

use codec::entities::{AttachmentPoint, EntityCommon, LwPolyline, LwVertex, MText};
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{Document, EntityType, Handle, Vector2, Vector3};
use kernel::geom2d::{
    contains, distance_to, segment_crossing, signed_area, triangulate, triangulate_rings, Curve,
    Line, SegmentCrossing, Tolerance,
};
use rustc_hash::FxHashMap;

use crate::command::WorkingPlane;
use crate::scene::{boundary_polyline_entities, BoundarySource};

use super::report::Notice;
use crate::scene::view::dispatch::set_entity_xdata;
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
const KEY_ANCHOR: &str = "ancora";
const KEY_ORIGIN: &str = "origine";
const KEY_HEIGHT: &str = "altezza";
/// L'unica origine di un locale nel primo giro.
const ORIGIN_POINT: &str = "punto-interno";

/// Il nome di un locale di progetto senza seme.
pub const UNNAMED: &str = "(senza nome)";
/// L'unione meccanica dei nomi (e dei numeri) di fatto.
const JOIN: &str = " + ";
/// Dopo il nome di un locale fuso.
const TO_CONFIRM: &str = " (da confermare)";

/// L'altezza dei testi dei locali, in metri, quando il disegno non ne dà una
/// plausibile (`TEXTSIZE` non positiva o oltre [`MAX_ROOM_TEXT_M`]): un
/// parametro di aspetto, non una misura.
pub const DEFAULT_ROOM_TEXT_M: f64 = 0.25;

/// Oltre questa altezza, in metri, `TEXTSIZE` non è un testo di pianta: è un
/// disegno in metri nato da un modello in millimetri (acadiso ha 2,5, cioè
/// etichette alte 2,5 m).
pub const MAX_ROOM_TEXT_M: f64 = 1.0;

/// Perché i testi dei locali hanno l'altezza di ripiego.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFallback {
    /// `TEXTSIZE` non positiva (o non un numero).
    NotPositive,
    /// `TEXTSIZE` oltre [`MAX_ROOM_TEXT_M`].
    Implausible,
}

/// L'altezza dei testi dei locali (`AL_LOCALE`, estrazione): `TEXTSIZE` del
/// disegno se positiva e non oltre [`MAX_ROOM_TEXT_M`], altrimenti
/// [`DEFAULT_ROOM_TEXT_M`] in unità del disegno, con il motivo (fix finale,
/// punto 6, per `Implausible`).
pub fn room_text_height(doc: &Document, unit: DrawingUnit) -> (f64, Option<TextFallback>) {
    let height = doc.header.text_height;
    let fallback = if !(height.is_finite() && height > 0.0) {
        Some(TextFallback::NotPositive)
    } else if height > unit.metres(MAX_ROOM_TEXT_M) {
        Some(TextFallback::Implausible)
    } else {
        None
    };
    match fallback {
        None => (height, None),
        Some(_) => (unit.metres(DEFAULT_ROOM_TEXT_M), fallback),
    }
}

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
    /// Altezza propria del locale, in unità del disegno; `None` = nessuna
    /// (vale quella di default del progetto, parte 2).
    pub height: Option<f64>,
}

fn key(text: &str) -> XDataValue {
    XDataValue::String(text.to_string())
}

/// La sequenza dei valori del record `ARCHLINE_LOCALE`.
pub fn encode(room: &Room, part: RoomPart) -> Vec<XDataValue> {
    let mut values = vec![
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
        XDataValue::Position3D(Vector3::new(room.seed[0], room.seed[1], 0.0)),
        key(KEY_ORIGIN),
        key(ORIGIN_POINT),
        key(KEY_ANCHOR),
        XDataValue::Point3D(Vector3::new(room.seed[0], room.seed[1], 0.0)),
    ];
    if let Some(height) = room.height {
        values.push(key(KEY_HEIGHT));
        values.push(XDataValue::Real(height));
    }
    values
}

/// Legge il record per chiave, saltando ciò che non conosce. `None` senza una
/// fase, una parte e un punto leggibili; nome e numero assenti valgono vuoti.
pub fn decode(rec: &ExtendedDataRecord) -> Option<(Room, RoomPart)> {
    let (room, part, _) = decode_with_anchor(rec)?;
    Some((room, part))
}

/// Come [`decode`], più l'ancora del record: il punto in cui il locale è nato,
/// scritto come `Point3D` (1010) perché non si trasforma. I record senza
/// ancora (scritti prima) hanno per ancora il seme.
fn decode_with_anchor(rec: &ExtendedDataRecord) -> Option<(Room, RoomPart, [f64; 2])> {
    let mut anchor = None;
    let (mut phase, mut part, mut seed) = (None, None, None);
    let (mut name, mut number) = (String::new(), String::new());
    let mut height = None;
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
            (KEY_POINT, Some(XDataValue::Position3D(p) | XDataValue::Point3D(p))) => {
                seed = Some([p.x, p.y]);
                2
            }
            (KEY_ANCHOR, Some(XDataValue::Point3D(p) | XDataValue::Position3D(p))) => {
                anchor = Some([p.x, p.y]);
                2
            }
            (KEY_HEIGHT, Some(XDataValue::Real(h))) => {
                height = Some(*h).filter(|h| h.is_finite() && *h > 0.0);
                2
            }
            _ => 1,
        };
        i += used;
    }
    let seed = seed?;
    Some((
        Room {
            phase: phase?,
            name,
            number,
            seed,
            height,
        },
        part?,
        anchor.unwrap_or(seed),
    ))
}

/// Il record portato al formato corrente, o `None` se lo è già (o non si
/// legge): il seme da `Point3D` a `Position3D` e l'ancora, se manca, uguale al
/// seme. Il resto dei valori resta com'è, chiavi sconosciute comprese.
pub fn migrated_values(rec: &ExtendedDataRecord) -> Option<Vec<XDataValue>> {
    let (_, _, anchor) = decode_with_anchor(rec)?;
    let mut values = rec.values.clone();
    let mut changed = false;
    let mut has_anchor = false;
    for i in 0..values.len() {
        match (&values[i], values.get(i + 1)) {
            (XDataValue::String(field), Some(XDataValue::Point3D(p))) if field == KEY_POINT => {
                values[i + 1] = XDataValue::Position3D(*p);
                changed = true;
            }
            (XDataValue::String(field), _) if field == KEY_ANCHOR => has_anchor = true,
            _ => {}
        }
    }
    if !has_anchor {
        values.push(key(KEY_ANCHOR));
        values.push(XDataValue::Point3D(Vector3::new(anchor[0], anchor[1], 0.0)));
        changed = true;
    }
    changed.then_some(values)
}

/// Le entità di locale dello spazio modello (non quelle in carta o dentro i
/// blocchi), in ordine di disegno. Il testo di un locale di fatto staccato dal
/// suo contorno (uno dei due spostato) torna con il seme del contorno, ma solo
/// dove l'abbinamento è certo ([`pair_fatto`]); gli ambigui restano con il
/// loro seme ([`ambiguous_parts`]).
pub fn rooms_in(doc: &Document) -> Vec<(Handle, Room, RoomPart)> {
    resolved_parts(doc)
        .into_iter()
        .map(|(handle, room, part, _)| (handle, room, part))
        .collect()
}

/// Le parti di locale con i semi dei testi staccati già riportati a quelli dei
/// loro contorni ([`pair_fatto`]).
fn resolved_parts(doc: &Document) -> Vec<Part> {
    let mut found = collect_parts(doc);
    for (index, seed) in pair_fatto(&found).repaired {
        found[index].1.seed = seed;
    }
    found
}

/// Una parte di locale come sta nel disegno: l'entità, il suo record e
/// l'ancora.
type Part = (Handle, Room, RoomPart, [f64; 2]);

fn collect_parts(doc: &Document) -> Vec<Part> {
    doc.block_records
        .iter()
        .filter(|record| record.is_model_space())
        .flat_map(|record| record.entity_handles.iter())
        .filter_map(|&handle| {
            let entity = doc.get_entity(handle)?;
            let (room, part, anchor) =
                decode_with_anchor(entity.common().extended_data.get_record(APP_LOCALE)?)?;
            Some((handle, room, part, anchor))
        })
        .collect()
}

/// Le parti dei locali di fatto il cui abbinamento contorno/etichetta non è
/// certo, con il nome del locale, in ordine di disegno. Succede quando due
/// etichette (o due contorni) hanno lo stesso seme, per esempio dopo la copia
/// di un locale e lo spostamento di un'etichetta sul seme dell'altra, oppure
/// quando più etichette staccate dal loro contorno potrebbero appartenere a
/// più contorni. Chi modifica il disegno (`AL_ALTEZZA`) le rifiuta; chi misura
/// ([`super::refresh::measure_fatto`]) lo dice con [`super::report::Notice::LocaleAmbiguo`].
pub fn ambiguous_parts(doc: &Document) -> Vec<(Handle, String)> {
    let found = collect_parts(doc);
    pair_fatto(&found)
        .ambiguous
        .into_iter()
        .map(|index| (found[index].0, found[index].1.name.clone()))
        .collect()
}

/// Distanza sotto cui due semi (o due ancore) sono lo stesso punto: copie
/// dello stesso valore passate dalla stessa trasformazione.
const SAME_POINT_EPS: f64 = 1.0e-6;

fn same_point(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) <= SAME_POINT_EPS
}

/// L'esito dell'abbinamento dei locali di fatto.
struct Pairing {
    /// I testi che prendono il seme del loro contorno: (indice, seme).
    repaired: Vec<(usize, [f64; 2])>,
    /// Le parti (indici, in ordine di disegno) il cui abbinamento non è certo.
    ambiguous: Vec<usize>,
}

/// Abbina i testi ai contorni dei locali di fatto, solo dove è certo.
///
/// Si lavora per **famiglia**: le parti con la stessa ancora. Una famiglia
/// nasce con `AL_LOCALE` e ogni copia fatta da ArchLine ne apre una nuova
/// ([`renew_anchors`]), quindi una famiglia di un contorno e un testo è un
/// locale, e l'etichetta gli appartiene dovunque la si sposti.
///
/// * Un contorno e un testo soli: sono la coppia, anche se un pezzo è stato
///   spostato; il testo prende il seme del contorno.
/// * Più contorni o più testi insieme a almeno uno dell'altro tipo: ambigua.
///   Sono copie fatte fuori da ArchLine (o da versioni precedenti), con record
///   identici all'originale. Neanche semi che coincidono 1:1 provano
///   l'appartenenza: due etichette con lo stesso nome possono essere state
///   scambiate, o una cancellata e l'altra portata sul suo posto, senza che il
///   disegno lo dica. Si ripara cancellando le parti sbagliate e rifacendole
///   con `AL_LOCALE`.
/// * Solo contorni o solo testi: niente da abbinare (lo dicono gli avvisi di
///   contorno o testo mancante).
fn pair_fatto(found: &[Part]) -> Pairing {
    let mut families: Vec<([f64; 2], Vec<usize>)> = Vec::new();
    for (index, part) in found.iter().enumerate() {
        if part.1.phase != Phase::Fatto || part.2 == RoomPart::Seme {
            continue;
        }
        match families.iter_mut().find(|(anchor, _)| same_point(*anchor, part.3)) {
            Some((_, members)) => members.push(index),
            None => families.push((part.3, vec![index])),
        }
    }
    let mut repaired = Vec::new();
    let mut ambiguous = Vec::new();
    for (_, members) in families {
        let outlines: Vec<usize> =
            members.iter().copied().filter(|&i| found[i].2 == RoomPart::Contorno).collect();
        let texts: Vec<usize> =
            members.iter().copied().filter(|&i| found[i].2 == RoomPart::Testo).collect();
        match (outlines.as_slice(), texts.as_slice()) {
            ([], _) | (_, []) => {}
            ([o], [t]) => {
                if !same_point(found[*o].1.seed, found[*t].1.seed) {
                    repaired.push((*t, found[*o].1.seed));
                }
            }
            _ => ambiguous.extend(members),
        }
    }
    ambiguous.sort_unstable();
    Pairing { repaired, ambiguous }
}

/// Dà alle parti di locale appena copiate (`copies`) una famiglia nuova, la
/// loro, distinta da quella dell'originale e da quella di ogni altra copia.
/// Lo chiamano `Scene::copy_entities` e l'incolla, cioè COPY, MIRROR con copia,
/// ARRAY e incolla. Nel gruppo copiato, per ogni famiglia d'origine: un
/// contorno e un testo soli restano insieme (ancora = seme del contorno, anche
/// se il testo era staccato); con più parti, ogni contorno apre la sua famiglia
/// (ancora = suo seme) e un testo entra in quella del contorno che ha il suo
/// stesso seme, altrimenti ne apre una sua. Una copia del solo testo è un
/// testo senza contorno, e viceversa: copie parziali, mai agganciate
/// all'originale. Il resto del record non cambia.
pub fn renew_anchors(doc: &mut Document, copies: &[Handle]) {
    struct Copy {
        handle: Handle,
        part: RoomPart,
        old_anchor: [f64; 2],
        seed: [f64; 2],
        values: Vec<XDataValue>,
        anchor: [f64; 2],
    }
    let mut parts: Vec<Copy> = copies
        .iter()
        .filter(|handle| !handle.is_null())
        .filter_map(|&handle| {
            let record = doc.get_entity(handle)?.common().extended_data.get_record(APP_LOCALE)?;
            let (room, part, old_anchor) = decode_with_anchor(record)?;
            Some(Copy {
                handle,
                part,
                old_anchor,
                seed: room.seed,
                values: record.values.clone(),
                anchor: room.seed,
            })
        })
        .collect();
    for i in 0..parts.len() {
        if parts[i].part != RoomPart::Testo {
            continue;
        }
        let family = |c: &Copy, part: RoomPart| c.part == part && same_point(c.old_anchor, parts[i].old_anchor);
        let outlines: Vec<usize> =
            (0..parts.len()).filter(|&o| family(&parts[o], RoomPart::Contorno)).collect();
        let texts = (0..parts.len()).filter(|&t| family(&parts[t], RoomPart::Testo)).count();
        let owner = match outlines.as_slice() {
            [o] if texts == 1 => Some(*o),
            many => many.iter().copied().find(|&o| same_point(parts[o].seed, parts[i].seed)),
        };
        if let Some(o) = owner {
            parts[i].anchor = parts[o].seed;
        }
    }
    for copy in parts {
        let mut values = copy.values;
        let anchor = XDataValue::Point3D(Vector3::new(copy.anchor[0], copy.anchor[1], 0.0));
        match values
            .iter()
            .position(|value| matches!(value, XDataValue::String(field) if field == KEY_ANCHOR))
        {
            Some(at) if at + 1 < values.len() => values[at + 1] = anchor,
            _ => {
                values.push(key(KEY_ANCHOR));
                values.push(anchor);
            }
        }
        set_entity_xdata(doc, copy.handle, APP_LOCALE, Some(values));
    }
}

/// Lo stesso locale: stessa fase e seme entro `tol`. Il contorno e il testo
/// di un locale di fatto portano lo stesso record, quindi lo stesso seme.
pub fn same_room(a: &Room, b: &Room, tol: f64) -> bool {
    a.phase == b.phase && (a.seed[0] - b.seed[0]).hypot(a.seed[1] - b.seed[1]) <= tol
}

/// Le parti di locale che `AL_ALTEZZA` riscrive (spec parte 2, §5.1): per
/// ogni parte di locale tra `selected`, tutte le parti dello stesso locale
/// ([`same_room`]), ognuna col suo `Room` e l'altezza `height` (in unità del
/// disegno; `None` la toglie). Così basta selezionare il contorno o il testo
/// di un locale di fatto perché cambino tutti e due. Senza doppioni, in ordine
/// di disegno; gli handle che non sono parti di locale non contano. Restano
/// anche le parti che hanno già quell'altezza: scartarle è di chi scrive.
pub fn room_height_edits(
    doc: &Document,
    selected: &[Handle],
    height: Option<f64>,
    _tol: f64,
) -> Vec<(Handle, Room, RoomPart)> {
    room_height_groups(doc, selected, height)
        .into_iter()
        .flatten()
        .collect()
}

/// Come [`room_height_edits`], un gruppo per locale: la **famiglia** (stessa
/// ancora) delle parti selezionate, non le parti che hanno lo stesso seme. Il
/// seme non è l'identità: una copia riportata sul seme dell'originale lo ha
/// uguale, e non per questo ne fa parte. Il seme di un locale di progetto è il
/// locale. I gruppi, e le parti in ognuno, sono in ordine di disegno.
pub fn room_height_groups(
    doc: &Document,
    selected: &[Handle],
    height: Option<f64>,
) -> Vec<Vec<(Handle, Room, RoomPart)>> {
    #[derive(PartialEq)]
    enum Family {
        Anchor([f64; 2]),
        Alone(usize),
    }
    let all = resolved_parts(doc);
    let family = |index: usize| match &all[index] {
        (_, room, RoomPart::Contorno | RoomPart::Testo, anchor) if room.phase == Phase::Fatto => {
            Family::Anchor(*anchor)
        }
        _ => Family::Alone(index),
    };
    let same = |a: &Family, b: &Family| match (a, b) {
        (Family::Anchor(x), Family::Anchor(y)) => same_point(*x, *y),
        (Family::Alone(x), Family::Alone(y)) => x == y,
        _ => false,
    };
    let picked: Vec<Family> = (0..all.len())
        .filter(|&index| selected.contains(&all[index].0))
        .map(family)
        .collect();
    let mut grouped = vec![false; all.len()];
    let mut groups = Vec::new();
    for first in 0..all.len() {
        if grouped[first] || !picked.iter().any(|seen| same(seen, &family(first))) {
            continue;
        }
        let this = family(first);
        let members: Vec<usize> = (first..all.len())
            .filter(|&index| !grouped[index] && same(&this, &family(index)))
            .collect();
        let mut group = Vec::with_capacity(members.len());
        for index in members {
            grouped[index] = true;
            let (handle, room, part, _) = &all[index];
            group.push((
                *handle,
                Room {
                    height,
                    ..room.clone()
                },
                *part,
            ));
        }
        groups.push(group);
    }
    groups
}

/// Il record `rec` con la sola chiave `altezza` cambiata (`None` la toglie):
/// tutto il resto, ancora compresa, resta com'è. `AL_ALTEZZA` riscrive così,
/// mai con [`encode`], che rifarebbe l'ancora dal seme e unirebbe di nuovo una
/// copia all'originale.
pub fn with_height(rec: &ExtendedDataRecord, height: Option<f64>) -> Vec<XDataValue> {
    let mut values = Vec::with_capacity(rec.values.len() + 2);
    let mut i = 0;
    while i < rec.values.len() {
        let is_height = matches!(&rec.values[i], XDataValue::String(field) if field == KEY_HEIGHT)
            && matches!(rec.values.get(i + 1), Some(XDataValue::Real(_)));
        if is_height {
            i += 2;
            continue;
        }
        values.push(rec.values[i].clone());
        i += 1;
    }
    if let Some(height) = height {
        values.push(key(KEY_HEIGHT));
        values.push(XDataValue::Real(height));
    }
    values
}

/// Il testo di un locale: `A\PS = 12,00 m²` (`\P` è l'a capo dell'MTEXT),
/// con ` (da confermare)` dopo il nome quando `confirm`. Nel nome barre e
/// graffe hanno la barra davanti, così restano testo.
pub fn label(name: &str, area_m2: f64, confirm: bool) -> String {
    let mark = if confirm { TO_CONFIRM } else { "" };
    format!("{}{mark}\\PS = {}", mtext_escape(name), format_m2(area_m2))
}

/// `\`, `{` e `}` sono codici di formato dell'MTEXT.
pub(super) fn mtext_escape(text: &str) -> String {
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
/// esatta e quante isole sono state sottratte. È [`measure_at`] senza le facce
/// alla tolleranza di base (calcolate qui se servono) e senza
/// [`Measured::by_tolerance`]; chi deve dare gli avvisi giusti (o misura molti
/// locali con `HPGAPTOL`) usa [`measure_at`].
pub fn compute_at(
    faces: &[Vec<[f64; 2]>],
    sources: &FxHashMap<Handle, BoundarySource>,
    seed: [f64; 2],
    unit: DrawingUnit,
    tol: f64,
) -> Option<(Region, f64, bool, usize)> {
    measure_at(faces, None, sources, seed, unit, tol)
        .map(|m| (m.region, m.area_m2, m.exact, m.islands))
}

/// Una misura attorno a un seme ([`measure_at`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    pub region: Region,
    pub area_m2: f64,
    /// Area dalle curve vere dei muri (`walls::net_area`), e locale chiuso
    /// anche alla tolleranza di base.
    pub exact: bool,
    pub islands: usize,
    /// Chiuso solo dalla tolleranza larga (R47): alla tolleranza di base il
    /// locale non è chiuso o misura un'altra area. Allora `exact` è `false` e
    /// l'avviso è [`Notice::ChiusoPerTolleranza`] (vedi [`quality_notices`]).
    pub by_tolerance: bool,
}

/// Il locale attorno a `seed`. `None` se nessuna faccia contiene il punto o se
/// nella regione restano muri staccati (R50, documentazione del modulo).
/// `faces` vengono da `walls::faces(sources, tol)`, con la stessa `tol` in
/// unità del disegno.
///
/// Con una tolleranza larga (`HPGAPTOL` sopra `unit.tolerance()`) la misura si
/// rifà sulle facce alla tolleranza di base: se lì il locale non è chiuso o
/// l'area cambia più di [`GAP_SLACK`] tolleranze di base per il perimetro, il
/// numero resta quello alla tolleranza larga, con `by_tolerance = true` ed
/// `exact = false`. `base_faces` sono `walls::faces(sources,
/// unit.tolerance())`, se il chiamante le ha già (si calcolano una volta per
/// molti locali); con `None` si calcolano qui quando servono.
pub fn measure_at(
    faces: &[Vec<[f64; 2]>],
    base_faces: Option<&[Vec<[f64; 2]>]>,
    sources: &FxHashMap<Handle, BoundarySource>,
    seed: [f64; 2],
    unit: DrawingUnit,
    tol: f64,
) -> Option<Measured> {
    let (region, area, exact, islands) = measure(faces, sources, seed, unit, tol)?;
    let by_tolerance = gap_tolerance(unit, tol) && {
        let base = unit.tolerance();
        let computed;
        let base_faces = match base_faces {
            Some(base_faces) => base_faces,
            None => {
                computed = wall_faces(sources, base);
                &computed
            }
        };
        let perimeter: f64 = std::iter::once(&region.outer)
            .chain(&region.holes)
            .flat_map(|ring| ring_lines(ring))
            .map(|line| length(&line))
            .sum();
        let slack = area_m2(GAP_SLACK * base * perimeter, unit);
        !measure(base_faces, sources, seed, unit, base)
            .is_some_and(|(_, base_area, _, _)| (base_area - area).abs() <= slack)
    };
    Some(Measured {
        region,
        area_m2: area,
        exact: exact && !by_tolerance,
        islands,
        by_tolerance,
    })
}

/// Quante tolleranze di base, per unità di perimetro, può cambiare l'area tra
/// la misura alla tolleranza larga e quella alla tolleranza di base (lo
/// spostamento ammesso di un lato, come la saldatura delle facce).
pub const GAP_SLACK: f64 = 4.0;

/// Oltre questa lunghezza, in metri, un gruppo di muri dentro una regione la
/// rende «non chiusa» (R50); sotto sono monconi, sporgenze, battute.
pub const OPEN_WALLS_M: f64 = 0.5;

/// `tol` supera la tolleranza di base oltre l'arrotondamento?
fn gap_tolerance(unit: DrawingUnit, tol: f64) -> bool {
    tol > unit.tolerance() * (1.0 + 1.0e-9)
}

/// La misura alla tolleranza `tol`, senza il controllo di R47.
fn measure(
    faces: &[Vec<[f64; 2]>],
    sources: &FxHashMap<Handle, BoundarySource>,
    seed: [f64; 2],
    unit: DrawingUnit,
    tol: f64,
) -> Option<(Region, f64, bool, usize)> {
    let region = closed_region(faces, sources, seed, unit, tol)?;
    let (area, exact) = net_area(&region, sources, tol);
    let islands = region.holes.len();
    Some((region, area_m2(area, unit), exact, islands))
}

/// La regione chiusa attorno a `seed` alla tolleranza `tol`, senza punte e
/// senza muri dentro (R50): la prima metà della misura, senza l'area.
/// [`measure_at`] dà `Some` se e solo se questa lo dà.
pub(crate) fn closed_region(
    faces: &[Vec<[f64; 2]>],
    sources: &FxHashMap<Handle, BoundarySource>,
    seed: [f64; 2],
    unit: DrawingUnit,
    tol: f64,
) -> Option<Region> {
    let region = without_spurs(region_at(faces, seed, tol)?, tol);
    (!has_walls_inside(&region, sources, unit.metres(OPEN_WALLS_M), tol)).then_some(region)
}

/// Gli avvisi di qualità di un locale misurato: isole → [`Notice::Isole`];
/// chiuso solo dalla tolleranza → [`Notice::ChiusoPerTolleranza`];
/// altrimenti area non esatta → [`Notice::ContornoApprossimato`].
pub fn quality_notices(name: &str, exact: bool, by_tolerance: bool, islands: usize) -> Vec<Notice> {
    let mut notices = Vec::new();
    if islands > 0 {
        notices.push(Notice::Isole {
            name: name.to_string(),
            count: islands,
        });
    }
    if by_tolerance {
        notices.push(Notice::ChiusoPerTolleranza {
            name: name.to_string(),
        });
    } else if !exact {
        notices.push(Notice::ContornoApprossimato {
            name: name.to_string(),
        });
    }
    notices
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
        height: None,
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
    let measure_here =
        |seed: [f64; 2]| measure_at(pg_faces, base_faces.as_deref(), pg_sources, seed, unit, tol);

    // 1. I semi, senza doppioni, raggruppati per faccia.
    let mut groups: Vec<Group> = Vec::new();
    let seeds = distinct(fatto, tol)
        .into_iter()
        .map(|room| (room, false))
        .chain(distinct(progetto, tol).into_iter().map(|room| (room, true)));
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
        let Some(found) = measure_here(room.seed) else {
            notices.push(Notice::LocaleNonChiuso {
                name: room.name.clone(),
            });
            continue;
        };
        // La faccia sta tutta in una zona? Decide il suo punto interno, come
        // per le facce senza seme: il seme può stare sul bordo della zona.
        if !zones.is_empty() {
            let probe = interior_point(&found.region, tol).unwrap_or(room.seed);
            if let Some(zone) = zones
                .iter()
                .find(|zone| zone.covers(&found.region.outer, probe, tolerance))
            {
                let name = room.name.clone();
                match zone.state {
                    ZoneState::Demolito if of_project && zone.varco => {
                        notices.push(Notice::SemeNelVarco { name })
                    }
                    ZoneState::Demolito => notices.push(Notice::SemeInDemolizione { name }),
                    // Un seme di fatto in un muro nuovo: le parti del locale
                    // escono «(senza nome)», ognuna con il suo avviso.
                    ZoneState::Nuovo => {}
                }
                continue;
            }
        }
        match groups
            .iter_mut()
            .find(|group| group.found.region.outer == found.region.outer)
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
    let room_holes: Vec<Vec<Curve>> = rooms
        .iter()
        .flat_map(|room| room.region.holes.iter().map(|hole| ring_curves(hole)))
        .collect();
    let mut unnamed: Vec<(Computed, bool)> = Vec::new();
    for face in face_regions(pg_faces, tol)
        .into_iter()
        .map(|face| without_spurs(face, tol))
    {
        if rooms.iter().any(|room| room.region.outer == face.outer) {
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
        // Dentro un'isola di un locale (il buco stesso, o ciò che contiene:
        // un pilastro a doppio contorno, un cavedio, una vasca).
        if room_holes
            .iter()
            .any(|hole| contains(hole, inner, tolerance))
        {
            continue;
        }
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
        match measure_here(inner) {
            Some(found) if found.region.outer == face.outer => unnamed.push((
                Computed {
                    name: UNNAMED.to_string(),
                    number: String::new(),
                    area_m2: found.area_m2,
                    region: found.region,
                    confirm: false,
                    exact: found.exact,
                    islands: found.islands,
                    seed: inner,
                },
                found.by_tolerance,
            )),
            // Una faccia chiusa con dentro dei muri (R50).
            _ => notices.push(Notice::LocaleNonChiuso {
                name: UNNAMED.to_string(),
            }),
        }
    }
    let unnamed_holes: Vec<Vec<Curve>> = unnamed
        .iter()
        .flat_map(|(room, _)| room.region.holes.iter().map(|hole| ring_curves(hole)))
        .collect();
    for (room, by_tolerance) in unnamed {
        if unnamed_holes
            .iter()
            .any(|hole| contains(hole, room.seed, tolerance))
        {
            continue;
        }
        notices.push(Notice::LocaleSenzaNome {
            area_m2: room.area_m2,
        });
        notices.extend(quality_notices(
            &room.name,
            room.exact,
            by_tolerance,
            room.islands,
        ));
        rooms.push(room);
    }
    (rooms, notices)
}

/// Il punto sta in una zona nuova (bordo compreso), come per
/// [`project_rooms`], che di un seme di progetto lì non fa un locale
/// ([`Notice::SemeInMuroNuovo`]).
pub(crate) fn in_new_zone(zones: &[ZoneGeom], p: [f64; 2], tol: f64) -> bool {
    let tolerance = Tolerance::new(tol);
    zones
        .iter()
        .filter_map(ZoneShape::new)
        .any(|zone| zone.state == ZoneState::Nuovo && zone.holds(p, tolerance))
}

/// I locali senza doppioni: lo stesso nome con il seme entro `tol` vale una
/// volta (il primo). Così il contorno e il testo dello stesso locale di fatto,
/// passati tutti e due, non danno «A + A».
pub(crate) fn distinct(rooms: &[Room], tol: f64) -> Vec<&Room> {
    let mut kept: Vec<&Room> = Vec::new();
    for room in rooms {
        let twin = kept.iter().any(|seen| {
            seen.name == room.name
                && (seen.seed[0] - room.seed[0]).hypot(seen.seed[1] - room.seed[1]) <= tol
        });
        if !twin {
            kept.push(room);
        }
    }
    kept
}

/// I semi che cadono nella stessa faccia di progetto.
struct Group<'a> {
    found: Measured,
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
        let Measured {
            region,
            area_m2,
            exact,
            islands,
            by_tolerance,
        } = self.found;
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
        notices.extend(quality_notices(&name, exact, by_tolerance, islands));
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

/// La regione senza le punte dei suoi anelli: un moncone attaccato al
/// contorno entra nell'anello della faccia come andata e ritorno sullo stesso
/// tratto (`… P → Q → P …`). Si tolgono finché ce ne sono, con i vertici
/// doppi che lasciano; un anello che resterebbe con meno di tre vertici resta
/// com'era. L'area non cambia; il moncone resta un muro dentro la regione
/// (con la regione da tutte e due le parti), che R50 vede.
fn without_spurs(region: Region, tol: f64) -> Region {
    let clean = |ring: Vec<[f64; 2]>| {
        let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= tol;
        let mut cleaned = ring.clone();
        loop {
            let before = cleaned.len();
            let mut kept: Vec<[f64; 2]> = Vec::with_capacity(cleaned.len());
            for &p in &cleaned {
                if kept.last().map_or(true, |&q| !near(p, q)) {
                    kept.push(p);
                }
            }
            while kept.len() > 1 && near(kept[0], kept[kept.len() - 1]) {
                kept.pop();
            }
            cleaned = kept;
            let count = cleaned.len();
            if count < 3 {
                break;
            }
            if let Some(tip) = (0..count)
                .find(|&i| near(cleaned[(i + count - 1) % count], cleaned[(i + 1) % count]))
            {
                cleaned.remove(tip);
                continue;
            }
            if cleaned.len() == before {
                break;
            }
        }
        if cleaned.len() >= 3 {
            cleaned
        } else {
            ring
        }
    };
    Region {
        outer: clean(region.outer),
        holes: region.holes.into_iter().map(clean).collect(),
    }
}

/// R50 (documentazione del modulo): nella regione restano muri? I segmenti
/// dei muri che toccano il riquadro della regione si spezzano dove incontrano
/// i suoi anelli; restano i pezzi strettamente dentro, col punto medio nella
/// regione e a più di `tol` dal contorno e da ogni buco (la regione da tutte e
/// due le parti). Si uniscono quando si toccano entro `tol` ([`touch`]), e un
/// gruppo lungo in tutto più di `limit` (unità del disegno) rende la regione
/// «non chiusa». I pezzi lunghi `tol` o meno si saltano, come fa la ricerca
/// delle facce.
fn has_walls_inside(
    region: &Region,
    sources: &FxHashMap<Handle, BoundarySource>,
    limit: f64,
    tol: f64,
) -> bool {
    let tolerance = Tolerance::new(tol);
    let ring_lines_all: Vec<Line> = std::iter::once(&region.outer)
        .chain(&region.holes)
        .flat_map(|ring| ring_lines(ring))
        .collect();
    if ring_lines_all.is_empty() {
        return false;
    }
    let ring_boxes: Vec<Bounds> = ring_lines_all
        .iter()
        .map(|line| bounds(&[line.start, line.end]))
        .collect();
    let edges: Vec<Curve> = ring_lines_all.iter().copied().map(Curve::Line).collect();
    let outer = ring_curves(&region.outer);
    let holes: Vec<Vec<Curve>> = region.holes.iter().map(|hole| ring_curves(hole)).collect();
    let strictly_inside = |p: [f64; 2]| {
        edges.iter().all(|edge| distance_to(edge, p) > tol)
            && contains(&outer, p, tolerance)
            && !holes.iter().any(|hole| contains(hole, p, tolerance))
    };

    // I pezzi strettamente dentro.
    let reach = bounds(&region.outer);
    let mut pieces: Vec<Line> = Vec::new();
    for segment in sources
        .values()
        .flat_map(|source| source.segments.iter().copied())
    {
        let segment_box = bounds(&[segment.start, segment.end]);
        if !(length(&segment) > tol) || !boxes_overlap(&segment_box, &reach, tol) {
            continue;
        }
        let mut cuts = vec![0.0, 1.0];
        for (edge, edge_box) in ring_lines_all.iter().zip(&ring_boxes) {
            if !boxes_overlap(&segment_box, edge_box, tol) {
                continue;
            }
            match segment_crossing(segment, *edge, tolerance) {
                SegmentCrossing::None => {}
                SegmentCrossing::Point { a, .. } => cuts.push(a.clamp(0.0, 1.0)),
                SegmentCrossing::Overlap { a, .. } => {
                    cuts.extend(a.iter().map(|t| t.clamp(0.0, 1.0)))
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        let at = |t: f64| {
            [
                segment.start[0] + t * (segment.end[0] - segment.start[0]),
                segment.start[1] + t * (segment.end[1] - segment.start[1]),
            ]
        };
        for pair in cuts.windows(2) {
            let piece = Line {
                start: at(pair[0]),
                end: at(pair[1]),
            };
            if length(&piece) > tol && strictly_inside(at((pair[0] + pair[1]) / 2.0)) {
                pieces.push(piece);
            }
        }
    }
    if pieces.is_empty() {
        return false;
    }

    // I gruppi, per contatto, e la loro lunghezza.
    let boxes: Vec<Bounds> = pieces
        .iter()
        .map(|line| bounds(&[line.start, line.end]))
        .collect();
    let mut groups = UnionFind::new(pieces.len());
    let mut order: Vec<usize> = (0..pieces.len()).collect();
    order.sort_by(|&a, &b| boxes[a].0[0].total_cmp(&boxes[b].0[0]));
    let mut active: Vec<usize> = Vec::new();
    for index in order {
        let front = boxes[index].0[0];
        active.retain(|&other| boxes[other].1[0] + tol >= front);
        for &other in &active {
            if !boxes_overlap(&boxes[index], &boxes[other], tol)
                || groups.find(index) == groups.find(other)
            {
                continue;
            }
            if touch(pieces[index], pieces[other], tol) {
                groups.union(index, other);
            }
        }
        active.push(index);
    }
    let mut lengths: FxHashMap<usize, f64> = FxHashMap::default();
    for (index, piece) in pieces.iter().enumerate() {
        *lengths.entry(groups.find(index)).or_insert(0.0) += length(piece);
    }
    lengths.values().any(|&total| total > limit)
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

/// `p` sta nella regione: dentro il contorno esterno, bordo compreso, e non
/// strettamente dentro un buco.
pub(crate) fn region_holds(region: &Region, p: [f64; 2], tol: f64) -> bool {
    let tolerance = Tolerance::new(tol);
    contains(&ring_curves(&region.outer), p, tolerance)
        && !region
            .holes
            .iter()
            .any(|hole| strictly_in(&ring_curves(hole), p, tolerance))
}

fn ring_lines(ring: &[[f64; 2]]) -> Vec<Line> {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(&start, &end)| Line { start, end })
        .filter(|line| length(line) > 0.0)
        .collect()
}

pub(crate) fn ring_curves(ring: &[[f64; 2]]) -> Vec<Curve> {
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
            height: None,
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

    /// Fix finale, punto 6: `TEXTSIZE` oltre 1 m non è un'altezza di testo
    /// plausibile (un disegno in metri nato da acadiso ha 2,5: etichette alte
    /// 2,5 m). Ripiego a 0,25 m, con il motivo. In millimetri 2,5 resta.
    #[test]
    fn room_text_height_falls_back_on_an_implausible_textsize() {
        let (m, mm) = (DrawingUnit::Metri, DrawingUnit::Millimetri);
        let mut doc = Document::new();
        for (unit, textsize, want) in [
            (m, 2.5, (0.25, Some(TextFallback::Implausible))),
            (m, 1.0, (1.0, None)),
            (m, 0.25, (0.25, None)),
            (m, 0.0, (0.25, Some(TextFallback::NotPositive))),
            (m, f64::NAN, (0.25, Some(TextFallback::NotPositive))),
            (mm, 2.5, (2.5, None)),
            (mm, 250.0, (250.0, None)),
            (mm, 1000.0, (1000.0, None)),
            (mm, 1500.0, (250.0, Some(TextFallback::Implausible))),
            (mm, -1.0, (250.0, Some(TextFallback::NotPositive))),
        ] {
            doc.header.text_height = textsize;
            assert_eq!(room_text_height(&doc, unit), want, "{unit:?} {textsize}");
        }
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
            height: None,
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
                XDataValue::Position3D(Vector3::new(2.0, 1.5, 0.0)),
                text("origine"),
                text("punto-interno"),
                text("ancora"),
                XDataValue::Point3D(Vector3::new(2.0, 1.5, 0.0)),
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
                        height: None,
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
    fn room_xdata_roundtrips_height() {
        let room = Room {
            height: Some(3.0),
            ..fatto("A", [2.0, 1.5])
        };
        let values = encode(&room, RoomPart::Testo);
        assert_eq!(
            values[values.len() - 2..],
            [text("altezza"), XDataValue::Real(3.0)]
        );
        assert_eq!(
            decode(&record(values)),
            Some((room, RoomPart::Testo)),
            "l'altezza torna uguale"
        );
    }

    #[test]
    fn room_xdata_without_height_key_decodes_none() {
        let room = fatto("A", [2.0, 1.5]);
        let values = encode(&room, RoomPart::Contorno);
        assert!(!values.contains(&text("altezza")), "None non si scrive");
        let (back, _) = decode(&record(values)).unwrap();
        assert_eq!(back.height, None);
    }

    #[test]
    fn room_xdata_ignores_non_positive_height() {
        for bad in [-1.0, 0.0, f64::NAN, f64::INFINITY] {
            let mut values = encode(&fatto("A", [2.0, 1.5]), RoomPart::Testo);
            values.push(text("altezza"));
            values.push(XDataValue::Real(bad));
            let (back, _) = decode(&record(values)).unwrap();
            assert_eq!(back.height, None, "altezza {bad}");
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
            // Premessa: senza la regola dei muri dentro (R50) il punto
            // cadrebbe nel contorno esterno, meno B: 30,96 − 11,70 = 19,26 m².
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
        // `measure_at` dice perché: chiuso solo dalla tolleranza (R51), con
        // le facce alla tolleranza di base calcolate da sé o date.
        let base_faces = crate::bim::walls::faces(&sources, M.tolerance());
        let measured = measure_at(&faces, None, &sources, [2.0, 1.5], M, hpgaptol).unwrap();
        assert!(measured.by_tolerance && !measured.exact);
        assert_eq!(
            measure_at(&faces, Some(&base_faces), &sources, [2.0, 1.5], M, hpgaptol),
            Some(measured.clone())
        );
        assert_eq!(
            quality_notices("A", measured.exact, measured.by_tolerance, measured.islands),
            [Notice::ChiusoPerTolleranza { name: "A".into() }]
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
            let measured = measure_at(&faces, None, &sources, [2.0, 2.5], M, hpgaptol).unwrap();
            assert!(!measured.by_tolerance, "{form:?}");
        }
    }

    // R47 e R51 anche nei locali di progetto: il contorno chiuso solo dalla
    // tolleranza ha il suo avviso.
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
        assert_eq!(notices, [Notice::ChiusoPerTolleranza { name: "A".into() }]);
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
    fn walls_inside_make_the_room_open() {
        // Senza il lato (0; 0)-(4; 0) i tre lati di A restano nel contorno
        // esterno (Task 14, `aggiorna_reports_open_room`).
        let mut e = case_a(Form::Continua, M);
        e.retain(|entity| !is_line(entity, [0.0, 0.0], [4.0, 0.0]));
        let (faces, sources) = survey(e, TOL);
        assert_eq!(compute_at(&faces, &sources, [2.0, 1.5], M, TOL), None);
        let (_, area, _, _) = compute_at(&faces, &sources, [6.0, 1.5], M, TOL).expect("B");
        assert_m2(area, 11.70, "B resta");

        // Una sporgenza di 5 cm oltre l'angolo (la faccia del tramezzo che
        // entra in A), una battuta da 5 cm staccata e una da 0,50 m giuste:
        // sotto la soglia, il locale resta chiuso ed esatto.
        for stub in [
            wall_line([4.0, 1.5], [3.95, 1.5]),
            wall_line([2.0, 1.0], [2.05, 1.0]),
            wall_line([4.0, 2.0], [3.5, 2.0]),
        ] {
            let mut e = case_a(Form::Continua, M);
            e.push(stub);
            let (faces, sources) = survey(e, TOL);
            let (_, area, exact, islands) =
                compute_at(&faces, &sources, [2.0, 2.5], M, TOL).expect("moncone");
            assert_m2(area, 12.0, "moncone");
            assert!(exact);
            assert_eq!(islands, 0);
        }
        // Un tratto doppio di 5 cm sul contorno di A e una sporgenza di 5 cm
        // oltre l'angolo (4; 0), nel muro sud: non stanno dentro A.
        let mut e = case_a(Form::Continua, M);
        e.push(wall_line([0.0, 3.0], [0.0, 2.95]));
        e.push(wall_line([4.0, 0.0], [4.0, -0.05]));
        let (faces, sources) = survey(e, TOL);
        let (_, area, exact, _) = compute_at(&faces, &sources, [2.0, 1.5], M, TOL).expect("A");
        assert_m2(area, 12.0, "sporgenze");
        assert!(exact);

        // Il costo della regola: una linea o una polilinea aperta dentro A
        // (un arredo lasciato acceso sul layer dei muri), anche attaccate al
        // contorno (una parete a una linea che entra per 1 m), danno «non
        // chiuso», mai un numero sbagliato.
        for stray in [
            wall_line([1.0, 1.0], [3.0, 1.0]),
            wall_polyline(
                &[([1.0, 1.0], 0.0), ([2.0, 1.0], 0.0), ([2.0, 2.0], 0.0)],
                false,
            ),
            wall_line([4.0, 1.5], [3.0, 1.5]),
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

    #[test]
    fn spurs_are_removed_from_the_rings() {
        let square = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]];
        let plain = Region {
            outer: square.clone(),
            holes: vec![vec![[1.0, 1.0], [1.3, 1.0], [1.3, 1.3], [1.0, 1.3]]],
        };
        assert_eq!(
            without_spurs(plain.clone(), TOL),
            plain,
            "senza punte: uguale"
        );
        // Una punta semplice e un albero (P → Q → R → Q → S → Q → P) sul lato
        // est, e una punta sul buco.
        let spiky = Region {
            outer: vec![
                [0.0, 0.0],
                [4.0, 0.0],
                [4.0, 1.5],
                [3.95, 1.5],
                [4.0, 1.5],
                [4.0, 2.0],
                [3.5, 2.0],
                [3.2, 2.0],
                [3.5, 2.0],
                [3.5, 2.4],
                [3.5, 2.0],
                [4.0, 2.0],
                [4.0, 3.0],
                [0.0, 3.0],
            ],
            holes: vec![vec![
                [1.0, 1.0],
                [1.3, 1.0],
                [1.4, 1.0],
                [1.3, 1.0],
                [1.3, 1.3],
                [1.0, 1.3],
            ]],
        };
        let cleaned = without_spurs(spiky, TOL);
        assert_eq!(
            cleaned.outer,
            vec![
                [0.0, 0.0],
                [4.0, 0.0],
                [4.0, 1.5],
                [4.0, 2.0],
                [4.0, 3.0],
                [0.0, 3.0]
            ]
        );
        assert_eq!(cleaned.holes, plain.holes);
        assert_m2(signed_area(&cleaned.outer).abs(), 12.0, "stessa area");

        // Sul disegno: il contorno di A con un moncone attaccato non lo segue.
        let mut e = case_a(Form::Continua, M);
        e.push(wall_line([4.0, 1.5], [3.95, 1.5]));
        let (faces, sources) = survey(e, TOL);
        let raw = region_at(&faces, [2.0, 2.5], TOL).unwrap();
        assert!(raw.outer.contains(&[3.95, 1.5]), "premessa: la punta c'è");
        let c = computed(&faces, &sources, "A", [2.0, 2.5]);
        assert!(!c.region.outer.contains(&[3.95, 1.5]));
        let EntityType::LwPolyline(outline) =
            &room_entities(&c, Phase::Fatto, &sources, 0.25, TOL)[0]
        else {
            panic!();
        };
        assert!(outline
            .vertices
            .iter()
            .all(|v| (v.location.x - 3.95).abs() > 1e-6));
    }

    /// Il caso A con un vano porta nel tramezzo tra y = 1,00 e y = 1,90:
    /// spallette a y = 1,00 e y = 1,90, linee di soglia su x = 4,00 (per A) e
    /// x = 4,10 (per B), e il vano fra le due soglie (0,09 m²). Con
    /// `corner_gap`, l'angolo (0; 0) di A aperto sulla faccia ovest.
    fn doorway(corner_gap: Option<f64>) -> Vec<EntityType> {
        let mut e = match corner_gap {
            Some(gap) => open_corner(gap, true),
            None => case_a(Form::Continua, M),
        };
        e.retain(|entity| {
            !is_line(entity, [4.0, 0.0], [4.0, 3.0]) && !is_line(entity, [4.1, 0.0], [4.1, 3.0])
        });
        assert_eq!(e.len(), 10, "tolti i due lati del tramezzo");
        for x in [4.0, 4.1] {
            e.push(wall_line([x, 0.0], [x, 1.0]));
            e.push(wall_line([x, 1.9], [x, 3.0]));
            e.push(wall_line([x, 1.0], [x, 1.9]));
        }
        e.push(wall_line([4.0, 1.0], [4.1, 1.0]));
        e.push(wall_line([4.0, 1.9], [4.1, 1.9]));
        e
    }

    // R50: il caso trovato in revisione.
    #[test]
    fn corner_gap_next_to_a_closed_room_is_open() {
        // Senza vuoti: A 12,00, B 11,70, il vano 0,09, tutti esatti.
        let (faces, sources) = survey(doorway(None), TOL);
        for (p, want) in [
            ([2.0, 1.5], 12.0),
            ([6.0, 1.5], 11.70),
            ([4.05, 1.45], 0.09),
        ] {
            let (_, area, exact, _) = compute_at(&faces, &sources, p, M, TOL).expect("chiuso");
            assert_m2(area, want, p);
            assert!(exact, "{p:?}");
        }

        // Con il vuoto di 2 mm in A i suoi lati toccano il vano chiuso (alle
        // spallette), ma restano dentro il contorno esterno: non chiuso.
        let (faces, sources) = survey(doorway(Some(0.002)), TOL);
        assert_eq!(compute_at(&faces, &sources, [2.0, 1.5], M, TOL), None);
        // Premessa: la regione sarebbe il contorno esterno meno B e il vano,
        // 30,96 − 11,70 − 0,09 = 19,17 m².
        let outer = region_at(&faces, [2.0, 1.5], TOL).expect("contorno esterno");
        assert_eq!(outer.holes.len(), 2);
        assert_m2(
            area_m2(net_area(&outer, &sources, TOL).0, M),
            19.17,
            "premessa",
        );
        let (_, area, exact, _) = compute_at(&faces, &sources, [6.0, 1.5], M, TOL).expect("B");
        assert_m2(area, 11.70, "B");
        assert!(exact);

        // Con HPGAPTOL 5 mm: chiuso, 12,00, ma solo dalla tolleranza (R47).
        let hpgaptol = 0.005;
        let (faces, sources) = survey(doorway(Some(0.002)), hpgaptol);
        let measured = measure_at(&faces, None, &sources, [2.0, 1.5], M, hpgaptol).expect("chiuso");
        assert!(
            (measured.area_m2 - 12.0).abs() < 1e-3,
            "{}",
            measured.area_m2
        );
        assert!(!measured.exact);
        assert!(measured.by_tolerance);
    }

    // R50 nello stato di progetto: il varco al posto del vano.
    #[test]
    fn corner_gap_next_to_a_varco_is_open_in_the_project() {
        let zones = vec![axis_zone([4.05, 1.0], [4.05, 1.9], true)];
        let (rooms, notices) = rooms_of(
            open_corner(0.002, true),
            &zones,
            &[fatto("B", [6.0, 1.5])],
            &[progetto("A", [2.0, 1.5])],
        );
        assert_eq!(names(&rooms), ["B"]);
        assert_m2(rooms[0].area_m2, 11.70, "B");
        assert!(
            notices.contains(&Notice::LocaleNonChiuso { name: "A".into() }),
            "{notices:?}"
        );
        assert!(notices
            .iter()
            .any(|notice| matches!(notice, Notice::Varco { .. })));
        assert_eq!(notices.len(), 2, "{notices:?}");
    }

    // R51 (b): ciò che sta dentro un'isola non è un locale.
    #[test]
    fn faces_inside_an_island_are_not_rooms() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            // Il pilastro a doppio contorno (0,30 e 0,20 di lato) e una vasca
            // a due linee staccata dai muri in B (1,70 × 0,70 e, dentro,
            // 1,60 × 0,60).
            let mut e = case_a(form, M);
            e.push(pilastro(M));
            e.push(wall_polyline(
                &[
                    ([1.05, 1.05], 0.0),
                    ([1.25, 1.05], 0.0),
                    ([1.25, 1.25], 0.0),
                    ([1.05, 1.25], 0.0),
                ],
                true,
            ));
            e.push(wall_polyline(
                &[
                    ([6.0, 0.5], 0.0),
                    ([7.7, 0.5], 0.0),
                    ([7.7, 1.2], 0.0),
                    ([6.0, 1.2], 0.0),
                ],
                true,
            ));
            e.push(wall_polyline(
                &[
                    ([6.05, 0.55], 0.0),
                    ([7.65, 0.55], 0.0),
                    ([7.65, 1.15], 0.0),
                    ([6.05, 1.15], 0.0),
                ],
                true,
            ));
            let (rooms, notices) = rooms_of(
                e,
                &[],
                &[fatto("A", [2.0, 2.5]), fatto("B", [5.0, 1.5])],
                &[],
            );
            assert_eq!(names(&rooms), ["A", "B"], "{form:?}: {notices:?}");
            assert_m2(named(&rooms, "A").area_m2, 11.91, form);
            assert_m2(named(&rooms, "B").area_m2, 11.70 - 1.19, form);
            assert_eq!(
                notices,
                [
                    Notice::Isole {
                        name: "A".into(),
                        count: 1
                    },
                    Notice::Isole {
                        name: "B".into(),
                        count: 1
                    }
                ],
                "{form:?}"
            );
        }
    }

    // R51 (c)
    #[test]
    fn seed_in_a_demolished_zone_is_reported() {
        // Un ripostiglio 1 × 1 dentro B, e una zona demolita senza asse sul
        // suo contorno: i bordi restano, la faccia sta tutta nella zona.
        let mut e = case_a(Form::Continua, M);
        e.push(wall_polyline(
            &[
                ([5.0, 1.0], 0.0),
                ([6.0, 1.0], 0.0),
                ([6.0, 2.0], 0.0),
                ([5.0, 2.0], 0.0),
            ],
            true,
        ));
        let zone = zone_geom(
            Zone {
                state: ZoneState::Demolito,
                origin: ZoneOrigin::Rettangolo,
                axis: None,
                thickness: None,
                height: None,
                varco: false,
            },
            vec![[5.0, 1.0], [6.0, 1.0], [6.0, 2.0], [5.0, 2.0]],
        );
        let (rooms, notices) = rooms_of(
            e,
            &[zone],
            &[fatto("B", [7.0, 2.5]), fatto("Ripostiglio", [5.5, 1.5])],
            &[progetto("Dispensa", [5.5, 1.6])],
        );
        assert_eq!(names(&rooms), ["B"], "{notices:?}");
        for name in ["Ripostiglio", "Dispensa"] {
            assert!(
                notices.contains(&Notice::SemeInDemolizione { name: name.into() }),
                "{name}: {notices:?}"
            );
        }
        assert!(
            !notices
                .iter()
                .any(|notice| matches!(notice, Notice::LocaleSenzaNome { .. })),
            "{notices:?}"
        );
    }

    // R51 (d)
    #[test]
    fn the_same_seed_twice_counts_once() {
        // Il contorno e il testo dello stesso locale di fatto (stesso nome,
        // stesso seme), passati tutti e due: niente «A + A».
        let twice = [
            fatto("A", [2.0, 1.5]),
            fatto("A", [2.0, 1.5]),
            fatto("B", [6.0, 1.5]),
            fatto("B", [6.0 + 0.5 * TOL, 1.5]),
        ];
        let (rooms, notices) = rooms_of(case_a(Form::Continua, M), &[], &twice, &[]);
        assert_eq!(names(&rooms), ["A", "B"]);
        assert!(rooms.iter().all(|room| !room.confirm));
        assert!(notices.is_empty(), "{notices:?}");
        let (rooms, notices) =
            rooms_of(case_a(Form::Continua, M), &total_demolition(), &twice, &[]);
        assert_eq!(names(&rooms), ["A + B"]);
        let [Notice::LocaleFuso { names: fused, .. }] = &notices[..] else {
            panic!("{notices:?}");
        };
        assert_eq!(fused, &["A", "B"]);
        // Due semi di progetto uguali: un nome solo, nessuna fusione.
        let (rooms, notices) = rooms_of(
            case_a(Form::Continua, M),
            &total_demolition(),
            &fatto_ab(),
            &[
                progetto("Soggiorno", [3.0, 1.0]),
                progetto("Soggiorno", [3.0, 1.0]),
            ],
        );
        assert_eq!(names(&rooms), ["Soggiorno"]);
        assert!(notices.is_empty(), "{notices:?}");
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
            height: None,
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
        let mut doc = Document::new();
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

    /// `AL_ALTEZZA` (spec parte 2, §5.1): di un locale di fatto basta una
    /// parte selezionata, e contorno e testo cambiano insieme; un locale è la
    /// fase più il seme, quindi un seme di progetto nello stesso punto resta
    /// fuori. Niente doppioni, in ordine di disegno.
    #[test]
    fn height_edits_cover_both_parts_of_a_fatto_room() {
        let (faces, sources) = survey(case_a(Form::Continua, M), TOL);
        let a = computed(&faces, &sources, "A", [2.0, 1.5]);
        let b = computed(&faces, &sources, "B", [6.0, 1.5]);
        let mut doc = Document::new();
        ensure_layers(&mut doc);
        let parts = |doc: &mut Document, room: &Computed| -> Vec<Handle> {
            room_entities(room, Phase::Fatto, &sources, 0.25, TOL)
                .into_iter()
                .map(|entity| doc.add_entity(entity).unwrap())
                .collect()
        };
        let a_parts = parts(&mut doc, &a);
        let b_parts = parts(&mut doc, &b);
        let soggiorno = progetto("Soggiorno", [2.0, 1.5]);
        let seed = doc.add_entity(seed_entity(&soggiorno, 0.25)).unwrap();
        let line = doc.add_entity(wall_line([0.0, 5.0], [1.0, 5.0])).unwrap();

        let tall_a = Room {
            height: Some(3.0),
            ..fatto("A", [2.0, 1.5])
        };
        let want = vec![
            (a_parts[0], tall_a.clone(), RoomPart::Contorno),
            (a_parts[1], tall_a, RoomPart::Testo),
        ];
        // Il solo testo basta.
        assert_eq!(room_height_edits(&doc, &[a_parts[1]], Some(3.0), TOL), want);
        // Due parti dello stesso locale, una ripetuta, e una linea: una volta sola.
        assert_eq!(
            room_height_edits(
                &doc,
                &[a_parts[0], a_parts[1], a_parts[0], line],
                Some(3.0),
                TOL
            ),
            want
        );
        // Togliere: le stesse parti, senza altezza.
        let removed = room_height_edits(&doc, &[a_parts[0]], None, TOL);
        assert_eq!(
            removed
                .iter()
                .map(|(handle, room, _)| (*handle, room.height))
                .collect::<Vec<_>>(),
            [(a_parts[0], None), (a_parts[1], None)]
        );
        // Il seme di progetto nello stesso punto di A è un altro locale.
        assert_eq!(
            room_height_edits(&doc, &[seed], Some(2.7), TOL),
            vec![(
                seed,
                Room {
                    height: Some(2.7),
                    ..soggiorno
                },
                RoomPart::Seme
            )]
        );
        // Due locali: quattro parti, in ordine di disegno.
        let both = room_height_edits(&doc, &[b_parts[1], a_parts[0]], Some(3.0), TOL);
        assert_eq!(
            both.iter().map(|(handle, ..)| *handle).collect::<Vec<_>>(),
            [a_parts[0], a_parts[1], b_parts[0], b_parts[1]]
        );
        // Nessuna parte di locale selezionata: niente.
        assert!(room_height_edits(&doc, &[line], Some(3.0), TOL).is_empty());
        assert!(room_height_edits(&doc, &[], Some(3.0), TOL).is_empty());
    }

    fn moved(entity: &mut EntityType, transform: crate::command::EntityTransform) {
        crate::entities::traits::EntityTypeOps::apply_transform(entity, &transform);
    }

    fn seed_of(entity: &EntityType) -> [f64; 2] {
        decode(entity.common().extended_data.get_record(APP_LOCALE).unwrap())
            .unwrap()
            .0
            .seed
    }

    /// Il seme segue l'entità in MOVE/COPY/ROTATE/SCALE/MIRROR: è una posizione
    /// (1011), che il codec trasforma come in AutoCAD.
    #[test]
    fn the_seed_follows_the_entity_through_transforms() {
        use crate::command::EntityTransform as T;
        use glam::DVec3;
        let base = seed_entity(&progetto("Soggiorno", [2.0, 1.5]), 0.25);
        let near = |got: [f64; 2], want: [f64; 2]| {
            assert!(
                (got[0] - want[0]).hypot(got[1] - want[1]) < 1e-9,
                "{got:?} != {want:?}"
            );
        };
        let mut e = base.clone();
        moved(&mut e, T::Translate(DVec3::new(4.1, 0.0, 0.0)));
        near(seed_of(&e), [6.1, 1.5]);
        let mut e = base.clone();
        moved(
            &mut e,
            T::Scale {
                center: DVec3::ZERO,
                factor: 2.0,
            },
        );
        near(seed_of(&e), [4.0, 3.0]);
        let mut e = base.clone();
        moved(
            &mut e,
            T::Rotate {
                center: DVec3::ZERO,
                axis: DVec3::Z,
                angle_rad: FRAC_PI_2,
            },
        );
        near(seed_of(&e), [-1.5, 2.0]);
        let mut e = base;
        moved(
            &mut e,
            T::Mirror {
                p1: DVec3::ZERO,
                p2: DVec3::X,
                working_normal: DVec3::Z,
            },
        );
        near(seed_of(&e), [2.0, -1.5]);
    }

    /// Il caso del rapporto del 2026-11-02: una copia traslata di un locale,
    /// selezionata da sola, non cambia l'altezza dell'originale.
    #[test]
    fn height_edit_on_a_moved_copy_leaves_the_original_alone() {
        use crate::command::EntityTransform as T;
        let mut doc = Document::new();
        ensure_layers(&mut doc);
        let original = seed_entity(&progetto("Soggiorno", [2.0, 1.5]), 0.25);
        let mut copy = original.clone();
        moved(&mut copy, T::Translate(glam::DVec3::new(4.1, 0.0, 0.0)));
        let original = doc.add_entity(original).unwrap();
        let copy = doc.add_entity(copy).unwrap();

        let edits = room_height_edits(&doc, &[copy], Some(3.2), TOL);
        assert_eq!(
            edits.iter().map(|(handle, ..)| *handle).collect::<Vec<_>>(),
            [copy]
        );
        assert!(!edits.iter().any(|(handle, ..)| *handle == original));
    }

    /// Il caso del secondo rapporto del 2026-11-02: spostare il solo testo di
    /// un locale (o il solo contorno) non lo spezza in due locali; e la copia
    /// pura di un locale resta un locale a sé, senza ambiguità.
    #[test]
    fn moving_only_the_label_keeps_the_room_in_one_piece() {
        use crate::command::EntityTransform as T;
        let (faces, sources) = survey(case_a(Form::Continua, M), TOL);
        let a = computed(&faces, &sources, "A", [2.0, 1.5]);
        let mut doc = Document::new();
        ensure_layers(&mut doc);
        let parts: Vec<EntityType> = room_entities(&a, Phase::Fatto, &sources, 0.25, TOL);
        let (outline, label) = (parts[0].clone(), parts[1].clone());
        let outline_handle = doc.add_entity(outline.clone()).unwrap();
        let mut label_moved = label.clone();
        moved(&mut label_moved, T::Translate(glam::DVec3::new(0.0, 1.0, 0.0)));
        let label_handle = doc.add_entity(label_moved).unwrap();

        let found = rooms_in(&doc);
        let seeds: Vec<[f64; 2]> = found.iter().map(|(_, room, _)| room.seed).collect();
        assert_eq!(seeds[0], seeds[1], "{found:?}");
        let edits = room_height_edits(&doc, &[label_handle], Some(3.0), TOL);
        assert_eq!(
            edits.iter().map(|(handle, ..)| *handle).collect::<Vec<_>>(),
            [outline_handle, label_handle]
        );

    }

    /// La copia pura di un locale (contorno ed etichetta insieme) è un locale a
    /// sé: nessuna ambiguità, e l'altezza della copia non tocca l'originale.
    #[test]
    fn a_plain_copy_of_a_room_is_a_room_of_its_own() {
        use crate::command::EntityTransform as T;
        let (faces, sources) = survey(case_a(Form::Continua, M), TOL);
        let a = computed(&faces, &sources, "A", [2.0, 1.5]);
        let mut doc = Document::new();
        ensure_layers(&mut doc);
        let parts = room_entities(&a, Phase::Fatto, &sources, 0.25, TOL);
        let outline = doc.add_entity(parts[0].clone()).unwrap();
        let label = doc.add_entity(parts[1].clone()).unwrap();
        let shifted = |entity: &EntityType| {
            let mut copy = entity.clone();
            moved(&mut copy, T::Translate(glam::DVec3::new(10.0, 0.0, 0.0)));
            copy
        };
        let copy_outline = doc.add_entity(shifted(&parts[0])).unwrap();
        let copy_label = doc.add_entity(shifted(&parts[1])).unwrap();
        renew_anchors(&mut doc, &[copy_outline, copy_label]);
        assert!(ambiguous_parts(&doc).is_empty(), "copia pura: nessuna ambiguità");
        let edits = room_height_edits(&doc, &[copy_label], Some(3.2), TOL);
        assert_eq!(
            edits.iter().map(|(handle, ..)| *handle).collect::<Vec<_>>(),
            [copy_outline, copy_label],
            "la copia è un locale a sé"
        );
        let edits = room_height_edits(&doc, &[outline], Some(3.2), TOL);
        assert_eq!(
            edits.iter().map(|(handle, ..)| *handle).collect::<Vec<_>>(),
            [outline, label],
            "e l'originale non la include"
        );
    }

    /// Il caso del terzo rapporto del 2026-11-02: copia di un locale, poi
    /// l'etichetta dell'originale portata sul seme della copia. Due etichette
    /// con lo stesso seme: il disegno non dice quale va con quale, quindi le
    /// parti sono ambigue e non si sceglie.
    #[test]
    fn a_label_moved_onto_the_copys_seed_is_reported_not_guessed() {
        use crate::command::EntityTransform as T;
        let (faces, sources) = survey(case_a(Form::Continua, M), TOL);
        let a = computed(&faces, &sources, "A", [2.0, 1.5]);
        let mut doc = Document::new();
        ensure_layers(&mut doc);
        let parts = room_entities(&a, Phase::Fatto, &sources, 0.25, TOL);
        let shifted = |entity: &EntityType| {
            let mut copy = entity.clone();
            moved(&mut copy, T::Translate(glam::DVec3::new(10.0, 0.0, 0.0)));
            copy
        };
        let outline = doc.add_entity(parts[0].clone()).unwrap();
        let label = doc.add_entity(shifted(&parts[1])).unwrap();
        let copy_outline = doc.add_entity(shifted(&parts[0])).unwrap();
        let copy_label = doc.add_entity(shifted(&parts[1])).unwrap();

        let ambiguous: Vec<Handle> = ambiguous_parts(&doc).into_iter().map(|(h, _)| h).collect();
        for handle in [label, copy_outline, copy_label] {
            assert!(ambiguous.contains(&handle), "{handle:?} in {ambiguous:?}");
        }
        assert!(ambiguous.contains(&outline), "tutta la famiglia è ambigua");
    }

    /// La variante del quarto rapporto: etichetta della copia spostata di un
    /// metro, etichetta dell'originale portata sul seme della copia. Una
    /// corrispondenza esatta più un'etichetta staccata: per esclusione
    /// resterebbe un solo contorno, ma l'esclusione nasce da un'associazione
    /// per coincidenza di seme che può essere quella sbagliata.
    #[test]
    fn an_exact_match_plus_a_detached_label_is_still_ambiguous() {
        use crate::command::EntityTransform as T;
        let (faces, sources) = survey(case_a(Form::Continua, M), TOL);
        let a = computed(&faces, &sources, "A", [2.0, 1.5]);
        let mut doc = Document::new();
        ensure_layers(&mut doc);
        let parts = room_entities(&a, Phase::Fatto, &sources, 0.25, TOL);
        let shift = |entity: &EntityType, by: [f64; 2]| {
            let mut copy = entity.clone();
            moved(&mut copy, T::Translate(glam::DVec3::new(by[0], by[1], 0.0)));
            copy
        };
        let handles = [
            doc.add_entity(parts[0].clone()).unwrap(),
            doc.add_entity(shift(&parts[1], [10.0, 0.0])).unwrap(),
            doc.add_entity(shift(&parts[0], [10.0, 0.0])).unwrap(),
            doc.add_entity(shift(&parts[1], [10.0, 1.0])).unwrap(),
        ];
        let ambiguous: Vec<Handle> = ambiguous_parts(&doc).into_iter().map(|(h, _)| h).collect();
        assert_eq!(ambiguous, handles);
        // Nessun testo è stato riabbinato "per esclusione".
        let seeds: Vec<[f64; 2]> = rooms_in(&doc).into_iter().map(|(_, room, _)| room.seed).collect();
        assert_eq!(seeds[1], [12.0, 1.5]);
        assert_eq!(seeds[3], [12.0, 2.5]);
    }

    /// Le due riproduzioni del quinto rapporto: con la copia fatta da ArchLine
    /// (`renew_anchors`) contorno ed etichetta di ogni copia sono una famiglia
    /// a sé, e l'etichetta resta del suo locale dovunque la si porti.
    #[test]
    fn labels_stay_with_their_own_room_after_a_copy() {
        use crate::command::EntityTransform as T;
        let (faces, sources) = survey(case_a(Form::Continua, M), TOL);
        let a = computed(&faces, &sources, "A", [2.0, 1.5]);
        let parts = room_entities(&a, Phase::Fatto, &sources, 0.25, TOL);
        let shift = |entity: &EntityType, by: [f64; 2]| {
            let mut copy = entity.clone();
            moved(&mut copy, T::Translate(glam::DVec3::new(by[0], by[1], 0.0)));
            copy
        };
        let build = |label_orig_at: [f64; 2], label_copy_at: Option<[f64; 2]>| {
            let mut doc = Document::new();
            ensure_layers(&mut doc);
            let outline = doc.add_entity(parts[0].clone()).unwrap();
            let label = doc.add_entity(shift(&parts[1], label_orig_at)).unwrap();
            let copy_outline = doc.add_entity(shift(&parts[0], [10.0, 0.0])).unwrap();
            let mut copies = vec![copy_outline];
            let copy_label = label_copy_at.map(|at| doc.add_entity(shift(&parts[1], at)).unwrap());
            copies.extend(copy_label);
            renew_anchors(&mut doc, &copies);
            (doc, outline, label, copy_outline, copy_label)
        };
        let ids = |edits: Vec<(Handle, Room, RoomPart)>| edits.into_iter().map(|(h, ..)| h).collect::<Vec<_>>();

        // 1. Etichetta della copia eliminata, quella originale sul seme della copia.
        let (doc, outline, label, copy_outline, _) = build([10.0, 0.0], None);
        assert!(ambiguous_parts(&doc).is_empty());
        let edits = ids(room_height_edits(&doc, &[label], Some(3.2), TOL));
        assert_eq!(edits, [outline, label], "il contorno originale, non quello della copia");
        assert!(!edits.contains(&copy_outline));

        // 2. Etichette scambiate.
        let mut doc = Document::new();
        ensure_layers(&mut doc);
        let outline = doc.add_entity(parts[0].clone()).unwrap();
        let label = doc.add_entity(shift(&parts[1], [10.0, 0.0])).unwrap();
        let copy_outline = doc.add_entity(shift(&parts[0], [10.0, 0.0])).unwrap();
        let copy_label = doc.add_entity(shift(&parts[1], [0.0, 0.0])).unwrap();
        // La copia nasce con le etichette al loro posto; poi le si scambia.
        renew_anchors(&mut doc, &[copy_outline, copy_label]);
        assert!(ambiguous_parts(&doc).is_empty());
        let edits = ids(room_height_edits(&doc, &[label], Some(3.2), TOL));
        assert_eq!(edits, [outline, label]);
        let edits = ids(room_height_edits(&doc, &[copy_label], Some(3.2), TOL));
        assert_eq!(edits, [copy_outline, copy_label]);
    }

    /// Una copia parziale (solo l'etichetta) non si aggancia all'originale.
    #[test]
    fn a_partial_copy_is_not_attached_to_the_original() {
        use crate::command::EntityTransform as T;
        let (faces, sources) = survey(case_a(Form::Continua, M), TOL);
        let a = computed(&faces, &sources, "A", [2.0, 1.5]);
        let parts = room_entities(&a, Phase::Fatto, &sources, 0.25, TOL);
        let mut doc = Document::new();
        ensure_layers(&mut doc);
        let outline = doc.add_entity(parts[0].clone()).unwrap();
        let label = doc.add_entity(parts[1].clone()).unwrap();
        let mut copy = parts[1].clone();
        moved(&mut copy, T::Translate(glam::DVec3::new(10.0, 0.0, 0.0)));
        let copy_label = doc.add_entity(copy).unwrap();
        renew_anchors(&mut doc, &[copy_label]);
        assert!(ambiguous_parts(&doc).is_empty(), "il testo copiato è un orfano, non un'ambiguità");
        let edits = room_height_edits(&doc, &[copy_label], Some(3.2), TOL);
        assert_eq!(edits.into_iter().map(|(h, ..)| h).collect::<Vec<_>>(), [copy_label]);
        let edits = room_height_edits(&doc, &[outline], Some(3.2), TOL);
        assert_eq!(edits.into_iter().map(|(h, ..)| h).collect::<Vec<_>>(), [outline, label]);
    }

    /// Due etichette staccate che potrebbero andare con due contorni (copia, poi
    /// spostate tutte e due): ambigue, non indovinate dalla distanza.
    #[test]
    fn two_detached_labels_and_two_outlines_are_ambiguous() {
        use crate::command::EntityTransform as T;
        let (faces, sources) = survey(case_a(Form::Continua, M), TOL);
        let a = computed(&faces, &sources, "A", [2.0, 1.5]);
        let mut doc = Document::new();
        ensure_layers(&mut doc);
        let parts = room_entities(&a, Phase::Fatto, &sources, 0.25, TOL);
        let shift = |entity: &EntityType, by: [f64; 2]| {
            let mut copy = entity.clone();
            moved(&mut copy, T::Translate(glam::DVec3::new(by[0], by[1], 0.0)));
            copy
        };
        doc.add_entity(parts[0].clone()).unwrap();
        doc.add_entity(shift(&parts[1], [0.0, 1.0])).unwrap();
        doc.add_entity(shift(&parts[0], [10.0, 0.0])).unwrap();
        doc.add_entity(shift(&parts[1], [10.0, 1.0])).unwrap();
        assert_eq!(ambiguous_parts(&doc).len(), 4);
    }

    /// I record scritti prima della correzione (seme `Point3D`, 1010) si
    /// leggono ancora, e alla prima riscrittura passano alla posizione.
    #[test]
    fn a_seed_written_as_a_plain_point_is_still_read() {
        let mut values = encode(&progetto("Bagno", [1.0, 2.0]), RoomPart::Seme);
        for value in &mut values {
            if let XDataValue::Position3D(p) = value {
                *value = XDataValue::Point3D(*p);
            }
        }
        let (room, part) = decode(&record(values)).unwrap();
        assert_eq!((room, part), (progetto("Bagno", [1.0, 2.0]), RoomPart::Seme));
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
