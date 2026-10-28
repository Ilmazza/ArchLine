//! Geometria dello stato di progetto (spec §5.4; varco §5.3).
//!
//! Dal rilievo (muri a due linee), dalle zone demolite (gialle) e da quelle
//! nuove (rosse) si ricava la pianta di progetto senza unioni booleane di
//! poligoni. Il disegno si legge e basta: le entità del rilievo non si
//! toccano. Le regole, in quest'ordine (l'ordine conta):
//!
//! 1. **Tagli.** Per ogni curva di ogni entità muro (`BoundarySource.curves`)
//!    e per ogni zona demolita, `inside_spans` dà gli intervalli dentro la
//!    zona, bordo compreso. Ogni intervallo si spezza dove la curva incontra
//!    il bordo e ogni tratto si classifica col suo punto medio: a più di `tol`
//!    dal bordo è interno e si toglie; sul bordo si toglie solo se la zona ha
//!    l'asse e il lato più vicino è un lato lungo (una faccia del muro tolto),
//!    altrimenti resta. Una zona senza asse che tocca un muro dà un solo
//!    [`Notice::ConfiniNonRimossi`], nel baricentro del suo contorno esterno.
//!    Rete di sicurezza (R40): un tratto che resta parallelo a un lato lungo,
//!    nella sua fascia e lungo l'asse, è una faccia del muro demolito non tolta
//!    (facce non parallele all'asse o tra loro) → [`Notice::FacciaNonTolta`].
//! 2. **Chiusure** (zone demolite con asse). Per ciascun lato di estremità:
//!    se coincide (collineare, sovrapposto per più di `tol`) con un tratto di
//!    muro che resta → [`Notice::GiunzioneDaVerificare`] nel punto medio del
//!    tratto comune e nessuna linea; altrimenti, con entrambi gli estremi a
//!    non più di `tol` da muri che restano → una `LINE` con gli attributi del
//!    muro più vicino al primo estremo; altrimenti [`Notice::TaglioNonChiuso`]
//!    a metà del lato. Poi i muri appoggiati a T su un lato lungo (R39): le
//!    loro teste libere sul lato si chiudono a coppie con una `LINE` sul lato,
//!    con gli attributi della curva della prima, se sono le due facce di uno
//!    stesso muro (parallele, non più lontane dello spessore massimo: R42);
//!    una testa spaiata, o una coppia che non lo è, dà
//!    [`Notice::TaglioNonChiuso`] (vedi [`abutment_closures`]).
//! 3. **Varco** (zona demolita con `varco`). Con l'asse, i due lati lunghi
//!    vanno solo in `room_entities`, come `LINE` (soglie per il calcolo dei
//!    locali): nel disegno il vano resta aperto, chiuso dalle due spallette
//!    del punto 2. Senza asse → [`Notice::VarcoNonSupportato`] nel baricentro.
//! 4. **Zone nuove.** I lati delle zone nuove e le chiusure del punto 2 si
//!    mettono insieme; i tratti collineari presenti in esattamente due lati si
//!    tolgono da entrambi (cuciture tra zone nuove adiacenti, chiusure coperte
//!    da un muro ricostruito); poi i lati nuovi collineari contigui si
//!    fondono. Un lato nuovo collineare con un tratto di muro esistente che
//!    resta non si aggiunge su quel tratto e dà una giunzione. Zone nuove i cui
//!    interni si intersecano senza coincidere → [`Notice::NuoveSovrapposte`].
//!    Le linee nuove vanno sul layer dei muri nuovi, colore e tipo di linea
//!    `ByLayer`; le chiusure che restano tengono gli attributi del punto 2.
//! 5. **Elementi puntuali.** `INSERT`, `TEXT` e `MTEXT` col punto di
//!    inserimento dentro una zona demolita (bordo compreso) finiscono in
//!    `excluded`, con un [`Notice::Esclusa`] ciascuno.
//! 6. **Entità non tagliabili.** Un'entità non muro con almeno un punto
//!    (vertice, punto di controllo o di presa, punto della curva) strettamente
//!    dentro una zona demolita e uno fuori resta intera, con
//!    [`Notice::NonTagliata`]. Non è un muro: non sta né in `drawn` né in
//!    `kept_originals`, la porta l'estrazione con le altre entità non muro.
//!
//! Scelte oltre la lettera delle regole:
//!
//! * Una zona con asse ma senza spessore vale come zona senza asse, e così
//!   una il cui contorno non è più il rettangolo ricostruito dall'asse con
//!   `rect_from_axis_thickness` (campitura modificata a mano): lati lunghi e
//!   chiusure finirebbero nel posto sbagliato.
//! * «Una zona senza asse che tocca un muro»: un tratto di muro sul bordo
//!   (lasciato), oppure un muro che arriva al bordo o lo attraversa (tagliato
//!   lì, senza chiusura perché senza asse non si sanno le facce).
//! * Zone uguali (stesso stato, stessi anelli entro [`RING_SLACK`] tolleranze)
//!   contano una volta: un doppione cancellerebbe a coppie le chiusure e i
//!   lati nuovi dell'originale, e il muro sparirebbe in silenzio.
//! * Un lato di estremità che sta dentro (o sul bordo di) un'altra zona
//!   demolita non è un taglio: la demolizione continua di là. Né linea né
//!   avviso (due demolizioni di seguito sullo stesso muro).
//! * Pulizia alla tolleranza prima di `kept_pieces`, che scarta solo tratti
//!   sotto 1e-9 della lunghezza dell'entità: per ogni curva, gli intervalli a
//!   meno di `tol` l'uno dall'altro si fondono, gli estremi a meno di `tol` da
//!   un capo vanno al capo e gli intervalli lunghi `tol` o meno si lasciano
//!   (salvo la curva intera). Un bordo che passa per un vertice non lascia
//!   monconi di micrometri.
//! * Di un lato nuovo si toglie solo il tratto che coincide con un muro
//!   esistente: il resto del lato si disegna.
//! * Una giunzione nello stesso punto (per esempio dal lato di estremità di
//!   una demolizione e dal lato del muro ricostruito sopra) si elenca una
//!   volta sola.
//! * Un'entità non muro tutta dentro una zona demolita, bordo compreso (la
//!   campitura del muro tolto, una quota del suo spessore, un'entità su un
//!   layer spento), esce dal progetto come un elemento puntuale, con
//!   [`Notice::Esclusa`]: altrimenti resterebbe nel disegno di progetto senza
//!   avviso.
//! * Le entità sui layer `AL-*` e quelle con un record `ARCHLINE_ZONA` o
//!   `ARCHLINE_LOCALE` (zone, locali) sono del modulo e non passano dai punti
//!   5 e 6: l'estrazione le tratta a parte.
//! * Le entità muro senza geometria sul piano XY (fuori piano, contate in
//!   `WallSet.off_plane`) non si tagliano: restano in `kept_originals`.

use codec::entities::Line as LineEntity;
use codec::xdata::ExtendedData;
use codec::{Document, Color, EntityType, Handle, Vector3};
use kernel::geom2d::{
    contains, distance_to, inside_spans, intersect, normalize_angle, segment_crossing,
    Arc as KernelArc, Curve, Extent, Line, SegmentCrossing, Tolerance,
};
use kernel::tessellation::DEFAULT_ANGLE;
use rustc_hash::FxHashSet;

use crate::entities::curve::{entity_curve_xy, ocs_plane};
use crate::entities::traits::EntityTypeOps;

use super::clip::kept_pieces;
use super::report::Notice;
use super::units::{resolve, UnitLookup};
use super::walls::{
    is_bim_layer, rect_from_axis_thickness, WallRect, WallSet, FACE_PARALLEL_DEGREES,
};
use super::zone::{ZoneGeom, ZoneState};
use super::{APP_LOCALE, APP_ZONA, BASE_TOLERANCE_M, MAX_WALL_THICKNESS_M};

/// Primo handle sintetico di [`ProjectGeometry::room_entities`].
pub const FIRST_SYNTHETIC_HANDLE: u64 = 0xF000_0000;

/// Quante tolleranze può distare un vertice del contorno di una zona da un
/// angolo del rettangolo ricostruito dal suo asse, o dal vertice di un
/// doppione.
const RING_SLACK: f64 = 4.0;

/// Layer delle soglie dei varchi in `room_entities`: non si disegnano mai.
const THRESHOLD_LAYER: &str = "0";

/// La pianta di progetto.
#[derive(Clone, Debug, Default)]
pub struct ProjectGeometry {
    /// Le entità nuove da disegnare, con handle `NULL`: i pezzi dei muri
    /// tagliati (stesso tipo e attributi dell'originale), poi le chiusure (con
    /// gli attributi del muro più vicino) e le linee dei muri nuovi (sul layer
    /// dei muri nuovi, `ByLayer`).
    pub drawn: Vec<EntityType>,
    /// Le entità muro non toccate, in ordine di disegno.
    pub kept_originals: Vec<Handle>,
    /// Tutto ciò che delimita i locali di progetto, da passare così com'è a
    /// `walls::sources_of`: le entità muro non toccate con il loro handle, poi
    /// `drawn` e i lati lunghi dei varchi (`LINE` sul layer `0`) con handle
    /// sintetici consecutivi da [`FIRST_SYNTHETIC_HANDLE`] in su (dal primo
    /// sopra gli handle dei muri, se il disegno arriva già lì), scritti anche
    /// in `common.handle`. Gli handle sono tutti diversi.
    pub room_entities: Vec<(Handle, EntityType)>,
    /// Le entità non muro che non entrano nel progetto, in ordine di disegno.
    pub excluded: Vec<Handle>,
    pub notices: Vec<Notice>,
}

/// La geometria dello stato di progetto (regole nella documentazione del
/// modulo). `tol` è in unità del disegno, finita e positiva
/// (`DrawingUnit::tolerance`). `zones` sono quelle dello spazio modello
/// (`zone::zones_in`), con il contorno esterno in `rings[0]`.
pub fn project_geometry(
    doc: &Document,
    walls: &WallSet,
    zones: &[ZoneGeom],
    new_walls_layer: &str,
    tol: f64,
) -> ProjectGeometry {
    let tolerance = Tolerance::new(tol);
    let infos = prepare(zones, tol);
    let demolished: Vec<&ZoneInfo> = infos
        .iter()
        .filter(|zone| zone.geom.zone.state == ZoneState::Demolito)
        .collect();
    let built: Vec<&ZoneInfo> = infos
        .iter()
        .filter(|zone| zone.geom.zone.state == ZoneState::Nuovo)
        .collect();
    let owner = model_space(doc).map(|(handle, _)| handle);
    let mut notices = Vec::new();

    // 1. Tagli.
    let removed = cut_spans(walls, &demolished, tolerance, &mut notices);
    let mut drawn = Vec::new();
    let mut kept_originals = Vec::new();
    // Ciò che resta dei muri, curva per curva, con l'indice dell'entità.
    let mut remaining: Vec<(usize, Curve)> = Vec::new();
    for (index, ((handle, entity), per_curve)) in walls.entities.iter().zip(&removed).enumerate() {
        let curves = source_curves(walls, handle);
        let cuts: Vec<(usize, [f64; 2])> = per_curve
            .iter()
            .enumerate()
            .flat_map(|(k, spans)| spans.iter().map(move |&span| (k, span)))
            .collect();
        if cuts.is_empty() {
            kept_originals.push(*handle);
            remaining.extend(curves.iter().map(|curve| (index, curve.clone())));
            continue;
        }
        drawn.extend(kept_pieces(entity, &cuts));
        for (curve, spans) in curves.iter().zip(per_curve) {
            remaining.extend(
                complement(spans)
                    .into_iter()
                    .filter_map(|[a, b]| sub_curve(curve, a, b))
                    .map(|piece| (index, piece)),
            );
        }
    }
    let remaining_lines: Vec<Segment> = remaining
        .iter()
        .filter_map(|(_, curve)| match curve {
            Curve::Line(line) => Some((line.start, line.end)),
            _ => None,
        })
        .collect();

    // Facce del muro demolito rimaste (R40): facce non parallele all'asse.
    leftover_faces(&demolished, &remaining_lines, tolerance, &mut notices);

    // 2. Chiusure: lati di estremità, poi teste dei muri appoggiati a T sui
    // lati lunghi (R39). Passano tutte dalla cancellazione del punto 4.
    let mut closures = closures(
        walls,
        &demolished,
        &remaining,
        &remaining_lines,
        tolerance,
        &mut notices,
    );
    // Lo spessore massimo di un muro in unità del disegno: dall'unità del
    // disegno, o dalla tolleranza (1 µm in unità del disegno) se l'unità non è
    // nota (i comandi allora non creano zone).
    let max_thickness = match resolve(doc) {
        UnitLookup::Known(unit) => unit.metres(MAX_WALL_THICKNESS_M),
        _ => MAX_WALL_THICKNESS_M * tol / BASE_TOLERANCE_M,
    };
    closures.extend(abutment_closures(
        walls,
        &demolished,
        &remaining,
        max_thickness,
        tolerance,
        &mut notices,
    ));

    // 3. Varchi.
    let mut thresholds = Vec::new();
    for zone in demolished.iter().filter(|zone| zone.geom.zone.varco) {
        match &zone.rect {
            Some(rect) => thresholds.extend(
                rect.long_sides()
                    .into_iter()
                    .map(|(a, b)| by_layer_line(a, b, THRESHOLD_LAYER, owner)),
            ),
            None => notices.push(Notice::VarcoNonSupportato { at: zone.centroid }),
        }
    }

    // 4. Zone nuove (e chiusure che restano).
    drawn.extend(new_walls(
        &built,
        closures,
        &remaining_lines,
        new_walls_layer,
        owner,
        tolerance,
        &mut notices,
    ));

    // 5 e 6. Elementi puntuali ed entità non tagliabili.
    let excluded = point_and_uncuttable(doc, walls, zones, &demolished, tolerance, &mut notices);

    let kept: FxHashSet<Handle> = kept_originals.iter().copied().collect();
    let mut room_entities: Vec<(Handle, EntityType)> = walls
        .entities
        .iter()
        .filter(|(handle, _)| kept.contains(handle))
        .cloned()
        .collect();
    // Sopra ogni handle dei muri: in `sources_of` un handle doppio
    // cancellerebbe un muro in silenzio.
    let first = walls
        .entities
        .iter()
        .map(|(handle, _)| handle.value().saturating_add(1))
        .fold(FIRST_SYNTHETIC_HANDLE, u64::max);
    for (offset, entity) in (0u64..).zip(drawn.iter().chain(&thresholds)) {
        let handle = Handle::new(first.saturating_add(offset));
        let mut entity = entity.clone();
        entity.common_mut().handle = handle;
        room_entities.push((handle, entity));
    }

    ProjectGeometry {
        drawn,
        kept_originals,
        room_entities,
        excluded,
        notices,
    }
}

// ---------------------------------------------------------------------------
// Zone
// ---------------------------------------------------------------------------

type Segment = ([f64; 2], [f64; 2]);
type Bounds = ([f64; 2], [f64; 2]);

/// Dove sta un punto rispetto a una zona.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// Dentro, a più di `tol` dal bordo.
    In,
    /// A non più di `tol` dal bordo.
    On,
    Out,
}

/// Una zona pronta per le regole.
struct ZoneInfo<'a> {
    geom: &'a ZoneGeom,
    /// Il rettangolo del muro, solo se l'asse vale (vedi [`axis_rect`]).
    rect: Option<WallRect>,
    /// I lati di tutti gli anelli, buchi compresi.
    edges: Vec<Curve>,
    bounds: Bounds,
    /// Il baricentro del contorno esterno.
    centroid: [f64; 2],
}

impl ZoneInfo<'_> {
    fn border_distance(&self, p: [f64; 2]) -> f64 {
        self.edges
            .iter()
            .map(|edge| distance_to(edge, p))
            .fold(f64::INFINITY, f64::min)
    }

    fn locate(&self, p: [f64; 2], tolerance: Tolerance) -> Place {
        let tol = tolerance.linear();
        let (lo, hi) = self.bounds;
        if p[0] < lo[0] - tol || p[0] > hi[0] + tol || p[1] < lo[1] - tol || p[1] > hi[1] + tol {
            Place::Out
        } else if self.border_distance(p) <= tol {
            Place::On
        } else if contains(&self.edges, p, tolerance) {
            Place::In
        } else {
            Place::Out
        }
    }
}

/// Le zone utili, senza doppioni e senza anelli esterni degeneri. Tra due
/// doppioni vince il primo, ma uno con il varco prende il posto di uno senza
/// (fix finale, punto 6: il varco non sparisce in silenzio).
fn prepare(zones: &[ZoneGeom], tol: f64) -> Vec<ZoneInfo<'_>> {
    let mut infos: Vec<ZoneInfo> = Vec::new();
    for geom in zones {
        let Some(outer) = geom.rings.first().filter(|ring| ring.len() >= 3) else {
            continue;
        };
        let twin = infos
            .iter()
            .position(|seen| same_zone(seen.geom, geom, RING_SLACK * tol));
        if twin.is_some_and(|k| !geom.zone.varco || infos[k].geom.zone.varco) {
            continue;
        }
        let edges = ring_edges(&geom.rings);
        if edges.is_empty() {
            continue;
        }
        let info = ZoneInfo {
            rect: axis_rect(geom, tol),
            bounds: bounds_of(geom.rings.iter().flatten().copied()),
            centroid: centroid(outer),
            edges,
            geom,
        };
        match twin {
            Some(k) => infos[k] = info,
            None => infos.push(info),
        }
    }
    infos
}

/// Il rettangolo del muro di una zona con asse e spessore, se il contorno
/// della zona è ancora quel rettangolo (un anello di quattro vertici, ognuno
/// entro [`RING_SLACK`] tolleranze da un angolo). Una zona con asse ma senza
/// spessore, o con il contorno cambiato, vale come zona senza asse.
fn axis_rect(geom: &ZoneGeom, tol: f64) -> Option<WallRect> {
    let (p1, p2) = geom.zone.axis?;
    let rect = rect_from_axis_thickness(p1, p2, geom.zone.thickness?)?;
    let [outer] = geom.rings.as_slice() else {
        return None;
    };
    same_ring(&rect.ring(), outer, RING_SLACK * tol).then_some(rect)
}

fn same_zone(a: &ZoneGeom, b: &ZoneGeom, eps: f64) -> bool {
    a.zone.state == b.zone.state
        && a.rings.len() == b.rings.len()
        && a.rings
            .iter()
            .zip(&b.rings)
            .all(|(r, s)| same_ring(r, s, eps))
}

/// Gli stessi vertici entro `eps`, in qualunque ordine, senza contare i
/// vertici ripetuti di seguito.
fn same_ring(r: &[[f64; 2]], s: &[[f64; 2]], eps: f64) -> bool {
    let (r, s) = (distinct_vertices(r, eps), distinct_vertices(s, eps));
    let covered = |from: &[[f64; 2]], to: &[[f64; 2]]| {
        from.iter()
            .all(|&p| to.iter().any(|&q| distance(p, q) <= eps))
    };
    r.len() == s.len() && covered(&r, &s) && covered(&s, &r)
}

/// L'anello senza i vertici a `eps` o meno dal precedente (l'ultimo anche
/// dal primo).
fn distinct_vertices(ring: &[[f64; 2]], eps: f64) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(ring.len());
    for &p in ring {
        if out.last().map_or(true, |&last| distance(last, p) > eps) {
            out.push(p);
        }
    }
    while out.len() > 1 && distance(out[0], out[out.len() - 1]) <= eps {
        out.pop();
    }
    out
}

fn ring_edges(rings: &[Vec<[f64; 2]>]) -> Vec<Curve> {
    rings
        .iter()
        .flat_map(|ring| ring.iter().zip(ring.iter().cycle().skip(1)))
        .filter(|(start, end)| distance(**start, **end) > 0.0)
        .map(|(&start, &end)| Curve::Line(Line { start, end }))
        .collect()
}

fn bounds_of(points: impl Iterator<Item = [f64; 2]>) -> Bounds {
    points.fold(
        ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]),
        |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        },
    )
}

/// Il baricentro dell'area dell'anello, calcolato attorno al primo vertice
/// (coordinate georiferite); la media dei vertici se l'area è nulla.
fn centroid(ring: &[[f64; 2]]) -> [f64; 2] {
    let origin = ring[0];
    let (mut area, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for (p, q) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        let (px, py) = (p[0] - origin[0], p[1] - origin[1]);
        let (qx, qy) = (q[0] - origin[0], q[1] - origin[1]);
        let cross = px * qy - qx * py;
        area += cross;
        cx += (px + qx) * cross;
        cy += (py + qy) * cross;
    }
    if area.abs() > 0.0 && area.is_finite() {
        [origin[0] + cx / (3.0 * area), origin[1] + cy / (3.0 * area)]
    } else {
        let n = ring.len() as f64;
        let (sx, sy) = ring
            .iter()
            .fold((0.0, 0.0), |(sx, sy), p| (sx + p[0], sy + p[1]));
        [sx / n, sy / n]
    }
}

/// Il blocco dello spazio modello e le sue entità, in ordine di disegno.
fn model_space(doc: &Document) -> Option<(Handle, &[Handle])> {
    doc.block_records
        .iter()
        .find(|record| record.is_model_space())
        .map(|record| (record.handle, record.entity_handles.as_slice()))
}

// ---------------------------------------------------------------------------
// 1. Tagli
// ---------------------------------------------------------------------------

fn source_curves<'a>(walls: &'a WallSet, handle: &Handle) -> &'a [Curve] {
    walls
        .sources
        .get(handle)
        .map_or(&[][..], |source| source.curves.as_slice())
}

/// Gli intervalli da togliere, per entità muro (indice in `walls.entities`) e
/// per curva della sua sorgente, nel parametro della curva del kernel e già
/// puliti alla tolleranza ([`tidy`]).
fn cut_spans(
    walls: &WallSet,
    demolished: &[&ZoneInfo],
    tolerance: Tolerance,
    notices: &mut Vec<Notice>,
) -> Vec<Vec<Vec<[f64; 2]>>> {
    let tol = tolerance.linear();
    let mut raw: Vec<Vec<Vec<[f64; 2]>>> = walls
        .entities
        .iter()
        .map(|(handle, _)| vec![Vec::new(); source_curves(walls, handle).len()])
        .collect();
    for zone in demolished {
        let mut touched = false;
        for ((handle, _), per_curve) in walls.entities.iter().zip(raw.iter_mut()) {
            for (curve, spans) in source_curves(walls, handle)
                .iter()
                .zip(per_curve.iter_mut())
            {
                let length = curve.length();
                if !(length > 0.0 && length.is_finite()) || !reaches(curve, &zone.bounds, tol) {
                    continue;
                }
                for span in inside_spans(&zone.edges, curve, tolerance) {
                    for [u, v] in split_on_border(curve, span, &zone.edges, tol / length, tolerance)
                    {
                        let middle = curve.point_at(0.5 * (u + v));
                        // Ogni tratto sta dentro per costruzione; la prova
                        // costa poco ed evita di togliere un tratto esterno.
                        if !contains(&zone.edges, middle, tolerance) {
                            continue;
                        }
                        let on_border = zone.border_distance(middle) <= tol;
                        match &zone.rect {
                            Some(rect) => {
                                if !on_border || on_long_side(rect, middle) {
                                    spans.push([u, v]);
                                }
                            }
                            None => {
                                touched |= on_border
                                    || [u, v]
                                        .into_iter()
                                        .any(|t| zone.border_distance(curve.point_at(t)) <= tol);
                                if !on_border {
                                    spans.push([u, v]);
                                }
                            }
                        }
                    }
                }
            }
        }
        if touched {
            notices.push(Notice::ConfiniNonRimossi { at: zone.centroid });
        }
    }
    walls
        .entities
        .iter()
        .zip(raw)
        .map(|((handle, _), per_curve)| {
            source_curves(walls, handle)
                .iter()
                .zip(per_curve)
                .map(|(curve, spans)| tidy(&spans, curve.length(), tol))
                .collect()
        })
        .collect()
}

/// Il riquadro della curva tocca quello della zona, allargato di `tol`? Per
/// le curve che non sono rette o archi si risponde sì.
fn reaches(curve: &Curve, zone: &Bounds, tol: f64) -> bool {
    let (lo, hi) = match curve {
        Curve::Line(line) => (
            [
                line.start[0].min(line.end[0]),
                line.start[1].min(line.end[1]),
            ],
            [
                line.start[0].max(line.end[0]),
                line.start[1].max(line.end[1]),
            ],
        ),
        Curve::Arc(arc) => {
            let r = arc.radius.abs();
            (
                [arc.centre[0] - r, arc.centre[1] - r],
                [arc.centre[0] + r, arc.centre[1] + r],
            )
        }
        _ => return true,
    };
    lo[0] <= zone.1[0] + tol
        && hi[0] >= zone.0[0] - tol
        && lo[1] <= zone.1[1] + tol
        && hi[1] >= zone.0[1] - tol
}

/// L'intervallo `[t0, t1]` spezzato dove la curva incontra il bordo: un
/// intervallo di `inside_spans` può unire un tratto sul bordo e uno interno
/// (una zona non convessa), che si classificano ognuno per sé. `eps` è la
/// tolleranza nel parametro.
fn split_on_border(
    curve: &Curve,
    [t0, t1]: [f64; 2],
    edges: &[Curve],
    eps: f64,
    tolerance: Tolerance,
) -> Vec<[f64; 2]> {
    let crossings = edges
        .iter()
        .flat_map(|edge| intersect(curve, edge, tolerance))
        .map(|crossing| crossing.t_a)
        .collect();
    breakpoints(crossings, t0, t1, eps)
        .windows(2)
        .map(|pair| [pair[0], pair[1]])
        .filter(|[u, v]| v > u)
        .collect()
}

/// `lo`, i valori di `inner` tra `lo` e `hi` distanti più di `eps` dal
/// precedente e da `hi`, poi `hi`.
fn breakpoints(mut inner: Vec<f64>, lo: f64, hi: f64, eps: f64) -> Vec<f64> {
    inner.retain(|t| t.is_finite());
    inner.sort_by(f64::total_cmp);
    let mut out = vec![lo];
    for t in inner {
        let last = out[out.len() - 1];
        if t - last > eps && hi - t > eps {
            out.push(t);
        }
    }
    out.push(hi);
    out
}

/// Il punto è più vicino a un lato lungo del rettangolo che ai lati di
/// estremità?
fn on_long_side(rect: &WallRect, p: [f64; 2]) -> bool {
    let nearest = |sides: [Segment; 2]| {
        sides
            .iter()
            .map(|&(a, b)| segment_distance(p, a, b))
            .fold(f64::INFINITY, f64::min)
    };
    nearest(rect.long_sides()) < nearest(rect.end_sides())
}

/// Gli intervalli da togliere da una curva lunga `length`, puliti alla
/// tolleranza: dentro 0..1, ordinati; fusi quando distano `tol` o meno;
/// portati a 0 o a 1 quando un estremo dista `tol` o meno da un capo; senza
/// quelli lunghi `tol` o meno, salvo la curva intera.
fn tidy(spans: &[[f64; 2]], length: f64, tol: f64) -> Vec<[f64; 2]> {
    if spans.is_empty() || !(length > 0.0 && length.is_finite()) {
        return Vec::new();
    }
    let eps = tol / length;
    let mut sorted: Vec<[f64; 2]> = spans
        .iter()
        .filter(|span| span[0].is_finite() && span[1].is_finite())
        .map(|&[a, b]| [a.min(b).clamp(0.0, 1.0), a.max(b).clamp(0.0, 1.0)])
        .collect();
    sorted.sort_by(|x, y| x[0].total_cmp(&y[0]));
    let merge = |spans: Vec<[f64; 2]>, gap: f64| {
        let mut merged: Vec<[f64; 2]> = Vec::with_capacity(spans.len());
        for span in spans {
            match merged.last_mut() {
                Some(last) if span[0] <= last[1] + gap => last[1] = last[1].max(span[1]),
                _ => merged.push(span),
            }
        }
        merged
    };
    let mut merged = merge(sorted, eps);
    for span in &mut merged {
        if span[0] <= eps {
            span[0] = 0.0;
        }
        if span[1] >= 1.0 - eps {
            span[1] = 1.0;
        }
    }
    let mut merged = merge(merged, 0.0);
    merged.retain(|&[a, b]| b - a > eps || (a == 0.0 && b == 1.0));
    merged
}

/// Il complemento in 0..1 di intervalli ordinati e fusi.
fn complement(removed: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut kept = Vec::with_capacity(removed.len() + 1);
    let mut cursor = 0.0;
    for &[a, b] in removed {
        if a > cursor {
            kept.push([cursor, a]);
        }
        cursor = f64::max(cursor, b);
    }
    if cursor < 1.0 {
        kept.push([cursor, 1.0]);
    }
    kept
}

/// Il tratto `[a, b]` di una retta o di un arco del kernel (parametro lineare
/// nella lunghezza o nell'angolo); gli altri tipi solo interi.
fn sub_curve(curve: &Curve, a: f64, b: f64) -> Option<Curve> {
    if a <= 0.0 && b >= 1.0 {
        return Some(curve.clone());
    }
    match curve {
        Curve::Line(line) => Some(Curve::Line(Line {
            start: if a <= 0.0 {
                line.start
            } else {
                curve.point_at(a)
            },
            end: if b >= 1.0 {
                line.end
            } else {
                curve.point_at(b)
            },
        })),
        Curve::Arc(arc) => {
            let start = normalize_angle(arc.start_angle);
            let sweep = arc.sweep();
            Some(Curve::Arc(KernelArc {
                centre: arc.centre,
                radius: arc.radius,
                start_angle: start + sweep * a,
                end_angle: start + sweep * b,
            }))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 2. Chiusure
// ---------------------------------------------------------------------------

/// Una chiusura: il lato di estremità e la sua `LINE`, con gli attributi del
/// muro più vicino al primo estremo.
struct Closure {
    side: Segment,
    line: EntityType,
}

fn closures(
    walls: &WallSet,
    demolished: &[&ZoneInfo],
    remaining: &[(usize, Curve)],
    remaining_lines: &[Segment],
    tolerance: Tolerance,
    notices: &mut Vec<Notice>,
) -> Vec<Closure> {
    let tol = tolerance.linear();
    let near_wall = |p: [f64; 2]| {
        remaining
            .iter()
            .any(|(_, curve)| distance_to(curve, p) <= tol)
    };
    let mut out = Vec::new();
    for (index, zone) in demolished.iter().enumerate() {
        let Some(rect) = &zone.rect else {
            continue;
        };
        for (a, b) in rect.end_sides() {
            let middle = midpoint(a, b);
            // La demolizione continua in un'altra zona: niente da chiudere.
            if demolished
                .iter()
                .enumerate()
                .any(|(other, z)| other != index && contains(&z.edges, middle, tolerance))
            {
                continue;
            }
            let common = stretches_on((a, b), remaining_lines, tol);
            if !common.is_empty() {
                for [u, v] in common {
                    push_junction(notices, lerp2(a, b, 0.5 * (u + v)), tol);
                }
                continue;
            }
            let nearest = remaining
                .iter()
                .min_by(|x, y| distance_to(&x.1, a).total_cmp(&distance_to(&y.1, a)));
            match nearest {
                Some(&(wall, _)) if near_wall(a) && near_wall(b) => out.push(Closure {
                    side: (a, b),
                    line: closure_line(a, b, &walls.entities[wall].1),
                }),
                _ => notices.push(Notice::TaglioNonChiuso { at: middle }),
            }
        }
    }
    out
}

/// Le teste dei muri che si appoggiano a T su un lato lungo di una zona
/// demolita con asse (R39): tolto il muro, le loro facce resterebbero aperte.
///
/// I lati lunghi si fondono quando sono collineari e si toccano (due
/// demolizioni di seguito sullo stesso muro), poi su ognuno si raccolgono le
/// teste: gli estremi delle curve che restano a non più di `tol` dal lato e a
/// più di `tol` dai suoi capi (gli angoli sono della regola 2), su curve non
/// parallele al lato entro [`FACE_PARALLEL_DEGREES`] (una faccia parallela è
/// una faccia non tolta, vedi [`leftover_faces`]). Gli estremi nello stesso
/// punto si contano per parità: due curve che si toccano lì continuano l'una
/// nell'altra, non sono una testa. Le teste, in ordine lungo il lato, si
/// chiudono a coppie (1-2, 3-4, …) con una `LINE` sul lato, con gli attributi
/// della curva della prima testa; una testa spaiata dà
/// [`Notice::TaglioNonChiuso`] dove sta. Una coppia si chiude solo se le due
/// curve sono parallele tra loro entro [`FACE_PARALLEL_DEGREES`] e le due
/// teste distano non più di `max_thickness` (lo spessore massimo di un muro,
/// in unità del disegno): sono le due facce di uno stesso muro. Altrimenti
/// (R42: per esempio una faccia che non arriva al lato e la sua compagna presa
/// con la testa del muro accanto) nessuna linea e un
/// [`Notice::TaglioNonChiuso`] su ciascuna delle due teste. Una chiusura con
/// il punto medio strettamente dentro un'altra zona demolita non serve: la
/// demolizione continua di là.
fn abutment_closures(
    walls: &WallSet,
    demolished: &[&ZoneInfo],
    remaining: &[(usize, Curve)],
    max_thickness: f64,
    tolerance: Tolerance,
    notices: &mut Vec<Notice>,
) -> Vec<Closure> {
    let tol = tolerance.linear();
    let sin_limit = FACE_PARALLEL_DEGREES.to_radians().sin();
    let sides = merge_collinear(
        demolished
            .iter()
            .filter_map(|zone| zone.rect.as_ref())
            .flat_map(|rect| rect.long_sides())
            .collect(),
        tol,
    );
    let mut out = Vec::new();
    for (a, b) in sides {
        let length = distance(a, b);
        if !(length > 2.0 * tol) {
            continue;
        }
        // Le teste si ordinano verso x crescenti (y crescenti su un lato
        // verticale): l'accoppiamento non dipende dal verso in cui si è dato
        // l'asse.
        let u = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
        let (a, b, u) = if u[0] < -1.0e-12 || (u[0].abs() <= 1.0e-12 && u[1] < 0.0) {
            (b, a, [-u[0], -u[1]])
        } else {
            (a, b, u)
        };
        let mut heads: Vec<(f64, Head)> = Vec::new();
        for (wall, curve) in remaining {
            for t in [0.0, 1.0] {
                let p = curve.point_at(t);
                let along = dot(sub(p, a), u);
                if segment_distance(p, a, b) > tol || along <= tol || along >= length - tol {
                    continue;
                }
                let tangent = curve.tangent_at(t);
                let speed = norm(tangent);
                if !(speed > 0.0) || cross(tangent, u).abs() <= sin_limit * speed {
                    continue;
                }
                let dir = [tangent[0] / speed, tangent[1] / speed];
                heads.push((
                    along,
                    Head {
                        at: p,
                        wall: *wall,
                        dir,
                    },
                ));
            }
        }
        heads.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut free: Vec<Head> = Vec::new();
        let mut first = 0;
        while first < heads.len() {
            let mut next = first + 1;
            while next < heads.len() && heads[next].0 - heads[next - 1].0 <= tol {
                next += 1;
            }
            if (next - first) % 2 == 1 {
                free.push(heads[first].1);
            }
            first = next;
        }
        let mut pairs = free.chunks_exact(2);
        for pair in &mut pairs {
            let (head, other) = (pair[0], pair[1]);
            let (p, q) = (head.at, other.at);
            // R42: due facce dello stesso muro, parallele e non più lontane
            // dello spessore massimo; altrimenti teste di muri diversi.
            if cross(head.dir, other.dir).abs() > sin_limit || distance(p, q) > max_thickness {
                notices.push(Notice::TaglioNonChiuso { at: p });
                notices.push(Notice::TaglioNonChiuso { at: q });
                continue;
            }
            let middle = midpoint(p, q);
            if demolished
                .iter()
                .any(|zone| zone.locate(middle, tolerance) == Place::In)
            {
                continue;
            }
            out.push(Closure {
                side: (p, q),
                line: closure_line(p, q, &walls.entities[head.wall].1),
            });
        }
        for head in pairs.remainder() {
            notices.push(Notice::TaglioNonChiuso { at: head.at });
        }
    }
    out
}

/// Una testa di muro su un lato lungo: il punto, l'entità muro della sua
/// curva e il verso unitario della curva lì.
#[derive(Clone, Copy, Debug)]
struct Head {
    at: [f64; 2],
    wall: usize,
    dir: [f64; 2],
}

/// La rete di sicurezza del punto 1 (R40): un tratto di muro che resta
/// parallelo (entro [`FACE_PARALLEL_DEGREES`]) a un lato lungo di una zona
/// demolita con asse, nella sua fascia (a non più di `tan 0,5° · |asse| + tol`
/// dal lato) e sovrapposto all'intervallo dell'asse per più di `tol`, è una
/// faccia del muro demolito rimasta in piedi: facce non parallele all'asse o
/// tra loro (muro a spessore variabile). Un [`Notice::FacciaNonTolta`] a metà
/// del tratto sovrapposto.
fn leftover_faces(
    demolished: &[&ZoneInfo],
    remaining_lines: &[Segment],
    tolerance: Tolerance,
    notices: &mut Vec<Notice>,
) {
    let tol = tolerance.linear();
    let angle = FACE_PARALLEL_DEGREES.to_radians();
    let (sin_limit, tan_limit) = (angle.sin(), angle.tan());
    for zone in demolished {
        let Some(rect) = &zone.rect else {
            continue;
        };
        let band = tan_limit * distance(rect.axis.0, rect.axis.1) + tol;
        for (a, b) in rect.long_sides() {
            let side = distance(a, b);
            if !(side > tol) {
                continue;
            }
            let u = [(b[0] - a[0]) / side, (b[1] - a[1]) / side];
            for &(p, q) in remaining_lines {
                let v = sub(q, p);
                let length = norm(v);
                if !(length > tol) || cross(v, u).abs() > sin_limit * length {
                    continue;
                }
                let (tp, tq) = (dot(sub(p, a), u), dot(sub(q, a), u));
                let (lo, hi) = (tp.min(tq).max(0.0), tp.max(tq).min(side));
                if !(hi - lo > tol) {
                    continue;
                }
                // tq ≠ tp: il tratto è parallelo al lato e lungo più di tol.
                let at = |t: f64| lerp2(p, q, (t - tp) / (tq - tp));
                let (from, to) = (at(lo), at(hi));
                let off = |x: [f64; 2]| cross(u, sub(x, a)).abs();
                if off(from) <= band && off(to) <= band {
                    notices.push(Notice::FacciaNonTolta {
                        at: midpoint(from, to),
                    });
                }
            }
        }
    }
}

/// Una `LINE` da `a` a `b` con gli attributi di `wall` (layer, colore, tipo
/// di linea e scala, spessore di linea, trasparenza, proprietario), senza la sua
/// identità (handle, dizionario di estensione, reattori, byte grezzi) né i
/// suoi XDATA.
fn closure_line(a: [f64; 2], b: [f64; 2], wall: &EntityType) -> EntityType {
    let mut line = segment_line(a, b);
    line.common = wall.common().clone();
    let common = &mut line.common;
    common.handle = Handle::NULL;
    common.xdictionary_handle = None;
    common.reactors.clear();
    common.raw_record = None;
    common.graphic_data = None;
    common.extended_data = ExtendedData::new();
    EntityType::Line(line)
}

fn segment_line(a: [f64; 2], b: [f64; 2]) -> LineEntity {
    LineEntity::from_points(Vector3::new(a[0], a[1], 0.0), Vector3::new(b[0], b[1], 0.0))
}

/// Una `LINE` sul layer dato, colore e tipo di linea `ByLayer`.
fn by_layer_line(a: [f64; 2], b: [f64; 2], layer: &str, owner: Option<Handle>) -> EntityType {
    let mut line = segment_line(a, b);
    line.common.layer = layer.to_string();
    line.common.color = Color::ByLayer;
    line.common.linetype = "ByLayer".to_string();
    if let Some(owner) = owner {
        line.common.owner_handle = owner;
    }
    EntityType::Line(line)
}

fn push_junction(notices: &mut Vec<Notice>, at: [f64; 2], tol: f64) {
    let known = notices.iter().any(|notice| {
        matches!(notice, Notice::GiunzioneDaVerificare { at: seen } if distance(*seen, at) <= tol)
    });
    if !known {
        notices.push(Notice::GiunzioneDaVerificare { at });
    }
}

// ---------------------------------------------------------------------------
// 4. Zone nuove
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    /// Lato di una zona nuova.
    New,
    /// Chiusura del punto 2 (indice in `closures`).
    Closure(usize),
}

#[derive(Clone, Copy, Debug)]
struct Piece {
    side: Segment,
    origin: Origin,
}

fn new_walls(
    built: &[&ZoneInfo],
    closures: Vec<Closure>,
    remaining_lines: &[Segment],
    layer: &str,
    owner: Option<Handle>,
    tolerance: Tolerance,
    notices: &mut Vec<Notice>,
) -> Vec<EntityType> {
    let tol = tolerance.linear();
    let mut pool: Vec<Piece> = closures
        .iter()
        .enumerate()
        .map(|(index, closure)| Piece {
            side: closure.side,
            origin: Origin::Closure(index),
        })
        .collect();
    for zone in built {
        for ring in &zone.geom.rings {
            for (&a, &b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                if distance(a, b) > tol {
                    pool.push(Piece {
                        side: (a, b),
                        origin: Origin::New,
                    });
                }
            }
        }
    }
    let survivors = cancel_pairs(&pool, tol);

    let mut out = Vec::new();
    for piece in &survivors {
        let Origin::Closure(index) = piece.origin else {
            continue;
        };
        let mut line = closures[index].line.clone();
        if let EntityType::Line(line) = &mut line {
            let (a, b) = piece.side;
            line.start = Vector3::new(a[0], a[1], 0.0);
            line.end = Vector3::new(b[0], b[1], 0.0);
        }
        out.push(line);
    }

    let fresh = merge_collinear(
        survivors
            .iter()
            .filter(|piece| piece.origin == Origin::New)
            .map(|piece| piece.side)
            .collect(),
        tol,
    );
    for (a, b) in fresh {
        let length = distance(a, b);
        let common = stretches_on((a, b), remaining_lines, tol);
        for &[u, v] in &common {
            push_junction(notices, lerp2(a, b, 0.5 * (u + v)), tol);
        }
        for [u, v] in complement(&common) {
            if (v - u) * length > tol {
                out.push(by_layer_line(lerp2(a, b, u), lerp2(a, b, v), layer, owner));
            }
        }
    }

    for (index, first) in built.iter().enumerate() {
        for second in &built[index + 1..] {
            if let Some(at) = overlap_witness(first, second, tolerance) {
                notices.push(Notice::NuoveSovrapposte { at });
            }
        }
    }
    out
}

/// Toglie da ogni lato i tratti presenti in esattamente due lati del
/// mucchio; i tratti in un solo lato o in tre e più restano. Si buttano i
/// pezzi lunghi `tol` o meno.
fn cancel_pairs(pool: &[Piece], tol: f64) -> Vec<Piece> {
    let mut out = Vec::new();
    for (index, piece) in pool.iter().enumerate() {
        let (a, b) = piece.side;
        let length = distance(a, b);
        if !(length > tol) {
            continue;
        }
        let shared: Vec<[f64; 2]> = pool
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .filter_map(|(_, other)| coincident_stretch(piece.side, other.side, tol))
            .collect();
        let cuts = breakpoints(
            shared.iter().flat_map(|stretch| *stretch).collect(),
            0.0,
            1.0,
            tol / length,
        );
        let mut kept: Vec<[f64; 2]> = Vec::new();
        for pair in cuts.windows(2) {
            let (u, v) = (pair[0], pair[1]);
            let middle = 0.5 * (u + v);
            let count = 1 + shared
                .iter()
                .filter(|stretch| stretch[0] < middle && middle < stretch[1])
                .count();
            if count == 2 {
                continue;
            }
            match kept.last_mut() {
                Some(last) if last[1] == u => last[1] = v,
                _ => kept.push([u, v]),
            }
        }
        for [u, v] in kept {
            if (v - u) * length > tol {
                out.push(Piece {
                    side: (lerp2(a, b, u), lerp2(a, b, v)),
                    origin: piece.origin,
                });
            }
        }
    }
    out
}

/// Fonde i segmenti collineari che si sovrappongono o si toccano (entro
/// `tol`), finché ce ne sono.
fn merge_collinear(mut segments: Vec<Segment>, tol: f64) -> Vec<Segment> {
    'again: loop {
        for i in 0..segments.len() {
            for j in i + 1..segments.len() {
                if let Some(joined) = join(segments[i], segments[j], tol) {
                    segments[i] = joined;
                    segments.remove(j);
                    continue 'again;
                }
            }
        }
        return segments;
    }
}

/// L'unione di due segmenti collineari (ognuno a non più di `tol` dalla retta
/// dell'altro) che si sovrappongono o si toccano; i suoi estremi sono due dei
/// quattro estremi dati.
fn join(s: Segment, o: Segment, tol: f64) -> Option<Segment> {
    let off_line = |line: Segment, p: [f64; 2]| {
        let d = sub(line.1, line.0);
        let length = norm(d);
        if !(length > tol) {
            return f64::INFINITY;
        }
        cross(d, sub(p, line.0)).abs() / length
    };
    if off_line(s, o.0) > tol
        || off_line(s, o.1) > tol
        || off_line(o, s.0) > tol
        || off_line(o, s.1) > tol
    {
        return None;
    }
    let d = sub(s.1, s.0);
    let length = norm(d);
    let along = |p: [f64; 2]| dot(sub(p, s.0), d) / length;
    let (o0, o1) = (along(o.0), along(o.1));
    if o0.min(o1) > length + tol || o0.max(o1) < -tol {
        return None;
    }
    let ends = [s.0, s.1, o.0, o.1];
    let by = |x: &&[f64; 2], y: &&[f64; 2]| along(**x).total_cmp(&along(**y));
    let first = ends.iter().min_by(by)?;
    let last = ends.iter().max_by(by)?;
    Some((*first, *last))
}

/// Il tratto (nel parametro 0..1 di `a`) su cui `b` coincide con `a`:
/// la proiezione di `b` su `a`, se è lunga più di `tol` e i suoi due estremi
/// su `a` distano non più di `tol` da `b` (due segmenti dritti: allora tutto
/// il tratto). Robusto a scarti d'angolo minimi, che `segment_crossing`
/// (parallelismo a 1e-12) non vede.
fn coincident_stretch(a: Segment, b: Segment, tol: f64) -> Option<[f64; 2]> {
    let (p, q) = a;
    let d = sub(q, p);
    let length = norm(d);
    if !(length > tol) {
        return None;
    }
    let along = |x: [f64; 2]| dot(sub(x, p), d) / (length * length);
    let (s0, s1) = (along(b.0), along(b.1));
    let (lo, hi) = (s0.min(s1).max(0.0), s0.max(s1).min(1.0));
    if !((hi - lo) * length > tol) {
        return None;
    }
    let (from, to) = (lerp2(p, q, lo), lerp2(p, q, hi));
    (segment_distance(from, b.0, b.1) <= tol && segment_distance(to, b.0, b.1) <= tol)
        .then_some([lo, hi])
}

/// I tratti di `side` che coincidono con le linee date, uniti e puliti alla
/// tolleranza (parametro 0..1 di `side`).
fn stretches_on(side: Segment, lines: &[Segment], tol: f64) -> Vec<[f64; 2]> {
    let found: Vec<[f64; 2]> = lines
        .iter()
        .filter_map(|&line| coincident_stretch(side, line, tol))
        .collect();
    tidy(&found, distance(side.0, side.1), tol)
}

/// Un punto dove gli interni delle due zone si sovrappongono: dove due lati
/// si incrociano all'interno di entrambi, oppure un vertice, un punto medio
/// di lato o il baricentro di una strettamente dentro l'altra (il baricentro
/// solo se sta dentro la sua zona: quello di una zona a U può cadere fuori).
/// `None` se si toccano soltanto (lati comuni, vertici sul bordo).
fn overlap_witness(a: &ZoneInfo, b: &ZoneInfo, tolerance: Tolerance) -> Option<[f64; 2]> {
    let tol = tolerance.linear();
    let (alo, ahi) = a.bounds;
    let (blo, bhi) = b.bounds;
    if alo[0] > bhi[0] + tol
        || blo[0] > ahi[0] + tol
        || alo[1] > bhi[1] + tol
        || blo[1] > ahi[1] + tol
    {
        return None;
    }
    for e in &a.edges {
        for f in &b.edges {
            let (Curve::Line(e), Curve::Line(f)) = (e, f) else {
                continue;
            };
            if let Some(p) = proper_crossing(*e, *f, tolerance) {
                return Some(p);
            }
        }
    }
    for (x, y) in [(a, b), (b, a)] {
        let probes = x
            .geom
            .rings
            .iter()
            .flat_map(|ring| {
                ring.iter().copied().chain(
                    ring.iter()
                        .zip(ring.iter().cycle().skip(1))
                        .map(|(&p, &q)| midpoint(p, q)),
                )
            })
            .chain(std::iter::once(x.centroid).filter(|&c| x.locate(c, tolerance) == Place::In));
        for p in probes {
            if y.locate(p, tolerance) == Place::In {
                return Some(p);
            }
        }
    }
    None
}

/// Il punto dove due lati si incrociano a più di `tol` dagli estremi di
/// entrambi.
fn proper_crossing(e: Line, f: Line, tolerance: Tolerance) -> Option<[f64; 2]> {
    let tol = tolerance.linear();
    let SegmentCrossing::Point { a, b } = segment_crossing(e, f, tolerance) else {
        return None;
    };
    let inside = |t: f64, length: f64| t * length > tol && (1.0 - t) * length > tol;
    (inside(a, distance(e.start, e.end)) && inside(b, distance(f.start, f.end)))
        .then(|| lerp2(e.start, e.end, a))
}

// ---------------------------------------------------------------------------
// 5 e 6. Elementi puntuali ed entità non tagliabili
// ---------------------------------------------------------------------------

fn point_and_uncuttable(
    doc: &Document,
    walls: &WallSet,
    zones: &[ZoneGeom],
    demolished: &[&ZoneInfo],
    tolerance: Tolerance,
    notices: &mut Vec<Notice>,
) -> Vec<Handle> {
    let mut excluded = Vec::new();
    let Some((_, handles)) = model_space(doc).filter(|_| !demolished.is_empty()) else {
        return excluded;
    };
    let skip: FxHashSet<Handle> = walls
        .entities
        .iter()
        .map(|(handle, _)| *handle)
        .chain(zones.iter().map(|zone| zone.handle))
        .collect();
    for &handle in handles {
        if skip.contains(&handle) {
            continue;
        }
        let Some(entity) = doc.get_entity(handle) else {
            continue;
        };
        if belongs_to_module(entity)
            || matches!(
                entity,
                EntityType::Block(_) | EntityType::BlockEnd(_) | EntityType::Seqend(_)
            )
        {
            continue;
        }
        let kind = || entity.as_entity().entity_type().to_string();

        // 5. Elementi puntuali.
        if let Some(p) = insertion_point(entity) {
            if demolished
                .iter()
                .any(|zone| zone.locate(p, tolerance) != Place::Out)
            {
                excluded.push(handle);
                notices.push(Notice::Esclusa {
                    handle,
                    kind: kind(),
                });
                continue;
            }
        }

        // 6. Entità non tagliabili (e quelle tutte dentro una zona).
        let points = sample_points(entity);
        if points.is_empty() {
            continue;
        }
        let (mut inside, mut crossing) = (false, false);
        for zone in demolished {
            let places: Vec<Place> = points.iter().map(|&p| zone.locate(p, tolerance)).collect();
            if !places.contains(&Place::Out) {
                inside = true;
                break;
            }
            crossing |= places.contains(&Place::In);
        }
        if inside {
            excluded.push(handle);
            notices.push(Notice::Esclusa {
                handle,
                kind: kind(),
            });
        } else if crossing {
            notices.push(Notice::NonTagliata {
                handle,
                kind: kind(),
            });
        }
    }
    excluded
}

/// Zone, locali e ogni entità sui layer `AL-*`.
fn belongs_to_module(entity: &EntityType) -> bool {
    let common = entity.common();
    is_bim_layer(&common.layer)
        || common.extended_data.get_record(APP_ZONA).is_some()
        || common.extended_data.get_record(APP_LOCALE).is_some()
}

/// Il punto di inserimento sul piano XY di `INSERT`, `TEXT` (in OCS nel
/// disegno) e `MTEXT` (già nel mondo).
fn insertion_point(entity: &EntityType) -> Option<[f64; 2]> {
    match entity {
        EntityType::Insert(insert) => Some(ocs_xy(insert.insert_point, insert.normal)),
        EntityType::Text(text) => Some(ocs_xy(text.insertion_point, text.normal)),
        EntityType::MText(mtext) => Some([mtext.insertion_point.x, mtext.insertion_point.y]),
        _ => None,
    }
}

fn ocs_xy(point: Vector3, normal: Vector3) -> [f64; 2] {
    let plane = ocs_plane(normal, point.z);
    [
        plane.origin[0] + plane.x_axis[0] * point.x + plane.y_axis[0] * point.y,
        plane.origin[1] + plane.x_axis[1] * point.x + plane.y_axis[1] * point.y,
    ]
}

/// I punti di un'entità sul piano XY: i punti di presa (vertici, punti di
/// controllo, di definizione), i punti della curva se è una curva limitata e
/// i vertici dei contorni se è una campitura.
fn sample_points(entity: &EntityType) -> Vec<[f64; 2]> {
    let mut points: Vec<[f64; 2]> = entity
        .grips()
        .iter()
        .map(|grip| [grip.world.x, grip.world.y])
        .collect();
    if let Some(curve) = entity_curve_xy(entity).filter(|curve| curve.extent() == Extent::Bounded) {
        points.extend(curve.tessellate_angle(DEFAULT_ANGLE));
    }
    if let EntityType::Hatch(hatch) = entity {
        points.extend(
            crate::scene::hatch_boundary_rings(hatch)
                .into_iter()
                .flatten(),
        );
    }
    points.retain(|p| p[0].is_finite() && p[1].is_finite());
    points
}

// ---------------------------------------------------------------------------
// Vettori
// ---------------------------------------------------------------------------

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn norm(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    norm(sub(a, b))
}

fn midpoint(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])]
}

/// Il punto a `t` da `a` verso `b`, esatto agli estremi.
fn lerp2(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    if t <= 0.0 {
        a
    } else if t >= 1.0 {
        b
    } else {
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
    }
}

fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = sub(b, a);
    let squared = dot(d, d);
    if !(squared > 0.0) {
        return distance(p, a);
    }
    let t = (dot(sub(p, a), d) / squared).clamp(0.0, 1.0);
    distance(p, lerp2(a, b, t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::hatch::BoundaryPathFlags;
    use codec::entities::{
        BoundaryEdge, BoundaryPath, Circle, Hatch, LwPolyline, PolylineEdge, Spline, Text,
    };
    use codec::{Color, Vector2, Vector3};
    use kernel::geom2d::{contains, distance_to, Curve, Line, Tolerance};
    use kernel::tessellation::DEFAULT_ANGLE;

    use crate::bim::fixtures::{case_a, scene_with, Form, LAYER_MURI};
    use crate::bim::units::DrawingUnit;
    use crate::bim::walls::{
        rect_from_axis, rect_from_axis_thickness, sources_of, wall_set, WallRect,
    };
    use crate::bim::zone::{read_zone, zone_hatch, Zone, ZoneOrigin, ZoneState};
    use crate::bim::MAX_WALL_THICKNESS_M;
    use crate::scene::Scene;

    const TOL: f64 = 1.0e-6;

    // -----------------------------------------------------------------------
    // Aiuti
    // -----------------------------------------------------------------------

    fn project_on(scene: &Scene, zones: Vec<ZoneGeom>) -> ProjectGeometry {
        let walls = wall_set(&scene.document, TOL);
        project_geometry(&scene.document, &walls, &zones, LAYER_MURI, TOL)
    }

    /// `project_geometry` su `scene_with(entities, 6)` (metri), muri nuovi
    /// sul layer `MURI`, tolleranza 1e-6.
    fn project(entities: Vec<EntityType>, zones: Vec<ZoneGeom>) -> ProjectGeometry {
        project_on(&scene_with(entities, 6), zones)
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

    /// Zona demolita «muro tra due punti»: il rettangolo di `rect_from_axis`
    /// sulle facce del caso A (il tramezzo è lo stesso nelle due forme).
    fn axis_zone(p1: [f64; 2], p2: [f64; 2], varco: bool) -> ZoneGeom {
        axis_zone_on(case_a(Form::Continua, DrawingUnit::Metri), p1, p2)
            .map(|mut zone| {
                zone.zone.varco = varco;
                zone
            })
            .expect("muro")
    }

    /// Zona demolita «muro tra due punti» sulle facce delle entità date.
    fn axis_zone_on(entities: Vec<EntityType>, p1: [f64; 2], p2: [f64; 2]) -> Option<ZoneGeom> {
        let scene = scene_with(entities, 6);
        let sources = wall_set(&scene.document, TOL).sources;
        let rect = rect_from_axis(&sources, p1, p2, MAX_WALL_THICKNESS_M, TOL).ok()?;
        Some(axis_zone_with(
            ZoneState::Demolito,
            ZoneOrigin::MuroDuePunti,
            rect,
            false,
        ))
    }

    /// Zona nuova «linea + spessore».
    fn new_zone(p1: [f64; 2], p2: [f64; 2], thickness: f64) -> ZoneGeom {
        let rect = rect_from_axis_thickness(p1, p2, thickness).expect("rettangolo");
        axis_zone_with(ZoneState::Nuovo, ZoneOrigin::LineaSpessore, rect, false)
    }

    /// Zona senza asse da un rettangolo a due punti.
    fn rect_zone(state: ZoneState, min: [f64; 2], max: [f64; 2], varco: bool) -> ZoneGeom {
        zone_geom(
            Zone {
                state,
                origin: ZoneOrigin::Rettangolo,
                axis: None,
                thickness: None,
                height: None,
                varco,
            },
            vec![min, [max[0], min[1]], max, [min[0], max[1]]],
        )
    }

    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() <= 1.0e-6 && (a[1] - b[1]).abs() <= 1.0e-6
    }

    fn line_ends(entity: &EntityType) -> Option<([f64; 2], [f64; 2])> {
        match entity {
            EntityType::Line(line) => {
                Some(([line.start.x, line.start.y], [line.end.x, line.end.y]))
            }
            _ => None,
        }
    }

    fn is_line(entity: &EntityType, a: [f64; 2], b: [f64; 2]) -> bool {
        line_ends(entity)
            .is_some_and(|(p, q)| (near(p, a) && near(q, b)) || (near(p, b) && near(q, a)))
    }

    /// In `drawn` c'è una `LINE` da `a` a `b`, in qualunque verso.
    fn has_line(pg: &ProjectGeometry, a: [f64; 2], b: [f64; 2]) -> bool {
        pg.drawn.iter().any(|entity| is_line(entity, a, b))
    }

    /// Le curve dell'entità sul piano XY.
    fn curves_of(entity: &EntityType) -> Vec<Curve> {
        let (sources, _) = sources_of(&[(Handle::new(1), entity.clone())], TOL);
        sources
            .into_values()
            .flat_map(|source| source.curves)
            .collect()
    }

    /// Qualcuna delle entità ha un tratto dritto su `x = xs[k]` con
    /// `y0 < y < y1`?
    fn stretch_on<'a>(
        entities: impl IntoIterator<Item = &'a EntityType>,
        xs: [f64; 2],
        (y0, y1): (f64, f64),
    ) -> bool {
        entities.into_iter().flat_map(curves_of).any(|curve| {
            let Curve::Line(line) = curve else {
                return false;
            };
            let (lo, hi) = (
                line.start[1].min(line.end[1]),
                line.start[1].max(line.end[1]),
            );
            xs.iter().any(|&x| {
                (line.start[0] - x).abs() <= 1.0e-6
                    && (line.end[0] - x).abs() <= 1.0e-6
                    && hi.min(y1) - lo.max(y0) > 1.0e-6
            })
        })
    }

    /// Le entità muro non toccate, prese da `room_entities` per handle.
    fn kept<'a>(pg: &'a ProjectGeometry) -> impl Iterator<Item = &'a EntityType> {
        pg.room_entities
            .iter()
            .filter(|(handle, _)| pg.kept_originals.contains(handle))
            .map(|(_, entity)| entity)
    }

    /// Nessun pezzo in `drawn` né originale in `kept_originals` ha un tratto
    /// su `x = xs[k]` con `y0 < y < y1`.
    fn no_wall_inside(pg: &ProjectGeometry, xs: [f64; 2], span: (f64, f64)) -> bool {
        !stretch_on(pg.drawn.iter().chain(kept(pg)), xs, span)
    }

    fn at_of(notice: &Notice) -> Option<[f64; 2]> {
        match notice {
            Notice::ConfiniNonRimossi { at }
            | Notice::GiunzioneDaVerificare { at }
            | Notice::TaglioNonChiuso { at }
            | Notice::NuoveSovrapposte { at }
            | Notice::VarcoNonSupportato { at }
            | Notice::FacciaNonTolta { at } => Some(*at),
            _ => None,
        }
    }

    fn same_notice(a: &Notice, b: &Notice) -> bool {
        match (at_of(a), at_of(b)) {
            (Some(p), Some(q)) => {
                std::mem::discriminant(a) == std::mem::discriminant(b) && near(p, q)
            }
            _ => a == b,
        }
    }

    /// Gli avvisi sono esattamente quelli attesi, in qualunque ordine, con le
    /// posizioni entro 1e-6.
    #[track_caller]
    fn assert_notices(got: &[Notice], want: &[Notice]) {
        let mut left: Vec<&Notice> = got.iter().collect();
        for notice in want {
            let Some(index) = left.iter().position(|seen| same_notice(seen, notice)) else {
                panic!("manca {notice:?}; avvisi: {got:?}");
            };
            left.remove(index);
        }
        assert!(left.is_empty(), "avvisi in più: {left:?}; tutti: {got:?}");
    }

    fn giunzione(at: [f64; 2]) -> Notice {
        Notice::GiunzioneDaVerificare { at }
    }

    /// Gli handle delle entità della scena del tipo dato, in ordine.
    fn handles_of(scene: &Scene, kind: &str) -> Vec<Handle> {
        let doc = &scene.document;
        doc.block_records
            .iter()
            .filter(|record| record.is_model_space())
            .flat_map(|record| record.entity_handles.iter().copied())
            .filter(|&handle| {
                doc.get_entity(handle)
                    .is_some_and(|entity| entity.as_entity().entity_type() == kind)
            })
            .collect()
    }

    fn solid_hatch(layer: &str, corners: &[[f64; 2]]) -> EntityType {
        let vertices = corners.iter().map(|c| Vector2::new(c[0], c[1])).collect();
        let mut path = BoundaryPath::with_flags(BoundaryPathFlags::from_bits(
            BoundaryPathFlags::OUTERMOST.bits() | BoundaryPathFlags::EXTERNAL.bits(),
        ));
        path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(vertices, true)));
        let mut hatch = Hatch::new();
        hatch.paths = vec![path];
        hatch.common.layer = layer.to_string();
        EntityType::Hatch(Box::new(hatch))
    }

    fn lw(layer: &str, vertices: &[([f64; 2], f64)]) -> EntityType {
        let mut poly = LwPolyline::from_points(
            vertices
                .iter()
                .map(|(p, _)| Vector2::new(p[0], p[1]))
                .collect(),
        );
        for (vertex, (_, bulge)) in poly.vertices.iter_mut().zip(vertices) {
            vertex.bulge = *bulge;
        }
        poly.common.layer = layer.to_string();
        EntityType::LwPolyline(poly)
    }

    fn is_by_layer(entity: &EntityType) -> bool {
        let common = entity.common();
        common.color == Color::ByLayer
            && (common.linetype.is_empty() || common.linetype.eq_ignore_ascii_case("ByLayer"))
    }

    // -----------------------------------------------------------------------
    // Casi del piano
    // -----------------------------------------------------------------------

    #[test]
    fn total_demolition_continuous_closes_the_outer_faces() {
        let pg = project(
            case_a(Form::Continua, DrawingUnit::Metri),
            vec![axis_zone([4.05, 0.0], [4.05, 3.0], false)],
        );
        assert!(
            has_line(&pg, [4.0, 0.0], [4.1, 0.0]) && has_line(&pg, [4.0, 3.0], [4.1, 3.0]),
            "{:?}",
            pg.drawn
        );
        // nessun tratto x = 4,00 o x = 4,10 con 0 < y < 3
        assert!(no_wall_inside(&pg, [4.0, 4.1], (0.0, 3.0)));
        assert!(pg.notices.is_empty(), "{:?}", pg.notices);
        // Solo le due chiusure: le due facce del tramezzo spariscono intere.
        assert_eq!(pg.drawn.len(), 2, "{:?}", pg.drawn);
        assert_eq!(pg.kept_originals.len(), 10);
        assert!(pg.excluded.is_empty());
    }

    #[test]
    fn total_demolition_closed_walls_reports_junctions() {
        let scene = scene_with(case_a(Form::ChiusaPerMuro, DrawingUnit::Metri), 6);
        let pg = project_on(&scene, vec![axis_zone([4.05, 0.0], [4.05, 3.0], false)]);
        assert_notices(
            &pg.notices,
            &[giunzione([4.05, 0.0]), giunzione([4.05, 3.0])],
        );
        // Nessuna chiusura: i lati di estremità coincidono con muri che restano.
        assert!(!has_line(&pg, [4.0, 0.0], [4.1, 0.0]));
        assert!(!has_line(&pg, [4.0, 3.0], [4.1, 3.0]));
        // La polilinea sud resta intera (y = 0 da -0,30 a 8,30), come quella nord.
        let walls = handles_of(&scene, "LWPOLYLINE");
        assert!(pg.kept_originals.contains(&walls[0]), "sud");
        assert!(pg.kept_originals.contains(&walls[1]), "nord");
        // Il tramezzo è tagliato: restano i suoi lati corti, nessuna faccia.
        assert!(!pg.kept_originals.contains(&walls[4]), "tramezzo");
        assert!(no_wall_inside(&pg, [4.0, 4.1], (0.0, 3.0)));
    }

    /// Fix finale, punto 1: l'asse cliccato a occhio sfora nei muri d'ambito
    /// (da y = −0,10 a y = 3,10). La zona si ferma sulle facce del tramezzo e
    /// la pianta di progetto è quella dell'asse esatto: nessun pezzo tolto
    /// dalle facce y = 0 e y = 3 dei muri d'ambito, nessun `TaglioNonChiuso`,
    /// nessuna chiusura ridisegnata sulla faccia demolita (R39).
    #[test]
    fn axis_past_the_faces_gives_the_plan_of_the_exact_axis() {
        let not_cut = |notices: &[Notice]| {
            !notices
                .iter()
                .any(|n| matches!(n, Notice::TaglioNonChiuso { .. }))
        };

        // Linee continue: le due chiusure, nient'altro.
        let entities = case_a(Form::Continua, DrawingUnit::Metri);
        let zone = axis_zone_on(entities.clone(), [4.05, -0.10], [4.05, 3.10]).expect("muro");
        let pg = project(entities, vec![zone]);
        assert!(not_cut(&pg.notices), "{:?}", pg.notices);
        assert!(pg.notices.is_empty(), "{:?}", pg.notices);
        assert!(has_line(&pg, [4.0, 0.0], [4.1, 0.0]), "{:?}", pg.drawn);
        assert!(has_line(&pg, [4.0, 3.0], [4.1, 3.0]), "{:?}", pg.drawn);
        assert_eq!(pg.drawn.len(), 2, "{:?}", pg.drawn);
        assert_eq!(pg.kept_originals.len(), 10);
        assert!(no_wall_inside(&pg, [4.0, 4.1], (0.0, 3.0)));

        // Una polilinea per muro: le due giunzioni, sud e nord intere.
        let entities = case_a(Form::ChiusaPerMuro, DrawingUnit::Metri);
        let scene = scene_with(entities.clone(), 6);
        let zone = axis_zone_on(entities, [4.05, -0.10], [4.05, 3.10]).expect("muro");
        let pg = project_on(&scene, vec![zone]);
        assert!(not_cut(&pg.notices), "{:?}", pg.notices);
        assert_notices(
            &pg.notices,
            &[giunzione([4.05, 0.0]), giunzione([4.05, 3.0])],
        );
        let walls = handles_of(&scene, "LWPOLYLINE");
        assert!(pg.kept_originals.contains(&walls[0]), "sud");
        assert!(pg.kept_originals.contains(&walls[1]), "nord");
        assert!(!has_line(&pg, [4.0, 0.0], [4.1, 0.0]), "{:?}", pg.drawn);
        assert!(!has_line(&pg, [4.0, 3.0], [4.1, 3.0]), "{:?}", pg.drawn);
        assert!(no_wall_inside(&pg, [4.0, 4.1], (0.0, 3.0)));
    }

    #[test]
    fn varco_keeps_jambs_and_soglie_only_for_rooms() {
        let pg = project(
            case_a(Form::Continua, DrawingUnit::Metri),
            vec![axis_zone([4.05, 1.0], [4.05, 1.9], true)],
        );
        // Le due chiusure (spallette) a y = 1,00 e y = 1,90.
        assert!(has_line(&pg, [4.0, 1.0], [4.1, 1.0]), "{:?}", pg.drawn);
        assert!(has_line(&pg, [4.0, 1.9], [4.1, 1.9]), "{:?}", pg.drawn);
        // Il vano è aperto nel disegno.
        assert!(!stretch_on(pg.drawn.iter(), [4.0, 4.1], (1.0, 1.9)));
        assert!(!stretch_on(kept(&pg), [4.0, 4.1], (1.0, 1.9)));
        // Le facce fuori dal varco restano.
        assert!(stretch_on(pg.drawn.iter(), [4.0, 4.0], (0.0, 1.0)));
        assert!(stretch_on(pg.drawn.iter(), [4.0, 4.0], (1.9, 3.0)));
        assert!(stretch_on(pg.drawn.iter(), [4.1, 4.1], (0.0, 1.0)));
        assert!(stretch_on(pg.drawn.iter(), [4.1, 4.1], (1.9, 3.0)));
        // Le soglie (lati lunghi) solo per i locali.
        let rooms: Vec<&EntityType> = pg.room_entities.iter().map(|(_, e)| e).collect();
        assert!(rooms.iter().any(|e| is_line(e, [4.0, 1.0], [4.0, 1.9])));
        assert!(rooms.iter().any(|e| is_line(e, [4.1, 1.0], [4.1, 1.9])));
        assert!(!has_line(&pg, [4.0, 1.0], [4.0, 1.9]));
        assert!(!has_line(&pg, [4.1, 1.0], [4.1, 1.9]));
        assert!(pg.notices.is_empty(), "{:?}", pg.notices);
        // Si passa così com'è a sources_of: handle tutti diversi, nessuna
        // entità persa.
        let (sources, off_plane) = sources_of(&pg.room_entities, TOL);
        assert_eq!((sources.len(), off_plane), (pg.room_entities.len(), 0));
        let synthetic: Vec<u64> = pg.room_entities[pg.kept_originals.len()..]
            .iter()
            .map(|(handle, entity)| {
                assert_eq!(entity.common().handle, *handle);
                handle.value()
            })
            .collect();
        let want: Vec<u64> = (0..synthetic.len() as u64)
            .map(|offset| FIRST_SYNTHETIC_HANDLE + offset)
            .collect();
        assert_eq!(synthetic, want);
    }

    /// Fix finale, punto 6: due zone doppie che differiscono solo per il varco
    /// (una demolizione ripetuta, una delle due con il varco): vale quella con
    /// il varco, in qualunque ordine. Prima vinceva la prima e il varco poteva
    /// sparire in silenzio.
    #[test]
    fn duplicate_zones_keep_the_varco() {
        for varco_first in [false, true] {
            let plain = axis_zone([4.05, 1.0], [4.05, 1.9], false);
            let varco = axis_zone([4.05, 1.0], [4.05, 1.9], true);
            let zones = if varco_first {
                vec![varco, plain]
            } else {
                vec![plain, varco]
            };
            let pg = project(case_a(Form::Continua, DrawingUnit::Metri), zones);
            let rooms: Vec<&EntityType> = pg.room_entities.iter().map(|(_, e)| e).collect();
            assert!(
                rooms.iter().any(|e| is_line(e, [4.0, 1.0], [4.0, 1.9])),
                "varco per primo: {varco_first}"
            );
            assert!(
                rooms.iter().any(|e| is_line(e, [4.1, 1.0], [4.1, 1.9])),
                "varco per primo: {varco_first}"
            );
            assert!(!has_line(&pg, [4.0, 1.0], [4.0, 1.9]), "{varco_first}");
            assert!(has_line(&pg, [4.0, 1.0], [4.1, 1.0]), "{varco_first}");
            assert!(pg.notices.is_empty(), "{:?}", pg.notices);
        }
    }

    /// Un disegno con handle già oltre 0xF000_0000: i sintetici partono sopra.
    #[test]
    fn synthetic_handles_never_collide_with_walls() {
        let scene = scene_with(Vec::new(), 6);
        let mut walls = wall_set(&scene.document, TOL);
        let high = Handle::new(FIRST_SYNTHETIC_HANDLE + 1);
        // Lontano dalla zona: resta. L'altro la attraversa: due pezzi.
        let mut line = codec::entities::Line::from_points(
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(8.0, 5.0, 0.0),
        );
        line.common.layer = LAYER_MURI.into();
        let mut other = line.clone();
        other.start = Vector3::new(0.0, 1.0, 0.0);
        other.end = Vector3::new(8.0, 1.0, 0.0);
        walls.entities = vec![
            (high, EntityType::Line(line)),
            (Handle::new(0x30), EntityType::Line(other)),
        ];
        walls.sources = sources_of(&walls.entities, TOL).0;
        let pg = project_geometry(
            &scene.document,
            &walls,
            &[axis_zone([4.05, 0.0], [4.05, 3.0], false)],
            LAYER_MURI,
            TOL,
        );
        assert_eq!(pg.kept_originals, vec![high]);
        assert_eq!(pg.drawn.len(), 2, "{:?}", pg.drawn);
        let (sources, _) = sources_of(&pg.room_entities, TOL);
        assert_eq!(sources.len(), 3);
        assert_eq!(pg.room_entities.len(), 3);
        assert!(pg.room_entities[1..]
            .iter()
            .all(|(handle, _)| handle.value() > high.value()));
    }

    /// Il baricentro di una zona a U cade nel suo incavo: non è un punto
    /// della zona e non fa da prova di sovrapposizione.
    #[test]
    fn new_zone_in_the_notch_of_a_u_is_not_an_overlap() {
        let u = zone_geom(
            Zone {
                state: ZoneState::Nuovo,
                origin: ZoneOrigin::Poligono,
                axis: None,
                thickness: None,
                height: None,
                varco: false,
            },
            vec![
                [0.0, 0.0],
                [3.0, 0.0],
                [3.0, 3.0],
                [2.0, 3.0],
                [2.0, 1.0],
                [1.0, 1.0],
                [1.0, 3.0],
                [0.0, 3.0],
            ],
        );
        let notch = rect_zone(ZoneState::Nuovo, [1.2, 1.2], [1.8, 2.8], false);
        let pg = project(Vec::new(), vec![u, notch]);
        assert!(pg.notices.is_empty(), "{:?}", pg.notices);
        assert_eq!(pg.drawn.len(), 12, "{:?}", pg.drawn);
    }

    /// Un vertice ripetuto nel contorno non toglie l'asse alla zona; un
    /// contorno cambiato o lo spessore mancante sì.
    #[test]
    fn axis_counts_only_while_the_outline_is_its_rectangle() {
        let mut repeated = axis_zone([4.05, 0.0], [4.05, 3.0], false);
        let first = repeated.rings[0][0];
        repeated.rings[0].insert(1, first);
        repeated.rings[0].push(first);
        let pg = project(case_a(Form::Continua, DrawingUnit::Metri), vec![repeated]);
        assert!(pg.notices.is_empty(), "{:?}", pg.notices);
        assert!(has_line(&pg, [4.0, 0.0], [4.1, 0.0]));
        assert!(has_line(&pg, [4.0, 3.0], [4.1, 3.0]));

        let mut edited = axis_zone([4.05, 0.0], [4.05, 3.0], false);
        edited.rings[0] = vec![[4.0, 0.0], [4.1, 0.0], [4.1, 2.5], [4.0, 2.5]];
        let mut thin = axis_zone([4.05, 0.0], [4.05, 3.0], false);
        thin.zone.thickness = None;
        for (zone, centre) in [(edited, [4.05, 1.25]), (thin, [4.05, 1.5])] {
            let pg = project(case_a(Form::Continua, DrawingUnit::Metri), vec![zone]);
            assert!(pg.drawn.is_empty(), "{:?}", pg.drawn);
            assert_eq!(pg.kept_originals.len(), 12);
            assert_notices(&pg.notices, &[Notice::ConfiniNonRimossi { at: centre }]);
        }
    }

    #[test]
    fn new_l_shape_has_no_seam() {
        // Disegno senza muri esistenti (R6).
        let pg = project(
            Vec::new(),
            vec![
                rect_zone(ZoneState::Nuovo, [1.00, 0.0], [1.12, 2.12], false),
                rect_zone(ZoneState::Nuovo, [1.12, 2.00], [2.50, 2.12], false),
            ],
        );
        assert_eq!(pg.drawn.len(), 6, "{:?}", pg.drawn);
        for entity in &pg.drawn {
            assert!(line_ends(entity).is_some(), "solo LINE: {entity:?}");
            assert_eq!(entity.common().layer, LAYER_MURI);
            assert!(is_by_layer(entity), "{:?}", entity.common());
        }
        for (a, b) in [
            ([1.00, 0.0], [1.12, 0.0]),
            ([1.12, 0.0], [1.12, 2.00]),
            ([1.12, 2.00], [2.50, 2.00]),
            ([2.50, 2.00], [2.50, 2.12]),
            ([2.50, 2.12], [1.00, 2.12]),
            ([1.00, 2.12], [1.00, 0.0]),
        ] {
            assert!(has_line(&pg, a, b), "manca {a:?}-{b:?}: {:?}", pg.drawn);
        }
        // Nessuna cucitura su x = 1,12 tra 2,00 e 2,12.
        assert!(!stretch_on(pg.drawn.iter(), [1.12, 1.12], (2.00, 2.12)));
        assert!(pg.notices.is_empty(), "{:?}", pg.notices);
    }

    #[test]
    fn rebuilt_on_demolished_leaves_no_caps() {
        let pg = project(
            case_a(Form::Continua, DrawingUnit::Metri),
            vec![
                axis_zone([4.05, 0.0], [4.05, 3.0], false),
                new_zone([4.05, 0.0], [4.05, 3.0], 0.10),
            ],
        );
        assert!(!has_line(&pg, [4.0, 0.0], [4.1, 0.0]), "{:?}", pg.drawn);
        assert!(!has_line(&pg, [4.0, 3.0], [4.1, 3.0]), "{:?}", pg.drawn);
        assert!(has_line(&pg, [4.0, 0.0], [4.0, 3.0]), "{:?}", pg.drawn);
        assert!(has_line(&pg, [4.1, 0.0], [4.1, 3.0]), "{:?}", pg.drawn);
        assert_eq!(pg.drawn.len(), 2, "{:?}", pg.drawn);
        for entity in &pg.drawn {
            assert_eq!(entity.common().layer, LAYER_MURI);
            assert!(is_by_layer(entity));
        }
        assert!(pg.notices.is_empty(), "{:?}", pg.notices);
    }

    #[test]
    fn new_against_existing_reports_junctions() {
        let pg = project(
            case_a(Form::Continua, DrawingUnit::Metri),
            vec![new_zone([2.0, 0.0], [2.0, 3.0], 0.12)],
        );
        assert_notices(&pg.notices, &[giunzione([2.0, 0.0]), giunzione([2.0, 3.0])]);
        assert!(has_line(&pg, [1.94, 0.0], [1.94, 3.0]), "{:?}", pg.drawn);
        assert!(has_line(&pg, [2.06, 0.0], [2.06, 3.0]), "{:?}", pg.drawn);
        assert_eq!(pg.drawn.len(), 2, "{:?}", pg.drawn);
        // Nessuna linea doppia sulle facce di A.
        assert!(!has_line(&pg, [1.94, 0.0], [2.06, 0.0]));
        assert!(!has_line(&pg, [1.94, 3.0], [2.06, 3.0]));
        assert_eq!(pg.kept_originals.len(), 12);
    }

    #[test]
    fn point_and_uncuttable_entities() {
        let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
        let mut text = Text::with_value("Tramezzo", Vector3::new(4.05, 1.5, 0.0)).with_height(0.2);
        text.common.layer = "TESTI".into();
        entities.push(EntityType::Text(text));
        let mut spline = Spline::from_control_points(
            2,
            vec![
                Vector3::new(3.5, 1.0, 0.0),
                Vector3::new(4.05, 1.2, 0.0),
                Vector3::new(4.6, 1.0, 0.0),
            ],
        );
        spline.common.layer = "ARREDI".into();
        entities.push(EntityType::Spline(spline));
        let scene = scene_with(entities, 6);
        let pg = project_on(&scene, vec![axis_zone([4.05, 0.0], [4.05, 3.0], false)]);

        let text = handles_of(&scene, "TEXT")[0];
        let spline = handles_of(&scene, "SPLINE")[0];
        assert_eq!(pg.excluded, vec![text]);
        assert_notices(
            &pg.notices,
            &[
                Notice::Esclusa {
                    handle: text,
                    kind: "TEXT".into(),
                },
                Notice::NonTagliata {
                    handle: spline,
                    kind: "SPLINE".into(),
                },
            ],
        );
        // La SPLINE non è un muro (R37): né in drawn né in kept_originals.
        assert!(!pg.kept_originals.contains(&spline));
        assert!(!pg
            .drawn
            .iter()
            .any(|entity| matches!(entity, EntityType::Spline(_))));
        assert!(!pg
            .room_entities
            .iter()
            .any(|(h, _)| *h == spline || *h == text));
    }

    #[test]
    fn zone_without_axis_leaves_borders() {
        let pg = project(
            case_a(Form::Continua, DrawingUnit::Metri),
            vec![rect_zone(
                ZoneState::Demolito,
                [4.00, 0.0],
                [4.10, 3.0],
                false,
            )],
        );
        assert!(pg.drawn.is_empty(), "{:?}", pg.drawn);
        assert_eq!(pg.kept_originals.len(), 12);
        assert!(stretch_on(kept(&pg), [4.0, 4.0], (0.0, 3.0)));
        assert!(stretch_on(kept(&pg), [4.1, 4.1], (0.0, 3.0)));
        assert_notices(
            &pg.notices,
            &[Notice::ConfiniNonRimossi { at: [4.05, 1.5] }],
        );
    }

    // -----------------------------------------------------------------------
    // Tolleranza, attributi, avvisi e casi limite
    // -----------------------------------------------------------------------

    /// Il bordo della zona passa per un vertice di polilinea, esattamente o a
    /// 0,4 µm (dentro la tolleranza): nessun pezzo, né lato di pezzo, più
    /// corto della tolleranza.
    #[test]
    fn border_through_a_vertex_leaves_no_slivers() {
        let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
        // Vertice esattamente sul lato lungo x = 4,10, poi lungo il lato.
        entities.push(lw(
            LAYER_MURI,
            &[
                ([2.0, 1.0], 0.0),
                ([4.1, 1.0], 0.0),
                ([4.1, 2.0], 0.0),
                ([6.0, 2.0], 0.0),
            ],
        ));
        // Lo stesso con il vertice 0,4 µm oltre il bordo.
        entities.push(lw(
            LAYER_MURI,
            &[
                ([2.0, 2.5], 0.0),
                ([4.100_000_4, 2.5], 0.0),
                ([4.100_000_4, 2.8], 0.0),
                ([6.0, 2.8], 0.0),
            ],
        ));
        // Un vertice esattamente su un vertice della zona.
        entities.push(lw(
            LAYER_MURI,
            &[([3.0, 0.5], 0.0), ([4.0, 0.0], 0.0), ([4.05, 0.5], 0.0)],
        ));
        let pg = project(entities, vec![axis_zone([4.05, 0.0], [4.05, 3.0], false)]);
        assert!(!pg.drawn.is_empty());
        for entity in &pg.drawn {
            let curves = curves_of(entity);
            let total: f64 = curves.iter().map(Curve::length).sum();
            assert!(total > TOL, "pezzo di {total} m: {entity:?}");
            for curve in &curves {
                assert!(
                    curve.length() > TOL,
                    "lato di {} m in {entity:?}",
                    curve.length()
                );
            }
        }
        // Le polilinee aggiunte restano fuori dalla zona.
        let zone = axis_zone([4.05, 0.0], [4.05, 3.0], false);
        assert!(!pg.drawn.iter().flat_map(curves_of).any(|curve| {
            curve
                .tessellate_angle(DEFAULT_ANGLE)
                .into_iter()
                .any(|p| strictly_inside(&zone, p))
        }));
    }

    fn strictly_inside(zone: &ZoneGeom, p: [f64; 2]) -> bool {
        let ring = &zone.rings[0];
        let edges: Vec<Curve> = ring
            .iter()
            .zip(ring.iter().cycle().skip(1))
            .map(|(&start, &end)| Curve::Line(Line { start, end }))
            .collect();
        contains(&edges, p, Tolerance::new(TOL)) && edges.iter().all(|e| distance_to(e, p) > TOL)
    }

    /// La chiusura prende layer, colore e tipo di linea del muro più vicino
    /// al suo primo estremo, non i suoi XDATA né la sua identità.
    #[test]
    fn closure_takes_the_attributes_of_the_nearest_wall() {
        let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
        // Il fondo del locale A, (0;0)-(4;0).
        let common = entities[4].common_mut();
        common.color = Color::from_index(3);
        common.linetype = "TRATTEGGIO".into();
        let pg = project(entities, vec![axis_zone([4.05, 0.0], [4.05, 3.0], false)]);
        let closure = pg
            .drawn
            .iter()
            .find(|e| is_line(e, [4.0, 0.0], [4.1, 0.0]))
            .expect("chiusura a y = 0");
        let common = closure.common();
        assert_eq!(common.layer, LAYER_MURI);
        assert_eq!(common.color, Color::from_index(3));
        assert_eq!(common.linetype, "TRATTEGGIO");
        assert_eq!(common.handle, Handle::NULL);
        let other = pg
            .drawn
            .iter()
            .find(|e| is_line(e, [4.0, 3.0], [4.1, 3.0]))
            .expect("chiusura a y = 3");
        assert_eq!(other.common().color, Color::ByLayer);
    }

    /// Un lato di estremità con un estremo nel vuoto: niente linea, avviso.
    #[test]
    fn open_end_reports_taglio_non_chiuso() {
        let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
        // Senza il fondo del locale B, (4,10;0)-(8;0).
        entities.remove(8);
        let pg = project(entities, vec![axis_zone([4.05, 0.0], [4.05, 3.0], false)]);
        assert_notices(&pg.notices, &[Notice::TaglioNonChiuso { at: [4.05, 0.0] }]);
        assert!(!has_line(&pg, [4.0, 0.0], [4.1, 0.0]));
        assert!(has_line(&pg, [4.0, 3.0], [4.1, 3.0]));
    }

    #[test]
    fn varco_without_axis_is_not_supported() {
        let pg = project(
            case_a(Form::Continua, DrawingUnit::Metri),
            vec![rect_zone(
                ZoneState::Demolito,
                [4.00, 1.0],
                [4.10, 1.9],
                true,
            )],
        );
        assert_notices(
            &pg.notices,
            &[
                Notice::ConfiniNonRimossi { at: [4.05, 1.45] },
                Notice::VarcoNonSupportato { at: [4.05, 1.45] },
            ],
        );
        assert_eq!(pg.room_entities.len(), 12, "nessuna soglia senza asse");
    }

    #[test]
    fn overlapping_new_zones_are_reported() {
        let pg = project(
            Vec::new(),
            vec![
                rect_zone(ZoneState::Nuovo, [1.0, 1.0], [2.0, 2.0], false),
                rect_zone(ZoneState::Nuovo, [1.5, 1.5], [2.5, 2.5], false),
            ],
        );
        let [Notice::NuoveSovrapposte { at }] = pg.notices.as_slice() else {
            panic!("{:?}", pg.notices);
        };
        assert!((1.5 - 1e-6..=2.0 + 1e-6).contains(&at[0]), "{at:?}");
        assert!((1.5 - 1e-6..=2.0 + 1e-6).contains(&at[1]), "{at:?}");
        // Non si risolvono: restano i due contorni.
        assert_eq!(pg.drawn.len(), 8, "{:?}", pg.drawn);
    }

    /// Due demolizioni di seguito sullo stesso muro: niente chiusura e niente
    /// «taglio non chiuso» dove si toccano.
    #[test]
    fn adjacent_demolitions_need_no_closure_between_them() {
        let pg = project(
            case_a(Form::Continua, DrawingUnit::Metri),
            vec![
                axis_zone([4.05, 0.0], [4.05, 1.5], false),
                axis_zone([4.05, 1.5], [4.05, 3.0], false),
            ],
        );
        assert!(pg.notices.is_empty(), "{:?}", pg.notices);
        assert!(has_line(&pg, [4.0, 0.0], [4.1, 0.0]));
        assert!(has_line(&pg, [4.0, 3.0], [4.1, 3.0]));
        assert!(!has_line(&pg, [4.0, 1.5], [4.1, 1.5]));
        assert_eq!(pg.drawn.len(), 2, "{:?}", pg.drawn);
        assert!(no_wall_inside(&pg, [4.0, 4.1], (0.0, 3.0)));
    }

    /// Una zona ripetuta conta una volta: le sue chiusure e i suoi lati non
    /// si cancellano a coppie con quelli del doppione.
    #[test]
    fn duplicated_zones_count_once() {
        let demolished = axis_zone([4.05, 0.0], [4.05, 3.0], false);
        let pg = project(
            case_a(Form::Continua, DrawingUnit::Metri),
            vec![demolished.clone(), demolished],
        );
        assert!(has_line(&pg, [4.0, 0.0], [4.1, 0.0]), "{:?}", pg.drawn);
        assert!(has_line(&pg, [4.0, 3.0], [4.1, 3.0]), "{:?}", pg.drawn);
        assert_eq!(pg.drawn.len(), 2);
        assert!(pg.notices.is_empty(), "{:?}", pg.notices);

        let built = new_zone([2.0, 0.0], [2.0, 3.0], 0.12);
        let pg = project(
            case_a(Form::Continua, DrawingUnit::Metri),
            vec![built.clone(), built],
        );
        assert!(has_line(&pg, [1.94, 0.0], [1.94, 3.0]), "{:?}", pg.drawn);
        assert!(has_line(&pg, [2.06, 0.0], [2.06, 3.0]), "{:?}", pg.drawn);
        assert_eq!(pg.drawn.len(), 2);
        assert_notices(&pg.notices, &[giunzione([2.0, 0.0]), giunzione([2.0, 3.0])]);
    }

    /// Un'entità non muro tutta dentro la zona demolita (la campitura del
    /// muro, un cerchio) esce dal progetto ed è elencata; una campitura che
    /// tocca la zona solo sul bordo resta, senza avvisi.
    #[test]
    fn non_wall_entities_inside_a_demolition_are_excluded() {
        let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
        entities.push(solid_hatch(
            "CAMPITURE",
            &[[4.0, 0.0], [4.1, 0.0], [4.1, 3.0], [4.0, 3.0]],
        ));
        entities.push(solid_hatch(
            "PAVIMENTI",
            &[[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]],
        ));
        let mut circle = Circle::from_center_radius(Vector3::new(4.05, 2.0, 0.0), 0.02);
        circle.common.layer = "ARREDI".into();
        entities.push(EntityType::Circle(circle));
        let scene = scene_with(entities, 6);
        let pg = project_on(&scene, vec![axis_zone([4.05, 0.0], [4.05, 3.0], false)]);
        let hatches = handles_of(&scene, "HATCH");
        let circle = handles_of(&scene, "CIRCLE")[0];
        assert_eq!(pg.excluded, vec![hatches[0], circle]);
        assert_notices(
            &pg.notices,
            &[
                Notice::Esclusa {
                    handle: hatches[0],
                    kind: "HATCH".into(),
                },
                Notice::Esclusa {
                    handle: circle,
                    kind: "CIRCLE".into(),
                },
            ],
        );
    }

    /// Un muro con un arco a bulge negativo attraversa una zona senza asse:
    /// il pezzo che resta parte dal bordo e non entra nella zona.
    #[test]
    fn arc_wall_is_cut_at_the_border() {
        let wall = lw(LAYER_MURI, &[([0.0, 5.0], -0.5), ([4.0, 5.0], 0.0)]);
        let zone = rect_zone(ZoneState::Demolito, [-1.0, 4.0], [1.5, 8.0], false);
        let pg = project(vec![wall], vec![zone.clone()]);
        assert_eq!(pg.drawn.len(), 1, "{:?}", pg.drawn);
        let curves = curves_of(&pg.drawn[0]);
        let points: Vec<[f64; 2]> = curves
            .iter()
            .flat_map(|c| c.tessellate_angle(DEFAULT_ANGLE))
            .collect();
        assert!(
            points.iter().all(|&p| !strictly_inside(&zone, p)),
            "{points:?}"
        );
        assert!(points.iter().any(|&p| near(p, [4.0, 5.0])));
        assert!(points.iter().any(|&p| (p[0] - 1.5).abs() <= 1e-6));
        assert!(pg.kept_originals.is_empty());
        assert_notices(
            &pg.notices,
            &[Notice::ConfiniNonRimossi { at: [0.25, 6.0] }],
        );
    }

    // -----------------------------------------------------------------------
    // Fix round 1: muri appoggiati a T (R39), facce fuori squadra (R40)
    // -----------------------------------------------------------------------

    fn wall_line(a: [f64; 2], b: [f64; 2]) -> EntityType {
        let mut line = codec::entities::Line::from_points(
            Vector3::new(a[0], a[1], 0.0),
            Vector3::new(b[0], b[1], 0.0),
        );
        line.common.layer = LAYER_MURI.into();
        EntityType::Line(line)
    }

    fn closed_lw(corners: &[[f64; 2]]) -> EntityType {
        let mut poly = lw(
            LAYER_MURI,
            &corners.iter().map(|&c| (c, 0.0)).collect::<Vec<_>>(),
        );
        if let EntityType::LwPolyline(poly) = &mut poly {
            poly.is_closed = true;
        }
        poly
    }

    /// Il caso A con un muro W nel locale A, facce y = 1,00 e y = 1,20 da
    /// x = 1,00 a x = 4,00, appoggiato a T al tramezzo. Continua: la faccia del
    /// tramezzo x = 4,00 è interrotta tra 1,00 e 1,20 e W è chiuso da una linea
    /// a x = 1,00; ChiusaPerMuro: W è una polilinea chiusa.
    fn case_a_with_w(form: Form) -> Vec<EntityType> {
        let mut entities = case_a(form, DrawingUnit::Metri);
        match form {
            Form::Continua => {
                // Il lato destro del locale A, (4;0)-(4;3).
                entities[5] = wall_line([4.0, 0.0], [4.0, 1.0]);
                entities.push(wall_line([4.0, 1.2], [4.0, 3.0]));
                entities.push(wall_line([1.0, 1.0], [4.0, 1.0]));
                entities.push(wall_line([1.0, 1.2], [4.0, 1.2]));
                entities.push(wall_line([1.0, 1.0], [1.0, 1.2]));
            }
            Form::ChiusaPerMuro => {
                entities.push(closed_lw(&[[1.0, 1.0], [4.0, 1.0], [4.0, 1.2], [1.0, 1.2]]));
            }
        }
        entities
    }

    /// R39: tolto il tramezzo, la testa di W si chiude sul lato lungo.
    #[test]
    fn wall_abutting_the_long_side_is_closed() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let scene = scene_with(case_a_with_w(form), 6);
            let pg = project_on(&scene, vec![axis_zone([4.05, 0.0], [4.05, 3.0], false)]);
            assert!(
                has_line(&pg, [4.0, 1.0], [4.0, 1.2]),
                "{form:?}: {:?}",
                pg.drawn
            );
            let head = pg
                .drawn
                .iter()
                .find(|e| is_line(e, [4.0, 1.0], [4.0, 1.2]))
                .expect("testa");
            assert_eq!(head.common().layer, LAYER_MURI);
            assert_eq!(head.common().handle, Handle::NULL);
            // Il resto del tramezzo è sparito.
            let left = pg.drawn.iter().chain(kept(&pg));
            assert!(!stretch_on(left, [4.0, 4.0], (0.0, 1.0)), "{form:?}");
            assert!(!stretch_on(
                pg.drawn.iter().chain(kept(&pg)),
                [4.0, 4.0],
                (1.2, 3.0)
            ));
            assert!(!stretch_on(
                pg.drawn.iter().chain(kept(&pg)),
                [4.1, 4.1],
                (0.0, 3.0)
            ));
            // La testa chiude anche i locali.
            assert!(pg
                .room_entities
                .iter()
                .any(|(_, e)| is_line(e, [4.0, 1.0], [4.0, 1.2])));
            match form {
                Form::Continua => {
                    assert!(pg.notices.is_empty(), "{:?}", pg.notices);
                    assert!(has_line(&pg, [4.0, 0.0], [4.1, 0.0]));
                    assert!(has_line(&pg, [4.0, 3.0], [4.1, 3.0]));
                }
                Form::ChiusaPerMuro => assert_notices(
                    &pg.notices,
                    &[giunzione([4.05, 0.0]), giunzione([4.05, 3.0])],
                ),
            }
        }
    }

    /// R39: una testa sola sul lato lungo (una faccia senza la sua compagna).
    #[test]
    fn unpaired_head_on_the_long_side_reports_taglio_non_chiuso() {
        let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
        entities.push(wall_line([1.0, 1.0], [4.0, 1.0]));
        let pg = project(entities, vec![axis_zone([4.05, 0.0], [4.05, 3.0], false)]);
        assert_notices(&pg.notices, &[Notice::TaglioNonChiuso { at: [4.0, 1.0] }]);
        assert!(!pg
            .drawn
            .iter()
            .filter_map(line_ends)
            .any(|(a, b)| (a[0] - 4.0).abs() < 1e-6 && (b[0] - 4.0).abs() < 1e-6));
    }

    /// R39: la testa passa dalla cancellazione a coppie come le altre
    /// chiusure: un tramezzo ricostruito sopra la copre, e la sua faccia resta
    /// aperta dove W si appoggia, come nel rilievo.
    #[test]
    fn head_closure_is_cancelled_by_a_rebuilt_wall() {
        let pg = project(
            case_a_with_w(Form::Continua),
            vec![
                axis_zone([4.05, 0.0], [4.05, 3.0], false),
                new_zone([4.05, 0.0], [4.05, 3.0], 0.10),
            ],
        );
        assert!(!has_line(&pg, [4.0, 1.0], [4.0, 1.2]), "{:?}", pg.drawn);
        assert!(has_line(&pg, [4.0, 0.0], [4.0, 1.0]), "{:?}", pg.drawn);
        assert!(has_line(&pg, [4.0, 1.2], [4.0, 3.0]), "{:?}", pg.drawn);
        assert!(has_line(&pg, [4.1, 0.0], [4.1, 3.0]), "{:?}", pg.drawn);
        assert_eq!(pg.drawn.len(), 3, "{:?}", pg.drawn);
        assert!(pg.notices.is_empty(), "{:?}", pg.notices);
    }

    /// R39: con due demolizioni di seguito i lati lunghi collineari si fondono
    /// e la testa di W, a cavallo del confine tra le zone, si chiude lo stesso.
    #[test]
    fn head_across_two_adjacent_demolitions_is_closed() {
        let pg = project(
            case_a_with_w(Form::Continua),
            vec![
                axis_zone([4.05, 0.0], [4.05, 1.1], false),
                axis_zone([4.05, 1.1], [4.05, 3.0], false),
            ],
        );
        assert!(has_line(&pg, [4.0, 1.0], [4.0, 1.2]), "{:?}", pg.drawn);
        assert!(pg.notices.is_empty(), "{:?}", pg.notices);
    }

    /// Rotazione di `deg` gradi attorno a (4,10; 1,5).
    fn rotate(p: [f64; 2], deg: f64) -> [f64; 2] {
        let (sin, cos) = deg.to_radians().sin_cos();
        let (x, y) = (p[0] - 4.10, p[1] - 1.5);
        [4.10 + x * cos - y * sin, 1.5 + x * sin + y * cos]
    }

    fn rotated(entities: Vec<EntityType>, deg: f64) -> Vec<EntityType> {
        entities
            .into_iter()
            .map(|mut entity| {
                match &mut entity {
                    EntityType::Line(line) => {
                        for p in [&mut line.start, &mut line.end] {
                            let q = rotate([p.x, p.y], deg);
                            (p.x, p.y) = (q[0], q[1]);
                        }
                    }
                    EntityType::LwPolyline(poly) => {
                        for v in &mut poly.vertices {
                            let q = rotate([v.location.x, v.location.y], deg);
                            (v.location.x, v.location.y) = (q[0], q[1]);
                        }
                    }
                    _ => {}
                }
                entity
            })
            .collect()
    }

    /// Il punto con x = 4,05 sulla retta y = `y` del caso A ruotato: un asse
    /// indicato a occhio, verticale, con gli estremi sulle facce dei muri sud e
    /// nord.
    fn vertical_click(y: f64, deg: f64) -> [f64; 2] {
        let (sin, cos) = deg.to_radians().sin_cos();
        let qx = 4.10 + (4.05 - 4.10 + (y - 1.5) * sin) / cos;
        [4.05, rotate([qx, y], deg)[1]]
    }

    /// Un tratto dritto sulla retta per `a` e `b`, dentro il segmento `ab`?
    fn stretch_along<'a>(
        entities: impl IntoIterator<Item = &'a EntityType>,
        a: [f64; 2],
        b: [f64; 2],
    ) -> bool {
        let length = (b[0] - a[0]).hypot(b[1] - a[1]);
        let u = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
        let off = |p: [f64; 2]| ((p[0] - a[0]) * u[1] - (p[1] - a[1]) * u[0]).abs();
        let along = |p: [f64; 2]| (p[0] - a[0]) * u[0] + (p[1] - a[1]) * u[1];
        entities.into_iter().flat_map(curves_of).any(|curve| {
            let Curve::Line(line) = curve else {
                return false;
            };
            let (s, e) = (along(line.start), along(line.end));
            off(line.start) <= 1e-6
                && off(line.end) <= 1e-6
                && s.max(e).min(length) - s.min(e).max(0.0) > 1e-6
        })
    }

    /// R40 (a): il caso A ruotato di 0,3° attorno a (4,10; 1,5) e un asse
    /// indicato verticale a x = 4,05: la zona segue le facce del tramezzo, la
    /// demolizione non lascia tratti di faccia e chiude (o segnala le
    /// giunzioni) come nel caso dritto.
    #[test]
    fn demolition_follows_faces_out_of_square_with_the_axis() {
        let deg = 0.3;
        let (p1, p2) = (vertical_click(0.0, deg), vertical_click(3.0, deg));
        assert!((p1[0] - p2[0]).abs() < 1e-15, "asse verticale");
        let r = |x: f64, y: f64| rotate([x, y], deg);
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let entities = rotated(case_a(form, DrawingUnit::Metri), deg);
            let zone = axis_zone_on(entities.clone(), p1, p2).expect("muro");
            let pg = project(entities, vec![zone]);
            let left = pg.drawn.iter().chain(kept(&pg));
            assert!(!stretch_along(left, r(4.0, 0.0), r(4.0, 3.0)), "{form:?}");
            let left = pg.drawn.iter().chain(kept(&pg));
            assert!(!stretch_along(left, r(4.1, 0.0), r(4.1, 3.0)), "{form:?}");
            match form {
                Form::Continua => {
                    assert!(pg.notices.is_empty(), "{:?}", pg.notices);
                    assert!(has_line(&pg, r(4.0, 0.0), r(4.1, 0.0)), "{:?}", pg.drawn);
                    assert!(has_line(&pg, r(4.0, 3.0), r(4.1, 3.0)), "{:?}", pg.drawn);
                    assert_eq!(pg.drawn.len(), 2, "{:?}", pg.drawn);
                }
                Form::ChiusaPerMuro => assert_notices(
                    &pg.notices,
                    &[giunzione(r(4.05, 0.0)), giunzione(r(4.05, 3.0))],
                ),
            }
        }
    }

    /// R40 (b) e R41: tramezzo a spessore variabile (faccia sinistra
    /// verticale, destra a 0,4°). Il rettangolo resta sull'asse verticale
    /// indicato, con la faccia destra misurata a metà dell'asse (y = 1,5,
    /// x = 4,10 + 1,5·tan): la faccia sinistra si toglie intera, della destra
    /// resta la metà alta (segnalata).
    #[test]
    fn variable_thickness_wall_reports_leftover_faces() {
        let tan = 0.4_f64.to_radians().tan();
        let top = [4.1 + 3.0 * tan, 3.0];
        let right = 4.1 + 1.5 * tan;
        let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
        // Il locale B: alto (8;3)-(4,10;3) e lato sinistro (4,10;3)-(4,10;0).
        entities[10] = wall_line([8.0, 3.0], top);
        entities[11] = wall_line(top, [4.1, 0.0]);
        let zone = axis_zone_on(entities.clone(), [4.05, 0.0], [4.05, 3.0]).expect("muro");
        let pg = project(entities, vec![zone]);
        assert_notices(
            &pg.notices,
            &[
                // Metà alta della faccia destra, da (right; 1,5) a `top`.
                Notice::FacciaNonTolta {
                    at: [4.1 + 2.25 * tan, 2.25],
                },
                // In basso il fondo del locale B corre sul lato di estremità
                // da x = 4,10 a `right`: giunzione nel mezzo, nessuna chiusura.
                giunzione([4.1 + 0.75 * tan, 0.0]),
                // In alto l'angolo (right; 3) non tocca nessun muro: il fondo
                // del locale B parte da `top`, 1 cm più in là.
                Notice::TaglioNonChiuso {
                    at: [(4.0 + right) / 2.0, 3.0],
                },
            ],
        );
        // La faccia sinistra non c'è più.
        assert!(!stretch_on(
            pg.drawn.iter().chain(kept(&pg)),
            [4.0, 4.0],
            (0.0, 3.0)
        ));
    }

    /// R42: un muro appoggiato con una sola faccia sul lato (l'altra si ferma
    /// 5 mm prima) sotto un secondo muro appoggiato: la sua testa e la prima
    /// del muro accanto distano 2 m, più dello spessore massimo, e non si
    /// chiudono tra loro. Tre teste, tre avvisi, nessuna linea.
    #[test]
    fn heads_of_two_different_walls_are_not_closed_together() {
        let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
        // W1, y da 0,40 a 0,60: solo la faccia a 0,40 arriva al tramezzo.
        entities.push(wall_line([1.0, 0.4], [4.0, 0.4]));
        entities.push(wall_line([1.0, 0.6], [3.995, 0.6]));
        entities.push(wall_line([1.0, 0.4], [1.0, 0.6]));
        // W2, y da 2,40 a 2,60, completo.
        entities.push(wall_line([1.0, 2.4], [4.0, 2.4]));
        entities.push(wall_line([1.0, 2.6], [4.0, 2.6]));
        entities.push(wall_line([1.0, 2.4], [1.0, 2.6]));
        let pg = project(entities, vec![axis_zone([4.05, 0.0], [4.05, 3.0], false)]);
        assert_notices(
            &pg.notices,
            &[
                Notice::TaglioNonChiuso { at: [4.0, 0.4] },
                Notice::TaglioNonChiuso { at: [4.0, 2.4] },
                Notice::TaglioNonChiuso { at: [4.0, 2.6] },
            ],
        );
        assert!(!pg
            .drawn
            .iter()
            .filter_map(line_ends)
            .any(|(a, b)| (a[0] - 4.0).abs() < 1e-6 && (b[0] - 4.0).abs() < 1e-6));
        // Le chiusure di estremità restano.
        assert!(has_line(&pg, [4.0, 0.0], [4.1, 0.0]));
        assert!(has_line(&pg, [4.0, 3.0], [4.1, 3.0]));
    }

    /// R42: due teste vicine ma su curve non parallele (una faccia e una
    /// linea obliqua) non sono le due facce di un muro.
    #[test]
    fn heads_on_curves_that_are_not_parallel_are_not_closed() {
        let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
        entities.push(wall_line([1.0, 1.0], [4.0, 1.0]));
        entities.push(wall_line([3.0, 2.0], [4.0, 1.5]));
        let pg = project(entities, vec![axis_zone([4.05, 0.0], [4.05, 3.0], false)]);
        assert_notices(
            &pg.notices,
            &[
                Notice::TaglioNonChiuso { at: [4.0, 1.0] },
                Notice::TaglioNonChiuso { at: [4.0, 1.5] },
            ],
        );
        assert!(!has_line(&pg, [4.0, 1.0], [4.0, 1.5]));
    }

    /// L'accoppiamento non dipende dal verso in cui si dà l'asse.
    #[test]
    fn heads_pair_the_same_whatever_the_axis_direction() {
        for (p1, p2) in [([4.05, 0.0], [4.05, 3.0]), ([4.05, 3.0], [4.05, 0.0])] {
            let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
            // W1 completo in basso, W2 con una faccia sola in alto.
            entities.push(wall_line([1.0, 0.4], [4.0, 0.4]));
            entities.push(wall_line([1.0, 0.6], [4.0, 0.6]));
            entities.push(wall_line([1.0, 2.4], [4.0, 2.4]));
            let zone =
                axis_zone_on(case_a(Form::Continua, DrawingUnit::Metri), p1, p2).expect("muro");
            let pg = project(entities, vec![zone]);
            assert!(
                has_line(&pg, [4.0, 0.4], [4.0, 0.6]),
                "{p1:?}: {:?}",
                pg.drawn
            );
            assert_notices(&pg.notices, &[Notice::TaglioNonChiuso { at: [4.0, 2.4] }]);
        }
    }

    /// R40 (b) non scatta nei casi dritti né su una faccia parallela lontana
    /// più della fascia.
    #[test]
    fn no_leftover_face_without_a_face_in_the_band() {
        let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
        // Una linea parallela a 3 cm dalla faccia x = 4,00, fuori dalla fascia
        // (tan 0,5° · 3 m = 2,6 cm).
        entities.push(wall_line([3.97, 0.5], [3.97, 2.5]));
        // E una a 2 cm: dentro la fascia, segnalata.
        entities.push(wall_line([4.12, 0.5], [4.12, 2.5]));
        let pg = project(entities, vec![axis_zone([4.05, 0.0], [4.05, 3.0], false)]);
        assert_notices(&pg.notices, &[Notice::FacciaNonTolta { at: [4.12, 1.5] }]);
    }
}
