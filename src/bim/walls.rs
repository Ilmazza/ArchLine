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
use kernel::geom2d::{contains, distance_to, signed_area, Curve, Line, Tolerance};
use kernel::tessellation::DEFAULT_ANGLE;
use rustc_hash::FxHashMap;

use crate::command::WorkingPlane;
use crate::entities::hatch::{boundary_area, edge_curve};
use crate::scene::{
    boundary_faces, entity_boundary_source_on_plane, exact_hatch_paths, BoundarySource,
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
/// Un percorso esatto vale solo se, per il suo anello:
/// - segue l'anello ([`path_follows_ring`]): ogni punto del percorso sta a
///   meno della freccia delle corde (più la tolleranza) dall'anello, e ogni
///   vertice dell'anello dal percorso. È il controllo che conta:
///   `exact_hatch_paths` chiude una polilinea a «D» (un arco e la sua corda
///   nella stessa entità) come un cerchio intero, e quando la corda è un lato
///   di un locale grande (una nicchia) l'arco in più vale pochi punti
///   percentuali dell'area, ma sta a un raggio dalla corda;
/// - ha un'area finita, positiva e vicina a quella dell'anello
///   ([`MAX_EXACT_DRIFT`]). Costa poco e copre ciò che la geometria non vede:
///   un verso sbagliato nell'integrale di `boundary_area` su un contorno giusto.
pub fn net_area(
    region: &Region,
    sources: &FxHashMap<Handle, BoundarySource>,
    tol: f64,
) -> (f64, bool) {
    let rings: Vec<Vec<[f64; 2]>> = std::iter::once(region.outer.clone())
        .chain(region.holes.iter().cloned())
        .collect();
    let exterior: Vec<bool> = (0..rings.len()).map(|index| index == 0).collect();
    let paths = exact_hatch_paths(&rings, &exterior, sources, tol);
    let faithful = paths.len() == rings.len()
        && paths.iter().zip(&rings).all(|(path, ring)| {
            path_follows_ring(path, ring, tol)
                && credible(
                    boundary_area(&hatch_of(vec![path.clone()])),
                    signed_area(ring).abs(),
                )
        });
    let exact = faithful.then(|| boundary_area(&hatch_of(paths)));
    let polygon = signed_area(&region.outer).abs()
        - region
            .holes
            .iter()
            .map(|hole| signed_area(hole).abs())
            .sum::<f64>();
    pick_area(exact, polygon)
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

    /// Mezzo disco di raggio 1 sopra `centre`: una `LWPOLYLINE` chiusa con un
    /// arco e la sua corda.
    fn d_shape(centre: [f64; 2]) -> EntityType {
        let mut d = LwPolyline::new();
        d.add_vertex(LwVertex::with_bulge(
            Vector2::new(centre[0] + 1.0, centre[1]),
            1.0,
        ));
        d.add_vertex(LwVertex::new(Vector2::new(centre[0] - 1.0, centre[1])));
        d.is_closed = true;
        on(LAYER_MURI, EntityType::LwPolyline(d))
    }

    #[test]
    fn d_shaped_polyline_falls_back_to_chords() {
        // Limite di upstream: `exact_hatch_paths` chiude la «D» come un cerchio
        // intero (π invece di π/2), perché la corda ha gli estremi anche
        // sull'arco. L'area resta quella a corde, dichiarata non esatta.
        let half_disc = std::f64::consts::FRAC_PI_2;
        let tol = M.tolerance();
        let scene = scene_with(vec![d_shape([0.0, 0.0])], 6);
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
        entities.push(d_shape([0.0, 0.0]));
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

    #[test]
    fn d_niche_on_a_room_side_is_not_exact() {
        // Locale 8 × 5 con una nicchia semicircolare (r = 1) nella parete nord,
        // disegnata come «D» chiusa: la sua corda è il lato del locale tra
        // (3; 5) e (5; 5), la faccia del muro si interrompe lì. La superficie
        // vera è 40,00 (tutti lati dritti).
        let half_disc = std::f64::consts::FRAC_PI_2;
        let tol = M.tolerance();
        let mut entities: Vec<EntityType> = [
            ([0.0, 0.0], [8.0, 0.0]),
            ([8.0, 0.0], [8.0, 5.0]),
            ([8.0, 5.0], [5.0, 5.0]),
            ([3.0, 5.0], [0.0, 5.0]),
            ([0.0, 5.0], [0.0, 0.0]),
        ]
        .iter()
        .map(|&(a, b)| on(LAYER_MURI, line([a[0], a[1], 0.0], [b[0], b[1], 0.0])))
        .collect();
        entities.push(d_shape([4.0, 5.0]));
        let scene = scene_with(entities, 6);
        let walls = wall_set(&scene.document, tol);
        let region = region_at(&faces(&walls.sources, tol), [4.0, 2.5], tol).unwrap();
        assert!(region.holes.is_empty(), "la nicchia è fuori dal locale");

        // Premessa: upstream ricostruisce la corda come il mezzo cerchio della
        // nicchia (40 + π/2) e lo scarto, 3,9 %, passerebbe il controllo
        // sull'area. Se upstream corregge `edge_curve`, la premessa cade e il
        // test va rivisto: allora 40,00 può tornare esatto.
        let wrong = boundary_area(&hatch_of(exact_hatch_paths(
            &[region.outer.clone()],
            &[true],
            &walls.sources,
            tol,
        )));
        assert!(
            (wrong - (40.0 + half_disc)).abs() < 1e-9,
            "premessa: {wrong}"
        );
        assert!(credible(wrong, 40.0), "premessa: sotto il 5 %");

        let (area, exact) = net_area(&region, &walls.sources, tol);
        assert!(!exact, "nessuna area sbagliata dichiarata esatta: {area}");
        assert_m2(area, 40.0, "nicchia");
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
}
