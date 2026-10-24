//! Estrazione degli elaborati in DWG 2D ordinari (spec §5.5): lo stato di
//! fatto (il rilievo senza zone, con i locali di fatto ricalcolati),
//! demolizioni e costruzioni (il rilievo con le campiture gialle e rosse) e lo
//! stato di progetto (la geometria di [`project_geometry`], spec §5.4, con i
//! muri nuovi sul layer scelto e i locali di progetto con il loro contorno).
//! I locali si misurano alla tolleranza dei locali (`ExtractOptions::room_tol`,
//! con `HPGAPTOL`), come in `AL_LOCALE` e `AL_AGGIORNA`.
//!
//! Il file estratto si apre senza ArchLine: solo spazio modello, nessun XDATA
//! e nessun APPID `ARCHLINE_*`, nessun record `ARCHLINE_PROGETTO` (il
//! documento è nuovo e la copia non porta le voci del dizionario radice). La
//! copia è quella di `WBLOCK` (`extract_entities_into`): handle nuovi, con i
//! layer, i tipi di linea, gli stili e i blocchi che le entità usano, più le
//! impostazioni del disegno che un documento nuovo avrebbe ai valori
//! predefiniti (vedi `copy_drawing_settings`) e gli stili delle multilinee
//! (`copy_mline_styles`). La copia lascia nei record gli handle di `src`:
//! stili di quota (stile di testo, frecce, tipi di linea), tipi di linea
//! complessi (stile di testo) e stili di stampa dei layer si ripuntano per nome
//! su `out`, portando ciò che manca; i blocchi inseriti dentro i blocchi `*D`
//! delle quote arrivano con le quote.
//!
//! Restano fuori, ognuno con un `Notice::NonCopiata`: ciò che la copia non
//! porta intero o che non è 2D (immagini, underlay, OLE, riferimenti esterni,
//! multileader, tabelle e altre entità con stili per handle, solidi 3D: vedi
//! `Portability`), gli handle che la copia non restituisce e, per layout, lo
//! spazio carta.
//!
//! Il disegno di lavoro non si tocca: si lavora su una sua copia. I locali di
//! fatto ricalcolati cambiano lì, sugli stessi handle, e le entità nuove dello
//! stato di progetto (pezzi dei muri tagliati, chiusure, muri nuovi, contorni
//! e testi dei locali) entrano solo lì, prima della copia nel documento nuovo.

use std::path::{Path, PathBuf};

use codec::entities::{Insert, MLine};
use codec::objects::{MLineStyle, ObjectType};
use codec::tables::{BlockRecord, DimStyle, Layer, TextStyle};
use codec::xdata::ExtendedData;
use codec::{CadDocument, DxfVersion, EntityType, Handle, Vector3};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::app::{copy_to_clipboard_kernel, paste_entities_kernel, ClipboardDeps, PasteSpace};
use crate::modules::insert::wblock::extract_entities_into;
use crate::scene::view::dispatch::set_entity_xdata;

use super::project::project_geometry;
use super::refresh::{fatto_rooms, refresh_rooms};
use super::report::Notice;
use super::rooms::{
    measure_at, project_rooms, room_entities, room_text_height, rooms_in, Phase, Room, RoomPart,
};
use super::units::DrawingUnit;
use super::walls::{faces as wall_faces, is_wall_entity, sources_of, wall_set, Region, WallSet};
use super::zone::{zones_in, ZoneGeom};
use super::{APP_ZONA, LAYER_LOCALI, LAYER_LOCALI_PROGETTO};

/// Il layer dei muri nuovi quando il nome dato è vuoto.
const DEFAULT_LAYER: &str = "0";

/// Prefisso degli APPID di ArchLine (`ARCHLINE_ZONA`, `ARCHLINE_LOCALE`).
const APP_PREFIX: &str = "ARCHLINE_";

/// Il tipo nell'avviso quando l'handle non porta a nessuna entità.
const UNKNOWN_KIND: &str = "Entità";

/// Il colore del layer dei locali quando lo si crea (come `zone::ensure_layers`).
const ACI_ROOMS: i16 = 7;

/// Gli elaborati in pianta che si estraggono.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elaborato {
    StatoDiFatto,
    DemolizioniCostruzioni,
    StatoDiProgetto,
}

impl Elaborato {
    /// La parte del nome del file dopo il nome del disegno.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::StatoDiFatto => "stato-di-fatto",
            Self::DemolizioniCostruzioni => "demolizioni-costruzioni",
            Self::StatoDiProgetto => "stato-di-progetto",
        }
    }
}

/// Cosa estrarre e con quali impostazioni.
pub struct ExtractOptions {
    pub elaborati: Vec<Elaborato>,
    /// Layer dei muri nuovi dello stato di progetto, senza badare agli spazi
    /// attorno né alle maiuscole; vuoto vale «0». Può non esistere nel disegno:
    /// si crea nel DWG di progetto.
    pub new_walls_layer: String,
    pub unit: DrawingUnit,
    /// Tolleranza geometrica, in unità del disegno: muri, tagli e geometria di
    /// progetto.
    pub tol: f64,
    /// Tolleranza dei locali, in unità del disegno: `max(unit.tolerance(),
    /// HPGAPTOL)`, la stessa di `AL_LOCALE` e `AL_AGGIORNA` (R61). Sotto `tol`
    /// vale `tol`.
    pub room_tol: f64,
}

/// I documenti estratti, nell'ordine chiesto, e gli avvisi.
pub struct Extracted {
    pub docs: Vec<(Elaborato, CadDocument)>,
    pub notices: Vec<Notice>,
}

/// Un documento per elaborato chiesto (una volta sola, nell'ordine chiesto),
/// più gli avvisi senza doppioni.
///
/// - **fatto**: lo spazio modello senza le zone (ogni entità con un record
///   `ARCHLINE_ZONA`, leggibile o no) e senza i layer `AL-LOCALI-PROGETTO`,
///   con i locali di fatto ricalcolati da capo come in `AL_AGGIORNA`
///   ([`refresh_fatto_rooms`]);
/// - **comparativa**: lo spazio modello senza i layer `AL-LOCALI` e
///   `AL-LOCALI-PROGETTO`, cioè con le zone;
/// - **progetto**: la pianta di [`project_geometry`] — i muri non toccati, i
///   pezzi di quelli tagliati, le chiusure e i muri nuovi (questi sul layer dei
///   muri nuovi, creato se manca: [`ensure_new_walls_layer`]) — più le entità
///   non muro dello spazio modello tranne le zone, i layer `AL-LOCALI*` e le
///   entità escluse (dentro una zona demolita), più i locali di progetto
///   ([`project_room_entities`]) con contorno e testo su `AL-LOCALI`; prima
///   gli avvisi della geometria, poi [`Notice::FuoriPiano`] per i muri fuori
///   piano, che non si tagliano, poi quelli dei locali.
///
/// I tre documenti passano dalla stessa copia ([`copy_out`]). In fondo, se è
/// uscito almeno un documento, un avviso per ogni layout con entità: lo
/// spazio carta non si estrae.
pub fn extract(src: &CadDocument, opts: &ExtractOptions) -> Extracted {
    let mut ex = Extracted {
        docs: Vec::new(),
        notices: Vec::new(),
    };
    let zone_app = src.app_ids.get(APP_ZONA).map(|app| app.handle.value());
    for &elaborato in &opts.elaborati {
        if ex.docs.iter().any(|(done, _)| *done == elaborato) {
            continue;
        }
        let mut work = src.clone();
        let (out, notices) = match elaborato {
            Elaborato::StatoDiFatto => {
                let rooms = refresh_fatto_rooms(&mut work, opts.unit, room_tol(opts));
                push_unique(&mut ex.notices, rooms);
                let handles = model_space_handles(src, |_, entity| {
                    !carries_zone_record(entity, zone_app)
                        && !on_layers(entity, &[LAYER_LOCALI_PROGETTO])
                });
                copy_out(src, &mut work, &handles, None)
            }
            Elaborato::DemolizioniCostruzioni => {
                let handles = model_space_handles(src, |_, entity| {
                    !on_layers(entity, &[LAYER_LOCALI, LAYER_LOCALI_PROGETTO])
                });
                copy_out(src, &mut work, &handles, None)
            }
            Elaborato::StatoDiProgetto => {
                let layer = new_walls_layer_name(src, &opts.new_walls_layer);
                let walls = wall_set(src, opts.tol);
                let zones = zones_in(src);
                let pg = project_geometry(src, &walls, &zones, &layer, opts.tol);
                let wall_handles: FxHashSet<Handle> =
                    walls.entities.iter().map(|(handle, _)| *handle).collect();
                let kept: FxHashSet<Handle> = pg.kept_originals.iter().copied().collect();
                let excluded: FxHashSet<Handle> = pg.excluded.iter().copied().collect();
                let mut handles = model_space_handles(src, |handle, entity| {
                    if wall_handles.contains(&handle) {
                        kept.contains(&handle)
                    } else {
                        !carries_zone_record(entity, zone_app)
                            && !on_rooms_layer(entity)
                            && !excluded.contains(&handle)
                    }
                });
                handles.extend(add_drawn(&mut work, &pg.drawn));
                let (rooms, room_notices) =
                    project_room_entities(src, &walls, &pg.room_entities, &zones, opts);
                handles.extend(add_drawn(&mut work, &rooms));
                push_unique(&mut ex.notices, pg.notices);
                if walls.off_plane > 0 {
                    push_unique(
                        &mut ex.notices,
                        vec![Notice::FuoriPiano {
                            count: walls.off_plane,
                        }],
                    );
                }
                push_unique(&mut ex.notices, room_notices);
                let (mut out, notices) = copy_out(src, &mut work, &handles, Some(&layer));
                if !rooms.is_empty() {
                    ensure_rooms_layer(src, &mut out);
                }
                (out, notices)
            }
        };
        ex.docs.push((elaborato, out));
        push_unique(&mut ex.notices, notices);
    }
    if !ex.docs.is_empty() {
        push_unique(&mut ex.notices, layout_notices(src));
    }
    ex
}

fn push_unique(notices: &mut Vec<Notice>, more: Vec<Notice>) {
    for notice in more {
        if !notices.contains(&notice) {
            notices.push(notice);
        }
    }
}

/// La tolleranza dei locali: `opts.room_tol`, ma mai sotto `opts.tol` (né
/// non numerica).
fn room_tol(opts: &ExtractOptions) -> f64 {
    if opts.room_tol.is_finite() && opts.room_tol > opts.tol {
        opts.room_tol
    } else {
        opts.tol
    }
}

/// Stato di fatto (spec §4.2, §5.5: «l'estrazione ricalcola sempre da capo e
/// non si fida mai di ciò che è scritto»): i locali di fatto di `work` si
/// rimisurano come in `AL_AGGIORNA` ([`refresh_rooms`], alla tolleranza dei
/// locali `room_tol`) e le modifiche vanno sulle loro entità in `work`, con
/// gli stessi handle; `src` non si tocca. Come in `AL_AGGIORNA` restano
/// com'erano, con il loro avviso, i locali non più chiusi e quelli fusi da un
/// vuoto; tutti i locali si ricalcolano, anche quelli su un layer bloccato
/// (l'estratto è un documento nuovo). Gli avvisi del ricalcolo, poi una riga
/// ([`Notice::EtichettaDaAggiornare`]) per ogni locale la cui etichetta
/// nell'estratto non è quella scritta nel disegno di lavoro, che è quindi
/// vecchio. Il solo contorno cambiato non dà righe.
fn refresh_fatto_rooms(work: &mut CadDocument, unit: DrawingUnit, room_tol: f64) -> Vec<Notice> {
    let refresh = refresh_rooms(work, unit, room_tol);
    let mut notices = refresh.notices;
    for change in refresh.changed {
        for (handle, edit) in &change.edits {
            if let Some(entity) = work.get_entity_mut(*handle) {
                edit.apply_to(entity);
            }
        }
        if change.label_changed {
            notices.push(Notice::EtichettaDaAggiornare {
                name: change.name,
                drawing_m2: change.old_area_m2,
                extract_m2: change.new_area_m2,
            });
        }
    }
    notices
}

/// I locali di progetto (spec §5.4) come entità da disegnare nello stato di
/// progetto, con i loro avvisi. Si calcolano con `rooms::project_rooms` sulla
/// geometria di progetto (`pg_room_entities`, con le soglie dei varchi:
/// sorgenti a `opts.tol`, facce alla tolleranza dei locali), dai semi del
/// disegno: un `Room` per locale di fatto (contorno e testo accoppiati per
/// seme, [`fatto_rooms`]) e i semi di progetto. Le regioni di fatto si
/// misurano sul rilievo, alla stessa tolleranza. Ogni locale dà contorno e
/// testo (`rooms::room_entities`) su `AL-LOCALI`, con l'altezza dei testi di
/// `AL_LOCALE` (`room_text_height`); l'XDATA lo toglie [`add_drawn`]. Senza
/// semi niente, nemmeno gli avvisi (un varco senza locali non ha nulla da
/// dire).
fn project_room_entities(
    src: &CadDocument,
    walls: &WallSet,
    pg_room_entities: &[(Handle, EntityType)],
    zones: &[ZoneGeom],
    opts: &ExtractOptions,
) -> (Vec<EntityType>, Vec<Notice>) {
    let (unit, tol) = (opts.unit, room_tol(opts));
    let fatto = fatto_rooms(src, tol);
    let progetto: Vec<Room> = rooms_in(src)
        .into_iter()
        .filter(|(_, room, part)| room.phase == Phase::Progetto && *part == RoomPart::Seme)
        .map(|(_, room, _)| room)
        .collect();
    if fatto.is_empty() && progetto.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let fatto_regions: Vec<Region> = if fatto.is_empty() {
        Vec::new()
    } else {
        let faces = wall_faces(&walls.sources, tol);
        // R47: le facce alla tolleranza di base una volta sola, se servono
        // (come in `refresh_rooms`).
        let base = unit.tolerance();
        let base_faces = (tol > base * (1.0 + 1.0e-9)).then(|| wall_faces(&walls.sources, base));
        fatto
            .iter()
            .filter_map(|room| {
                measure_at(
                    &faces,
                    base_faces.as_deref(),
                    &walls.sources,
                    room.seed,
                    unit,
                    tol,
                )
                .map(|found| found.region)
            })
            .collect()
    };
    let (pg_sources, _) = sources_of(pg_room_entities, opts.tol);
    let pg_faces = wall_faces(&pg_sources, tol);
    let (computed, notices) = project_rooms(
        &pg_faces,
        &pg_sources,
        &fatto,
        &fatto_regions,
        &progetto,
        zones,
        unit,
        tol,
    );
    let text_height = room_text_height(src, unit).0;
    let entities = computed
        .iter()
        .flat_map(|room| room_entities(room, Phase::Progetto, &pg_sources, text_height, tol))
        .collect();
    (entities, notices)
}

/// Il layer dei locali in `out` (stato di progetto). La copia lo porta con le
/// entità che lo usano quando `src` lo ha; altrimenti (un disegno con i soli
/// semi, o il layer tolto) lo si crea qui con un handle nuovo, da quello di
/// `src` se c'è, o come `zone::ensure_layers` (colore 7). Senza, il DWG
/// metterebbe i locali sul layer «0».
fn ensure_rooms_layer(src: &CadDocument, out: &mut CadDocument) {
    if out.layers.contains(LAYER_LOCALI) {
        return;
    }
    let mut layer = Layer::new(LAYER_LOCALI);
    match src.layers.get(LAYER_LOCALI) {
        Some(from) => {
            layer.color = from.color;
            layer.color_name = from.color_name.clone();
            layer.book_name = from.book_name.clone();
            layer.line_type = from.line_type.clone();
            layer.line_weight = from.line_weight;
        }
        None => layer.color = codec::Color::from_index(ACI_ROOMS),
    }
    let line_type = layer.line_type.clone();
    layer.handle = out.allocate_handle();
    let _ = out.layers.add(layer);
    copy_missing_linetypes(src, out, vec![line_type]);
}

/// L'entità porta un record `ARCHLINE_ZONA` (nome senza distinguere le
/// maiuscole, come la tabella degli APPID), anche illeggibile o solo come
/// blocco grezzo letto da un DWG (`zone_app` = handle dell'APPID)?
fn carries_zone_record(entity: &EntityType, zone_app: Option<u64>) -> bool {
    let xdata = &entity.common().extended_data;
    xdata
        .records()
        .iter()
        .any(|record| record.application_name.eq_ignore_ascii_case(APP_ZONA))
        || zone_app.is_some_and(|app| xdata.raw_dwg_eed.iter().any(|(raw, _)| *raw == app))
}

/// Un avviso per ogni layout (blocco `*Paper_Space*`) con entità, a parte il
/// viewport generale (id 1) che ogni layout ha: `kind` = `"Layout <nome>"`,
/// `handle` = il record del blocco.
fn layout_notices(doc: &CadDocument) -> Vec<Notice> {
    doc.block_records
        .iter()
        .filter(|record| record.is_paper_space())
        .filter(|record| {
            record
                .entity_handles
                .iter()
                .filter_map(|&handle| doc.get_entity(handle))
                .any(|entity| !matches!(entity, EntityType::Viewport(v) if v.id == 1))
        })
        .map(|record| Notice::NonCopiata {
            handle: record.handle,
            kind: format!("Layout {}", layout_name(doc, record)),
        })
        .collect()
}

/// Il nome del layout del blocco di carta: l'oggetto `LAYOUT` che lo indica,
/// altrimenti il nome del blocco.
fn layout_name(doc: &CadDocument, record: &BlockRecord) -> String {
    doc.objects
        .values()
        .find_map(|object| match object {
            ObjectType::Layout(layout) if layout.block_record == record.handle => {
                Some(layout.name.clone())
            }
            _ => None,
        })
        .or_else(|| match doc.objects.get(&record.layout) {
            Some(ObjectType::Layout(layout)) => Some(layout.name.clone()),
            _ => None,
        })
        .unwrap_or_else(|| record.name.clone())
}

/// Gli handle dello spazio modello, in ordine di disegno, che `keep` accetta.
/// Un handle senza entità resta: la copia non lo restituirà e finirà tra le
/// non copiate.
fn model_space_handles(
    doc: &CadDocument,
    keep: impl Fn(Handle, &EntityType) -> bool,
) -> Vec<Handle> {
    doc.block_records
        .iter()
        .filter(|record| record.is_model_space())
        .flat_map(|record| record.entity_handles.iter().copied())
        .filter(|&handle| {
            doc.get_entity(handle)
                .map_or(true, |entity| keep(handle, entity))
        })
        .collect()
}

/// L'entità sta su uno dei layer dati? Come tutti i nomi dei layer, senza
/// distinguere le maiuscole.
fn on_layers(entity: &EntityType, layers: &[&str]) -> bool {
    let layer = &entity.common().layer;
    layers.iter().any(|name| layer.eq_ignore_ascii_case(name))
}

/// L'entità sta su un layer `AL-LOCALI*` (locali di fatto e di progetto)?
fn on_rooms_layer(entity: &EntityType) -> bool {
    entity
        .common()
        .layer
        .get(..LAYER_LOCALI.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(LAYER_LOCALI))
}

/// Il layer dei muri nuovi: il nome senza spazi attorno («0» se vuoto), scritto
/// come nella tabella dei layer di `src` se c'è.
fn new_walls_layer_name(src: &CadDocument, name: &str) -> String {
    let name = match name.trim() {
        "" => DEFAULT_LAYER,
        name => name,
    };
    src.layers
        .get(name)
        .map_or_else(|| name.to_string(), |layer| layer.name.clone())
}

/// Le entità disegnate dalla geometria di progetto (pezzi, chiusure, muri
/// nuovi) entrano in `work`, mai in `src`, come entità nuove dello spazio
/// modello: handle nuovi, layer risolto per nome in `work` (nessuno se manca:
/// lo crea [`ensure_new_walls_layer`] nel documento d'uscita) e nessun XDATA,
/// nemmeno di altre applicazioni (quello dei pezzi era dell'entità intera).
/// Restituisce i loro handle, nell'ordine.
fn add_drawn(work: &mut CadDocument, drawn: &[EntityType]) -> Vec<Handle> {
    let model = work
        .block_records
        .iter()
        .find(|record| record.is_model_space())
        .map(|record| record.handle);
    drawn
        .iter()
        .cloned()
        .filter_map(|mut entity| {
            let layer_handle = work
                .layers
                .get(&entity.common().layer)
                .map(|layer| layer.handle)
                .filter(|handle| !handle.is_null());
            let common = entity.common_mut();
            common.handle = Handle::NULL;
            common.layer_handle = layer_handle;
            common.extended_data = ExtendedData::new();
            if let Some(model) = model {
                common.owner_handle = model;
            }
            work.add_entity(entity).ok()
        })
        .collect()
}

/// Il layer di `src` da cui il layer dei muri nuovi `name` prende colore, tipo
/// di linea e spessore quando il documento d'uscita non lo ha: l'omonimo, se
/// `src` lo ha; altrimenti (spec §5.5, «con colore e tipo di linea dei muri
/// esistenti»; R46) il layer più frequente dei muri,
/// [`propose_new_walls_layer`]. `None` senza né l'uno né l'altro: valori
/// predefiniti.
pub fn new_walls_layer_template<'a>(src: &'a CadDocument, name: &str) -> Option<&'a Layer> {
    src.layers
        .get(name)
        .or_else(|| propose_new_walls_layer(src).and_then(|walls| src.layers.get(&walls)))
}

/// Il layer dei muri nuovi in `out`. La copia lo porta quando un'entità
/// copiata lo usa; altrimenti (nessun muro nuovo, o un layer che in `src` non
/// c'è) lo si crea qui, con un handle nuovo e con colore, tipo di linea e
/// spessore di linea di [`new_walls_layer_template`] (l'omonimo di `src` o il
/// layer più frequente dei muri), o con i valori predefiniti se non c'è.
/// Senza, il DWG metterebbe i muri nuovi sul layer «0». Il tipo di linea, se
/// manca, lo porta poi [`copy_layer_linetypes`].
fn ensure_new_walls_layer(src: &CadDocument, out: &mut CadDocument, name: &str) {
    if out.layers.contains(name) {
        return;
    }
    let mut layer = match new_walls_layer_template(src, name) {
        Some(from) => {
            let mut layer = Layer::new(name);
            layer.color = from.color;
            layer.color_name = from.color_name.clone();
            layer.book_name = from.book_name.clone();
            layer.line_type = from.line_type.clone();
            layer.line_weight = from.line_weight;
            layer
        }
        None => Layer::new(name),
    };
    layer.handle = out.allocate_handle();
    let _ = out.layers.add(layer);
}

/// Passi 3-6 per gli handle scelti in `work` (copia di `src`): fuori ciò che
/// la copia non porta intero, via i nostri record, copia in un documento nuovo
/// con versione, unità e impostazioni di `src`, il layer dei muri nuovi se
/// dato, tipi di linea dei layer, avvisi per ciò che non si copia (nell'ordine
/// degli handle).
fn copy_out(
    src: &CadDocument,
    work: &mut CadDocument,
    handles: &[Handle],
    new_walls_layer: Option<&str>,
) -> (CadDocument, Vec<Notice>) {
    let left_out: FxHashSet<Handle> = {
        let mut portable = Portability::new(work);
        handles
            .iter()
            .copied()
            .filter(|&handle| {
                work.get_entity(handle)
                    .is_some_and(|entity| !portable.entity(entity))
            })
            .collect()
    };
    let handles_in: Vec<Handle> = handles
        .iter()
        .copied()
        .filter(|handle| !left_out.contains(handle))
        .collect();
    strip_our_xdata(work, &handles_in);

    let mut out = CadDocument::new();
    out.version = output_version(src.version);
    // L'errore dice solo che non c'era nulla da copiare (nessun handle, o
    // nessuno copiabile): il documento resta vuoto e gli avvisi sotto
    // elencano gli handle non copiati.
    let _ = extract_entities_into(work, &handles_in, &mut out);
    if let Some(name) = new_walls_layer {
        ensure_new_walls_layer(src, &mut out, name);
    }
    out.header.insertion_units = src.header.insertion_units;
    copy_drawing_settings(src, &mut out);
    copy_mline_styles(src, &mut out);
    copy_inserted_blocks(src, &mut out);
    copy_dim_style_references(src, &mut out);
    copy_layer_linetypes(src, &mut out);
    copy_complex_linetype_styles(src, &mut out);
    copy_layer_plot_styles(src, &mut out);
    scrub_our_data(&mut out);

    // La copia di WBLOCK passa a `copy_to_clipboard_kernel` gli handle senza
    // `BLOCK`/`ENDBLK`; ciò che il kernel non restituisce non è nel documento.
    let copyable: Vec<Handle> = handles_in
        .iter()
        .copied()
        .filter(|&handle| {
            !matches!(
                work.get_entity(handle),
                Some(EntityType::Block(_) | EntityType::BlockEnd(_))
            )
        })
        .collect();
    let (copied, _) = copy_to_clipboard_kernel(work, &copyable);
    let copied: FxHashSet<Handle> = copied.iter().map(|e| e.common().handle).collect();
    let notices = handles
        .iter()
        .filter(|handle| !copied.contains(handle))
        .map(|&handle| Notice::NonCopiata {
            handle,
            kind: work
                .get_entity(handle)
                .map_or(UNKNOWN_KIND, |entity| entity.as_entity().entity_type())
                .to_string(),
        })
        .collect();
    (out, notices)
}

/// Che cosa la copia di WBLOCK porta intero (spec §5.5: immagini, riferimenti
/// esterni e 3D si elencano, non si copiano).
///
/// Non si portano:
/// - le immagini raster, gli underlay (PDF/DWF/DGN) e gli OLE, che rimandano a
///   definizioni o dati che la copia non prende;
/// - le entità che rimandano per handle a stili che la copia non porta e non
///   rimappa: multileader (stile, stile di testo, blocchi delle frecce e del
///   contenuto), tabelle (stile, blocco `*T`, stili delle celle), tolleranze
///   (stile di quota), forme (file di forme) e simboli di sezione (stile di
///   vista); una multilinea il cui stile manca anche in `src` (le altre
///   portano il loro stile: `copy_mline_styles`);
/// - i solidi 3D (`3DSOLID`, `REGION`, `BODY`, `SURFACE`, `MESH`);
/// - un `INSERT` di un riferimento esterno, di un blocco che manca o di un
///   blocco che contiene (anche in un blocco annidato) una delle entità sopra,
///   3D escluso: i solidi 3D dentro un blocco restano, la copia li porta interi.
struct Portability<'a> {
    doc: &'a CadDocument,
    /// Blocco (nome in maiuscolo) → si copia intero.
    blocks: FxHashMap<String, bool>,
}

impl<'a> Portability<'a> {
    fn new(doc: &'a CadDocument) -> Self {
        Self {
            doc,
            blocks: FxHashMap::default(),
        }
    }

    /// L'entità dello spazio modello si copia intera?
    fn entity(&mut self, entity: &EntityType) -> bool {
        !matches!(
            entity,
            EntityType::Solid3D(_)
                | EntityType::Region(_)
                | EntityType::Body(_)
                | EntityType::Surface(_)
                | EntityType::Mesh(_)
        ) && self.member(entity)
    }

    /// Lo stesso per un'entità dentro un blocco, dove il 3D resta.
    fn member(&mut self, entity: &EntityType) -> bool {
        match entity {
            EntityType::RasterImage(_)
            | EntityType::Underlay(_)
            | EntityType::Ole2Frame(_)
            | EntityType::MultiLeader(_)
            | EntityType::Table(_)
            | EntityType::Tolerance(_)
            | EntityType::Shape(_)
            | EntityType::SectionSymbol(_) => false,
            EntityType::MLine(mline) => mline_style(self.doc, mline).is_some(),
            EntityType::Insert(insert) => self.block(&insert.block_name),
            _ => true,
        }
    }

    fn block(&mut self, name: &str) -> bool {
        let key = name.to_ascii_uppercase();
        if let Some(&known) = self.blocks.get(&key) {
            return known;
        }
        let doc = self.doc;
        let Some(record) = doc.block_records.get(name) else {
            self.blocks.insert(key, false);
            return false;
        };
        if record.flags.is_xref || record.flags.is_xref_overlay {
            self.blocks.insert(key, false);
            return false;
        }
        // In corso: un blocco che contiene sé stesso non si giudica due volte.
        self.blocks.insert(key.clone(), true);
        let whole = record
            .entity_handles
            .iter()
            .filter_map(|&handle| doc.get_entity(handle))
            .all(|entity| self.member(entity));
        self.blocks.insert(key, whole);
        whole
    }
}

/// Le impostazioni del disegno che un documento nuovo avrebbe ai valori
/// predefiniti (solo valori, mai handle): LTSCALE, PSLTSCALE, CELTSCALE,
/// MEASUREMENT, LUNITS, LUPREC, AUNITS, AUPREC, TEXTSIZE, DIMSCALE; colore,
/// tipo di linea, spessore e stato del layer «0»; carattere, altezza,
/// larghezza e inclinazione dello stile di testo «Standard». Il tipo di linea
/// del layer «0», se manca, lo aggiunge [`copy_layer_linetypes`].
fn copy_drawing_settings(src: &CadDocument, out: &mut CadDocument) {
    let (from, to) = (&src.header, &mut out.header);
    to.linetype_scale = from.linetype_scale;
    to.paper_space_linetype_scaling = from.paper_space_linetype_scaling;
    to.current_entity_linetype_scale = from.current_entity_linetype_scale;
    to.measurement = from.measurement;
    to.linear_unit_format = from.linear_unit_format;
    to.linear_unit_precision = from.linear_unit_precision;
    to.angular_unit_format = from.angular_unit_format;
    to.angular_unit_precision = from.angular_unit_precision;
    to.text_height = from.text_height;
    to.dim_scale = from.dim_scale;

    if let (Some(from), Some(to)) = (src.layers.get("0"), out.layers.get_mut("0")) {
        to.color = from.color;
        to.color_name = from.color_name.clone();
        to.book_name = from.book_name.clone();
        to.line_type = from.line_type.clone();
        to.line_weight = from.line_weight;
        to.flags = from.flags;
    }
    if let (Some(from), Some(to)) = (
        src.text_styles.get("Standard"),
        out.text_styles.get_mut("Standard"),
    ) {
        to.font_file = from.font_file.clone();
        to.big_font_file = from.big_font_file.clone();
        to.true_type_font = from.true_type_font.clone();
        to.true_type_font_flags = from.true_type_font_flags;
        to.height = from.height;
        to.width_factor = from.width_factor;
        to.oblique_angle = from.oblique_angle;
    }
}

/// La versione del disegno di partenza, se sta tra AC1014 (R14) e AC1032
/// (2018); altrimenti AC1032.
fn output_version(version: DxfVersion) -> DxfVersion {
    if (DxfVersion::AC1014..=DxfVersion::AC1032).contains(&version) {
        version
    } else {
        DxfVersion::AC1032
    }
}

/// APPID di ArchLine (`ARCHLINE_*`, senza distinguere le maiuscole come la
/// tabella degli APPID).
fn is_ours(app: &str) -> bool {
    app.get(..APP_PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(APP_PREFIX))
}

/// Passo 3: toglie da `handles` i record `ARCHLINE_ZONA`, `ARCHLINE_LOCALE`
/// (e ogni altro `ARCHLINE_*`), compresi i blocchi grezzi letti da un DWG.
/// Fatto prima della copia, né i record né i loro APPID passano in `out`.
fn strip_our_xdata(work: &mut CadDocument, handles: &[Handle]) {
    let our_apps: Vec<(String, u64)> = work
        .app_ids
        .iter()
        .filter(|app| is_ours(&app.name))
        .map(|app| (app.name.clone(), app.handle.value()))
        .collect();
    for &handle in handles {
        let Some(entity) = work.get_entity(handle) else {
            continue;
        };
        let xdata = &entity.common().extended_data;
        let mut apps: Vec<String> = xdata
            .records()
            .iter()
            .map(|record| record.application_name.clone())
            .filter(|name| is_ours(name))
            .collect();
        apps.extend(
            our_apps
                .iter()
                .filter(|(_, app)| xdata.raw_dwg_eed.iter().any(|(raw, _)| raw == app))
                .map(|(name, _)| name.clone()),
        );
        apps.sort();
        apps.dedup();
        for app in apps {
            set_entity_xdata(work, handle, &app, None);
        }
    }
}

/// Dopo la copia: le entità delle definizioni di blocco copiate come
/// dipendenze (e gli attributi) possono ancora portare XDATA nostro, e con
/// esso il suo APPID. Via record, blocchi grezzi e APPID `ARCHLINE_*`.
fn scrub_our_data(out: &mut CadDocument) {
    let our_apps: Vec<(String, u64)> = out
        .app_ids
        .iter()
        .filter(|app| is_ours(&app.name))
        .map(|app| (app.name.clone(), app.handle.value()))
        .collect();
    let raw_apps: FxHashSet<u64> = our_apps.iter().map(|(_, handle)| *handle).collect();
    let scrub = |xdata: &mut ExtendedData| {
        let names: Vec<String> = xdata
            .records()
            .iter()
            .map(|record| record.application_name.clone())
            .filter(|name| is_ours(name))
            .collect();
        for name in names {
            xdata.remove_record(&name);
        }
        xdata.raw_dwg_eed.retain(|(app, _)| !raw_apps.contains(app));
    };
    for entity in out.entities_mut() {
        scrub(&mut entity.common_mut().extended_data);
        if let EntityType::Insert(insert) = entity {
            for attribute in &mut insert.attributes {
                scrub(&mut attribute.common.extended_data);
            }
        }
    }
    for (name, _) in our_apps {
        out.app_ids.remove(&name);
    }
}

/// Passo 5: `ClipboardDeps` copia i tipi di linea delle entità, non quelli dei
/// layer. Un layer di `out` con un tipo di linea che `out` non ha lo riceve da
/// `src`, con un handle nuovo.
fn copy_layer_linetypes(src: &CadDocument, out: &mut CadDocument) {
    let names: Vec<String> = out
        .layers
        .iter()
        .map(|layer| layer.line_type.clone())
        .collect();
    copy_missing_linetypes(src, out, names);
}

/// Lo stile di una multilinea in `doc`: per handle, altrimenti per nome.
fn mline_style<'a>(doc: &'a CadDocument, mline: &MLine) -> Option<&'a MLineStyle> {
    let by_handle = mline.style_handle.and_then(|h| match doc.objects.get(&h) {
        Some(ObjectType::MLineStyle(style)) => Some(style),
        _ => None,
    });
    by_handle.or_else(|| {
        doc.objects.values().find_map(|object| match object {
            ObjectType::MLineStyle(style) if style.name.eq_ignore_ascii_case(&mline.style_name) => {
                Some(style)
            }
            _ => None,
        })
    })
}

/// R30: `ClipboardDeps` non porta gli stili di multilinea, e la copia lascia
/// alle multilinee l'handle dello stile di `src`. Ogni stile usato (anche dalle
/// multilinee dentro i blocchi copiati) va in `out`, nel dizionario
/// `ACAD_MLINESTYLE` con un handle nuovo; uno stile con lo stesso nome (lo
/// «Standard» del documento nuovo) prende i valori di `src` e tiene il suo
/// handle. Poi i tipi di linea degli elementi, se mancano. Le multilinee il
/// cui stile manca anche in `src` non arrivano qui (`Portability`).
fn copy_mline_styles(src: &CadDocument, out: &mut CadDocument) {
    let styles: Vec<MLineStyle> = out
        .entities()
        .filter_map(|entity| match entity {
            EntityType::MLine(mline) => mline_style(src, mline).cloned(),
            _ => None,
        })
        .collect();
    if styles.is_empty() {
        return;
    }
    let dict = out.header.acad_mlinestyle_dict_handle;
    let mut placed: FxHashMap<String, Handle> = FxHashMap::default();
    let mut linetypes = Vec::new();
    for mut style in styles {
        let key = style.name.to_ascii_uppercase();
        if placed.contains_key(&key) {
            continue;
        }
        let Some(ObjectType::Dictionary(entries)) = out.objects.get(&dict) else {
            // Un documento nuovo il dizionario lo ha sempre; senza, la
            // multilinea prende lo stile corrente di `out` al salvataggio.
            placed.insert(key, Handle::NULL);
            continue;
        };
        let existing = entries
            .entries
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&style.name))
            .map(|(_, handle)| *handle);
        let handle = existing.unwrap_or_else(|| out.allocate_handle());
        if existing.is_none() {
            if let Some(ObjectType::Dictionary(entries)) = out.objects.get_mut(&dict) {
                entries.add_entry(&style.name, handle);
            }
        }
        style.handle = handle;
        style.owner = dict;
        linetypes.extend(style.elements.iter().map(|e| e.linetype.clone()));
        out.objects.insert(handle, ObjectType::MLineStyle(style));
        placed.insert(key, handle);
    }
    for entity in out.entities_mut() {
        if let EntityType::MLine(mline) = entity {
            let Some(style) = mline_style(src, mline) else {
                continue;
            };
            let handle = placed
                .get(&style.name.to_ascii_uppercase())
                .copied()
                .filter(|handle| !handle.is_null());
            mline.style_name = style.name.clone();
            mline.style_handle = handle;
        }
    }
    copy_missing_linetypes(src, out, linetypes);
}

/// I tipi di linea nominati che `out` non ha, copiati da `src` con un handle
/// nuovo.
fn copy_missing_linetypes(src: &CadDocument, out: &mut CadDocument, names: Vec<String>) {
    for name in names {
        if name.is_empty() {
            continue;
        }
        if out.line_types.contains(&name) {
            continue;
        }
        let Some(line_type) = src.line_types.get(&name) else {
            continue;
        };
        let mut line_type = line_type.clone();
        line_type.handle = out.allocate_handle();
        let _ = out.line_types.add(line_type);
    }
}

/// Le definizioni di blocco di `src` nominate che `out` non ha, con i blocchi
/// annidati e i layer, i tipi di linea e gli stili dei loro membri: la stessa
/// unione della copia di WBLOCK (`ClipboardDeps` di un `INSERT` di prova,
/// incollato senza entità).
fn merge_blocks(src: &CadDocument, out: &mut CadDocument, names: Vec<String>) {
    let probes: Vec<EntityType> = names
        .into_iter()
        .filter(|name| {
            !name.is_empty()
                && out.block_records.get(name).is_none()
                && src.block_records.get(name).is_some()
        })
        .map(|name| EntityType::Insert(Insert::new(name, Vector3::ZERO)))
        .collect();
    if probes.is_empty() {
        return;
    }
    let deps = ClipboardDeps::capture(src, &probes);
    paste_entities_kernel(out, &[], &deps, None, PasteSpace::Model);
}

/// I blocchi `*D` delle quote si copiano con le loro entità, ma i blocchi che
/// quelle inseriscono (le frecce, nei file di AutoCAD) no: ogni `INSERT` di
/// `out`, anche dentro un blocco, che nomina un blocco mancante lo riceve da
/// `src`.
fn copy_inserted_blocks(src: &CadDocument, out: &mut CadDocument) {
    let missing: Vec<String> = out
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Insert(insert) if out.block_records.get(&insert.block_name).is_none() => {
                Some(insert.block_name.clone())
            }
            _ => None,
        })
        .collect();
    merge_blocks(src, out, missing);
}

/// Lo stile di testo `style` di `src` in `out`: l'omonimo, oppure, per gli
/// stili senza nome dei file di forme, quello con lo stesso file; se manca,
/// una copia con un handle nuovo.
fn ensure_text_style(out: &mut CadDocument, style: &TextStyle) -> Option<Handle> {
    let found = if style.name.is_empty() {
        out.text_styles
            .iter()
            .find(|s| s.name.is_empty() && s.font_file.eq_ignore_ascii_case(&style.font_file))
    } else {
        out.text_styles.get(&style.name)
    };
    if let Some(found) = found {
        return Some(found.handle);
    }
    let mut copy = style.clone();
    copy.handle = out.allocate_handle();
    copy.xref_block_record_handle = Handle::NULL;
    let handle = copy.handle;
    if copy.name.is_empty() {
        out.text_styles.add_allow_duplicate(copy);
    } else {
        out.text_styles.add(copy).ok()?;
    }
    Some(handle)
}

/// Gli stili di quota arrivano in `out` per nome (`ClipboardDeps`) ma con gli
/// handle di `src` dentro, e uno stile che `out` ha già (lo «Standard») non
/// arriva affatto. Ogni stile di `out` con un omonimo in `src` ne prende i
/// valori (tiene il suo handle); stile di testo (DIMTXSTY), blocchi delle
/// frecce (DIMBLK, DIMBLK1, DIMBLK2, DIMLDRBLK) e tipi di linea (DIMLTYPE,
/// DIMLTEX1, DIMLTEX2) si risolvono in `src` per handle (o per nome), si
/// portano in `out` se mancano e si ripuntano; ciò che in `src` non si
/// risolve resta nullo (il predefinito).
fn copy_dim_style_references(src: &CadDocument, out: &mut CadDocument) {
    let styles: Vec<DimStyle> = out
        .dim_styles
        .iter()
        .filter_map(|style| src.dim_styles.get(&style.name).cloned())
        .collect();
    if styles.is_empty() {
        return;
    }
    let text_style = |s: &DimStyle| {
        src.text_styles
            .iter()
            .find(|t| !s.dimtxsty_handle.is_null() && t.handle == s.dimtxsty_handle)
            .or_else(|| {
                (!s.dimtxsty.is_empty())
                    .then(|| src.text_styles.get(&s.dimtxsty))
                    .flatten()
            })
            .cloned()
    };
    let block = |handle: Handle, name: &str| {
        src.block_records
            .iter()
            .find(|r| !handle.is_null() && r.handle == handle)
            .map(|r| r.name.clone())
            .or_else(|| (!name.is_empty()).then(|| name.to_string()))
    };
    let blocks = |s: &DimStyle| {
        [
            block(s.dimblk, &s.dimblk_name),
            block(s.dimblk1, &s.dimblk1_name),
            block(s.dimblk2, &s.dimblk2_name),
            block(s.dimldrblk, ""),
        ]
    };
    let line_types = |s: &DimStyle| {
        [s.dimltex_handle, s.dimltex1_handle, s.dimltex2_handle].map(|handle| {
            src.line_types
                .iter()
                .find(|l| !handle.is_null() && l.handle == handle)
                .map(|l| l.name.clone())
        })
    };

    merge_blocks(src, out, styles.iter().flat_map(blocks).flatten().collect());
    copy_missing_linetypes(
        src,
        out,
        styles.iter().flat_map(line_types).flatten().collect(),
    );
    for from in styles {
        let text = text_style(&from);
        let text_handle = text.as_ref().and_then(|t| ensure_text_style(out, t));
        let [blk, blk1, blk2, ldr] = blocks(&from).map(|name| {
            name.and_then(|n| out.block_records.get(&n).map(|r| r.handle))
                .unwrap_or(Handle::NULL)
        });
        let [lt, lt1, lt2] = line_types(&from).map(|name| {
            name.and_then(|n| out.line_types.get(&n).map(|l| l.handle))
                .unwrap_or(Handle::NULL)
        });
        let name = from.name.clone();
        let Some(to) = out.dim_styles.get_mut(&name) else {
            continue;
        };
        let (handle, xref) = (to.handle, to.xref_handle);
        *to = from;
        to.handle = handle;
        to.xref_handle = xref;
        to.dimtxsty_handle = text_handle.unwrap_or(Handle::NULL);
        if let Some(text) = text {
            to.dimtxsty = text.name;
        }
        (to.dimblk, to.dimblk1, to.dimblk2, to.dimldrblk) = (blk, blk1, blk2, ldr);
        (to.dimltex_handle, to.dimltex1_handle, to.dimltex2_handle) = (lt, lt1, lt2);
    }
}

/// Un tipo di linea complesso (testo o forme) nomina il suo stile di testo per
/// handle, che resta quello di `src` in ogni copia del record. Per ogni tipo
/// di linea di `out` con un omonimo in `src`, lo stile di ogni elemento
/// complesso si porta in `out` e si ripunta (nullo se in `src` non si
/// risolve).
fn copy_complex_linetype_styles(src: &CadDocument, out: &mut CadDocument) {
    let plan: Vec<(String, Vec<(usize, Option<TextStyle>)>)> = out
        .line_types
        .iter()
        .filter_map(|line_type| {
            let from = src.line_types.get(&line_type.name)?;
            let elements: Vec<_> = from
                .elements
                .iter()
                .enumerate()
                .filter_map(|(index, element)| {
                    let complex = element.complex.as_ref()?;
                    let style = src
                        .text_styles
                        .iter()
                        .find(|s| {
                            !complex.style_handle.is_null() && s.handle == complex.style_handle
                        })
                        .cloned();
                    Some((index, style))
                })
                .collect();
            (!elements.is_empty()).then(|| (line_type.name.clone(), elements))
        })
        .collect();
    for (name, elements) in plan {
        let handles: Vec<(usize, Handle)> = elements
            .into_iter()
            .map(|(index, style)| {
                let handle = style.and_then(|s| ensure_text_style(out, &s));
                (index, handle.unwrap_or(Handle::NULL))
            })
            .collect();
        let Some(line_type) = out.line_types.get_mut(&name) else {
            continue;
        };
        for (index, handle) in handles {
            if let Some(complex) = line_type
                .elements
                .get_mut(index)
                .and_then(|element| element.complex.as_mut())
            {
                complex.style_handle = handle;
            }
        }
    }
}

/// Le voci del dizionario `ACAD_PLOTSTYLENAME` (dall'intestazione o dal
/// dizionario radice).
fn plot_style_entries(doc: &CadDocument) -> Vec<(String, Handle)> {
    let root = || match doc.objects.get(&doc.header.named_objects_dict_handle) {
        Some(ObjectType::Dictionary(root)) => root.get("ACAD_PLOTSTYLENAME"),
        _ => None,
    };
    let handle = Some(doc.header.acad_plotstylename_dict_handle)
        .filter(|h| doc.objects.contains_key(h))
        .or_else(root);
    match handle.and_then(|h| doc.objects.get(&h)) {
        Some(ObjectType::Dictionary(d)) => d.entries.clone(),
        Some(ObjectType::DictionaryWithDefault(d)) => d.entries.clone(),
        _ => Vec::new(),
    }
}

/// `Layer.plotstyle_handle` arriva con l'handle di `src`: si ripunta per nome
/// allo stile di stampa omonimo di `out`, o a nulla se `out` non lo ha.
fn copy_layer_plot_styles(src: &CadDocument, out: &mut CadDocument) {
    let src_names: FxHashMap<Handle, String> = plot_style_entries(src)
        .into_iter()
        .map(|(name, handle)| (handle, name))
        .collect();
    let out_entries = plot_style_entries(out);
    let layers: Vec<String> = out.layers.iter().map(|l| l.name.clone()).collect();
    for name in layers {
        let Some(from) = src.layers.get(&name) else {
            continue;
        };
        let style = Some(from.plot_style.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| src_names.get(&from.plotstyle_handle).cloned());
        let handle = style
            .and_then(|style| {
                out_entries
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(&style))
                    .map(|(_, handle)| *handle)
            })
            .unwrap_or(Handle::NULL);
        if let Some(to) = out.layers.get_mut(&name) {
            to.plotstyle_handle = handle;
        }
    }
}

/// Il layer da proporre per i muri nuovi (spec §5.5): il più frequente tra le
/// entità muro dello spazio modello ([`is_wall_entity`]), senza distinguere le
/// maiuscole come tutti i nomi dei layer, scritto come nella tabella dei layer
/// (o come la prima entità, se la tabella non lo ha). A parità vince l'ordine
/// alfabetico. `None` senza entità muro.
pub fn propose_new_walls_layer(doc: &CadDocument) -> Option<String> {
    // Nome in maiuscolo → (nome da mostrare, entità).
    let mut counts: FxHashMap<String, (String, usize)> = FxHashMap::default();
    for handle in model_space_handles(doc, |_, entity| is_wall_entity(doc, entity)) {
        let Some(entity) = doc.get_entity(handle) else {
            continue;
        };
        let layer = &entity.common().layer;
        if layer.trim().is_empty() {
            continue;
        }
        let entry = counts.entry(layer.to_uppercase()).or_insert_with(|| {
            let name = doc
                .layers
                .get(layer)
                .map_or_else(|| layer.clone(), |found| found.name.clone());
            (name, 0)
        });
        entry.1 += 1;
    }
    counts
        .into_iter()
        .max_by(|(a_key, (_, a)), (b_key, (_, b))| a.cmp(b).then_with(|| b_key.cmp(a_key)))
        .map(|(_, (name, _))| name)
}

/// `"{base}_{suffisso}.dwg"`.
pub fn file_name(base: &str, e: Elaborato) -> String {
    format!("{base}_{}.dwg", e.suffix())
}

#[derive(Debug, PartialEq)]
pub enum WriteError {
    /// I file che esistono già: non si è scritto nulla.
    Exists(Vec<PathBuf>),
    /// Errore prima di scrivere il primo file: non si è scritto nulla.
    Io(String),
    /// Errore dopo aver scritto `written`, che restano sul disco; `message`
    /// dice quale file non si è scritto e perché.
    Partial {
        written: Vec<PathBuf>,
        message: String,
    },
}

/// Scrive un DWG per documento in `dir`, con il nome di [`file_name`] e la
/// versione del documento. Senza `overwrite`, se anche uno solo dei file
/// esiste non scrive nulla e restituisce tutti quelli che esistono. Un errore
/// di scrittura ferma il giro: al primo file è `Io`, dopo è `Partial` con i
/// file già scritti, che restano (ognuno è scritto accanto e poi sostituito,
/// quindi mai a metà).
pub fn write(
    ex: &Extracted,
    dir: &Path,
    base: &str,
    overwrite: bool,
) -> Result<Vec<PathBuf>, WriteError> {
    let paths: Vec<PathBuf> = ex
        .docs
        .iter()
        .map(|(elaborato, _)| dir.join(file_name(base, *elaborato)))
        .collect();
    if !overwrite {
        let mut existing = Vec::new();
        for path in &paths {
            if path
                .try_exists()
                .map_err(|e| WriteError::Io(format!("{}: {e}", path.display())))?
            {
                existing.push(path.clone());
            }
        }
        if !existing.is_empty() {
            return Err(WriteError::Exists(existing));
        }
    }
    for (done, ((_, doc), path)) in ex.docs.iter().zip(&paths).enumerate() {
        if let Err(e) = crate::io::save_as_version(doc, path, doc.version) {
            let message = format!("{}: {e}", path.display());
            return Err(match done {
                0 => WriteError::Io(message),
                _ => WriteError::Partial {
                    written: paths[..done].to_vec(),
                    message,
                },
            });
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::{Insert, LwPolyline, MText};
    use codec::objects::ObjectType;
    use codec::tables::{Layer, LineType};
    use codec::xdata::{ExtendedDataRecord, XDataValue};
    use codec::{Color, DxfVersion, EntityType, Handle, Vector2, Vector3};

    use crate::bim::fixtures::{case_a, Form, LAYER_MURI};
    use crate::bim::record::{self, ProjectRecord};
    use crate::bim::rooms::{
        measure_at, room_entities, room_text_height, seed_entity, Computed, Phase, Room,
    };
    use crate::bim::walls::{
        faces as wall_faces, rect_from_axis, rect_from_axis_thickness, wall_set,
    };
    use crate::bim::zone::{ensure_layers, zone_hatch, Zone, ZoneOrigin, ZoneState};
    use crate::bim::{
        APP_LOCALE, APP_ZONA, LAYER_LOCALI, LAYER_LOCALI_PROGETTO, MAX_WALL_THICKNESS_M,
        RECORD_PROGETTO,
    };

    const M: DrawingUnit = DrawingUnit::Metri;

    fn opts(e: &[Elaborato]) -> ExtractOptions {
        ExtractOptions {
            elaborati: e.to_vec(),
            new_walls_layer: "MURI".into(),
            unit: DrawingUnit::Metri,
            tol: 1e-6,
            room_tol: 1e-6,
        }
    }

    /// Il caso A in metri (INSUNITS 6) nella forma data, con i layer `AL-*`.
    fn case_a_doc(form: Form) -> CadDocument {
        case_a_doc_in(form, M)
    }

    /// Il caso A nell'unità data (con il suo INSUNITS), con i layer `AL-*`.
    fn case_a_doc_in(form: Form, unit: DrawingUnit) -> CadDocument {
        let mut doc = CadDocument::new();
        doc.header.insertion_units = unit.insunits();
        let mut muri = Layer::new(LAYER_MURI);
        muri.handle = doc.allocate_handle();
        doc.layers.add(muri).unwrap();
        for entity in case_a(form, unit) {
            doc.add_entity(entity).unwrap();
        }
        ensure_layers(&mut doc);
        doc
    }

    /// Una zona demolita «muro tra due punti» sull'asse `p1`-`p2`, sulle facce
    /// dei muri di `doc`.
    fn add_demolition(doc: &mut CadDocument, p1: [f64; 2], p2: [f64; 2]) -> Handle {
        add_demolition_in(doc, M, p1, p2)
    }

    /// Lo stesso nell'unità data; `p1` e `p2` in metri.
    fn add_demolition_in(
        doc: &mut CadDocument,
        unit: DrawingUnit,
        p1: [f64; 2],
        p2: [f64; 2],
    ) -> Handle {
        let tol = unit.tolerance();
        let walls = wall_set(doc, tol);
        let at = |p: [f64; 2]| [unit.metres(p[0]), unit.metres(p[1])];
        let rect = rect_from_axis(
            &walls.sources,
            at(p1),
            at(p2),
            unit.metres(MAX_WALL_THICKNESS_M),
            tol,
        )
        .expect("muro sull'asse");
        let demolita = Zone {
            state: ZoneState::Demolito,
            origin: ZoneOrigin::MuroDuePunti,
            axis: Some(rect.axis),
            thickness: Some(rect.thickness),
            height: None,
            varco: false,
        };
        doc.add_entity(zone_hatch(&demolita, &[rect.ring()]))
            .unwrap()
    }

    /// Una zona nuova «linea + spessore».
    fn add_new_wall(doc: &mut CadDocument, p1: [f64; 2], p2: [f64; 2], thickness: f64) -> Handle {
        let rect = rect_from_axis_thickness(p1, p2, thickness).unwrap();
        let nuova = Zone {
            state: ZoneState::Nuovo,
            origin: ZoneOrigin::LineaSpessore,
            axis: Some(rect.axis),
            thickness: Some(thickness),
            height: None,
            varco: false,
        };
        doc.add_entity(zone_hatch(&nuova, &[rect.ring()])).unwrap()
    }

    /// Il caso A in metri nella forma data, con il tramezzo demolito per
    /// intero (asse (4,05; 0)-(4,05; 3)).
    fn doc_with_demolition(form: Form) -> CadDocument {
        let mut doc = case_a_doc(form);
        add_demolition(&mut doc, [4.05, 0.0], [4.05, 3.0]);
        doc
    }

    /// Caso A in metri (linee continue, INSUNITS 6), una zona demolita sul
    /// tramezzo (asse (4,05; 0)-(4,05; 3)) e una zona nuova da 0,12 sull'asse
    /// (2; 0,5)-(2; 2,5), più il record `ARCHLINE_PROGETTO`.
    fn doc_with_zones() -> CadDocument {
        let mut doc = doc_with_demolition(Form::Continua);
        add_new_wall(&mut doc, [2.0, 0.5], [2.0, 2.5], 0.12);

        record::write(
            &mut doc,
            &ProjectRecord {
                unit: None,
                new_walls_layer: Some(LAYER_MURI.into()),
            },
        );
        doc
    }

    fn hatches(doc: &CadDocument) -> Vec<&codec::entities::Hatch> {
        doc.entities()
            .filter_map(|e| match e {
                EntityType::Hatch(h) => Some(h.as_ref()),
                _ => None,
            })
            .collect()
    }

    fn lines_on<'a>(doc: &'a CadDocument, layer: &str) -> Vec<&'a codec::entities::Line> {
        doc.entities()
            .filter_map(|e| match e {
                EntityType::Line(l) if l.common.layer == layer => Some(l),
                _ => None,
            })
            .collect()
    }

    fn model_space_count(doc: &CadDocument) -> usize {
        doc.block_records
            .iter()
            .filter(|r| r.is_model_space())
            .map(|r| r.entity_handles.len())
            .sum()
    }

    /// Nessun XDATA, nessun APPID e nessun record del dizionario radice di
    /// ArchLine.
    #[track_caller]
    fn assert_no_archline_data(out: &CadDocument) {
        let our_record = |e: &EntityType| {
            e.common()
                .extended_data
                .records()
                .iter()
                .any(|r| r.application_name.starts_with("ARCHLINE"))
        };
        assert!(!out.entities().any(our_record), "XDATA ARCHLINE_* rimasto");
        assert!(
            !out.app_ids.iter().any(|a| a.name.starts_with("ARCHLINE")),
            "APPID ARCHLINE_* rimasto"
        );
        assert_eq!(record::read(out), ProjectRecord::default());
        let Some(ObjectType::Dictionary(root)) =
            out.objects.get(&out.header.named_objects_dict_handle)
        else {
            panic!("dizionario radice mancante");
        };
        assert!(
            !root
                .entries
                .iter()
                .any(|(key, _)| key.to_ascii_uppercase().starts_with("ARCHLINE")),
            "voce ARCHLINE_* nel dizionario radice"
        );
        assert!(
            !out.objects.values().any(|o| matches!(
                o,
                ObjectType::XRecord(x) if x.name.eq_ignore_ascii_case(RECORD_PROGETTO)
            )),
            "XRecord {RECORD_PROGETTO} copiato"
        );
    }

    /// Una cartella nuova sotto la cartella temporanea, cancellata alla fine.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir().join(format!(
                "archline_bim_extract_{tag}_{}_{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn both() -> [Elaborato; 2] {
        [Elaborato::StatoDiFatto, Elaborato::DemolizioniCostruzioni]
    }

    fn all_three() -> [Elaborato; 3] {
        [
            Elaborato::StatoDiFatto,
            Elaborato::DemolizioniCostruzioni,
            Elaborato::StatoDiProgetto,
        ]
    }

    #[test]
    fn suffixes_and_file_names() {
        assert_eq!(Elaborato::StatoDiFatto.suffix(), "stato-di-fatto");
        assert_eq!(
            Elaborato::DemolizioniCostruzioni.suffix(),
            "demolizioni-costruzioni"
        );
        assert_eq!(Elaborato::StatoDiProgetto.suffix(), "stato-di-progetto");
        assert_eq!(
            file_name("Rilievo Via Roma", Elaborato::StatoDiFatto),
            "Rilievo Via Roma_stato-di-fatto.dwg"
        );
        assert_eq!(
            file_name("casa", Elaborato::DemolizioniCostruzioni),
            "casa_demolizioni-costruzioni.dwg"
        );
        assert_eq!(
            file_name("casa", Elaborato::StatoDiProgetto),
            "casa_stato-di-progetto.dwg"
        );
    }

    #[test]
    fn comparativa_has_yellow_and_red_without_our_data() {
        let src = doc_with_zones();
        let ex = extract(&src, &opts(&[Elaborato::DemolizioniCostruzioni]));
        assert_eq!(ex.docs.len(), 1);
        let (e, out) = &ex.docs[0];
        assert_eq!(*e, Elaborato::DemolizioniCostruzioni);
        let hatches: Vec<_> = out
            .entities()
            .filter_map(|e| {
                if let EntityType::Hatch(h) = e {
                    Some(h)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(hatches.len(), 2);
        assert!(hatches.iter().any(|h| h.common.layer == "AL-DEMOLIZIONI"
            && h.common.color == Color::from_index(2)
            && h.is_solid));
        assert!(hatches.iter().any(|h| h.common.layer == "AL-NUOVE"
            && h.common.color == Color::from_index(1)
            && h.is_solid));
        assert!(out.entities().all(|e| e
            .common()
            .extended_data
            .records()
            .iter()
            .all(|r| !r.application_name.starts_with("ARCHLINE"))));
        assert!(!out.app_ids.iter().any(|a| a.name.starts_with("ARCHLINE")));
        assert_no_archline_data(out);
        // Tutto il rilievo c'è, con le zone: 12 LINE e 2 HATCH.
        assert_eq!(lines_on(out, LAYER_MURI).len(), 12);
        assert_eq!(model_space_count(out), 14);
        assert!(ex.notices.is_empty(), "{:?}", ex.notices);
    }

    #[test]
    fn fatto_has_no_zones_and_same_lines() {
        let src = doc_with_zones();
        let ex = extract(&src, &opts(&[Elaborato::StatoDiFatto]));
        assert_eq!(ex.docs.len(), 1);
        let (e, out) = &ex.docs[0];
        assert_eq!(*e, Elaborato::StatoDiFatto);
        assert!(
            !hatches(out)
                .iter()
                .any(|h| h.common.layer.to_ascii_uppercase().starts_with("AL-")),
            "zone nello stato di fatto"
        );
        assert!(hatches(out).is_empty());
        let src_lines = lines_on(&src, LAYER_MURI);
        let out_lines = lines_on(out, LAYER_MURI);
        assert_eq!(src_lines.len(), 12);
        assert_eq!(out_lines.len(), src_lines.len());
        let close = |a: Vector3, b: Vector3| (a - b).length() <= 1e-6;
        for line in &src_lines {
            assert!(
                out_lines
                    .iter()
                    .any(|o| close(o.start, line.start) && close(o.end, line.end)),
                "manca la LINE {:?}-{:?}",
                line.start,
                line.end
            );
        }
        assert_eq!(model_space_count(out), 12);
        assert_no_archline_data(out);
        assert!(ex.notices.is_empty(), "{:?}", ex.notices);
    }

    #[test]
    fn source_is_never_modified() {
        let src = doc_with_zones();
        let snapshot = |doc: &CadDocument| {
            (
                doc.entities().cloned().collect::<Vec<EntityType>>(),
                doc.app_ids
                    .iter()
                    .map(|a| a.name.clone())
                    .collect::<Vec<_>>(),
                doc.objects.len(),
                record::read(doc),
                doc.layers
                    .iter()
                    .map(|l| l.name.clone())
                    .collect::<Vec<_>>(),
            )
        };
        let before = snapshot(&src);
        let next_handle = src.next_handle();
        let ex = extract(&src, &opts(&all_three()));
        assert_eq!(ex.docs.len(), 3);
        assert!(snapshot(&src) == before, "il disegno di lavoro è cambiato");
        assert_eq!(src.next_handle(), next_handle, "handle presi da src");
        // Le zone di src hanno ancora il loro record.
        assert_eq!(crate::bim::zone::zones_in(&src).len(), 2);
    }

    #[test]
    fn rooms_layers_follow_the_elaborato_and_lose_our_xdata() {
        let mut src = doc_with_zones();
        let locale = |values: &[&str]| {
            let mut rec = ExtendedDataRecord::new(APP_LOCALE);
            rec.add_value(XDataValue::Integer16(1));
            for v in values {
                rec.add_value(XDataValue::String((*v).into()));
            }
            rec
        };
        let mut contorno = LwPolyline::from_points(vec![
            Vector2::new(0.5, 0.5),
            Vector2::new(3.5, 0.5),
            Vector2::new(3.5, 2.5),
            Vector2::new(0.5, 2.5),
        ]);
        contorno.is_closed = true;
        contorno.common.layer = LAYER_LOCALI.into();
        contorno
            .common
            .extended_data
            .add_record(locale(&["fase", "fatto"]));
        src.add_entity(EntityType::LwPolyline(contorno)).unwrap();
        let mut testo = MText::with_value("A\\PS = 12,00 m²", Vector3::new(2.0, 1.5, 0.0));
        testo.common.layer = LAYER_LOCALI_PROGETTO.into();
        testo
            .common
            .extended_data
            .add_record(locale(&["fase", "progetto"]));
        src.add_entity(EntityType::MText(Box::new(testo))).unwrap();
        // Il layer dei locali si confronta senza badare alle maiuscole.
        let mut minuscolo = LwPolyline::from_points(vec![
            Vector2::new(4.5, 0.5),
            Vector2::new(7.5, 0.5),
            Vector2::new(7.5, 2.5),
        ]);
        minuscolo.common.layer = LAYER_LOCALI_PROGETTO.to_ascii_lowercase();
        src.add_entity(EntityType::LwPolyline(minuscolo)).unwrap();

        let ex = extract(&src, &opts(&both()));
        let (_, fatto_out) = &ex.docs[0];
        let (_, comparativa) = &ex.docs[1];
        let on = |doc: &CadDocument, layer: &str| {
            doc.entities()
                .filter(|e| e.common().layer.eq_ignore_ascii_case(layer))
                .count()
        };
        // Fatto: il contorno di fatto resta (senza il nostro record), quello
        // di progetto no.
        assert_eq!(on(fatto_out, LAYER_LOCALI), 1);
        assert_eq!(on(fatto_out, LAYER_LOCALI_PROGETTO), 0);
        assert_no_archline_data(fatto_out);
        // Comparativa: niente locali.
        assert_eq!(on(comparativa, LAYER_LOCALI), 0);
        assert_eq!(on(comparativa, LAYER_LOCALI_PROGETTO), 0);
        assert_no_archline_data(comparativa);
        assert!(ex.notices.is_empty(), "{:?}", ex.notices);
    }

    #[test]
    fn our_xdata_is_dropped_but_other_apps_stay() {
        let mut src = doc_with_zones();
        // Una LINE del rilievo con un record di un'altra applicazione e uno
        // nostro (di una versione futura).
        let mut altra = codec::tables::AppId::new("ALTRA_APP");
        altra.handle = src.allocate_handle();
        src.app_ids.add(altra).unwrap();
        let mut futura = codec::tables::AppId::new("ARCHLINE_FUTURA");
        futura.handle = src.allocate_handle();
        src.app_ids.add(futura).unwrap();
        let line = src
            .entities()
            .find_map(|e| match e {
                EntityType::Line(l) if l.common.layer == LAYER_MURI => Some(l.common.handle),
                _ => None,
            })
            .unwrap();
        let entity = src.get_entity_mut(line).unwrap();
        let mut other = ExtendedDataRecord::new("ALTRA_APP");
        other.add_value(XDataValue::Integer16(7));
        entity.common_mut().extended_data.add_record(other.clone());
        let mut future = ExtendedDataRecord::new("ARCHLINE_FUTURA");
        future.add_value(XDataValue::Integer16(1));
        entity.common_mut().extended_data.add_record(future);

        let ex = extract(&src, &opts(&both()));
        for (_, out) in &ex.docs {
            assert_no_archline_data(out);
            let kept: Vec<_> = out
                .entities()
                .filter_map(|e| e.common().extended_data.get_record("ALTRA_APP"))
                .collect();
            assert_eq!(kept, vec![&other]);
            assert!(out.app_ids.contains("ALTRA_APP"));
        }
        // src conserva tutto.
        let entity = src.get_entity(line).unwrap();
        assert!(entity
            .common()
            .extended_data
            .get_record("ARCHLINE_FUTURA")
            .is_some());
    }

    #[test]
    fn zones_inside_a_copied_block_lose_our_data_too() {
        let mut src = doc_with_zones();
        let ring = vec![[10.0, 0.0], [11.0, 0.0], [11.0, 1.0], [10.0, 1.0]];
        let zona = Zone {
            state: ZoneState::Nuovo,
            origin: ZoneOrigin::Rettangolo,
            axis: None,
            thickness: None,
            height: None,
            varco: false,
        };
        crate::app::define_block_raw_doc(
            &mut src,
            "BLOCCO_ZONA",
            Vector3::ZERO,
            &[zone_hatch(&zona, &[ring])],
        );
        src.add_entity(EntityType::Insert(Insert::new(
            "BLOCCO_ZONA",
            Vector3::ZERO,
        )))
        .unwrap();
        let ex = extract(&src, &opts(&both()));
        for (_, out) in &ex.docs {
            assert!(out.block_records.get("BLOCCO_ZONA").is_some());
            assert_no_archline_data(out);
        }
    }

    #[test]
    fn version_units_and_layer_linetype_follow_source() {
        let mut src = doc_with_zones();
        src.version = DxfVersion::AC1015;
        let mut dashed = LineType::new("DASHED");
        dashed.description = "__ __ __".into();
        dashed.handle = src.allocate_handle();
        src.line_types.add(dashed).unwrap();
        src.layers.get_mut(LAYER_MURI).unwrap().line_type = "DASHED".into();

        let ex = extract(&src, &opts(&both()));
        assert_eq!(ex.docs.len(), 2);
        for (_, out) in &ex.docs {
            assert_eq!(out.version, DxfVersion::AC1015);
            assert_eq!(out.header.insertion_units, 6);
            let lt = out.line_types.get("DASHED").expect("tipo di linea DASHED");
            assert_eq!(lt.description, "__ __ __");
            assert!(!lt.handle.is_null());
            assert!(
                out.line_types
                    .iter()
                    .filter(|l| l.handle == lt.handle)
                    .count()
                    == 1,
                "handle doppio"
            );
            assert_eq!(out.layers.get(LAYER_MURI).unwrap().line_type, "DASHED");
        }

        // Fuori da AC1014..AC1032 si scrive AC1032; dentro, la versione resta.
        for (from, to) in [
            (DxfVersion::AC1009, DxfVersion::AC1032),
            (DxfVersion::AC1012, DxfVersion::AC1032),
            (DxfVersion::Unknown, DxfVersion::AC1032),
            (DxfVersion::AC1014, DxfVersion::AC1014),
            (DxfVersion::AC1027, DxfVersion::AC1027),
        ] {
            src.version = from;
            let ex = extract(&src, &opts(&[Elaborato::StatoDiFatto]));
            assert_eq!(ex.docs[0].1.version, to, "{from:?}");
        }
        // Le unità seguono il disegno, anche 0.
        src.header.insertion_units = 0;
        let ex = extract(&src, &opts(&[Elaborato::StatoDiFatto]));
        assert_eq!(ex.docs[0].1.header.insertion_units, 0);
    }

    #[test]
    fn each_elaborato_once_in_the_order_asked() {
        let src = doc_with_zones();
        let ex = extract(
            &src,
            &opts(&[
                Elaborato::DemolizioniCostruzioni,
                Elaborato::StatoDiProgetto,
                Elaborato::StatoDiFatto,
                Elaborato::DemolizioniCostruzioni,
                Elaborato::StatoDiProgetto,
            ]),
        );
        let kinds: Vec<_> = ex.docs.iter().map(|(e, _)| *e).collect();
        assert_eq!(
            kinds,
            vec![
                Elaborato::DemolizioniCostruzioni,
                Elaborato::StatoDiProgetto,
                Elaborato::StatoDiFatto
            ]
        );
        assert!(ex.notices.is_empty(), "{:?}", ex.notices);
        assert!(extract(&src, &opts(&[])).docs.is_empty());
    }

    #[test]
    fn handles_that_do_not_copy_are_listed_once() {
        let mut src = doc_with_zones();
        let ghost = Handle::new(0xDEAD);
        assert!(src.get_entity(ghost).is_none());
        src.block_records
            .iter_mut()
            .find(|r| r.is_model_space())
            .unwrap()
            .entity_handles
            .push(ghost);
        let ex = extract(&src, &opts(&both()));
        assert_eq!(ex.docs.len(), 2);
        assert_eq!(
            ex.notices,
            vec![Notice::NonCopiata {
                handle: ghost,
                kind: "Entità".into(),
            }]
        );
        assert_eq!(
            ex.notices[0].text(),
            "Entità DEAD non copiata nel DWG estratto."
        );
        // Il resto è copiato.
        assert_eq!(model_space_count(&ex.docs[0].1), 12);
        assert_eq!(model_space_count(&ex.docs[1].1), 14);
    }

    #[test]
    fn empty_drawing_gives_empty_documents() {
        let mut src = CadDocument::new();
        src.header.insertion_units = 5;
        let ex = extract(&src, &opts(&both()));
        assert_eq!(ex.docs.len(), 2);
        for (_, out) in &ex.docs {
            assert_eq!(model_space_count(out), 0);
            assert_eq!(out.header.insertion_units, 5);
        }
        assert!(ex.notices.is_empty());
    }

    fn mtimes(paths: &[PathBuf]) -> Vec<std::time::SystemTime> {
        paths
            .iter()
            .map(|p| std::fs::metadata(p).unwrap().modified().unwrap())
            .collect()
    }

    #[test]
    fn write_refuses_existing_files() {
        let tmp = TempDir::new("refuse");
        let ex = extract(&doc_with_zones(), &opts(&both()));

        let written = write(&ex, &tmp.0, "casa", false).unwrap();
        assert_eq!(
            written,
            vec![
                tmp.0.join("casa_stato-di-fatto.dwg"),
                tmp.0.join("casa_demolizioni-costruzioni.dwg"),
            ]
        );
        assert!(written.iter().all(|p| p.is_file()));
        let before = mtimes(&written);
        let lens: Vec<u64> = written
            .iter()
            .map(|p| std::fs::metadata(p).unwrap().len())
            .collect();

        // Senza conferma: rifiuto, con tutti i file esistenti, e nulla scritto.
        assert_eq!(
            write(&ex, &tmp.0, "casa", false),
            Err(WriteError::Exists(written.clone()))
        );
        assert_eq!(mtimes(&written), before);

        // Uno solo esiste: rifiuto con quello solo, l'altro non si crea.
        std::fs::remove_file(&written[0]).unwrap();
        assert_eq!(
            write(&ex, &tmp.0, "casa", false),
            Err(WriteError::Exists(vec![written[1].clone()]))
        );
        assert!(
            !written[0].exists(),
            "scritto un file nonostante il rifiuto"
        );
        assert_eq!(mtimes(&written[1..]), before[1..]);

        // Con conferma: si scrive tutto.
        assert_eq!(write(&ex, &tmp.0, "casa", true), Ok(written.clone()));
        assert!(written.iter().all(|p| p.is_file()));
        let after: Vec<u64> = written
            .iter()
            .map(|p| std::fs::metadata(p).unwrap().len())
            .collect();
        assert_eq!(after, lens);
        // Nessun file temporaneo rimasto nella cartella.
        assert_eq!(std::fs::read_dir(&tmp.0).unwrap().count(), 2);
    }

    #[test]
    fn write_reports_io_errors() {
        let tmp = TempDir::new("io");
        let ex = extract(&doc_with_zones(), &opts(&[Elaborato::StatoDiFatto]));
        let missing = tmp.0.join("non-esiste");
        assert!(matches!(
            write(&ex, &missing, "casa", false),
            Err(WriteError::Io(_))
        ));
    }

    /// Il secondo file non si scrive (al suo posto c'è una cartella): `Partial`
    /// con il primo, che resta sul disco; se si ferma al primo, `Io`.
    #[test]
    fn write_error_after_the_first_file_is_partial() {
        let tmp = TempDir::new("parziale");
        let ex = extract(&doc_with_zones(), &opts(&both()));
        let first = tmp.0.join("casa_stato-di-fatto.dwg");
        let second = tmp.0.join("casa_demolizioni-costruzioni.dwg");
        std::fs::create_dir(&second).unwrap();

        let Err(WriteError::Partial { written, message }) = write(&ex, &tmp.0, "casa", true) else {
            panic!("atteso un errore dopo il primo file");
        };
        assert_eq!(written, vec![first.clone()]);
        assert!(first.is_file());
        assert!(
            message.starts_with(&format!("{}: ", second.display())),
            "{message}"
        );

        std::fs::remove_file(&first).unwrap();
        std::fs::create_dir(&first).unwrap();
        let Err(WriteError::Io(message)) = write(&ex, &tmp.0, "casa", true) else {
            panic!("atteso un errore al primo file");
        };
        assert!(
            message.starts_with(&format!("{}: ", first.display())),
            "{message}"
        );
    }

    #[test]
    fn written_dwg_reloads() {
        let tmp = TempDir::new("reload");
        let mut src = doc_with_zones();
        src.version = DxfVersion::AC1018;
        let ex = extract(&src, &opts(&both()));
        let written = write(&ex, &tmp.0, "casa", false).unwrap();
        assert_eq!(written.len(), ex.docs.len());
        for ((elaborato, doc), path) in ex.docs.iter().zip(&written) {
            let loaded =
                crate::io::load_file(path).unwrap_or_else(|e| panic!("{elaborato:?}: {e}"));
            assert_eq!(
                loaded.entities().count(),
                doc.entities().count(),
                "{elaborato:?}"
            );
            assert_eq!(model_space_count(&loaded), model_space_count(doc));
            assert_eq!(loaded.version, DxfVersion::AC1018, "{elaborato:?}");
            assert_eq!(loaded.header.insertion_units, 6);
            assert_no_archline_data(&loaded);
            assert!(!loaded.app_ids.contains(APP_ZONA));
            let expected = match elaborato {
                Elaborato::DemolizioniCostruzioni => 2,
                _ => 0,
            };
            assert_eq!(hatches(&loaded).len(), expected, "{elaborato:?}");
            assert_eq!(lines_on(&loaded, LAYER_MURI).len(), 12);
        }
    }

    /// Un disegno letto da DWG porta anche i blocchi grezzi dell'XDATA: neanche
    /// quelli devono arrivare nel file estratto.
    #[test]
    fn source_read_from_dwg_loses_raw_xdata_too() {
        let bytes = codec::DwgWriter::write_to_vec(&doc_with_zones()).unwrap();
        let src = codec::DwgReader::from_stream(std::io::Cursor::new(bytes))
            .read()
            .unwrap();
        let zones = crate::bim::zone::zones_in(&src);
        assert_eq!(zones.len(), 2);
        let zona = src.app_ids.get(APP_ZONA).unwrap().handle.value();
        assert!(
            zones.iter().any(|z| src
                .get_entity(z.handle)
                .unwrap()
                .common()
                .extended_data
                .raw_dwg_eed
                .iter()
                .any(|(app, _)| *app == zona)),
            "la lettura DWG non ha lasciato blocchi grezzi: il test non prova nulla"
        );

        let tmp = TempDir::new("raw");
        let ex = extract(&src, &opts(&both()));
        for (_, out) in &ex.docs {
            assert_no_archline_data(out);
            assert!(out
                .entities()
                .all(|e| e.common().extended_data.raw_dwg_eed.is_empty()));
        }
        let written = write(&ex, &tmp.0, "rilievo", false).unwrap();
        for path in &written {
            let loaded = crate::io::load_file(path).unwrap();
            assert_no_archline_data(&loaded);
        }
        let comparativa = crate::io::load_file(&written[1]).unwrap();
        assert_eq!(hatches(&comparativa).len(), 2);
        assert!(crate::bim::zone::zones_in(&comparativa).is_empty());
    }

    // ---- Correzione prima della revisione (R25-R28) ----

    fn non_copiata(handle: Handle, kind: &str) -> Notice {
        Notice::NonCopiata {
            handle,
            kind: kind.into(),
        }
    }

    /// Un'immagine raster la cui definizione sta negli oggetti di `doc`.
    fn image_entity(doc: &mut CadDocument) -> EntityType {
        let definition = doc.allocate_handle();
        let mut def = codec::objects::ImageDefinition::new("foto.png");
        def.handle = definition;
        def.owner = doc.header.named_objects_dict_handle;
        doc.objects
            .insert(definition, ObjectType::ImageDefinition(def));
        let mut image = codec::entities::RasterImage::new("foto.png", Vector3::ZERO, 100.0, 100.0);
        image.definition_handle = Some(definition);
        EntityType::RasterImage(image)
    }

    /// R25: immagini, underlay, OLE, riferimenti esterni e solidi 3D restano
    /// fuori, ognuno con il suo avviso; anche un blocco che contiene
    /// un'immagine o che manca.
    #[test]
    fn images_xrefs_and_3d_are_listed_not_copied() {
        let mut src = doc_with_zones();
        let image = image_entity(&mut src);
        let image = src.add_entity(image).unwrap();
        crate::app::define_block_raw_doc(&mut src, "XREF_A", Vector3::ZERO, &[]);
        src.block_records.get_mut("XREF_A").unwrap().flags.is_xref = true;
        let xref = src
            .add_entity(EntityType::Insert(Insert::new("XREF_A", Vector3::ZERO)))
            .unwrap();
        let solid = src
            .add_entity(EntityType::Solid3D(codec::entities::Solid3D::new()))
            .unwrap();
        // Un blocco qualunque con un'immagine dentro: l'INSERT non si porta
        // intero.
        let inner_image = image_entity(&mut src);
        crate::app::define_block_raw_doc(&mut src, "CON_FOTO", Vector3::ZERO, &[inner_image]);
        let with_image = src
            .add_entity(EntityType::Insert(Insert::new("CON_FOTO", Vector3::ZERO)))
            .unwrap();
        // Un INSERT di un blocco che non esiste.
        let missing = src
            .add_entity(EntityType::Insert(Insert::new("NON_ESISTE", Vector3::ZERO)))
            .unwrap();
        // Un blocco normale (con una linea) si copia.
        let line = lines_on(&src, LAYER_MURI)[0].clone();
        crate::app::define_block_raw_doc(
            &mut src,
            "NORMALE",
            Vector3::ZERO,
            &[EntityType::Line(line)],
        );
        src.add_entity(EntityType::Insert(Insert::new("NORMALE", Vector3::ZERO)))
            .unwrap();

        let ex = extract(&src, &opts(&both()));
        assert_eq!(
            ex.notices,
            vec![
                non_copiata(image, "IMAGE"),
                non_copiata(xref, "INSERT"),
                non_copiata(solid, "3DSOLID"),
                non_copiata(with_image, "INSERT"),
                non_copiata(missing, "INSERT"),
            ]
        );
        assert_eq!(
            ex.notices[0].text(),
            format!("IMAGE {image:X} non copiata nel DWG estratto.")
        );
        for (elaborato, out) in &ex.docs {
            assert!(
                !out.entities()
                    .any(|e| matches!(e, EntityType::RasterImage(_) | EntityType::Solid3D(_))),
                "{elaborato:?}"
            );
            let inserts: Vec<&str> = out
                .entities()
                .filter_map(|e| match e {
                    EntityType::Insert(i) => Some(i.block_name.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(inserts, vec!["NORMALE"], "{elaborato:?}");
            assert!(out.block_records.get("XREF_A").is_none());
            assert!(out.block_records.get("CON_FOTO").is_none());
            // Rilievo + zone (comparativa) + l'INSERT normale.
            let expected = match elaborato {
                Elaborato::DemolizioniCostruzioni => 15,
                _ => 13,
            };
            assert_eq!(model_space_count(out), expected, "{elaborato:?}");
        }

        let tmp = TempDir::new("r25");
        let written = write(&ex, &tmp.0, "casa", false).unwrap();
        for ((elaborato, doc), path) in ex.docs.iter().zip(&written) {
            let loaded =
                crate::io::load_file(path).unwrap_or_else(|e| panic!("{elaborato:?}: {e}"));
            assert_eq!(model_space_count(&loaded), model_space_count(doc));
            assert!(loaded.block_records.get("NORMALE").is_some());
        }
    }

    /// R26: un avviso per ogni layout con entità; il solo viewport generale
    /// (id 1) non conta.
    #[test]
    fn layouts_with_entities_are_listed() {
        let mut src = doc_with_zones();
        let paper = src.header.paper_space_block_handle;
        assert!(!paper.is_null());
        let mut overall = codec::entities::Viewport::new();
        overall.id = 1;
        overall.common.owner_handle = paper;
        src.add_entity(EntityType::Viewport(Box::new(overall)))
            .unwrap();
        assert!(extract(&src, &opts(&both())).notices.is_empty());

        let mut note = codec::entities::Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
        note.common.owner_handle = paper;
        src.add_entity(EntityType::Line(note)).unwrap();
        let ex = extract(&src, &opts(&both()));
        assert_eq!(ex.notices, vec![non_copiata(paper, "Layout Layout1")]);
        assert_eq!(
            ex.notices[0].text(),
            format!("Layout Layout1 {paper:X} non copiata nel DWG estratto.")
        );
        // Nello spazio modello estratto la linea della carta non c'è.
        assert_eq!(model_space_count(&ex.docs[0].1), 12);
        // Anche il solo progetto elenca il layout.
        let ex = extract(&src, &opts(&[Elaborato::StatoDiProgetto]));
        assert_eq!(ex.docs.len(), 1);
        assert_eq!(ex.notices, vec![non_copiata(paper, "Layout Layout1")]);
    }

    /// R27: variabili di testata, layer «0» e stile di testo «Standard».
    #[test]
    fn header_layer_zero_and_standard_follow_source() {
        let mut src = doc_with_zones();
        let h = &mut src.header;
        h.linetype_scale = 50.0;
        h.paper_space_linetype_scaling = !h.paper_space_linetype_scaling;
        h.current_entity_linetype_scale = 2.0;
        h.measurement = 1;
        h.linear_unit_format = 2;
        h.linear_unit_precision = 3;
        h.angular_unit_format = 1;
        h.angular_unit_precision = 2;
        h.text_height = 0.25;
        h.dim_scale = 50.0;
        let psltscale = h.paper_space_linetype_scaling;

        let mut tratteggio = LineType::new("TRATTO_0");
        tratteggio.description = "_ _ _".into();
        tratteggio.handle = src.allocate_handle();
        src.line_types.add(tratteggio).unwrap();
        let zero = src.layers.get_mut("0").unwrap();
        zero.color = Color::from_index(3);
        zero.line_type = "TRATTO_0".into();
        zero.line_weight = codec::LineWeight::Value(25);
        zero.flags.locked = true;
        let standard = src.text_styles.get_mut("Standard").unwrap();
        standard.font_file = "romans.shx".into();
        standard.big_font_file = "bigfont.shx".into();
        standard.height = 0.2;
        standard.width_factor = 0.8;
        standard.oblique_angle = 0.25;

        #[track_caller]
        fn check(out: &CadDocument, psltscale: bool, what: &str) {
            let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
            let h = &out.header;
            assert!(close(h.linetype_scale, 50.0), "{what}: LTSCALE");
            assert_eq!(
                h.paper_space_linetype_scaling, psltscale,
                "{what}: PSLTSCALE"
            );
            assert!(
                close(h.current_entity_linetype_scale, 2.0),
                "{what}: CELTSCALE"
            );
            assert_eq!(h.measurement, 1, "{what}: MEASUREMENT");
            assert_eq!(h.linear_unit_format, 2, "{what}: LUNITS");
            assert_eq!(h.linear_unit_precision, 3, "{what}: LUPREC");
            assert_eq!(h.angular_unit_format, 1, "{what}: AUNITS");
            assert_eq!(h.angular_unit_precision, 2, "{what}: AUPREC");
            assert!(close(h.text_height, 0.25), "{what}: TEXTSIZE");
            assert!(close(h.dim_scale, 50.0), "{what}: DIMSCALE");
            let zero = out.layers.get("0").unwrap();
            assert_eq!(
                zero.color,
                Color::from_index(3),
                "{what}: colore del layer 0"
            );
            assert_eq!(zero.line_type, "TRATTO_0", "{what}");
            assert_eq!(zero.line_weight, codec::LineWeight::Value(25), "{what}");
            assert!(zero.flags.locked, "{what}");
            let lt = out
                .line_types
                .get("TRATTO_0")
                .expect("tipo di linea del layer 0");
            assert!(!lt.handle.is_null());
            let standard = out.text_styles.get("Standard").unwrap();
            assert_eq!(standard.font_file, "romans.shx", "{what}");
            assert_eq!(standard.big_font_file, "bigfont.shx", "{what}");
            assert!(close(standard.height, 0.2), "{what}");
            assert!(close(standard.width_factor, 0.8), "{what}");
            assert!(close(standard.oblique_angle, 0.25), "{what}");
        }

        let ex = extract(&src, &opts(&both()));
        for (_, out) in &ex.docs {
            check(out, psltscale, "in memoria");
        }
        let tmp = TempDir::new("r27");
        for path in write(&ex, &tmp.0, "casa", false).unwrap() {
            let loaded = crate::io::load_file(&path).unwrap();
            check(&loaded, psltscale, "dopo il DWG");
        }
        // Gli handle restano quelli di `out`.
        let out = &ex.docs[0].1;
        let fresh = CadDocument::new();
        assert_eq!(
            out.layers.get("0").unwrap().handle,
            fresh.layers.get("0").unwrap().handle
        );
        assert_eq!(
            out.text_styles.get("Standard").unwrap().handle,
            fresh.text_styles.get("Standard").unwrap().handle
        );
    }

    /// R28: un record `ARCHLINE_ZONA` illeggibile basta per escludere
    /// l'entità dallo stato di fatto; nella comparativa resta, senza record.
    #[test]
    fn garbage_zone_record_counts_as_zone() {
        let mut src = doc_with_zones();
        let zona = Zone {
            state: ZoneState::Nuovo,
            origin: ZoneOrigin::Rettangolo,
            axis: None,
            thickness: None,
            height: None,
            varco: false,
        };
        let mut garbage = zone_hatch(&zona, &[vec![[5.0, 1.0], [6.0, 1.0], [6.0, 2.0]]]);
        let mut rec = ExtendedDataRecord::new(APP_ZONA);
        rec.add_value(XDataValue::Integer16(9));
        rec.add_value(XDataValue::String("rotto".into()));
        let common = garbage.common_mut();
        common.layer = "ALTRO".into();
        common.extended_data = codec::xdata::ExtendedData::new();
        common.extended_data.add_record(rec);
        let garbage = src.add_entity(garbage).unwrap();
        assert!(crate::bim::zone::read_zone(garbage, src.get_entity(garbage).unwrap()).is_none());
        // Anche una LINE con il record (in minuscolo) conta come zona.
        let mut segnata = codec::entities::Line::from_points(Vector3::ZERO, Vector3::UNIT_Y);
        let mut rec = ExtendedDataRecord::new(APP_ZONA.to_ascii_lowercase());
        rec.add_value(XDataValue::Integer16(1));
        segnata.common.extended_data.add_record(rec);
        src.add_entity(EntityType::Line(segnata)).unwrap();

        let ex = extract(&src, &opts(&both()));
        let (_, fatto) = &ex.docs[0];
        let (_, comparativa) = &ex.docs[1];
        assert!(hatches(fatto).is_empty());
        assert_eq!(model_space_count(fatto), 12);
        assert_eq!(hatches(comparativa).len(), 3);
        assert_eq!(model_space_count(comparativa), 16);
        assert_no_archline_data(fatto);
        assert_no_archline_data(comparativa);
        assert!(comparativa
            .entities()
            .all(|e| e.common().extended_data.records().is_empty()));
        assert!(ex.notices.is_empty(), "{:?}", ex.notices);
    }

    /// Lo stile di multilinea `name` nel dizionario `ACAD_MLINESTYLE` di `doc`.
    fn mline_style_named<'a>(
        doc: &'a CadDocument,
        name: &str,
    ) -> Option<(Handle, &'a codec::objects::MLineStyle)> {
        let Some(ObjectType::Dictionary(dict)) =
            doc.objects.get(&doc.header.acad_mlinestyle_dict_handle)
        else {
            return None;
        };
        let handle = dict
            .entries
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, h)| *h)?;
        match doc.objects.get(&handle) {
            Some(ObjectType::MLineStyle(style)) => Some((handle, style)),
            _ => None,
        }
    }

    /// R30: una multilinea porta con sé il suo stile (con i tipi di linea
    /// degli elementi), anche quando si chiama come lo «Standard» del
    /// documento nuovo; multileader e tabelle, che rimandano per handle a
    /// stili, stili di testo e blocchi che la copia non porta, restano fuori
    /// ed elencati.
    #[test]
    fn mline_styles_travel_and_mleaders_and_tables_are_listed() {
        let mut src = doc_with_zones();
        let mut tratto = LineType::new("TRATTO_ML");
        tratto.handle = src.allocate_handle();
        src.line_types.add(tratto).unwrap();
        let dict = src.header.acad_mlinestyle_dict_handle;
        let add_style = |src: &mut CadDocument, name: &str, offset: f64| {
            let mut style = codec::objects::MLineStyle::new(name);
            let mut outer = codec::objects::MLineStyleElement::new(offset);
            outer.linetype = "TRATTO_ML".into();
            style.add_element(outer);
            style.add_element(codec::objects::MLineStyleElement::new(-offset));
            style.description = format!("muro {name}");
            let handle = src.allocate_handle();
            style.handle = handle;
            style.owner = dict;
            if let Some(ObjectType::Dictionary(d)) = src.objects.get_mut(&dict) {
                d.entries.retain(|(key, _)| !key.eq_ignore_ascii_case(name));
                d.add_entry(name, handle);
            }
            src.objects.insert(handle, ObjectType::MLineStyle(style));
            handle
        };
        let muro30 = add_style(&mut src, "MURO30", 0.15);
        let standard = add_style(&mut src, "Standard", 0.05);
        let mline = |style: Handle, name: &str, y: f64| {
            let mut m = codec::entities::MLine::from_points(&[
                Vector3::new(10.0, y, 0.0),
                Vector3::new(14.0, y, 0.0),
            ]);
            m.style_handle = Some(style);
            m.style_name = name.into();
            EntityType::MLine(m)
        };
        src.add_entity(mline(muro30, "MURO30", 0.0)).unwrap();
        src.add_entity(mline(standard, "Standard", 1.0)).unwrap();
        let mleader = src
            .add_entity(EntityType::MultiLeader(Box::new(
                codec::entities::MultiLeader::new(),
            )))
            .unwrap();
        let table = src
            .add_entity(EntityType::Table(Box::new(codec::entities::Table::new(
                Vector3::new(20.0, 0.0, 0.0),
                2,
                2,
            ))))
            .unwrap();

        let ex = extract(&src, &opts(&both()));
        assert_eq!(
            ex.notices,
            vec![
                non_copiata(mleader, "MULTILEADER"),
                non_copiata(table, "ACAD_TABLE"),
            ]
        );
        #[track_caller]
        fn check(out: &CadDocument, what: &str) {
            let mlines: Vec<&codec::entities::MLine> = out
                .entities()
                .filter_map(|e| match e {
                    EntityType::MLine(m) => Some(m),
                    _ => None,
                })
                .collect();
            assert_eq!(mlines.len(), 2, "{what}");
            for (name, offset) in [("MURO30", 0.15), ("Standard", 0.05)] {
                let (handle, style) =
                    mline_style_named(out, name).unwrap_or_else(|| panic!("{what}: {name}"));
                assert_eq!(style.description, format!("muro {name}"), "{what}");
                assert_eq!(style.elements.len(), 2, "{what}");
                assert!((style.elements[0].offset - offset).abs() < 1e-12, "{what}");
                assert!(
                    style.elements[0].linetype.eq_ignore_ascii_case("TRATTO_ML"),
                    "{what}"
                );
                let users = mlines
                    .iter()
                    .filter(|m| m.style_handle == Some(handle))
                    .count();
                assert_eq!(users, 1, "{what}: multilinea con lo stile {name}");
            }
            assert!(out.line_types.contains("TRATTO_ML"), "{what}");
            assert!(!out
                .entities()
                .any(|e| matches!(e, EntityType::MultiLeader(_) | EntityType::Table(_))));
        }
        for (_, out) in &ex.docs {
            check(out, "in memoria");
        }
        let tmp = TempDir::new("r30");
        for path in write(&ex, &tmp.0, "casa", false).unwrap() {
            let loaded = crate::io::load_file(&path).unwrap();
            check(&loaded, "dopo il DWG");
        }
    }

    /// R30: una multilinea il cui stile manca anche nel disegno di partenza
    /// non si copia: resta fuori ed elencata.
    #[test]
    fn mline_without_a_style_is_listed() {
        let mut src = doc_with_zones();
        let mut m = codec::entities::MLine::from_points(&[Vector3::ZERO, Vector3::UNIT_X]);
        m.style_handle = Some(Handle::new(0xBEEF));
        m.style_name = "INESISTENTE".into();
        let orphan = src.add_entity(EntityType::MLine(m)).unwrap();
        let ex = extract(&src, &opts(&[Elaborato::StatoDiFatto]));
        assert_eq!(ex.notices, vec![non_copiata(orphan, "MLINE")]);
        assert_eq!(model_space_count(&ex.docs[0].1), 12);
    }

    // ---- Fix round 1 (R31): riferimenti per handle delle tabelle ----

    fn add_text_style(doc: &mut CadDocument, name: &str, font: &str) -> Handle {
        let mut style = codec::tables::TextStyle::new(name);
        style.font_file = font.into();
        style.handle = doc.allocate_handle();
        let handle = style.handle;
        doc.text_styles.add(style).unwrap();
        handle
    }

    fn add_line_type(doc: &mut CadDocument, line_type: LineType) -> Handle {
        let mut line_type = line_type;
        line_type.handle = doc.allocate_handle();
        let handle = line_type.handle;
        doc.line_types.add(line_type).unwrap();
        handle
    }

    fn handle_of_text_style(
        doc: &CadDocument,
        handle: Handle,
    ) -> Option<&codec::tables::TextStyle> {
        doc.text_styles.iter().find(|s| s.handle == handle)
    }

    fn block_named_by(doc: &CadDocument, handle: Handle) -> Option<String> {
        doc.block_records
            .iter()
            .find(|r| r.handle == handle)
            .map(|r| r.name.clone())
    }

    fn line_type_named_by(doc: &CadDocument, handle: Handle) -> Option<String> {
        doc.line_types
            .iter()
            .find(|l| l.handle == handle)
            .map(|l| l.name.clone())
    }

    /// Il segnaposto `name` del dizionario `ACAD_PLOTSTYLENAME`.
    fn plot_style_named(doc: &CadDocument, name: &str) -> Option<Handle> {
        match doc
            .objects
            .get(&doc.header.acad_plotstylename_dict_handle)?
        {
            ObjectType::DictionaryWithDefault(d) => d
                .entries
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, h)| *h),
            ObjectType::Dictionary(d) => d
                .entries
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, h)| *h),
            _ => None,
        }
    }

    /// R31: uno stile di quota porta con sé stile di testo, tipi di linea e
    /// blocchi delle frecce (per nome, con gli handle di `out`); lo «Standard»
    /// modificato tiene i valori di `src`; i blocchi inseriti dentro i blocchi
    /// `*D` delle quote arrivano; un tipo di linea complesso punta al suo stile
    /// di testo; lo stile di stampa dei layer punta a quello di `out` o a nulla.
    #[test]
    fn dimension_styles_bring_their_text_style_linetypes_and_arrows() {
        let mut src = doc_with_zones();
        add_text_style(&mut src, "ROMANS", "romans.shx");
        let lt_text = add_text_style(&mut src, "LT_TESTO", "simplex.shx");
        let mut dashed = LineType::new("TRATTEGGIO_Q");
        dashed.add_element(codec::tables::LineTypeElement::dash(0.5));
        dashed.add_element(codec::tables::LineTypeElement::space(0.25));
        dashed.pattern_length = 0.75;
        add_line_type(&mut src, dashed);
        let mut gas = LineType::new("GAS_LT");
        gas.add_element(codec::tables::LineTypeElement::dash(0.5));
        let mut text = codec::tables::LineTypeElement::space(0.2);
        text.complex = Some(codec::tables::LineTypeComplexData {
            content: codec::tables::LineTypeComplexContent::Text { text: "GAS".into() },
            style_handle: lt_text,
            scale: 0.1,
            ..Default::default()
        });
        gas.add_element(text);
        gas.pattern_length = 0.7;
        add_line_type(&mut src, gas);
        let mut tubo = codec::entities::Line::from_points(
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(8.0, 5.0, 0.0),
        );
        tubo.common.linetype = "GAS_LT".into();
        src.add_entity(EntityType::Line(tubo)).unwrap();

        let arch_tick = crate::entities::arrowheads::resolve(&mut src, "_ArchTick").unwrap();
        crate::entities::arrowheads::resolve(&mut src, "_Oblique").unwrap();
        let mut arch = codec::tables::DimStyle::standard();
        arch.name = "ARCH".into();
        arch.handle = src.allocate_handle();
        arch.dimtxsty_handle = src.text_styles.get("ROMANS").unwrap().handle;
        arch.dimtxsty = "ROMANS".into();
        arch.dimblk = arch_tick;
        arch.dimblk1 = arch_tick;
        arch.dimblk2 = arch_tick;
        arch.dimldrblk = arch_tick;
        let dashed = src.line_types.get("TRATTEGGIO_Q").unwrap().handle;
        arch.dimltex_handle = dashed;
        arch.dimltex1_handle = dashed;
        arch.dimltex2_handle = dashed;
        arch.dimasz = 0.2;
        src.dim_styles.add(arch).unwrap();
        let standard = src.dim_styles.get_mut("Standard").unwrap();
        standard.dimasz = 0.33;
        standard.dimtxt = 0.44;

        // Il blocco della quota, come lo scrive AutoCAD: le frecce sono
        // INSERT di blocchi (qui uno che lo stile non nomina).
        crate::app::define_block_raw_doc(
            &mut src,
            "*D7",
            Vector3::ZERO,
            &[
                EntityType::Line(codec::entities::Line::from_points(
                    Vector3::new(0.0, -1.0, 0.0),
                    Vector3::new(4.0, -1.0, 0.0),
                )),
                EntityType::Insert(Insert::new("_Oblique", Vector3::new(0.0, -1.0, 0.0))),
            ],
        );
        let mut quota = codec::entities::DimensionLinear::new(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(4.0, 0.0, 0.0),
        );
        quota.definition_point = Vector3::new(4.0, -1.0, 0.0);
        quota.base.style_name = "ARCH".into();
        quota.base.block_name = "*D7".into();
        src.add_entity(EntityType::Dimension(Box::new(
            codec::entities::Dimension::Linear(quota),
        )))
        .unwrap();
        let mut sullo_standard = codec::entities::DimensionLinear::new(
            Vector3::new(4.1, 0.0, 0.0),
            Vector3::new(8.0, 0.0, 0.0),
        );
        sullo_standard.definition_point = Vector3::new(8.0, -1.0, 0.0);
        sullo_standard.base.style_name = "Standard".into();
        src.add_entity(EntityType::Dimension(Box::new(
            codec::entities::Dimension::Linear(sullo_standard),
        )))
        .unwrap();

        // Stili di stampa dei layer: MURI su «Normal», TESTI su uno che `out`
        // non ha.
        let normal = plot_style_named(&src, "Normal").expect("segnaposto Normal");
        src.layers.get_mut(LAYER_MURI).unwrap().plotstyle_handle = normal;
        let mut testi = Layer::new("TESTI");
        testi.handle = src.allocate_handle();
        testi.plotstyle_handle = Handle::new(0xFEED);
        src.layers.add(testi).unwrap();
        let mut nota = codec::entities::Line::from_points(Vector3::ZERO, Vector3::UNIT_Y);
        nota.common.layer = "TESTI".into();
        src.add_entity(EntityType::Line(nota)).unwrap();

        #[track_caller]
        fn check(out: &CadDocument, what: &str) {
            let arch = out
                .dim_styles
                .get("ARCH")
                .unwrap_or_else(|| panic!("{what}: ARCH"));
            let text = handle_of_text_style(out, arch.dimtxsty_handle)
                .unwrap_or_else(|| panic!("{what}: DIMTXSTY non risolto"));
            assert!(text.name.eq_ignore_ascii_case("ROMANS"), "{what}");
            assert_eq!(text.font_file, "romans.shx", "{what}");
            for handle in [arch.dimblk, arch.dimblk1, arch.dimblk2, arch.dimldrblk] {
                assert_eq!(
                    block_named_by(out, handle).as_deref(),
                    Some("_ArchTick"),
                    "{what}: freccia"
                );
            }
            for handle in [
                arch.dimltex_handle,
                arch.dimltex1_handle,
                arch.dimltex2_handle,
            ] {
                assert_eq!(
                    line_type_named_by(out, handle).as_deref(),
                    Some("TRATTEGGIO_Q"),
                    "{what}: tipo di linea della quota"
                );
            }
            assert!((arch.dimasz - 0.2).abs() < 1e-12, "{what}");
            let standard = out.dim_styles.get("Standard").unwrap();
            assert!(
                (standard.dimasz - 0.33).abs() < 1e-12,
                "{what}: DIMASZ di Standard"
            );
            assert!(
                (standard.dimtxt - 0.44).abs() < 1e-12,
                "{what}: DIMTXT di Standard"
            );
            // Ogni INSERT, anche dentro i blocchi `*D`, punta a un blocco che c'è.
            assert!(out.block_records.get("_ArchTick").is_some(), "{what}");
            assert!(
                out.block_records.get("_Oblique").is_some(),
                "{what}: freccia del *D"
            );
            for entity in out.entities() {
                if let EntityType::Insert(insert) = entity {
                    assert!(
                        out.block_records.get(&insert.block_name).is_some(),
                        "{what}: INSERT di {} senza blocco",
                        insert.block_name
                    );
                }
            }
            // Il tipo di linea complesso punta al suo stile di testo.
            let gas = out
                .line_types
                .get("GAS_LT")
                .unwrap_or_else(|| panic!("{what}: GAS_LT"));
            let complex = gas
                .elements
                .iter()
                .find_map(|e| e.complex.as_ref())
                .unwrap_or_else(|| panic!("{what}: elemento complesso perso"));
            let style = handle_of_text_style(out, complex.style_handle)
                .unwrap_or_else(|| panic!("{what}: stile del tipo di linea non risolto"));
            assert_eq!(style.font_file, "simplex.shx", "{what}");
            // Stili di stampa dei layer.
            assert_eq!(
                Some(out.layers.get(LAYER_MURI).unwrap().plotstyle_handle),
                plot_style_named(out, "Normal"),
                "{what}: stile di stampa di MURI"
            );
            let testi = out.layers.get("TESTI").unwrap().plotstyle_handle;
            assert!(
                testi.is_null() || out.objects.contains_key(&testi),
                "{what}: stile di stampa di TESTI {testi:?}"
            );
        }

        let ex = extract(&src, &opts(&both()));
        assert!(ex.notices.is_empty(), "{:?}", ex.notices);
        for (_, out) in &ex.docs {
            check(out, "in memoria");
            assert!(out.layers.get("TESTI").unwrap().plotstyle_handle.is_null());
            assert_eq!(
                out.entities()
                    .filter(|e| matches!(e, EntityType::Dimension(_)))
                    .count(),
                2
            );
        }
        let tmp = TempDir::new("r31");
        for path in write(&ex, &tmp.0, "casa", false).unwrap() {
            let loaded = crate::io::load_file(&path).unwrap();
            check(&loaded, "dopo il DWG");
        }
    }

    // ---- Stato di progetto (Task 12) ----

    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() <= 1e-6 && (a[1] - b[1]).abs() <= 1e-6
    }

    fn xy(v: Vector3) -> [f64; 2] {
        [v.x, v.y]
    }

    /// Le `LINE` dello spazio modello di `doc`, con il layer.
    fn model_lines(doc: &CadDocument) -> Vec<(String, [f64; 2], [f64; 2])> {
        let model = doc
            .block_records
            .iter()
            .find(|r| r.is_model_space())
            .unwrap();
        model
            .entity_handles
            .iter()
            .filter_map(|&h| match doc.get_entity(h) {
                Some(EntityType::Line(l)) => Some((l.common.layer.clone(), xy(l.start), xy(l.end))),
                _ => None,
            })
            .collect()
    }

    /// I tratti dritti dello spazio modello: le `LINE` e i lati delle
    /// `LWPOLYLINE` (chiusura compresa).
    fn model_segments(doc: &CadDocument) -> Vec<([f64; 2], [f64; 2])> {
        let mut segments: Vec<_> = model_lines(doc)
            .into_iter()
            .map(|(_, a, b)| (a, b))
            .collect();
        for entity in doc.entities() {
            let EntityType::LwPolyline(p) = entity else {
                continue;
            };
            let points: Vec<[f64; 2]> = p
                .vertices
                .iter()
                .map(|v| [v.location.x, v.location.y])
                .collect();
            let count = if p.is_closed {
                points.len()
            } else {
                points.len().saturating_sub(1)
            };
            for k in 0..count {
                segments.push((points[k], points[(k + 1) % points.len()]));
            }
        }
        segments
    }

    /// C'è una `LINE` da `a` a `b`, in qualunque verso, entro 1e-6?
    fn has_line(doc: &CadDocument, a: [f64; 2], b: [f64; 2]) -> bool {
        model_lines(doc)
            .iter()
            .any(|(_, p, q)| (near(*p, a) && near(*q, b)) || (near(*p, b) && near(*q, a)))
    }

    /// Un tratto dritto su `x` (entro 1e-6) con una parte in `y0 < y < y1`?
    fn stretch_on_x(doc: &CadDocument, x: f64, (y0, y1): (f64, f64)) -> bool {
        model_segments(doc).iter().any(|(a, b)| {
            let on_x = (a[0] - x).abs() <= 1e-6 && (b[0] - x).abs() <= 1e-6;
            let (lo, hi) = (a[1].min(b[1]), a[1].max(b[1]));
            on_x && lo.max(y0) < hi.min(y1) - 1e-6
        })
    }

    fn sorted_texts(notices: &[Notice]) -> Vec<String> {
        let mut texts: Vec<String> = notices.iter().map(Notice::text).collect();
        texts.sort();
        texts
    }

    /// Nessuna campitura sui layer `AL-*`: le zone non entrano nel progetto.
    #[track_caller]
    fn assert_no_zone_hatches(out: &CadDocument) {
        assert!(
            !hatches(out)
                .iter()
                .any(|h| h.common.layer.to_ascii_uppercase().starts_with("AL-")),
            "campitura sui layer AL-* nel progetto"
        );
    }

    /// Caso A a linee continue, tramezzo demolito per intero: le facce del
    /// tramezzo spariscono, le due facce esterne si chiudono con una `LINE`
    /// ciascuna, il resto del rilievo resta com'è; il DWG si rilegge uguale.
    #[test]
    fn progetto_closes_outer_faces_and_has_no_tramezzo() {
        let src = doc_with_demolition(Form::Continua);
        let ex = extract(&src, &opts(&[Elaborato::StatoDiProgetto]));
        assert_eq!(ex.docs.len(), 1);
        let (e, out) = &ex.docs[0];
        assert_eq!(*e, Elaborato::StatoDiProgetto);
        assert!(ex.notices.is_empty(), "{:?}", ex.notices);

        #[track_caller]
        fn check(out: &CadDocument, src: &CadDocument, what: &str) {
            assert!(
                has_line(out, [4.0, 0.0], [4.1, 0.0]),
                "{what}: chiusura sud"
            );
            assert!(
                has_line(out, [4.0, 3.0], [4.1, 3.0]),
                "{what}: chiusura nord"
            );
            for x in [4.0, 4.1] {
                assert!(
                    !stretch_on_x(out, x, (0.0, 3.0)),
                    "{what}: faccia del tramezzo x = {x} rimasta"
                );
            }
            assert_no_zone_hatches(out);
            assert!(hatches(out).is_empty(), "{what}");
            assert_no_archline_data(out);
            // Le linee del rilievo fuori dalla zona sono le stesse, al micron.
            let out_lines = model_lines(out);
            let tramezzo_face = |a: &[f64; 2], b: &[f64; 2]| {
                [4.0, 4.1]
                    .iter()
                    .any(|x| (a[0] - x).abs() <= 1e-6 && (b[0] - x).abs() <= 1e-6)
            };
            let kept: Vec<_> = model_lines(src)
                .into_iter()
                .filter(|(_, a, b)| !tramezzo_face(a, b))
                .collect();
            assert_eq!(kept.len(), 10, "{what}");
            for (layer, a, b) in &kept {
                assert!(
                    out_lines
                        .iter()
                        .any(|(l, p, q)| l == layer && near(*p, *a) && near(*q, *b)),
                    "{what}: manca la LINE {a:?}-{b:?}"
                );
            }
            // 10 linee del rilievo e 2 chiusure, tutte sul layer dei muri.
            assert_eq!(out_lines.len(), 12, "{what}");
            assert!(out_lines.iter().all(|(l, _, _)| l == LAYER_MURI), "{what}");
            assert_eq!(model_space_count(out), 12, "{what}");
        }
        check(out, &src, "in memoria");

        let tmp = TempDir::new("progetto");
        let written = write(&ex, &tmp.0, "casa", false).unwrap();
        assert_eq!(written, vec![tmp.0.join("casa_stato-di-progetto.dwg")]);
        let loaded = crate::io::load_file(&written[0]).unwrap();
        assert_eq!(loaded.entities().count(), out.entities().count());
        assert_eq!(loaded.header.insertion_units, 6);
        check(&loaded, &src, "dopo il DWG");
    }

    /// Una polilinea chiusa per muro: le teste del tramezzo coincidono con i
    /// muri sud e nord, quindi niente chiusure e due giunzioni negli avvisi. I
    /// pezzi della polilinea tagliata non portano XDATA (nemmeno di altre
    /// applicazioni); i muri non toccati tengono quello degli altri.
    #[test]
    fn progetto_closed_walls_lists_junctions_and_pieces_lose_xdata() {
        let mut src = doc_with_demolition(Form::ChiusaPerMuro);
        for name in ["ALTRA_APP", "ARCHLINE_FUTURA"] {
            let mut app = codec::tables::AppId::new(name);
            app.handle = src.allocate_handle();
            src.app_ids.add(app).unwrap();
        }
        let first_vertex = |doc: &CadDocument, at: [f64; 2]| {
            doc.entities()
                .find_map(|e| match e {
                    EntityType::LwPolyline(p) if near(xy_of(p.vertices[0].location), at) => {
                        Some(p.common.handle)
                    }
                    _ => None,
                })
                .unwrap()
        };
        fn xy_of(v: Vector2) -> [f64; 2] {
            [v.x, v.y]
        }
        let tramezzo = first_vertex(&src, [4.0, 0.0]);
        let sud = first_vertex(&src, [-0.3, -0.3]);
        let mut other = ExtendedDataRecord::new("ALTRA_APP");
        other.add_value(XDataValue::Integer16(7));
        for handle in [tramezzo, sud] {
            let common = src.get_entity_mut(handle).unwrap().common_mut();
            common.extended_data.add_record(other.clone());
        }
        let mut future = ExtendedDataRecord::new("ARCHLINE_FUTURA");
        future.add_value(XDataValue::Integer16(1));
        src.get_entity_mut(tramezzo)
            .unwrap()
            .common_mut()
            .extended_data
            .add_record(future);
        let entities_before = src.entities().count();

        let ex = extract(&src, &opts(&[Elaborato::StatoDiProgetto]));
        let (_, out) = &ex.docs[0];
        assert_eq!(
            sorted_texts(&ex.notices),
            sorted_texts(&[
                Notice::GiunzioneDaVerificare { at: [4.05, 0.0] },
                Notice::GiunzioneDaVerificare { at: [4.05, 3.0] },
            ])
        );
        assert!(ex.notices.iter().all(|n| matches!(
            n,
            Notice::GiunzioneDaVerificare { at } if near(*at, [4.05, 0.0]) || near(*at, [4.05, 3.0])
        )));
        for x in [4.0, 4.1] {
            assert!(!stretch_on_x(out, x, (0.0, 3.0)), "faccia x = {x} rimasta");
        }
        assert!(model_lines(out).is_empty(), "nessuna chiusura: giunzioni");
        assert_no_zone_hatches(out);
        assert_no_archline_data(out);

        let polylines: Vec<&LwPolyline> = out
            .entities()
            .filter_map(|e| match e {
                EntityType::LwPolyline(p) => Some(p),
                _ => None,
            })
            .collect();
        // Sud, nord, ovest, est intere; del tramezzo restano le due teste.
        let (pieces, whole): (Vec<&LwPolyline>, Vec<&LwPolyline>) =
            polylines.into_iter().partition(|p| !p.is_closed);
        assert_eq!(whole.len(), 4);
        assert_eq!(pieces.len(), 2, "le teste del tramezzo");
        for piece in &pieces {
            assert_eq!(piece.common.layer, LAYER_MURI);
            assert!(piece
                .vertices
                .iter()
                .all(|v| (4.0 - 1e-6..=4.1 + 1e-6).contains(&v.location.x)));
            let xdata = &piece.common.extended_data;
            assert!(xdata.records().is_empty(), "XDATA sul pezzo: {xdata:?}");
            assert!(xdata.raw_dwg_eed.is_empty());
        }
        let with_other: Vec<_> = whole
            .iter()
            .filter(|p| p.common.extended_data.get_record("ALTRA_APP").is_some())
            .collect();
        assert_eq!(with_other.len(), 1, "il muro sud tiene l'XDATA di altri");
        assert!(out.app_ids.contains("ALTRA_APP"));

        // Il disegno di partenza non riceve né pezzi né chiusure.
        assert_eq!(src.entities().count(), entities_before);
        assert!(src
            .get_entity(tramezzo)
            .unwrap()
            .common()
            .extended_data
            .get_record("ARCHLINE_FUTURA")
            .is_some());

        let tmp = TempDir::new("giunzioni");
        let written = write(&ex, &tmp.0, "casa", false).unwrap();
        let loaded = crate::io::load_file(&written[0]).unwrap();
        assert_no_archline_data(&loaded);
        assert_eq!(model_space_count(&loaded), model_space_count(out));
    }

    /// I muri nuovi sul layer scelto: se manca nel disegno, e il disegno non ha
    /// muri da cui prendere colore e tipo di linea, lo si crea con i valori
    /// predefiniti (e un handle suo), e il DWG lo tiene (senza, le linee
    /// finirebbero sul layer 0). Il nome si legge senza spazi attorno.
    #[test]
    fn new_walls_go_on_the_chosen_layer_created_with_defaults() {
        let mut src = CadDocument::new();
        src.header.insertion_units = 6;
        ensure_layers(&mut src);
        add_new_wall(&mut src, [2.0, 0.5], [2.0, 2.5], 0.12);
        assert_eq!(propose_new_walls_layer(&src), None, "nessun muro");
        assert!(src.layers.get("MURI-NUOVI").is_none());
        let mut o = opts(&[Elaborato::StatoDiProgetto]);
        o.new_walls_layer = "  MURI-NUOVI ".into();
        let ex = extract(&src, &o);
        assert!(ex.notices.is_empty(), "{:?}", ex.notices);
        let (_, out) = &ex.docs[0];

        #[track_caller]
        fn check(out: &CadDocument, what: &str) {
            let new: Vec<_> = model_lines(out)
                .into_iter()
                .filter(|(l, _, _)| l.eq_ignore_ascii_case("MURI-NUOVI"))
                .collect();
            assert_eq!(new.len(), 4, "{what}: lati del muro nuovo");
            for (a, b) in [
                ([1.94, 0.5], [1.94, 2.5]),
                ([2.06, 0.5], [2.06, 2.5]),
                ([1.94, 0.5], [2.06, 0.5]),
                ([1.94, 2.5], [2.06, 2.5]),
            ] {
                assert!(
                    new.iter()
                        .any(|(_, p, q)| (near(*p, a) && near(*q, b))
                            || (near(*p, b) && near(*q, a))),
                    "{what}: manca il lato {a:?}-{b:?}"
                );
            }
            let layer = out.layers.get("MURI-NUOVI").expect("layer dei muri nuovi");
            assert!(!layer.handle.is_null(), "{what}");
            assert_eq!(
                out.layers
                    .iter()
                    .filter(|l| l.handle == layer.handle)
                    .count(),
                1,
                "{what}: handle doppio"
            );
            assert!(!out.objects.contains_key(&layer.handle), "{what}");
            let defaults = Layer::new("MURI-NUOVI");
            assert_eq!(layer.color, defaults.color, "{what}");
            assert!(layer.line_type.eq_ignore_ascii_case(&defaults.line_type));
            assert_eq!(layer.line_weight, defaults.line_weight, "{what}");
        }
        check(out, "in memoria");
        let tmp = TempDir::new("layer-nuovo");
        let written = write(&ex, &tmp.0, "casa", false).unwrap();
        check(&crate::io::load_file(&written[0]).unwrap(), "dopo il DWG");

        // Un nome vuoto vale il layer 0.
        let mut o = opts(&[Elaborato::StatoDiProgetto]);
        o.new_walls_layer = "   ".into();
        let ex = extract(&src, &o);
        let on_zero = model_lines(&ex.docs[0].1)
            .into_iter()
            .filter(|(l, _, _)| l == "0")
            .count();
        assert_eq!(on_zero, 4);
        assert!(ex.docs[0].1.layers.get("").is_none());
    }

    /// R46 (spec §5.5, «con colore e tipo di linea dei muri esistenti»): un
    /// layer dei muri nuovi che il disegno non ha prende colore, tipo di linea
    /// (copiato) e spessore del layer più frequente dei muri, anche dopo il DWG.
    #[test]
    fn new_walls_layer_missing_from_the_drawing_takes_the_walls_layer() {
        let mut src = doc_with_demolition(Form::Continua);
        let mut dashed = LineType::new("DASHED");
        dashed.description = "__ __ __".into();
        dashed.handle = src.allocate_handle();
        src.line_types.add(dashed).unwrap();
        let muri = src.layers.get_mut(LAYER_MURI).unwrap();
        muri.color = Color::from_index(3);
        muri.line_type = "DASHED".into();
        muri.line_weight = codec::LineWeight::Value(35);
        // Un layer di muri meno frequente, d'altro colore: non conta.
        let mut tramezzi = Layer::new("TRAMEZZI");
        tramezzi.handle = src.allocate_handle();
        tramezzi.color = Color::from_index(5);
        src.layers.add(tramezzi).unwrap();
        for y in [10.0, 11.0] {
            let mut line = codec::entities::Line::from_points(
                Vector3::new(20.0, y, 0.0),
                Vector3::new(25.0, y, 0.0),
            );
            line.common.layer = "TRAMEZZI".into();
            src.add_entity(EntityType::Line(line)).unwrap();
        }
        add_new_wall(&mut src, [2.0, 0.5], [2.0, 2.5], 0.12);
        assert!(src.layers.get("MURI_NUOVI").is_none());
        let mut o = opts(&[Elaborato::StatoDiProgetto]);
        o.new_walls_layer = "MURI_NUOVI".into();

        #[track_caller]
        fn check(out: &CadDocument, what: &str) {
            let layer = out.layers.get("MURI_NUOVI").expect("layer dei muri nuovi");
            assert!(!layer.handle.is_null(), "{what}");
            assert_eq!(layer.color, Color::from_index(3), "{what}");
            assert_eq!(layer.line_type, "DASHED", "{what}");
            assert_eq!(layer.line_weight, codec::LineWeight::Value(35), "{what}");
            let lt = out.line_types.get("DASHED").expect("tipo di linea DASHED");
            assert_eq!(lt.description, "__ __ __", "{what}");
            assert!(!lt.handle.is_null(), "{what}");
            let on_new = model_lines(out)
                .into_iter()
                .filter(|(l, _, _)| l.eq_ignore_ascii_case("MURI_NUOVI"))
                .count();
            assert_eq!(on_new, 4, "{what}: lati del muro nuovo");
        }
        let ex = extract(&src, &o);
        let (_, out) = &ex.docs[0];
        check(out, "in memoria");
        let tmp = TempDir::new("layer-dei-muri");
        let written = write(&ex, &tmp.0, "casa", false).unwrap();
        check(&crate::io::load_file(&written[0]).unwrap(), "dopo il DWG");
        assert_eq!(
            new_walls_layer_template(&src, "MURI_NUOVI").map(|l| l.name.as_str()),
            Some(LAYER_MURI)
        );
        assert_eq!(
            new_walls_layer_template(&src, "tramezzi").map(|l| l.name.as_str()),
            Some("TRAMEZZI"),
            "l'omonimo prima del più frequente"
        );
    }

    /// Il layer dei muri nuovi omonimo di un layer del disegno ne prende
    /// colore, tipo di linea (copiato) e spessore, anche quando nessuna entità
    /// del progetto lo usa.
    #[test]
    fn new_walls_layer_takes_the_source_layer_settings() {
        let mut src = doc_with_demolition(Form::Continua);
        let mut dashed = LineType::new("DASHED");
        dashed.description = "__ __ __".into();
        dashed.handle = src.allocate_handle();
        src.line_types.add(dashed).unwrap();
        let mut nuovi = Layer::new("NUOVI");
        nuovi.handle = src.allocate_handle();
        nuovi.color = Color::from_index(3);
        nuovi.line_type = "DASHED".into();
        nuovi.line_weight = codec::LineWeight::Value(35);
        src.layers.add(nuovi).unwrap();
        let mut o = opts(&[Elaborato::StatoDiProgetto]);
        o.new_walls_layer = "nuovi".into();

        #[track_caller]
        fn check(out: &CadDocument, what: &str) {
            let layer = out.layers.get("NUOVI").expect("layer dei muri nuovi");
            assert!(!layer.handle.is_null(), "{what}");
            assert_eq!(layer.color, Color::from_index(3), "{what}");
            assert_eq!(layer.line_type, "DASHED", "{what}");
            assert_eq!(layer.line_weight, codec::LineWeight::Value(35), "{what}");
            let lt = out.line_types.get("DASHED").expect("tipo di linea DASHED");
            assert_eq!(lt.description, "__ __ __", "{what}");
            assert!(!lt.handle.is_null(), "{what}");
        }

        // Nessun muro nuovo: il layer si crea dai valori di src.
        let ex = extract(&src, &o);
        let (_, out) = &ex.docs[0];
        assert!(model_lines(out).iter().all(|(l, _, _)| l == LAYER_MURI));
        check(out, "senza muri nuovi");
        let tmp = TempDir::new("layer-src");
        let written = write(&ex, &tmp.0, "casa", false).unwrap();
        check(&crate::io::load_file(&written[0]).unwrap(), "dopo il DWG");

        // Con un muro nuovo le sue linee lo usano: arriva con la copia.
        add_new_wall(&mut src, [2.0, 0.5], [2.0, 2.5], 0.12);
        let ex = extract(&src, &o);
        let (_, out) = &ex.docs[0];
        let on_new = model_lines(out)
            .into_iter()
            .filter(|(l, _, _)| l.eq_ignore_ascii_case("NUOVI"))
            .count();
        assert_eq!(on_new, 4);
        check(out, "con muri nuovi");
    }

    /// Le entità che non sono muri restano, tranne zone, locali ed elementi
    /// dentro le zone demolite (elencati); i muri fuori piano non si tagliano e
    /// lo dice un avviso.
    #[test]
    fn progetto_keeps_other_entities_and_lists_what_it_leaves_out() {
        let mut src = doc_with_zones();
        // Un testo dentro la zona demolita: escluso.
        let mut dentro = codec::entities::Text::with_value("via", Vector3::new(4.05, 1.5, 0.0));
        dentro.common.layer = "TESTI".into();
        let dentro = src.add_entity(EntityType::Text(dentro)).unwrap();
        // Un testo nel locale A: resta.
        let mut fuori = codec::entities::Text::with_value("A", Vector3::new(1.0, 1.0, 0.0));
        fuori.common.layer = "TESTI".into();
        src.add_entity(EntityType::Text(fuori)).unwrap();
        // Un locale (contorno e testo): non entra nel progetto.
        let mut contorno = LwPolyline::from_points(vec![
            Vector2::new(0.5, 0.5),
            Vector2::new(3.5, 0.5),
            Vector2::new(3.5, 2.5),
        ]);
        contorno.common.layer = LAYER_LOCALI.into();
        src.add_entity(EntityType::LwPolyline(contorno)).unwrap();
        let mut seme = MText::with_value("Soggiorno", Vector3::new(2.0, 1.5, 0.0));
        seme.common.layer = LAYER_LOCALI_PROGETTO.into();
        src.add_entity(EntityType::MText(Box::new(seme))).unwrap();
        // Un muro fuori dal piano XY, lontano dalle zone.
        let alto = codec::entities::Line::from_points(
            Vector3::new(10.0, 0.0, 1.0),
            Vector3::new(12.0, 0.0, 1.0),
        );
        let mut alto = EntityType::Line(alto);
        alto.common_mut().layer = LAYER_MURI.into();
        src.add_entity(alto).unwrap();

        let ex = extract(&src, &opts(&[Elaborato::StatoDiProgetto]));
        let (_, out) = &ex.docs[0];
        assert!(
            ex.notices.contains(&Notice::Esclusa {
                handle: dentro,
                kind: "TEXT".into()
            }),
            "{:?}",
            ex.notices
        );
        assert!(
            ex.notices.contains(&Notice::FuoriPiano { count: 1 }),
            "{:?}",
            ex.notices
        );
        assert_eq!(ex.notices.len(), 2, "{:?}", ex.notices);
        let texts: Vec<&str> = out
            .entities()
            .filter_map(|e| match e {
                EntityType::Text(t) => Some(t.value.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["A"]);
        assert!(!out.entities().any(|e| e
            .common()
            .layer
            .to_ascii_uppercase()
            .starts_with(LAYER_LOCALI)));
        assert!(!out.entities().any(|e| matches!(e, EntityType::MText(_))));
        assert!(out.entities().any(|e| matches!(
            e,
            EntityType::Line(l) if (l.start.z - 1.0).abs() < 1e-9
        )));
        assert_no_zone_hatches(out);
        assert_no_archline_data(out);
        // Rilievo: 10 linee + 2 chiusure + 4 lati del muro nuovo + quella fuori
        // piano; più il testo del locale A.
        assert_eq!(model_lines(out).len(), 17);
        assert_eq!(model_space_count(out), 18);
    }

    #[test]
    fn proposal_is_most_frequent_wall_layer() {
        let line_on = |layer: &str, y: f64| {
            let mut line = codec::entities::Line::from_points(
                Vector3::new(20.0, y, 0.0),
                Vector3::new(25.0, y, 0.0),
            );
            line.common.layer = layer.into();
            EntityType::Line(line)
        };
        let mut doc = CadDocument::new();
        assert_eq!(propose_new_walls_layer(&doc), None, "disegno vuoto");
        // Ciò che non è muro non conta: testi, layer spenti o congelati, AL-*.
        for k in 0..20 {
            let mut text =
                codec::entities::Text::with_value("x", Vector3::new(0.0, f64::from(k), 0.0));
            text.common.layer = "TESTI".into();
            doc.add_entity(EntityType::Text(text)).unwrap();
        }
        for (name, freeze) in [("SPENTO", false), ("CONGELATO", true)] {
            let mut layer = Layer::new(name);
            layer.handle = doc.allocate_handle();
            if freeze {
                layer.freeze();
            } else {
                layer.turn_off();
            }
            doc.layers.add(layer).unwrap();
        }
        for k in 0..20 {
            for layer in ["SPENTO", "CONGELATO", "AL-NUOVE"] {
                doc.add_entity(line_on(layer, f64::from(k))).unwrap();
            }
        }
        assert_eq!(propose_new_walls_layer(&doc), None, "nessun muro");

        for entity in case_a(Form::Continua, M) {
            doc.add_entity(entity).unwrap();
        }
        for y in [10.0, 11.0] {
            doc.add_entity(line_on("TRAMEZZI", y)).unwrap();
        }
        assert_eq!(propose_new_walls_layer(&doc), Some("MURI".into()));

        // A parità vince l'ordine alfabetico; il nome è quello della tabella.
        let mut doc = CadDocument::new();
        let mut layer = Layer::new("Bordi");
        layer.handle = doc.allocate_handle();
        doc.layers.add(layer).unwrap();
        for (layer, y) in [("bordi", 0.0), ("BORDI", 1.0), ("Zeta", 2.0), ("zeta", 3.0)] {
            doc.add_entity(line_on(layer, y)).unwrap();
        }
        doc.add_entity(line_on("ALFA", 4.0)).unwrap();
        assert_eq!(propose_new_walls_layer(&doc), Some("Bordi".into()));
    }

    // ---- Locali (Task 15) ----

    /// Il locale di fatto attorno a `seed` (in unità del disegno) scritto in
    /// `doc` come lo scrive `AL_LOCALE`: contorno e testo misurati sul rilievo
    /// alla tolleranza dei locali `tol`, con l'altezza di testo del disegno.
    /// Gli handle del contorno e del testo.
    fn write_room(
        doc: &mut CadDocument,
        unit: DrawingUnit,
        tol: f64,
        name: &str,
        seed: [f64; 2],
    ) -> (Handle, Handle) {
        let walls = wall_set(doc, unit.tolerance());
        let faces = wall_faces(&walls.sources, tol);
        let found =
            measure_at(&faces, None, &walls.sources, seed, unit, tol).expect("locale chiuso");
        let computed = Computed {
            name: name.into(),
            number: String::new(),
            area_m2: found.area_m2,
            region: found.region,
            confirm: false,
            exact: found.exact,
            islands: found.islands,
            seed,
        };
        let height = room_text_height(doc, unit).0;
        let handles: Vec<Handle> =
            room_entities(&computed, Phase::Fatto, &walls.sources, height, tol)
                .into_iter()
                .map(|entity| doc.add_entity(entity).unwrap())
                .collect();
        (handles[0], handles[1])
    }

    /// Il seme di un locale di progetto, come lo scrive `AL_LOCALE` in fase di
    /// progetto.
    fn write_seed(doc: &mut CadDocument, name: &str, seed: [f64; 2]) -> Handle {
        let room = Room {
            phase: Phase::Progetto,
            name: name.into(),
            number: String::new(),
            seed,
        };
        doc.add_entity(seed_entity(&room, 2.5)).unwrap()
    }

    /// Le entità dello spazio modello su `layer`, in ordine di disegno.
    fn model_on<'a>(doc: &'a CadDocument, layer: &str) -> Vec<&'a EntityType> {
        doc.block_records
            .iter()
            .filter(|r| r.is_model_space())
            .flat_map(|r| r.entity_handles.iter())
            .filter_map(|&h| doc.get_entity(h))
            .filter(|e| e.common().layer.eq_ignore_ascii_case(layer))
            .collect()
    }

    /// I testi (`MTEXT`) su `layer`, in ordine di disegno.
    fn labels_on(doc: &CadDocument, layer: &str) -> Vec<String> {
        model_on(doc, layer)
            .into_iter()
            .filter_map(|e| match e {
                EntityType::MText(t) => Some(t.value.clone()),
                _ => None,
            })
            .collect()
    }

    /// Le polilinee su `layer`, in ordine di disegno.
    fn outlines_on<'a>(doc: &'a CadDocument, layer: &str) -> Vec<&'a LwPolyline> {
        model_on(doc, layer)
            .into_iter()
            .filter_map(|e| match e {
                EntityType::LwPolyline(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    /// L'area dei vertici (lati dritti).
    fn polygon_area(p: &LwPolyline) -> f64 {
        let v = &p.vertices;
        let twice: f64 = (0..v.len())
            .map(|k| {
                let (a, b) = (v[k].location, v[(k + 1) % v.len()].location);
                a.x * b.y - b.x * a.y
            })
            .sum();
        twice.abs() / 2.0
    }

    /// Toglie un'entità dal disegno e dallo spazio modello (come CANCELLA).
    fn erase(doc: &mut CadDocument, handle: Handle) {
        doc.remove_entity(handle).expect("entità da cancellare");
        for record in doc.block_records.iter_mut() {
            record.entity_handles.retain(|&h| h != handle);
        }
    }

    /// La faccia del tramezzo verso A passa da x = 4,00 a 3,90 (in metri):
    /// A diventa 3,90 × 3,00 = 11,70 m². Una modifica a mano del rilievo,
    /// senza `AL_AGGIORNA`.
    fn move_tramezzo_face(doc: &mut CadDocument, form: Form, unit: DrawingUnit) {
        let (from, to) = (unit.metres(4.0), unit.metres(3.9));
        let on = |x: f64| (x - from).abs() < 1e-9;
        let handle = doc
            .entities()
            .find(|e| match e {
                EntityType::Line(l) => form == Form::Continua && on(l.start.x) && on(l.end.x),
                EntityType::LwPolyline(p) => {
                    form == Form::ChiusaPerMuro
                        && p.common.layer == LAYER_MURI
                        && p.vertices.iter().filter(|v| on(v.location.x)).count() == 2
                        && p.vertices
                            .iter()
                            .any(|v| (v.location.x - unit.metres(4.1)).abs() < 1e-9)
                }
                _ => false,
            })
            .map(|e| e.common().handle)
            .expect("faccia del tramezzo");
        match doc.get_entity_mut(handle) {
            Some(EntityType::Line(l)) => {
                l.start.x = to;
                l.end.x = to;
            }
            Some(EntityType::LwPolyline(p)) => {
                for v in &mut p.vertices {
                    if on(v.location.x) {
                        v.location.x = to;
                    }
                }
            }
            other => panic!("{other:?}"),
        }
    }

    /// Il tramezzo del caso A a polilinee si ferma 2 mm prima del muro nord: A
    /// e B diventano una regione sola (R54).
    fn open_tramezzo(doc: &mut CadDocument) {
        let handle = doc
            .entities()
            .find(|e| {
                matches!(e, EntityType::LwPolyline(p) if p.vertices.iter().any(|v| near(xy2(v.location), [4.1, 3.0])))
            })
            .map(|e| e.common().handle)
            .expect("tramezzo");
        if let Some(EntityType::LwPolyline(p)) = doc.get_entity_mut(handle) {
            for v in &mut p.vertices {
                if (v.location.y - 3.0).abs() < 1e-9 {
                    v.location.y = 2.998;
                }
            }
        }
    }

    fn xy2(v: Vector2) -> [f64; 2] {
        [v.x, v.y]
    }

    /// Il caso A a linee continue con l'angolo (0; 0) di A aperto di 2 mm sulla
    /// faccia ovest (Review Focus 3).
    fn case_a_with_corner_gap() -> CadDocument {
        let mut doc = case_a_doc(Form::Continua);
        let west = doc
            .entities()
            .find(|e| matches!(e, EntityType::Line(l) if near(xy(l.start), [0.0, 3.0]) && near(xy(l.end), [0.0, 0.0])))
            .map(|e| e.common().handle)
            .expect("faccia ovest di A");
        if let Some(EntityType::Line(l)) = doc.get_entity_mut(west) {
            l.end = Vector3::new(0.0, 0.002, 0.0);
        }
        doc
    }

    /// Spec §4.2 e §5.5: lo stato di fatto ricalcola da capo i locali di fatto
    /// e non si fida di ciò che è scritto. Locale A scritto (12,00), poi la
    /// faccia del tramezzo da x = 4,00 a 3,90 senza `AL_AGGIORNA`: nel DWG di
    /// fatto contorno e testo dicono 11,70, senza il nostro XDATA; il disegno
    /// di lavoro resta a 12,00; una riga dice che la sua etichetta è vecchia.
    #[test]
    fn fatto_recomputes_stale_labels() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let mut src = case_a_doc(form);
            let (outline, text) = write_room(&mut src, M, 1e-6, "A", [2.0, 1.5]);
            move_tramezzo_face(&mut src, form, M);
            let written = (
                src.get_entity(outline).unwrap().clone(),
                src.get_entity(text).unwrap().clone(),
            );

            let ex = extract(&src, &opts(&[Elaborato::StatoDiFatto]));
            assert_eq!(
                sorted_texts(&ex.notices),
                ["Locale A: etichetta nel disegno 12,00 → 11,70 m² nell'estratto."],
                "{form:?}"
            );

            #[track_caller]
            fn check(out: &CadDocument, what: &str) {
                assert_eq!(labels_on(out, LAYER_LOCALI), ["A\\PS = 11,70 m²"], "{what}");
                let outlines = outlines_on(out, LAYER_LOCALI);
                assert_eq!(outlines.len(), 1, "{what}");
                let outline = outlines[0];
                assert!(outline.is_closed, "{what}");
                assert!((polygon_area(outline) - 11.7).abs() < 1e-9, "{what}");
                let at = |x: f64| {
                    outline
                        .vertices
                        .iter()
                        .any(|v| (v.location.x - x).abs() < 1e-9)
                };
                assert!(at(3.9) && !at(4.0), "{what}: il contorno segue il tramezzo");
                assert_no_archline_data(out);
            }
            check(&ex.docs[0].1, &format!("{form:?} in memoria"));
            // Il disegno di lavoro non cambia: contorno e testo di prima.
            assert_eq!(src.get_entity(outline), Some(&written.0), "{form:?}");
            assert_eq!(src.get_entity(text), Some(&written.1), "{form:?}");
            let EntityType::MText(old) = &written.1 else {
                panic!("testo del locale");
            };
            assert_eq!(old.value, "A\\PS = 12,00 m²");

            let tmp = TempDir::new("fatto-ricalcolo");
            let paths = write(&ex, &tmp.0, "casa", false).unwrap();
            let loaded = crate::io::load_file(&paths[0]).unwrap();
            check(&loaded, &format!("{form:?} dopo il DWG"));
        }
    }

    /// Spec §5.4 e §7, caso A: con i locali A e B scritti e il tramezzo
    /// demolito per intero, nello stato di progetto i due locali sono uno solo,
    /// «A + B (da confermare)» da 24,00 m², con contorno e testo nuovi sul
    /// layer `AL-LOCALI` (testo sul seme di A), senza XDATA; il seme di
    /// progetto non c'è. Lo stato di fatto tiene A e B com'erano, la
    /// comparativa nessun locale. Un solo `LocaleFuso`.
    #[test]
    fn progetto_has_merged_room_label() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let mut src = case_a_doc(form);
            write_room(&mut src, M, 1e-6, "A", [2.0, 1.5]);
            write_room(&mut src, M, 1e-6, "B", [6.0, 1.5]);
            add_demolition(&mut src, [4.05, 0.0], [4.05, 3.0]);
            let before: Vec<EntityType> = src.entities().cloned().collect();

            let ex = extract(&src, &opts(&all_three()));
            let fused = Notice::LocaleFuso {
                names: vec!["A".into(), "B".into()],
                area_m2: 24.0,
            }
            .text();
            assert_eq!(
                ex.notices.iter().filter(|n| n.text() == fused).count(),
                1,
                "{form:?}: {:?}",
                ex.notices
            );
            assert!(
                ex.notices.iter().all(|n| matches!(
                    n,
                    Notice::LocaleFuso { .. } | Notice::GiunzioneDaVerificare { .. }
                )),
                "{form:?}: {:?}",
                ex.notices
            );
            let (fatto, comparativa) = (&ex.docs[0].1, &ex.docs[1].1);
            assert_eq!(
                labels_on(fatto, LAYER_LOCALI),
                ["A\\PS = 12,00 m²", "B\\PS = 11,70 m²"],
                "{form:?}"
            );
            assert!(model_on(comparativa, LAYER_LOCALI).is_empty(), "{form:?}");

            #[track_caller]
            fn check(out: &CadDocument, what: &str) {
                assert_eq!(
                    labels_on(out, LAYER_LOCALI),
                    ["A + B (da confermare)\\PS = 24,00 m²"],
                    "{what}"
                );
                let label = model_on(out, LAYER_LOCALI)
                    .into_iter()
                    .find_map(|e| match e {
                        EntityType::MText(t) => Some(t),
                        _ => None,
                    })
                    .unwrap();
                assert!(near(xy(label.insertion_point), [2.0, 1.5]), "{what}");
                assert!((label.height - 2.5).abs() < 1e-9, "{what}: TEXTSIZE");
                let outlines = outlines_on(out, LAYER_LOCALI);
                assert_eq!(outlines.len(), 1, "{what}");
                assert!(outlines[0].is_closed, "{what}");
                assert!((polygon_area(outlines[0]) - 24.0).abs() < 1e-9, "{what}");
                assert!(model_on(out, LAYER_LOCALI_PROGETTO).is_empty(), "{what}");
                let layer = out.layers.get(LAYER_LOCALI).expect("layer dei locali");
                assert!(!layer.handle.is_null(), "{what}");
                assert_eq!(
                    out.layers
                        .iter()
                        .filter(|l| l.handle == layer.handle)
                        .count(),
                    1,
                    "{what}: handle doppio"
                );
                assert_no_zone_hatches(out);
                assert_no_archline_data(out);
            }
            check(&ex.docs[2].1, &format!("{form:?} in memoria"));
            assert!(
                src.entities().cloned().collect::<Vec<_>>() == before,
                "{form:?}: il disegno di lavoro è cambiato"
            );

            let tmp = TempDir::new("progetto-locali");
            let paths = write(&ex, &tmp.0, "casa", false).unwrap();
            let loaded = crate::io::load_file(&paths[2]).unwrap();
            check(&loaded, &format!("{form:?} dopo il DWG"));
        }
    }

    /// I semi di progetto danno il nome; i loro avvisi vanno tra quelli
    /// dell'estrazione, una volta sola; i semi non entrano nel DWG. Un disegno
    /// con i soli semi, senza il layer `AL-LOCALI` (tolto a mano): il layer
    /// nasce nel progetto con un handle suo e il colore 7.
    #[test]
    fn progetto_seeds_name_the_rooms_and_bring_the_rooms_layer() {
        let mut src = doc_with_demolition(Form::Continua);
        add_new_wall(&mut src, [6.0, 0.0], [6.0, 3.0], 0.12);
        write_seed(&mut src, "Soggiorno", [2.0, 1.5]);
        write_seed(&mut src, "Bagno", [6.0, 1.5]);
        src.layers.remove(LAYER_LOCALI).expect("layer dei locali");
        assert!(src.layers.get(LAYER_LOCALI).is_none());

        let ex = extract(&src, &opts(&all_three()));
        let in_wall = Notice::SemeInMuroNuovo {
            name: "Bagno".into(),
        };
        assert_eq!(
            ex.notices.iter().filter(|n| **n == in_wall).count(),
            1,
            "{:?}",
            ex.notices
        );
        // Fatto e comparativa: niente locali, nessun avviso dei locali.
        for (_, out) in &ex.docs[..2] {
            assert!(model_on(out, LAYER_LOCALI).is_empty());
            assert!(model_on(out, LAYER_LOCALI_PROGETTO).is_empty());
        }

        #[track_caller]
        fn check(out: &CadDocument, what: &str) {
            // A + tramezzo + la parte di B fino al muro nuovo: 5,94 × 3.
            assert_eq!(
                labels_on(out, LAYER_LOCALI),
                ["Soggiorno\\PS = 17,82 m²"],
                "{what}"
            );
            assert!(model_on(out, LAYER_LOCALI_PROGETTO).is_empty(), "{what}");
            assert!(
                !out.entities()
                    .any(|e| matches!(e, EntityType::MText(t) if t.value == "Bagno")),
                "{what}: il seme è nel DWG"
            );
            let layer = out.layers.get(LAYER_LOCALI).expect("layer dei locali");
            assert!(!layer.handle.is_null(), "{what}");
            assert_eq!(layer.color, Color::from_index(7), "{what}");
            assert_no_archline_data(out);
        }
        check(&ex.docs[2].1, "in memoria");
        let tmp = TempDir::new("progetto-semi");
        let paths = write(&ex, &tmp.0, "casa", false).unwrap();
        check(&crate::io::load_file(&paths[2]).unwrap(), "dopo il DWG");
    }

    /// R55 e R50 nell'estrazione: un locale non più chiuso e due locali fusi da
    /// un vuoto restano com'erano nello stato di fatto, con i loro avvisi, e
    /// non danno la riga dell'etichetta.
    #[test]
    fn fatto_leaves_open_and_fused_rooms_as_they_were() {
        // Non chiuso: la faccia sud di A cancellata dopo aver scritto A.
        let mut src = case_a_doc(Form::Continua);
        write_room(&mut src, M, 1e-6, "A", [2.0, 1.5]);
        let written: Vec<EntityType> = model_on(&src, LAYER_LOCALI).into_iter().cloned().collect();
        let south = src
            .entities()
            .find(|e| matches!(e, EntityType::Line(l) if near(xy(l.start), [0.0, 0.0]) && near(xy(l.end), [4.0, 0.0])))
            .map(|e| e.common().handle)
            .expect("faccia sud di A");
        erase(&mut src, south);
        let ex = extract(&src, &opts(&[Elaborato::StatoDiFatto]));
        assert_eq!(
            ex.notices,
            [Notice::LocaleNonChiuso { name: "A".into() }],
            "{:?}",
            ex.notices
        );
        let out = &ex.docs[0].1;
        assert_eq!(labels_on(out, LAYER_LOCALI), ["A\\PS = 12,00 m²"]);
        let outlines = outlines_on(out, LAYER_LOCALI);
        let EntityType::LwPolyline(old) = &written[0] else {
            panic!("contorno");
        };
        assert_eq!(outlines.len(), 1);
        assert_eq!(outlines[0].vertices, old.vertices, "contorno com'era");

        // Fusi: il tramezzo a polilinea si ferma 2 mm prima del muro nord.
        let mut src = case_a_doc(Form::ChiusaPerMuro);
        write_room(&mut src, M, 1e-6, "A", [2.0, 1.5]);
        write_room(&mut src, M, 1e-6, "B", [6.0, 1.5]);
        let written: Vec<Vec<codec::entities::LwVertex>> = outlines_on(&src, LAYER_LOCALI)
            .into_iter()
            .map(|p| p.vertices.clone())
            .collect();
        open_tramezzo(&mut src);
        let ex = extract(&src, &opts(&[Elaborato::StatoDiFatto]));
        assert_eq!(
            sorted_texts(&ex.notices),
            [
                "Locali A + B fusi in uno (23,70 m²): nome da confermare.",
                "Locali A + B lasciati com'erano.",
            ],
            "{:?}",
            ex.notices
        );
        let out = &ex.docs[0].1;
        assert_eq!(
            labels_on(out, LAYER_LOCALI),
            ["A\\PS = 12,00 m²", "B\\PS = 11,70 m²"]
        );
        let outlines: Vec<_> = outlines_on(out, LAYER_LOCALI)
            .into_iter()
            .map(|p| p.vertices.clone())
            .collect();
        assert_eq!(outlines, written, "contorni com'erano");
    }

    /// R61: i locali dell'estratto si misurano alla tolleranza dei locali
    /// (`room_tol`, cioè con `HPGAPTOL`), i muri e i tagli a `tol`. Un vuoto di
    /// 2 mm all'angolo di A: A scritto con HPGAPTOL 5 mm (12,00), poi la faccia
    /// del tramezzo a 3,90. Senza HPGAPTOL A non è chiuso, nel fatto e nel
    /// progetto, e resta com'era; con HPGAPTOL 5 mm vale 11,70 in tutti e due,
    /// con l'avviso della tolleranza.
    #[test]
    fn rooms_use_the_rooms_tolerance() {
        let hpgaptol = 0.005;
        let mut src = case_a_with_corner_gap();
        write_room(&mut src, M, hpgaptol, "A", [2.0, 1.5]);
        assert_eq!(labels_on(&src, LAYER_LOCALI), ["A\\PS = 12,00 m²"]);
        move_tramezzo_face(&mut src, Form::Continua, M);
        let elaborati = [Elaborato::StatoDiFatto, Elaborato::StatoDiProgetto];

        let ex = extract(&src, &opts(&elaborati));
        assert_eq!(
            ex.notices,
            [Notice::LocaleNonChiuso { name: "A".into() }],
            "una volta sola per i due elaborati"
        );
        assert_eq!(labels_on(&ex.docs[0].1, LAYER_LOCALI), ["A\\PS = 12,00 m²"]);
        assert!(model_on(&ex.docs[1].1, LAYER_LOCALI).is_empty());

        let mut o = opts(&elaborati);
        o.room_tol = hpgaptol;
        let ex = extract(&src, &o);
        assert_eq!(
            sorted_texts(&ex.notices),
            [
                "Locale A: chiuso solo dalla tolleranza HPGAPTOL, superficie da verificare.",
                "Locale A: etichetta nel disegno 12,00 → 11,70 m² nell'estratto.",
            ],
            "{:?}",
            ex.notices
        );
        assert_eq!(labels_on(&ex.docs[0].1, LAYER_LOCALI), ["A\\PS = 11,70 m²"]);
        assert_eq!(labels_on(&ex.docs[1].1, LAYER_LOCALI), ["A\\PS = 11,70 m²"]);
        // Una `room_tol` sotto `tol` vale `tol`.
        let mut o = opts(&elaborati);
        o.room_tol = 0.0;
        assert_eq!(
            extract(&src, &o).notices,
            [Notice::LocaleNonChiuso { name: "A".into() }]
        );
    }

    /// Caso C nell'estrazione: in millimetri e in centimetri il locale fuso di
    /// progetto dice 24,00 m²; senza `TEXTSIZE` il testo è alto 0,25 m nelle
    /// unità del disegno, come in `AL_LOCALE`. Il contorno è in unità del
    /// disegno.
    #[test]
    fn progetto_rooms_in_mm_and_cm() {
        for unit in [DrawingUnit::Millimetri, DrawingUnit::Centimetri] {
            let mut src = case_a_doc_in(Form::Continua, unit);
            src.header.text_height = 0.0;
            let tol = unit.tolerance();
            let at = |x: f64, y: f64| [unit.metres(x), unit.metres(y)];
            write_room(&mut src, unit, tol, "A", at(2.0, 1.5));
            write_room(&mut src, unit, tol, "B", at(6.0, 1.5));
            add_demolition_in(&mut src, unit, [4.05, 0.0], [4.05, 3.0]);
            let o = ExtractOptions {
                elaborati: all_three().to_vec(),
                new_walls_layer: LAYER_MURI.into(),
                unit,
                tol,
                room_tol: tol,
            };
            let ex = extract(&src, &o);
            assert_eq!(
                sorted_texts(&ex.notices),
                ["Locali A + B fusi in uno (24,00 m²): nome da confermare."],
                "{unit:?}"
            );
            assert_eq!(
                labels_on(&ex.docs[0].1, LAYER_LOCALI),
                ["A\\PS = 12,00 m²", "B\\PS = 11,70 m²"],
                "{unit:?}"
            );
            let out = &ex.docs[2].1;
            assert_eq!(
                labels_on(out, LAYER_LOCALI),
                ["A + B (da confermare)\\PS = 24,00 m²"],
                "{unit:?}"
            );
            let label = model_on(out, LAYER_LOCALI)
                .into_iter()
                .find_map(|e| match e {
                    EntityType::MText(t) => Some(t),
                    _ => None,
                })
                .unwrap();
            assert!(
                (label.height - unit.metres(0.25)).abs() < 1e-9,
                "{unit:?}: {}",
                label.height
            );
            let outlines = outlines_on(out, LAYER_LOCALI);
            assert_eq!(outlines.len(), 1, "{unit:?}");
            let k = unit.per_metre();
            assert!(
                (polygon_area(outlines[0]) - 24.0 * k * k).abs() < 1e-6 * k * k,
                "{unit:?}"
            );
            assert_eq!(out.header.insertion_units, unit.insunits());
        }
    }

    /// Il ricalcolo dei locali lavora sulla copia: il disegno di lavoro, i suoi
    /// handle e i locali scritti restano quelli di prima anche quando
    /// l'estratto cambia contorni e testi.
    #[test]
    fn room_recalculation_never_touches_the_source() {
        let mut src = case_a_doc(Form::Continua);
        write_room(&mut src, M, 1e-6, "A", [2.0, 1.5]);
        write_room(&mut src, M, 1e-6, "B", [6.0, 1.5]);
        write_seed(&mut src, "Studio", [6.0, 1.0]);
        move_tramezzo_face(&mut src, Form::Continua, M);
        let before: Vec<EntityType> = src.entities().cloned().collect();
        let next_handle = src.next_handle();
        let ex = extract(&src, &opts(&all_three()));
        assert_eq!(ex.docs.len(), 3);
        assert!(
            src.entities().cloned().collect::<Vec<_>>() == before,
            "il disegno di lavoro è cambiato"
        );
        assert_eq!(src.next_handle(), next_handle, "handle presi da src");
        assert_eq!(
            labels_on(&src, LAYER_LOCALI),
            ["A\\PS = 12,00 m²", "B\\PS = 11,70 m²"]
        );
        assert_eq!(
            labels_on(&ex.docs[0].1, LAYER_LOCALI),
            ["A\\PS = 11,70 m²", "B\\PS = 11,70 m²"]
        );
        // Progetto (nessuna zona): B prende il nome del seme di progetto.
        assert_eq!(
            labels_on(&ex.docs[2].1, LAYER_LOCALI),
            ["A\\PS = 11,70 m²", "Studio\\PS = 11,70 m²"]
        );
    }
}
