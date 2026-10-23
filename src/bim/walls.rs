//! Geometria dei muri (spec §5.0), facce, locale attorno a un punto e
//! superficie netta (spec §4.2).
//!
//! Muri = solo `LINE`, `ARC`, `LWPOLYLINE` e `POLYLINE` 2D dello spazio
//! modello, su layer accesi e non congelati (i bloccati contano), esclusi i
//! layer `AL-*`: testi, quote, blocchi, campiture e immagini non chiudono
//! locali. Si lavora sempre sul piano XY del mondo; le entità muro fuori
//! piano si ignorano e si contano, mai in silenzio. `boundary_sources_on_plane`
//! di upstream (tutti i fili visibili) non si tocca.

use codec::entities::{BoundaryEdge, BoundaryPath, Hatch, HatchStyleType};
use codec::{CadDocument, EntityType, Handle};
use kernel::geom2d::{
    contains, distance_to, segment_crossing, signed_area, Curve, Line, SegmentCrossing, Tolerance,
    Transform,
};
use kernel::tessellation::DEFAULT_ANGLE;
use rustc_hash::FxHashMap;

use crate::command::WorkingPlane;
use crate::entities::hatch::{boundary_area, edge_curve};
use crate::scene::{
    boundary_faces, entity_boundary_source_on_plane, exact_hatch_paths, sources_near_rings,
    BoundarySource,
};

use super::LAYER_PREFIX;

/// Scarto relativo massimo tra l'area esatta (archi veri, da
/// `exact_hatch_paths`) e quella del poligono delle facce (archi in corde da
/// 7,5°: su un cerchio circa lo 0,3 % in meno). Oltre, la ricostruzione ha
/// preso una curva sbagliata: vale il poligono, dichiarato approssimato.
const MAX_EXACT_DRIFT: f64 = 0.05;

/// Quante tolleranze può distare un vertice delle facce (saldato entro la
/// tolleranza) dal percorso esatto, e viceversa.
const WELD_SLACK: f64 = 4.0;

/// I muri del disegno.
#[derive(Clone)]
pub struct WallSet {
    /// Le entità muro con il loro handle, in ordine di disegno, comprese
    /// quelle fuori piano.
    pub entities: Vec<(Handle, EntityType)>,
    /// La geometria sul piano XY delle entità muro, per handle.
    pub sources: FxHashMap<Handle, BoundarySource>,
    /// Entità muro ignorate perché fuori dal piano XY (o senza geometria).
    pub off_plane: usize,
}

/// L'entità conta come muro? `LINE`, `ARC`, `LWPOLYLINE` o `POLYLINE` 2D,
/// nello spazio modello, su un layer acceso e non congelato che non inizia con
/// `AL-`. Un layer bloccato conta; uno che manca nella tabella vale acceso.
pub fn is_wall_entity(doc: &CadDocument, entity: &EntityType) -> bool {
    model_space(doc).is_some_and(|(model, _)| is_wall_in(doc, model, entity))
}

/// Il blocco dello spazio modello e le sue entità, in ordine di disegno.
fn model_space(doc: &CadDocument) -> Option<(Handle, &[Handle])> {
    doc.block_records
        .iter()
        .find(|record| record.is_model_space())
        .map(|record| (record.handle, record.entity_handles.as_slice()))
}

fn is_wall_in(doc: &CadDocument, model: Handle, entity: &EntityType) -> bool {
    let common = entity.common();
    matches!(
        entity,
        EntityType::Line(_)
            | EntityType::Arc(_)
            | EntityType::LwPolyline(_)
            | EntityType::Polyline2D(_)
    ) && common.owner_handle == model
        && !is_bim_layer(&common.layer)
        && doc
            .layers
            .get(&common.layer)
            .map_or(true, |layer| layer.is_visible())
}

/// Layer `AL-*`; come tutti i nomi dei layer, senza distinguere le maiuscole.
fn is_bim_layer(name: &str) -> bool {
    name.get(..LAYER_PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(LAYER_PREFIX))
}

/// Le entità muro dello spazio modello e la loro geometria sul piano XY.
pub fn wall_set(doc: &CadDocument, tol: f64) -> WallSet {
    let entities: Vec<(Handle, EntityType)> = model_space(doc)
        .map(|(model, handles)| {
            handles
                .iter()
                .filter_map(|&handle| {
                    let entity = doc.get_entity(handle)?;
                    is_wall_in(doc, model, entity).then(|| (handle, entity.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    let (sources, off_plane) = sources_of(&entities, tol);
    WallSet {
        entities,
        sources,
        off_plane,
    }
}

/// La geometria delle entità sul piano XY del mondo, per handle, e quante ne
/// restano fuori: quelle fuori piano e le curve che non danno geometria (per
/// esempio una linea di lunghezza nulla).
pub fn sources_of(
    entities: &[(Handle, EntityType)],
    tol: f64,
) -> (FxHashMap<Handle, BoundarySource>, usize) {
    let plane = WorkingPlane::default();
    let mut sources = FxHashMap::default();
    let mut off_plane = 0;
    for (handle, entity) in entities {
        match entity_boundary_source_on_plane(entity, plane, tol, None) {
            Some(source) => {
                sources.insert(*handle, source);
            }
            None => off_plane += 1,
        }
    }
    (sources, off_plane)
}

/// Le facce piane chiuse dai muri, anelli antiorari. Le linee si spezzano agli
/// incroci e i tratti sovrapposti (linee doppie, lati comuni di due muri
/// chiusi) valgono una volta sola.
pub fn faces(sources: &FxHashMap<Handle, BoundarySource>, tol: f64) -> Vec<Vec<[f64; 2]>> {
    boundary_faces(sources, tol)
}

/// Un locale: il contorno esterno e i buchi (isole: pilastri, cavedi, arredi
/// chiusi lasciati accesi).
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub outer: Vec<[f64; 2]>,
    pub holes: Vec<Vec<[f64; 2]>>,
}

/// Il locale attorno a `p`: la faccia più piccola che lo contiene, con i suoi
/// figli diretti come buchi, cioè le facce dentro di essa che non contengono
/// `p` e senza nessuna faccia in mezzo (un'isola dentro un pilastro è del
/// pilastro). È la regola di `resolve_hatch_rings`
/// (`modules/draw/draw/hatch.rs`, privata), con l'inclusione di
/// [`face_inside`]. `None` se nessuna faccia contiene `p`.
pub fn region_at(faces: &[Vec<[f64; 2]>], p: [f64; 2], tol: f64) -> Option<Region> {
    let tolerance = Tolerance::new(tol);
    let boxes: Vec<_> = faces.iter().map(|face| bounds(face)).collect();
    let curves: Vec<Vec<Curve>> = faces.iter().map(|face| face_curves(face)).collect();
    let areas: Vec<f64> = faces.iter().map(|face| signed_area(face).abs()).collect();
    let holds_p = |index: usize| {
        box_within(&(p, p), &boxes[index], tol) && contains(&curves[index], p, tolerance)
    };
    let outer = (0..faces.len())
        .filter(|&index| holds_p(index))
        .min_by(|&a, &b| areas[a].total_cmp(&areas[b]))?;
    let inside = |outer: usize, inner: usize| {
        inner != outer
            && areas[inner] < areas[outer]
            && box_within(&boxes[inner], &boxes[outer], tol)
            && face_inside(&curves[outer], &faces[inner], tol)
    };
    let within: Vec<usize> = (0..faces.len())
        .filter(|&index| inside(outer, index))
        .collect();
    let holes = within
        .iter()
        .copied()
        .filter(|&hole| !holds_p(hole))
        .filter(|&hole| !within.iter().any(|&between| inside(between, hole)))
        .map(|hole| faces[hole].clone())
        .collect();
    Some(Region {
        outer: faces[outer].clone(),
        holes,
    })
}

fn face_curves(face: &[[f64; 2]]) -> Vec<Curve> {
    face.iter()
        .zip(face.iter().cycle().skip(1))
        .map(|(&start, &end)| Curve::Line(Line { start, end }))
        .collect()
}

type Bounds = ([f64; 2], [f64; 2]);

fn bounds(face: &[[f64; 2]]) -> Bounds {
    face.iter().fold(
        ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]),
        |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        },
    )
}

/// Il riquadro `inner` sta in `outer`, allargato di `tol`?
fn box_within(inner: &Bounds, outer: &Bounds, tol: f64) -> bool {
    (0..2).all(|axis| inner.0[axis] >= outer.0[axis] - tol && inner.1[axis] <= outer.1[axis] + tol)
}

/// La faccia `inner` sta dentro il contorno `outer`? Decide il primo vertice
/// di `inner` che non tocca `outer` (come `ring_strictly_contains` del kernel,
/// con la tolleranza del disegno). Per le facce di `boundary_faces` basta: due
/// facce delle stesse linee si toccano solo sul contorno, e un gruppo di linee
/// staccato (un pilastro) sta tutto dentro o tutto fuori. Contare tutti i
/// vertici, come `resolve_hatch_rings`, scambierebbe per buco una faccia
/// esterna con i vertici tutti sul contorno (una rientranza chiusa da una
/// diagonale) e la toglierebbe dall'area.
fn face_inside(outer: &[Curve], inner: &[[f64; 2]], tol: f64) -> bool {
    inner
        .iter()
        .find(|&&vertex| outer.iter().all(|edge| distance_to(edge, vertex) > tol))
        .is_some_and(|&vertex| contains(outer, vertex, Tolerance::new(tol)))
}

/// Superficie netta della regione, in unità del disegno al quadrato: contorno
/// meno buchi. Esatta (`true`) quando viene dai percorsi che
/// `exact_hatch_paths` ricostruisce sulle curve vere dei muri (archi compresi),
/// misurati con `boundary_area` come una campitura. Altrimenti vale la formula
/// del laccio sugli anelli (archi in corde) e il risultato è `false`.
///
/// Tutto si misura attorno a un'origine locale, il primo vertice del
/// contorno: `boundary_area` integra rispetto all'origine del disegno, e a
/// coordinate georiferite (Gauss-Boaga, milioni di metri) i termini da 10¹³ si
/// cancellerebbero lasciando millesimi di m² di errore.
///
/// Un percorso esatto vale solo se, per il suo anello:
/// - nessun lato è attribuibile a due curve diverse dei muri
///   ([`has_ambiguous_edge`]). `exact_hatch_paths` sceglie la curva di un lato
///   per sovrapposizione e poi per distanza degli estremi: su una «D» (un arco
///   e la sua corda nella stessa entità) o su una LINE e un ARC poco aperto
///   con gli stessi estremi sceglie a caso, e un arco poco aperto preso al
///   posto della corda resta dentro qualsiasi soglia geometrica;
/// - segue l'anello ([`path_follows_ring`]): ogni punto del percorso sta a
///   meno della freccia delle corde (più la tolleranza) dall'anello, e ogni
///   vertice dell'anello dal percorso;
/// - ha un'area finita, positiva e vicina a quella dell'anello
///   ([`MAX_EXACT_DRIFT`]): copre un verso sbagliato nell'integrale di
///   `boundary_area` su un contorno giusto.
pub fn net_area(
    region: &Region,
    sources: &FxHashMap<Handle, BoundarySource>,
    tol: f64,
) -> (f64, bool) {
    let origin = region.outer.first().copied().unwrap_or([0.0, 0.0]);
    let global: Vec<Vec<[f64; 2]>> = std::iter::once(region.outer.clone())
        .chain(region.holes.iter().cloned())
        .collect();
    let rings: Vec<Vec<[f64; 2]>> = global
        .iter()
        .map(|ring| ring.iter().map(|&p| local(p, origin)).collect())
        .collect();
    let polygon = signed_area(&rings[0]).abs()
        - rings[1..]
            .iter()
            .map(|hole| signed_area(hole).abs())
            .sum::<f64>();
    let near = sources_near_rings(&global, sources, WELD_SLACK * tol.max(1.0e-6));
    let Some(sources) = localized(&near, origin) else {
        return (polygon, false);
    };
    let exterior: Vec<bool> = (0..rings.len()).map(|index| index == 0).collect();
    let paths = exact_hatch_paths(&rings, &exterior, &sources, tol);
    let faithful = paths.len() == rings.len()
        && paths.iter().zip(&rings).all(|(path, ring)| {
            !has_ambiguous_edge(ring, &sources, tol)
                && path_follows_ring(path, ring, tol)
                && credible(
                    boundary_area(&hatch_of(vec![path.clone()])),
                    signed_area(ring).abs(),
                )
        });
    let exact = faithful.then(|| boundary_area(&hatch_of(paths)));
    pick_area(exact, polygon)
}

fn local(p: [f64; 2], origin: [f64; 2]) -> [f64; 2] {
    [p[0] - origin[0], p[1] - origin[1]]
}

/// Le sorgenti spostate nell'origine locale; `None` se una curva non si lascia
/// spostare (non succede con rette e archi).
fn localized(
    sources: &FxHashMap<Handle, BoundarySource>,
    origin: [f64; 2],
) -> Option<FxHashMap<Handle, BoundarySource>> {
    let shift = Transform::translation([-origin[0], -origin[1]]);
    sources
        .iter()
        .map(|(&handle, source)| {
            let segments = source
                .segments
                .iter()
                .map(|segment| Line {
                    start: local(segment.start, origin),
                    end: local(segment.end, origin),
                })
                .collect();
            let curves = source
                .curves
                .iter()
                .map(|curve| curve.transformed(&shift))
                .collect::<Option<Vec<_>>>()?;
            Some((handle, BoundarySource { segments, curves }))
        })
        .collect()
}

/// Un lato dell'anello si può attribuire a due curve diverse dei muri? Si
/// rifà la scelta di `edge_curve` (privata, `scene/boundary.rs`): vincono le
/// sorgenti con la sovrapposizione più lunga sul lato (alla pari entro `tol`:
/// l'ordine della tabella decide), e in ognuna le curve con la somma minima
/// delle distanze dagli estremi (alla pari entro [`WELD_SLACK`] tolleranze);
/// senza sovrapposizioni, le curve vicine a un estremo. Se tra le candidate ci
/// sono due percorsi diversi (una retta e un arco, o archi di cerchi diversi),
/// il lato è ambiguo.
fn has_ambiguous_edge(
    ring: &[[f64; 2]],
    sources: &FxHashMap<Handle, BoundarySource>,
    tol: f64,
) -> bool {
    let tolerance = Tolerance::new(tol);
    let reach = WELD_SLACK * tol;
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .any(|(&start, &end)| {
            let edge = Line { start, end };
            let length = (end[0] - start[0]).hypot(end[1] - start[1]);
            let overlaps: Vec<(f64, &BoundarySource)> = sources
                .values()
                .filter_map(|source| {
                    let overlap = source
                        .segments
                        .iter()
                        .filter_map(
                            |segment| match segment_crossing(edge, *segment, tolerance) {
                                SegmentCrossing::Overlap { a, .. } => {
                                    Some((a[1] - a[0]).abs() * length)
                                }
                                _ => None,
                            },
                        )
                        .fold(0.0, f64::max);
                    (overlap > 0.0).then_some((overlap, source))
                })
                .collect();
            let best = overlaps
                .iter()
                .map(|(overlap, _)| *overlap)
                .fold(0.0, f64::max);
            let pools: Vec<Vec<&Curve>> = if overlaps.is_empty() {
                vec![sources
                    .values()
                    .flat_map(|source| source.curves.iter())
                    .filter(|curve| distance_to(curve, start).min(distance_to(curve, end)) <= reach)
                    .collect()]
            } else {
                overlaps
                    .iter()
                    .filter(|(overlap, _)| *overlap >= best - tol)
                    .map(|(_, source)| source.curves.iter().collect())
                    .collect()
            };
            let error = |curve: &Curve| distance_to(curve, start) + distance_to(curve, end);
            let mut picks: Vec<&Curve> = Vec::new();
            for pool in pools {
                let least = pool
                    .iter()
                    .map(|curve| error(curve))
                    .fold(f64::INFINITY, f64::min);
                for curve in pool {
                    if error(curve) <= least + reach
                        && !picks.iter().any(|pick| same_course(pick, curve, reach))
                    {
                        picks.push(curve);
                    }
                }
            }
            picks.len() > 1
        })
}

/// Due curve candidate per lo stesso lato danno lo stesso percorso? Due rette
/// sì (passano per gli stessi estremi); due archi solo sullo stesso cerchio.
fn same_course(a: &Curve, b: &Curve, reach: f64) -> bool {
    match (a, b) {
        (Curve::Line(_), Curve::Line(_)) => true,
        (Curve::Arc(p), Curve::Arc(q)) => {
            (p.centre[0] - q.centre[0]).hypot(p.centre[1] - q.centre[1]) <= reach
                && (p.radius - q.radius).abs() <= reach
        }
        _ => a == b,
    }
}

/// Il percorso esatto `path` ricalca l'anello `ring` delle facce? Le facce
/// vedono ogni arco come corde da [`DEFAULT_ANGLE`] al più, quindi un punto
/// dell'arco vero dista dalla sua corda al più la freccia `r (1 − cos θ/2)`.
/// Si ammette il doppio della freccia del raggio maggiore del percorso (i
/// vertici agli incroci con le corde stanno sulle corde, non sull'arco) più
/// [`WELD_SLACK`] tolleranze, e si controlla nei due versi: i punti del
/// percorso (campionati come le facce) verso l'anello, i vertici dell'anello
/// verso le curve del percorso. Ellissi e spline non hanno una freccia nota:
/// il percorso non vale come esatto.
fn path_follows_ring(path: &BoundaryPath, ring: &[[f64; 2]], tol: f64) -> bool {
    let mut curves = Vec::with_capacity(path.edges.len());
    let mut radius: f64 = 0.0;
    for edge in &path.edges {
        match edge {
            BoundaryEdge::Line(_) => {}
            BoundaryEdge::CircularArc(arc) => radius = radius.max(arc.radius.abs()),
            _ => return false,
        }
        let Some(curve) = edge_curve(edge) else {
            return false;
        };
        curves.push(curve);
    }
    if curves.is_empty() || ring.len() < 3 {
        return false;
    }
    let allowed = WELD_SLACK * tol + 2.0 * radius * (1.0 - (DEFAULT_ANGLE / 2.0).cos());
    let ring_edges = face_curves(ring);
    let near_ring = |point: [f64; 2]| {
        ring_edges
            .iter()
            .any(|edge| distance_to(edge, point) <= allowed)
    };
    let near_path = |point: [f64; 2]| {
        curves
            .iter()
            .any(|curve| distance_to(curve, point) <= allowed)
    };
    curves.iter().all(|curve| {
        curve
            .tessellate_angle(DEFAULT_ANGLE)
            .into_iter()
            .all(&near_ring)
    }) && ring.iter().all(|&vertex| near_path(vertex))
}

/// Una campitura (stile normale: pari pieno, dispari vuoto) sui percorsi dati.
fn hatch_of(paths: Vec<BoundaryPath>) -> Hatch {
    let mut hatch = Hatch::new();
    hatch.style = HatchStyleType::Normal;
    hatch.paths = paths;
    hatch
}

/// `exact` è un'area credibile per un contorno che, ad archi in corde, misura
/// `polygon`?
fn credible(exact: f64, polygon: f64) -> bool {
    exact.is_finite() && exact > 0.0 && (exact - polygon).abs() <= MAX_EXACT_DRIFT * polygon.abs()
}

/// L'area esatta se c'è ed è credibile, altrimenti quella del poligono.
fn pick_area(exact: Option<f64>, polygon: f64) -> (f64, bool) {
    match exact {
        Some(area) if credible(area, polygon) => (area, true),
        _ => (polygon, false),
    }
}

/// Il rettangolo di un muro: la zona da demolire o da costruire, con i lati che
/// servono a tagliare e a chiudere la geometria di progetto.
///
/// `corners` gira in senso antiorario: `c0` e `c1` sono sul lato destro
/// dell'asse (rispetto al verso `p1 → p2`), `c2` e `c3` sul sinistro; `c0` e
/// `c3` sono all'estremo `p1`, `c1` e `c2` all'estremo `p2`.
#[derive(Clone, Debug, PartialEq)]
pub struct WallRect {
    pub corners: [[f64; 2]; 4],
    /// La mezzeria del muro, da un estremo all'altro dell'asse indicato.
    pub axis: ([f64; 2], [f64; 2]),
    /// Distanza tra le due facce, in unità del disegno.
    pub thickness: f64,
}

impl WallRect {
    /// L'anello `c0..c3`, antiorario (area con segno positiva).
    pub fn ring(&self) -> Vec<[f64; 2]> {
        self.corners.to_vec()
    }

    /// I lati lunghi, sulle facce: `(c0, c1)` a destra e `(c2, c3)` a sinistra.
    pub fn long_sides(&self) -> [([f64; 2], [f64; 2]); 2] {
        let [c0, c1, c2, c3] = self.corners;
        [(c0, c1), (c2, c3)]
    }

    /// I lati di estremità: `(c1, c2)` dalla parte di `p2` e `(c3, c0)` da
    /// quella di `p1`.
    pub fn end_sides(&self) -> [([f64; 2], [f64; 2]); 2] {
        let [c0, c1, c2, c3] = self.corners;
        [(c1, c2), (c3, c0)]
    }
}

/// Perché due punti sull'asse non danno un muro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisMiss {
    /// I due punti coincidono (entro la tolleranza): non c'è un asse.
    Degenerate,
    /// Nessuna faccia valida ai lati dell'asse, o nessuna coppia con uno
    /// spessore entro il limite.
    NoFaces,
    /// Una faccia sola: l'altra manca entro il limite di spessore.
    OneFace,
}

/// Quanto può scostarsi dall'asse una faccia per contare come parallela.
const FACE_PARALLEL_DEGREES: f64 = 0.5;

/// Il muro attorno all'asse `p1 → p2` (spec §5.1): le sue due facce si cercano
/// tra i segmenti retti delle sorgenti, una per lato dell'asse. `max_thickness`
/// e `tol` sono in unità del disegno.
///
/// Con `d = (p2 − p1) / |p2 − p1|` e `n = (−d.y, d.x)` (normale sinistra), un
/// segmento è candidato se è parallelo a `d` entro 0,5° e la sua proiezione su
/// `d` copre più di `tol` dell'intervallo `(0, |p2 − p1|)`; il suo scostamento
/// è `s = (inizio − p1) · n`, letto all'inizio del segmento. La faccia
/// sinistra è il candidato con `s > tol` più vicino a 0, la destra quello con
/// `s < −tol`. Una faccia vale solo con `|s| ≤ max_thickness`, e lo spessore
/// `s_l − s_r` deve rientrare nello stesso limite. Se manca un lato e c'è un
/// candidato con `|s| ≤ tol` (l'asse è stato dato sulla faccia stessa), quel
/// candidato fa da lato mancante.
///
/// Sono candidati solo i tratti dritti: le `LINE` e i lati dritti delle
/// polilinee (`Curve::Line` delle sorgenti). Gli `ARC` e i lati ad arco delle
/// polilinee (bulge) non sono facce, anche se una delle loro corde è parallela
/// all'asse.
///
/// Errori: [`AxisMiss::Degenerate`] se `p1 ≈ p2`; [`AxisMiss::OneFace`] se
/// resta una faccia sola (anche l'unica faccia su cui cade l'asse, senza
/// nient'altro entro il limite); [`AxisMiss::NoFaces`] se non ce n'è nessuna,
/// o se le due più vicine distano più del limite tra loro.
pub fn rect_from_axis(
    sources: &FxHashMap<Handle, BoundarySource>,
    p1: [f64; 2],
    p2: [f64; 2],
    max_thickness: f64,
    tol: f64,
) -> Result<WallRect, AxisMiss> {
    let Some((d, length)) = axis_direction(p1, p2).filter(|&(_, length)| length > tol) else {
        return Err(AxisMiss::Degenerate);
    };
    let n = [-d[1], d[0]];

    let mut left: Option<f64> = None;
    let mut right: Option<f64> = None;
    let mut on_face: Option<f64> = None;
    for s in face_offsets(sources, p1, d, n, length, tol) {
        // Negata, così scarta anche un limite non numerico.
        if !(s.abs() <= max_thickness) {
            continue;
        }
        if s > tol {
            left = Some(left.map_or(s, |nearest| nearest.min(s)));
        } else if s < -tol {
            right = Some(right.map_or(s, |nearest| nearest.max(s)));
        } else if on_face.map_or(true, |nearest| s.abs() < nearest.abs()) {
            on_face = Some(s);
        }
    }

    let (s_r, s_l) = match (right, left, on_face) {
        (Some(right), Some(left), _) => (right, left),
        (None, Some(left), Some(on_face)) => (on_face, left),
        (Some(right), None, Some(on_face)) => (right, on_face),
        (None, None, None) => return Err(AxisMiss::NoFaces),
        _ => return Err(AxisMiss::OneFace),
    };
    if s_l - s_r > max_thickness {
        return Err(AxisMiss::NoFaces);
    }
    Ok(wall_rect(p1, p2, n, s_r, s_l))
}

/// Il muro di spessore dato, centrato sull'asse `p1 → p2`. `None` se i punti
/// coincidono (o non sono numeri) o lo spessore non è positivo.
pub fn rect_from_axis_thickness(p1: [f64; 2], p2: [f64; 2], thickness: f64) -> Option<WallRect> {
    let (d, length) = axis_direction(p1, p2)?;
    if !(length > 0.0) || !(thickness > 0.0) || !thickness.is_finite() {
        return None;
    }
    let n = [-d[1], d[0]];
    Some(wall_rect(p1, p2, n, -thickness / 2.0, thickness / 2.0))
}

/// Il verso unitario di `p1 → p2` e la distanza; `None` se un valore non è
/// finito.
fn axis_direction(p1: [f64; 2], p2: [f64; 2]) -> Option<([f64; 2], f64)> {
    let delta = [p2[0] - p1[0], p2[1] - p1[1]];
    let length = delta[0].hypot(delta[1]);
    (p1.iter().chain(&p2).all(|v| v.is_finite()) && length.is_finite())
        .then(|| ([delta[0] / length, delta[1] / length], length))
}

/// Lo scostamento `s` dall'asse di ogni segmento dritto che lo accompagna:
/// parallelo entro [`FACE_PARALLEL_DEGREES`] e con una proiezione su `d` che
/// copre più di `tol` di `(0, length)`.
fn face_offsets(
    sources: &FxHashMap<Handle, BoundarySource>,
    p1: [f64; 2],
    d: [f64; 2],
    n: [f64; 2],
    length: f64,
    tol: f64,
) -> Vec<f64> {
    let sin_limit = FACE_PARALLEL_DEGREES.to_radians().sin();
    let dot = |a: [f64; 2], b: [f64; 2]| a[0] * b[0] + a[1] * b[1];
    let from_p1 = |p: [f64; 2]| [p[0] - p1[0], p[1] - p1[1]];
    sources
        .values()
        .flat_map(|source| source.curves.iter())
        .filter_map(|curve| {
            let Curve::Line(segment) = curve else {
                return None;
            };
            let v = [
                segment.end[0] - segment.start[0],
                segment.end[1] - segment.start[1],
            ];
            let extent = v[0].hypot(v[1]);
            // |v × d| / |v| è il seno dell'angolo, qualunque sia il verso.
            if !(extent > tol) || (v[0] * d[1] - v[1] * d[0]).abs() > sin_limit * extent {
                return None;
            }
            let t0 = dot(from_p1(segment.start), d);
            let t1 = dot(from_p1(segment.end), d);
            let covered = t0.max(t1).min(length) - t0.min(t1).max(0.0);
            (covered > tol).then(|| dot(from_p1(segment.start), n))
        })
        .collect()
}

/// Il rettangolo con le facce a `s_r` (destra) e `s_l` (sinistra) dall'asse
/// `p1 → p2`, `n` la normale sinistra.
fn wall_rect(p1: [f64; 2], p2: [f64; 2], n: [f64; 2], s_r: f64, s_l: f64) -> WallRect {
    let offset = |p: [f64; 2], s: f64| [p[0] + n[0] * s, p[1] + n[1] * s];
    let middle = (s_r + s_l) / 2.0;
    WallRect {
        corners: [
            offset(p1, s_r),
            offset(p2, s_r),
            offset(p2, s_l),
            offset(p1, s_l),
        ],
        axis: (offset(p1, middle), offset(p2, middle)),
        thickness: s_l - s_r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bim::fixtures::{
        case_a, disturbi, pilastro, scene_with, Form, LAYER_MURI, LAYER_PILASTRI,
    };
    use crate::bim::units::{area_m2, DrawingUnit};
    use codec::entities::{
        Arc, Circle, Insert, Line, LwPolyline, LwVertex, MText, Polyline2D, Text, Vertex2D,
    };
    use codec::tables::Layer;
    use codec::{Vector2, Vector3};

    const M: DrawingUnit = DrawingUnit::Metri;

    /// Aree in m² confrontate entro 1e-9 m².
    #[track_caller]
    fn assert_m2(got: f64, want: f64, what: impl std::fmt::Debug) {
        assert!(
            (got - want).abs() < 1e-9,
            "{what:?}: {got} m² invece di {want} m²"
        );
    }

    /// Regione e area in m² attorno a `p` (unità del disegno), con i muri di
    /// `doc`. L'area deve venire esatta.
    fn measure(doc: &CadDocument, p: [f64; 2]) -> (Region, f64) {
        let unit =
            DrawingUnit::from_insunits(doc.header.insertion_units).expect("unità supportata");
        let tol = unit.tolerance();
        let walls = wall_set(doc, tol);
        let faces = faces(&walls.sources, tol);
        let region = region_at(&faces, p, tol).expect("nessun locale attorno al punto");
        let (area, exact) = net_area(&region, &walls.sources, tol);
        assert!(exact, "area non esatta attorno a {p:?}");
        (region, area_m2(area, unit))
    }

    fn area_in(doc: &CadDocument, p: [f64; 2]) -> f64 {
        measure(doc, p).1
    }

    fn area_at(entities: Vec<EntityType>, insunits: i16, p: [f64; 2]) -> f64 {
        area_in(&scene_with(entities, insunits).document, p)
    }

    fn on(layer: &str, mut entity: EntityType) -> EntityType {
        entity.common_mut().layer = layer.to_string();
        entity
    }

    fn line(a: [f64; 3], b: [f64; 3]) -> EntityType {
        EntityType::Line(Line::from_points(
            Vector3::new(a[0], a[1], a[2]),
            Vector3::new(b[0], b[1], b[2]),
        ))
    }

    fn square(min: f64, max: f64) -> Vec<[f64; 2]> {
        vec![[min, min], [max, min], [max, max], [min, max]]
    }

    #[test]
    fn case_a_rooms_both_forms() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            assert_m2(area_at(case_a(form, M), 6, [2.0, 1.5]), 12.00, form);
            assert_m2(area_at(case_a(form, M), 6, [6.0, 1.5]), 11.70, form);
        }
    }

    #[test]
    fn case_c_centimetres_same_m2() {
        let cm = case_a(Form::Continua, DrawingUnit::Centimetri);
        assert_m2(area_at(cm, 5, [200.0, 150.0]), 12.00, "cm");
        // Anche in millimetri, e con l'altra forma.
        let mm = case_a(Form::ChiusaPerMuro, DrawingUnit::Millimetri);
        assert_m2(area_at(mm, 4, [6000.0, 1500.0]), 11.70, "mm");
    }

    #[test]
    fn case_d_island_and_layer_off() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let mut e = case_a(form, M);
            e.push(pilastro(M));
            assert_m2(area_at(e.clone(), 6, [2.0, 2.5]), 11.91, form);
            let (region, _) = measure(&scene_with(e.clone(), 6).document, [2.0, 2.5]);
            assert_eq!(region.holes.len(), 1, "il pilastro è l'unica isola");

            // Stesso disegno con il layer PILASTRI spento: 12,00.
            let mut off = scene_with(e.clone(), 6);
            off.document
                .layers
                .get_mut(LAYER_PILASTRI)
                .unwrap()
                .turn_off();
            assert_m2(area_in(&off.document, [2.0, 2.5]), 12.00, form);
            // Congelato: lo stesso.
            let mut frozen = scene_with(e, 6);
            frozen
                .document
                .layers
                .get_mut(LAYER_PILASTRI)
                .unwrap()
                .freeze();
            assert_m2(area_in(&frozen.document, [2.0, 2.5]), 12.00, form);
        }
    }

    #[test]
    fn case_f_disturbances_do_not_change_area() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let walls_only = case_a(form, M).len();
            let mut e = case_a(form, M);
            e.extend(disturbi(M));
            let scene = scene_with(e, 6);
            // Il punto sta anche dentro la polilinea su AL-LOCALI (6,00 m²): se
            // contasse come muro, il locale sarebbe quello.
            assert_m2(area_in(&scene.document, [2.0, 1.5]), 12.00, form);
            assert_m2(area_in(&scene.document, [6.0, 1.5]), 11.70, form);
            let walls = wall_set(&scene.document, M.tolerance());
            assert_eq!(walls.entities.len(), walls_only, "{form:?}");
            assert_eq!(walls.sources.len(), walls_only, "{form:?}");
            assert_eq!(walls.off_plane, 0, "{form:?}");
            assert!(walls
                .entities
                .iter()
                .all(|(_, e)| e.common().layer == LAYER_MURI));
        }
    }

    #[test]
    fn wall_filter_types_and_layers() {
        let mut doc = CadDocument::new();
        for (name, state) in [
            (LAYER_MURI, ""),
            ("GELO", "frozen"),
            ("SPENTO", "off"),
            ("BLOCCO", "locked"),
            ("AL-LOCALI", ""),
            ("al-nuove", ""),
            ("ALZATI", ""),
        ] {
            let mut layer = Layer::new(name);
            layer.handle = doc.allocate_handle();
            match state {
                "frozen" => layer.freeze(),
                "off" => layer.turn_off(),
                "locked" => layer.lock(),
                _ => {}
            }
            doc.layers.add(layer).unwrap();
        }
        let model = doc
            .block_records
            .iter()
            .find(|r| r.is_model_space())
            .unwrap()
            .handle;
        let placed = |layer: &str, entity: EntityType| {
            let mut entity = on(layer, entity);
            entity.common_mut().owner_handle = model;
            entity
        };
        let seg = || line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let wall = |entity: &EntityType| is_wall_entity(&doc, entity);

        // Tipi.
        assert!(wall(&placed(LAYER_MURI, seg())));
        let arc = Arc::from_center_radius_angles(Vector3::ZERO, 1.0, 0.0, 1.0);
        assert!(wall(&placed(LAYER_MURI, EntityType::Arc(arc))));
        let lw = LwPolyline::from_points(vec![Vector2::new(0.0, 0.0), Vector2::new(1.0, 0.0)]);
        assert!(wall(&placed(LAYER_MURI, EntityType::LwPolyline(lw))));
        let mut heavy = Polyline2D::new();
        heavy.add_vertex(Vertex2D::new(Vector3::new(0.0, 0.0, 0.0)));
        heavy.add_vertex(Vertex2D::new(Vector3::new(1.0, 0.0, 0.0)));
        assert!(wall(&placed(LAYER_MURI, EntityType::Polyline2D(heavy))));
        let text = Text::with_value("T", Vector3::ZERO);
        assert!(!wall(&placed(LAYER_MURI, EntityType::Text(text))));
        let mtext = MText::with_value("M", Vector3::ZERO);
        assert!(!wall(&placed(
            LAYER_MURI,
            EntityType::MText(Box::new(mtext))
        )));
        let hatch = crate::bim::fixtures::disturbi(M)
            .into_iter()
            .find(|e| matches!(e, EntityType::Hatch(_)))
            .unwrap();
        assert!(!wall(&placed(LAYER_MURI, hatch)));
        assert!(!wall(&placed(
            LAYER_MURI,
            EntityType::Circle(Circle::new())
        )));
        let insert = Insert::new("ARREDO", Vector3::ZERO);
        assert!(!wall(&placed(LAYER_MURI, EntityType::Insert(insert))));

        // Layer.
        assert!(!wall(&placed("AL-LOCALI", seg())));
        assert!(
            !wall(&placed("al-nuove", seg())),
            "i nomi dei layer ignorano le maiuscole"
        );
        assert!(
            wall(&placed("ALZATI", seg())),
            "conta il prefisso «AL-», non «AL»"
        );
        assert!(!wall(&placed("GELO", seg())), "layer congelato");
        assert!(!wall(&placed("SPENTO", seg())), "layer spento");
        assert!(wall(&placed("BLOCCO", seg())), "layer bloccato: conta");
        assert!(
            wall(&placed("NON_IN_TABELLA", seg())),
            "layer assente: acceso"
        );

        // Fuori dallo spazio modello.
        let mut paper = placed(LAYER_MURI, seg());
        paper.common_mut().owner_handle = doc.header.paper_space_block_handle;
        assert!(!doc.header.paper_space_block_handle.is_null());
        assert!(!wall(&paper));
        let mut in_block = placed(LAYER_MURI, seg());
        in_block.common_mut().owner_handle = Handle::new(0xABCDE);
        assert!(!wall(&in_block));
    }

    // Review Focus 1, 2, 4

    #[test]
    fn off_plane_entities_are_counted_not_used() {
        // Proiettata, la linea a z = 1 dividerebbe A in 4,00 + 8,00 m².
        let mut e = case_a(Form::Continua, M);
        e.push(on(LAYER_MURI, line([0.0, 1.0, 1.0], [4.0, 1.0, 1.0])));
        let scene = scene_with(e.clone(), 6);
        let walls = wall_set(&scene.document, M.tolerance());
        assert_eq!(walls.off_plane, 1);
        assert_eq!(walls.entities.len(), 13, "resta tra le entità muro");
        assert_eq!(walls.sources.len(), 12, "ma non dà geometria");
        assert_m2(area_in(&scene.document, [2.0, 1.5]), 12.00, "");

        // Una linea inclinata e una polilinea con quota: anche loro contate.
        e.push(on(LAYER_MURI, line([0.0, 2.0, 0.0], [4.0, 2.0, 0.5])));
        let mut raised =
            LwPolyline::from_points(vec![Vector2::new(0.0, 0.5), Vector2::new(4.0, 0.5)]);
        raised.elevation = 0.5;
        e.push(on(LAYER_MURI, EntityType::LwPolyline(raised)));
        let scene = scene_with(e, 6);
        assert_eq!(wall_set(&scene.document, M.tolerance()).off_plane, 3);
        assert_m2(area_in(&scene.document, [2.0, 1.5]), 12.00, "");
    }

    #[test]
    fn duplicate_lines_do_not_change_area() {
        let mut e = case_a(Form::Continua, M);
        // Copia identica, copia rovesciata e un pezzo sovrapposto della faccia
        // del tramezzo verso A.
        e.push(on(LAYER_MURI, line([4.0, 0.0, 0.0], [4.0, 3.0, 0.0])));
        e.push(on(LAYER_MURI, line([4.0, 3.0, 0.0], [4.0, 0.0, 0.0])));
        e.push(on(LAYER_MURI, line([4.0, 1.0, 0.0], [4.0, 2.5, 0.0])));
        assert_m2(area_at(e.clone(), 6, [2.0, 1.5]), 12.00, "");
        assert_m2(area_at(e, 6, [6.0, 1.5]), 11.70, "");

        // Forma chiusa per muro con il tramezzo disegnato due volte.
        let mut e = case_a(Form::ChiusaPerMuro, M);
        let tramezzo = e[4].clone();
        e.push(tramezzo);
        assert_m2(area_at(e.clone(), 6, [2.0, 1.5]), 12.00, "");
        assert_m2(area_at(e, 6, [6.0, 1.5]), 11.70, "");
    }

    #[test]
    fn locked_wall_layer_still_bounds() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let mut scene = scene_with(case_a(form, M), 6);
            scene.document.layers.get_mut(LAYER_MURI).unwrap().lock();
            assert_m2(area_in(&scene.document, [2.0, 1.5]), 12.00, form);
            assert_m2(area_in(&scene.document, [6.0, 1.5]), 11.70, form);
        }
    }

    #[test]
    fn region_holes_are_direct_children_only() {
        let faces = vec![square(4.0, 6.0), square(0.0, 10.0), square(2.0, 8.0)];
        let tol = 1e-6;
        let r = region_at(&faces, [1.0, 1.0], tol).unwrap();
        assert_eq!(r.outer, square(0.0, 10.0));
        assert_eq!(
            r.holes,
            vec![square(2.0, 8.0)],
            "il quadrato interno è un'isola dell'isola"
        );
        let r = region_at(&faces, [3.0, 3.0], tol).unwrap();
        assert_eq!(r.outer, square(2.0, 8.0));
        assert_eq!(r.holes, vec![square(4.0, 6.0)]);
        let r = region_at(&faces, [5.0, 5.0], tol).unwrap();
        assert_eq!(r.outer, square(4.0, 6.0));
        assert!(r.holes.is_empty());
        assert_eq!(region_at(&faces, [20.0, 20.0], tol), None);
        assert_eq!(region_at(&[], [0.0, 0.0], tol), None);
    }

    #[test]
    fn face_in_a_notch_is_not_a_hole() {
        // Un locale a L con una diagonale nella rientranza: il triangolo ha
        // tutti i vertici sul contorno della L ma sta fuori.
        let l_room = vec![
            [0.0, 0.0],
            [4.0, 0.0],
            [4.0, 2.0],
            [2.0, 2.0],
            [2.0, 4.0],
            [0.0, 4.0],
        ];
        let triangle = vec![[2.0, 2.0], [4.0, 2.0], [2.0, 4.0]];
        let faces = vec![l_room.clone(), triangle.clone()];
        let r = region_at(&faces, [1.0, 1.0], 1e-6).unwrap();
        assert_eq!(r.outer, l_room);
        assert!(r.holes.is_empty());
        let r = region_at(&faces, [2.5, 2.5], 1e-6).unwrap();
        assert_eq!(r.outer, triangle);
    }

    #[test]
    fn arc_walls_give_the_exact_area() {
        // Mezzo disco di raggio 1: π/2. Le facce lo vedono a corde.
        let half_disc = std::f64::consts::FRAC_PI_2;
        let chord = on(LAYER_MURI, line([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        let arc = Arc::from_center_radius_angles(Vector3::ZERO, 1.0, 0.0, std::f64::consts::PI);
        // Lo stesso arco specchiato (normale −Z), comune nei rilievi DWG.
        let mut mirrored = arc.clone();
        mirrored.normal = Vector3::new(0.0, 0.0, -1.0);
        // La corda chiusa dentro la polilinea, con un lato curvo: arco esatto.
        let mut bulged = LwPolyline::new();
        bulged.add_vertex(LwVertex::with_bulge(Vector2::new(1.0, 0.0), 1.0));
        bulged.add_vertex(LwVertex::new(Vector2::new(-1.0, 0.0)));
        bulged.add_vertex(LwVertex::new(Vector2::new(-1.0, -0.5)));
        bulged.add_vertex(LwVertex::new(Vector2::new(1.0, -0.5)));
        bulged.is_closed = true;
        let half_disc_and_strip = half_disc + 1.0;
        for (name, entities, want) in [
            (
                "LINE + ARC",
                vec![chord.clone(), on(LAYER_MURI, EntityType::Arc(arc))],
                half_disc,
            ),
            (
                "ARC specchiato",
                vec![chord, on(LAYER_MURI, EntityType::Arc(mirrored))],
                half_disc,
            ),
            (
                "LWPOLYLINE con bulge",
                vec![on(LAYER_MURI, EntityType::LwPolyline(bulged))],
                half_disc_and_strip,
            ),
        ] {
            let scene = scene_with(entities, 6);
            let tol = M.tolerance();
            let walls = wall_set(&scene.document, tol);
            assert_eq!(walls.off_plane, 0, "{name}");
            let faces = faces(&walls.sources, tol);
            let region = region_at(&faces, [0.0, 0.5], tol).expect(name);
            assert!(
                (signed_area(&region.outer).abs() - want).abs() > 1e-3,
                "{name}: le facce hanno le corde"
            );
            let (area, exact) = net_area(&region, &walls.sources, tol);
            assert!(exact, "{name}");
            assert_m2(area, want, name);
        }
    }

    /// Una «D» sopra la corda da `centre − (1; 0)` a `centre + (1; 0)`: una
    /// `LWPOLYLINE` chiusa con un arco (`bulge` 1 = mezzo disco di raggio 1) e
    /// la sua corda.
    fn d_shape(centre: [f64; 2], bulge: f64) -> EntityType {
        let mut d = LwPolyline::new();
        d.add_vertex(LwVertex::with_bulge(
            Vector2::new(centre[0] + 1.0, centre[1]),
            bulge,
        ));
        d.add_vertex(LwVertex::new(Vector2::new(centre[0] - 1.0, centre[1])));
        d.is_closed = true;
        on(LAYER_MURI, EntityType::LwPolyline(d))
    }

    /// Il segmento circolare tra la corda lunga 2 e l'arco della «D».
    fn d_segment(bulge: f64) -> f64 {
        let sweep = 4.0 * bulge.atan();
        let radius = 1.0 / (sweep / 2.0).sin();
        radius * radius * (sweep - sweep.sin()) / 2.0
    }

    #[test]
    fn d_shaped_polyline_falls_back_to_chords() {
        // Limite di upstream: `exact_hatch_paths` chiude la «D» come un cerchio
        // intero (π invece di π/2), perché la corda ha gli estremi anche
        // sull'arco. L'area resta quella a corde, dichiarata non esatta.
        let half_disc = std::f64::consts::FRAC_PI_2;
        let tol = M.tolerance();
        let scene = scene_with(vec![d_shape([0.0, 0.0], 1.0)], 6);
        let walls = wall_set(&scene.document, tol);
        let region = region_at(&faces(&walls.sources, tol), [0.0, 0.5], tol).unwrap();
        let (area, exact) = net_area(&region, &walls.sources, tol);
        assert!(!exact);
        assert_eq!(area, signed_area(&region.outer).abs());
        assert!((area - half_disc).abs() < 0.01 * half_disc, "{area}");

        // Come isola in un locale 6 × 6: sul totale lo scarto (π/2 su 34,4 m²)
        // resterebbe sotto il 5 %, i controlli per anello lo vedono.
        let corners = [[-3.0, -3.0], [3.0, -3.0], [3.0, 3.0], [-3.0, 3.0]];
        let mut entities: Vec<EntityType> = (0..4)
            .map(|i| {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                on(LAYER_MURI, line([a[0], a[1], 0.0], [b[0], b[1], 0.0]))
            })
            .collect();
        entities.push(d_shape([0.0, 0.0], 1.0));
        let scene = scene_with(entities, 6);
        let walls = wall_set(&scene.document, tol);
        let region = region_at(&faces(&walls.sources, tol), [2.0, 2.0], tol).unwrap();
        assert_eq!(region.holes.len(), 1);
        let (area, exact) = net_area(&region, &walls.sources, tol);
        assert!(!exact);
        let island = signed_area(&region.holes[0]).abs();
        assert!((area - (36.0 - island)).abs() < 1e-9, "{area}");
        assert!((area - (36.0 - half_disc)).abs() < 0.01, "{area}");
    }

    /// Locale 8 × 5 (40,00 m², tutti lati dritti) con la faccia del muro nord
    /// interrotta tra (3; 5) e (5; 5): lì il lato del locale lo danno `extra`.
    /// Sorgenti da `sources_of` con handle fissi (10… per il locale, 99… per
    /// `extra`), come le geometrie sintetiche dei task successivi: così
    /// l'ordine della tabella, che decide i pareggi di upstream, non cambia.
    fn room_with_gap(extra: Vec<EntityType>) -> (FxHashMap<Handle, BoundarySource>, Region) {
        let tol = M.tolerance();
        let room = [
            ([0.0, 0.0], [8.0, 0.0]),
            ([8.0, 0.0], [8.0, 5.0]),
            ([8.0, 5.0], [5.0, 5.0]),
            ([3.0, 5.0], [0.0, 5.0]),
            ([0.0, 5.0], [0.0, 0.0]),
        ]
        .iter()
        .map(|&(a, b)| line([a[0], a[1], 0.0], [b[0], b[1], 0.0]));
        let entities: Vec<(Handle, EntityType)> = room
            .enumerate()
            .map(|(i, e)| (Handle::new(10 + i as u64), e))
            .chain(
                extra
                    .into_iter()
                    .enumerate()
                    .map(|(i, e)| (Handle::new(99 + i as u64), e)),
            )
            .collect();
        let (sources, off_plane) = sources_of(&entities, tol);
        assert_eq!(off_plane, 0);
        let region = region_at(&faces(&sources, tol), [4.0, 2.5], tol).unwrap();
        assert!(region.holes.is_empty(), "ciò che sta oltre il lato è fuori");
        assert_m2(signed_area(&region.outer).abs(), 40.0, "contorno a corde");
        (sources, region)
    }

    #[test]
    fn d_niche_on_a_room_side_is_not_exact() {
        // Nicchia nella parete nord disegnata come «D» chiusa: la sua corda è
        // il lato del locale. Profonda (mezzo disco) o quasi piatta (bulge
        // 0,04 e 0,02, archi da 9,2° e 4,6°), la superficie vera è 40,00.
        let tol = M.tolerance();
        for bulge in [1.0, 0.04, 0.02] {
            let (sources, region) = room_with_gap(vec![d_shape([4.0, 5.0], bulge)]);

            // Premessa: la corda ha gli estremi sull'arco e sulla retta della
            // stessa polilinea, il lato è ambiguo; con questi handle upstream
            // sceglie l'arco (40 + segmento, sotto il 5 %), e da bulge 0,04 in
            // giù l'arco sta anche entro la freccia ammessa dal controllo
            // geometrico. Con altri handle (una scena) può scegliere la retta:
            // l'ordine di saldatura dei vertici decide il pareggio. Se upstream
            // corregge `edge_curve`, la premessa cade e il test va rivisto.
            assert!(has_ambiguous_edge(&region.outer, &sources, tol));
            let paths = exact_hatch_paths(&[region.outer.clone()], &[true], &sources, tol);
            let wrong = boundary_area(&hatch_of(paths.clone()));
            assert!(
                (wrong - (40.0 + d_segment(bulge))).abs() < 1e-9,
                "premessa {bulge}: {wrong}"
            );
            assert!(credible(wrong, 40.0), "premessa {bulge}: sotto il 5 %");
            assert_eq!(
                path_follows_ring(&paths[0], &region.outer, tol),
                bulge < 0.05,
                "premessa {bulge}: controllo geometrico"
            );

            let (area, exact) = net_area(&region, &sources, tol);
            assert!(
                !exact,
                "bulge {bulge}: area sbagliata dichiarata esatta: {area}"
            );
            assert_m2(area, 40.0, bulge);
        }
    }

    #[test]
    fn line_and_shallow_arc_with_the_same_ends_are_not_exact() {
        // Il lato (3; 5)–(5; 5) disegnato come LINE e anche come ARC verso
        // l'esterno di 6°: l'arco diventa una sola corda, uguale alla linea, e
        // upstream sceglie l'una o l'altro secondo l'ordine della tabella.
        let tol = M.tolerance();
        let half = 3.0_f64.to_radians();
        let radius = 1.0 / half.sin();
        let centre = Vector3::new(4.0, 5.0 - radius * half.cos(), 0.0);
        let right = std::f64::consts::FRAC_PI_2 - half;
        let arc = Arc::from_center_radius_angles(centre, radius, right, right + 2.0 * half);
        let (sources, region) = room_with_gap(vec![
            line([5.0, 5.0, 0.0], [3.0, 5.0, 0.0]),
            EntityType::Arc(arc),
        ]);
        assert!(has_ambiguous_edge(&region.outer, &sources, tol));
        let (area, exact) = net_area(&region, &sources, tol);
        assert!(!exact, "{area}");
        assert_m2(area, 40.0, "linea e arco");

        // Lo stesso arco da solo, senza la linea: nessun pareggio, esatto.
        // Il lato curvo verso l'esterno aggiunge il segmento all'area.
        let arc = Arc::from_center_radius_angles(centre, radius, right, right + 2.0 * half);
        let (sources, region) = room_with_gap(vec![EntityType::Arc(arc)]);
        assert!(!has_ambiguous_edge(&region.outer, &sources, tol));
        let (area, exact) = net_area(&region, &sources, tol);
        let segment = radius * radius * (2.0 * half - (2.0 * half).sin()) / 2.0;
        assert!(exact, "{area}");
        assert_m2(area, 40.0 + segment, "arco solo");
    }

    /// Le entità del caso A spostate di `by`.
    fn shifted(entities: Vec<EntityType>, by: [f64; 2]) -> Vec<EntityType> {
        entities
            .into_iter()
            .map(|mut entity| {
                match &mut entity {
                    EntityType::Line(l) => {
                        l.start.x += by[0];
                        l.start.y += by[1];
                        l.end.x += by[0];
                        l.end.y += by[1];
                    }
                    EntityType::LwPolyline(p) => {
                        for v in &mut p.vertices {
                            v.location.x += by[0];
                            v.location.y += by[1];
                        }
                    }
                    _ => unreachable!("il caso A ha solo LINE e LWPOLYLINE"),
                }
                entity
            })
            .collect()
    }

    #[test]
    fn georeferenced_case_a_keeps_exact_areas() {
        // Gauss-Boaga, Roma: E ≈ 2 300 000 m, N ≈ 4 640 000 m. Con l'integrale
        // attorno all'origine del disegno l'errore arriverebbe ai millesimi di
        // m²; attorno al primo vertice resta quello di rappresentazione delle
        // coordinate (4,10 → 2 300 004,1 m: meno di 1e-10 m), sotto 1e-9 m².
        let (east, north) = (2_300_000.0, 4_640_000.0);
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            for unit in [M, DrawingUnit::Millimetri] {
                let k = unit.per_metre();
                let by = [east * k, north * k];
                let entities = shifted(case_a(form, unit), by);
                let at = |x: f64, y: f64| [by[0] + x * k, by[1] + y * k];
                let code = unit.insunits();
                assert_m2(
                    area_at(entities.clone(), code, at(2.0, 1.5)),
                    12.00,
                    (form, unit),
                );
                assert_m2(area_at(entities, code, at(6.0, 1.5)), 11.70, (form, unit));
            }
        }
    }

    #[test]
    fn net_area_falls_back_to_the_polygon() {
        assert_eq!(pick_area(Some(12.0), 12.0), (12.0, true));
        // Archi veri contro corde: lo scarto dello 0,3 % è atteso.
        assert_eq!(pick_area(Some(1.5708), 1.5663), (1.5708, true));
        assert_eq!(pick_area(None, 12.0), (12.0, false));
        assert_eq!(pick_area(Some(f64::NAN), 12.0), (12.0, false));
        assert_eq!(pick_area(Some(f64::INFINITY), 12.0), (12.0, false));
        assert_eq!(pick_area(Some(0.0), 12.0), (12.0, false));
        // Ricostruzione sbagliata (per esempio l'arco preso dal lato lungo).
        assert_eq!(pick_area(Some(18.0), 12.0), (12.0, false));
        // Il poligono è la superficie del contorno meno i buchi.
        let region = Region {
            outer: square(0.0, 4.0),
            holes: vec![square(1.0, 1.5)],
        };
        let empty = FxHashMap::default();
        let (area, _) = net_area(&region, &empty, 1e-6);
        assert!((area - 15.75).abs() < 1e-12, "{area}");
    }

    // ---- Muro tra due punti (spec §5.1) ----

    /// Le sorgenti dei muri di `entities` (disegno in metri) e la tolleranza.
    fn sources_for(entities: Vec<EntityType>) -> (FxHashMap<Handle, BoundarySource>, f64) {
        let tol = 1.0e-6;
        let scene = scene_with(entities, 6);
        (wall_set(&scene.document, tol).sources, tol)
    }

    #[track_caller]
    fn assert_pt(got: [f64; 2], want: [f64; 2]) {
        assert!(
            (got[0] - want[0]).abs() < 1e-9 && (got[1] - want[1]).abs() < 1e-9,
            "{got:?} invece di {want:?}"
        );
    }

    #[track_caller]
    fn assert_corners(rect: &WallRect, want: [[f64; 2]; 4]) {
        for (got, want) in rect.corners.iter().zip(want) {
            assert_pt(*got, want);
        }
    }

    #[test]
    fn axis_on_tramezzo_finds_both_faces() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let (s, tol) = sources_for(case_a(form, M));
            let r = rect_from_axis(&s, [4.05, 0.0], [4.05, 3.0], 1.5, tol).unwrap();
            assert!((r.thickness - 0.10).abs() < 1e-9, "{form:?}");
            let xs: Vec<f64> = r.corners.iter().map(|c| c[0]).collect();
            assert!(
                xs.iter()
                    .all(|x| (x - 4.0).abs() < 1e-9 || (x - 4.1).abs() < 1e-9),
                "{form:?}: {xs:?}"
            );
            assert!((signed_area(&r.ring()) - 0.30).abs() < 1e-9, "{form:?}");
            // d = (0; 1), n = (-1; 0): la destra dell'asse è x = 4,10.
            assert_corners(&r, [[4.1, 0.0], [4.1, 3.0], [4.0, 3.0], [4.0, 0.0]]);
            assert_pt(r.axis.0, [4.05, 0.0]);
            assert_pt(r.axis.1, [4.05, 3.0]);
        }
    }

    #[test]
    fn axis_in_the_other_direction_keeps_the_ring_counter_clockwise() {
        let (s, tol) = sources_for(case_a(Form::Continua, M));
        let r = rect_from_axis(&s, [4.05, 3.0], [4.05, 0.0], 1.5, tol).unwrap();
        // d = (0; -1), n = (1; 0): la destra è x = 4,00.
        assert_corners(&r, [[4.0, 3.0], [4.0, 0.0], [4.1, 0.0], [4.1, 3.0]]);
        assert!((r.thickness - 0.10).abs() < 1e-9);
        assert!((signed_area(&r.ring()) - 0.30).abs() < 1e-9);
        assert_pt(r.axis.0, [4.05, 3.0]);
        assert_pt(r.axis.1, [4.05, 0.0]);
    }

    #[test]
    fn axis_off_centre_and_on_a_face() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let (s, tol) = sources_for(case_a(form, M));
            // (4,02; 0,5)–(4,02; 2,5): stesse facce, y da 0,5 a 2,5, area 0,20.
            let r = rect_from_axis(&s, [4.02, 0.5], [4.02, 2.5], 1.5, tol).unwrap();
            assert!((r.thickness - 0.10).abs() < 1e-9, "{form:?}");
            assert_corners(&r, [[4.1, 0.5], [4.1, 2.5], [4.0, 2.5], [4.0, 0.5]]);
            assert!((signed_area(&r.ring()) - 0.20).abs() < 1e-9, "{form:?}");
            // L'asse indicato è fuori centro: la mezzeria no.
            assert_pt(r.axis.0, [4.05, 0.5]);
            assert_pt(r.axis.1, [4.05, 2.5]);

            // (4,00; 0)–(4,00; 3,00), sulla faccia sinistra del tramezzo: la
            // faccia fa da lato mancante (l'altra, x = 0, è oltre il limite).
            let r = rect_from_axis(&s, [4.0, 0.0], [4.0, 3.0], 1.5, tol).unwrap();
            assert!((r.thickness - 0.10).abs() < 1e-9, "{form:?}");
            assert_corners(&r, [[4.1, 0.0], [4.1, 3.0], [4.0, 3.0], [4.0, 0.0]]);
            assert_pt(r.axis.0, [4.05, 0.0]);

            // Sull'altra faccia (x = 4,10), il lato mancante è il destro.
            let r = rect_from_axis(&s, [4.1, 0.0], [4.1, 3.0], 1.5, tol).unwrap();
            assert!((r.thickness - 0.10).abs() < 1e-9, "{form:?}");
            assert_corners(&r, [[4.1, 0.0], [4.1, 3.0], [4.0, 3.0], [4.0, 0.0]]);
        }
    }

    #[test]
    fn axis_without_faces() {
        let (s, tol) = sources_for(case_a(Form::Continua, M));
        // In mezzo al locale A: le facce più vicine (x = 0 e x = 4,00) distano
        // 2,00, oltre il limite.
        assert_eq!(
            rect_from_axis(&s, [2.0, 0.5], [2.0, 2.5], 1.5, tol),
            Err(AxisMiss::NoFaces)
        );
        // A 0,05 dalla faccia x = 0, dentro A, con limite 0,10: un solo lato.
        assert_eq!(
            rect_from_axis(&s, [0.05, 0.5], [0.05, 2.5], 0.10, tol),
            Err(AxisMiss::OneFace)
        );
        // Lo stesso a 0,05 dalla faccia x = 4,00, sul lato opposto.
        assert_eq!(
            rect_from_axis(&s, [3.95, 0.5], [3.95, 2.5], 0.10, tol),
            Err(AxisMiss::OneFace)
        );
        // Sulla faccia x = 0, nient'altro entro il limite: la faccia sola non
        // fa un muro.
        assert_eq!(
            rect_from_axis(&s, [0.0, 0.5], [0.0, 2.5], 0.10, tol),
            Err(AxisMiss::OneFace)
        );
        // Due punti coincidenti, o più vicini della tolleranza: nessun asse.
        assert_eq!(
            rect_from_axis(&s, [4.05, 1.0], [4.05, 1.0], 1.5, tol),
            Err(AxisMiss::Degenerate)
        );
        assert_eq!(
            rect_from_axis(&s, [4.05, 1.0], [4.05, 1.0 + 0.5 * tol], 1.5, tol),
            Err(AxisMiss::Degenerate)
        );
        // Niente muri.
        let (none, tol) = sources_for(Vec::new());
        assert_eq!(
            rect_from_axis(&none, [0.0, 0.0], [1.0, 0.0], 1.5, tol),
            Err(AxisMiss::NoFaces)
        );
    }

    #[test]
    fn axis_with_faces_thicker_than_the_limit_is_no_wall() {
        let (s, tol) = sources_for(case_a(Form::Continua, M));
        // Ognuna delle due facce sta entro 0,08 dall'asse (0,05), ma lo
        // spessore (0,10) no.
        assert_eq!(
            rect_from_axis(&s, [4.05, 0.0], [4.05, 3.0], 0.08, tol),
            Err(AxisMiss::NoFaces)
        );
        // Al limite esatto il muro c'è.
        assert!(rect_from_axis(&s, [4.05, 0.0], [4.05, 3.0], 0.10 + 1e-9, tol).is_ok());
    }

    #[test]
    fn axis_works_in_every_drawing_unit() {
        for unit in [DrawingUnit::Centimetri, DrawingUnit::Millimetri] {
            let k = unit.per_metre();
            let scene = scene_with(case_a(Form::Continua, unit), unit.insunits());
            let tol = unit.tolerance();
            let s = wall_set(&scene.document, tol).sources;
            let max = unit.metres(1.5);
            let r = rect_from_axis(&s, [4.05 * k, 0.0], [4.05 * k, 3.0 * k], max, tol).unwrap();
            assert!((r.thickness - 0.10 * k).abs() < 1e-9 * k, "{unit:?}");
            assert!(
                (signed_area(&r.ring()) - 0.30 * k * k).abs() < 1e-9 * k * k,
                "{unit:?}"
            );
        }
    }

    #[test]
    fn axis_of_a_rotated_wall() {
        // Due facce a 30°, a ±0,15 dall'asse del muro, lunghe oltre l'asse dato.
        let (sin, cos) = 30.0_f64.to_radians().sin_cos();
        let (d, n) = ([cos, sin], [-sin, cos]);
        let origin = [1.0, 1.0];
        let at = |t: f64, s: f64| {
            [
                origin[0] + d[0] * t + n[0] * s,
                origin[1] + d[1] * t + n[1] * s,
            ]
        };
        let face = |s: f64| {
            let (a, b) = (at(-1.0, s), at(5.0, s));
            line([a[0], a[1], 0.0], [b[0], b[1], 0.0])
        };
        let (sources, tol) = sources_for(vec![face(0.15), face(-0.15)]);
        // L'asse dato è spostato di 0,05 a sinistra della mezzeria.
        let r = rect_from_axis(&sources, at(1.0, 0.05), at(3.0, 0.05), 1.5, tol).unwrap();
        assert!((r.thickness - 0.30).abs() < 1e-9);
        assert_corners(
            &r,
            [at(1.0, -0.15), at(3.0, -0.15), at(3.0, 0.15), at(1.0, 0.15)],
        );
        assert_pt(r.axis.0, at(1.0, 0.0));
        assert_pt(r.axis.1, at(3.0, 0.0));
        assert!((signed_area(&r.ring()) - 0.30 * 2.0).abs() < 1e-9);
    }

    #[test]
    fn arcs_are_not_faces() {
        // Una faccia è una linea; l'altra un ARC di 6° (una sola corda, e la
        // corda è parallela all'asse): l'arco non è una faccia dritta.
        let half = 3.0_f64.to_radians();
        let radius = 6.0;
        let apex = 0.10; // dove l'arco tocca la sua tangente verticale
        let centre = Vector3::new(apex - radius, 1.5, 0.0);
        let arc = Arc::from_center_radius_angles(centre, radius, -half, half);
        let entities = vec![line([0.0, 0.0, 0.0], [0.0, 3.0, 0.0]), EntityType::Arc(arc)];
        let (sources, tol) = sources_for(entities);
        let chord_is_vertical = sources.values().any(|source| {
            source
                .segments
                .iter()
                .any(|seg| (seg.start[0] - seg.end[0]).abs() < 1e-12 && seg.start[0] > 0.05)
        });
        assert!(chord_is_vertical, "la prova richiede una corda verticale");
        // Asse tra la linea (x = 0) e la corda (x ≈ 0,092).
        assert_eq!(
            rect_from_axis(&sources, [0.04, 1.2], [0.04, 1.8], 1.5, tol),
            Err(AxisMiss::OneFace)
        );
    }

    #[test]
    fn faces_must_be_parallel_within_half_a_degree() {
        let tilted = |degrees: f64| {
            let dx = 10.0 * degrees.to_radians().tan();
            vec![
                line([0.0, 0.0, 0.0], [0.0, 10.0, 0.0]),
                line([0.2, 0.0, 0.0], [0.2 + dx, 10.0, 0.0]),
            ]
        };
        // 0,4°: è una faccia (lo scostamento si legge all'inizio del segmento).
        let (s, tol) = sources_for(tilted(0.4));
        let r = rect_from_axis(&s, [0.1, 0.0], [0.1, 3.0], 1.5, tol).unwrap();
        assert!((r.thickness - 0.20).abs() < 1e-9);
        // Anche l'altro verso di disegno.
        let (s, tol) = sources_for(tilted(-0.4));
        assert!(rect_from_axis(&s, [0.1, 0.0], [0.1, 3.0], 1.5, tol).is_ok());
        // 0,6°: no.
        let (s, tol) = sources_for(tilted(0.6));
        assert_eq!(
            rect_from_axis(&s, [0.1, 0.0], [0.1, 3.0], 1.5, tol),
            Err(AxisMiss::OneFace)
        );
        // Perpendicolare: no, ovviamente.
        let (s, tol) = sources_for(vec![
            line([0.0, 0.0, 0.0], [0.0, 10.0, 0.0]),
            line([-1.0, 0.2, 0.0], [1.0, 0.2, 0.0]),
        ]);
        assert_eq!(
            rect_from_axis(&s, [0.1, 0.0], [0.1, 3.0], 1.5, tol),
            Err(AxisMiss::OneFace)
        );
    }

    #[test]
    fn faces_must_cover_the_axis_by_more_than_the_tolerance() {
        let segment = |from: f64, to: f64| line([0.2, from, 0.0], [0.2, to, 0.0]);
        let wall = |other: EntityType| {
            let (s, tol) = sources_for(vec![line([0.0, 0.0, 0.0], [0.0, 10.0, 0.0]), other]);
            rect_from_axis(&s, [0.1, 0.0], [0.1, 3.0], 1.5, tol)
        };
        // Un tratto che inizia dove finisce l'asse non lo copre.
        assert_eq!(wall(segment(3.0, 5.0)), Err(AxisMiss::OneFace));
        // Uno che lo copre di 1 mm sì.
        assert!(wall(segment(2.999, 5.0)).is_ok());
        // Uno più corto dell'asse, tutto dentro: sì.
        assert!(wall(segment(1.0, 2.0)).is_ok());
        // Uno che finisce prima dell'inizio dell'asse: no.
        assert_eq!(wall(segment(-5.0, 0.0)), Err(AxisMiss::OneFace));
    }

    #[test]
    fn a_wall_from_an_axis_and_a_thickness() {
        let r = rect_from_axis_thickness([0.0, 0.0], [2.0, 0.0], 0.3).unwrap();
        assert_corners(&r, [[0.0, -0.15], [2.0, -0.15], [2.0, 0.15], [0.0, 0.15]]);
        assert!((r.thickness - 0.3).abs() < 1e-12);
        assert_eq!(r.axis, ([0.0, 0.0], [2.0, 0.0]));
        assert!((signed_area(&r.ring()) - 0.6).abs() < 1e-12);

        // In diagonale e nell'altro verso: sempre antioraria, centrata.
        let r = rect_from_axis_thickness([3.0, 3.0], [1.0, 1.0], 0.2).unwrap();
        assert!((r.thickness - 0.2).abs() < 1e-12);
        assert!(signed_area(&r.ring()) > 0.0);
        assert_pt(r.axis.0, [3.0, 3.0]);
        assert_pt(r.axis.1, [1.0, 1.0]);
        let h = 0.1 / 2.0_f64.sqrt();
        assert_corners(
            &r,
            [
                [3.0 - h, 3.0 + h],
                [1.0 - h, 1.0 + h],
                [1.0 + h, 1.0 - h],
                [3.0 + h, 3.0 - h],
            ],
        );

        assert_eq!(rect_from_axis_thickness([1.0, 1.0], [1.0, 1.0], 0.3), None);
        assert_eq!(rect_from_axis_thickness([0.0, 0.0], [2.0, 0.0], 0.0), None);
        assert_eq!(rect_from_axis_thickness([0.0, 0.0], [2.0, 0.0], -0.1), None);
        assert_eq!(
            rect_from_axis_thickness([0.0, 0.0], [2.0, 0.0], f64::NAN),
            None
        );
        assert_eq!(
            rect_from_axis_thickness([0.0, f64::NAN], [2.0, 0.0], 0.3),
            None
        );
    }

    #[test]
    fn long_and_end_sides_follow_the_corner_order() {
        let r = WallRect {
            corners: [[0.0, -1.0], [4.0, -1.0], [4.0, 1.0], [0.0, 1.0]],
            axis: ([0.0, 0.0], [4.0, 0.0]),
            thickness: 2.0,
        };
        assert_eq!(r.ring(), r.corners.to_vec());
        assert_eq!(
            r.long_sides(),
            [([0.0, -1.0], [4.0, -1.0]), ([4.0, 1.0], [0.0, 1.0])]
        );
        assert_eq!(
            r.end_sides(),
            [([4.0, -1.0], [4.0, 1.0]), ([0.0, 1.0], [0.0, -1.0])]
        );
    }
}
