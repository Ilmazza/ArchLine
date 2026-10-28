//! Disegni di prova sintetici (il repo è pubblico: mai disegni dei clienti).
//!
//! Caso A della spec §7: due locali affiancati con muri a due linee, A
//! 4,00 × 3,00 m (12,00 m²) e B 3,90 × 3,00 m (11,70 m²), separati da un
//! tramezzo da 0,10 m. Due forme del rilievo: linee continue sulle facce dei
//! muri, oppure una polilinea chiusa per muro. Coordinate in metri,
//! moltiplicate per `unit.per_metre()`.

use codec::entities::hatch::BoundaryPathFlags;
use codec::entities::{
    BoundaryEdge, BoundaryPath, Hatch, Line, LwPolyline, MText, PolylineEdge, Text,
};
use codec::tables::Layer;
use codec::{Document, EntityType, Handle, Vector2, Vector3};

use super::units::DrawingUnit;
use super::LAYER_LOCALI;

/// Layer dei muri del rilievo.
pub(crate) const LAYER_MURI: &str = "MURI";
/// Layer del pilastro del caso D.
pub(crate) const LAYER_PILASTRI: &str = "PILASTRI";
/// Layer degli arredi di disturbo.
pub(crate) const LAYER_ARREDI: &str = "ARREDI";
/// Layer dei testi di disturbo.
pub(crate) const LAYER_TESTI: &str = "TESTI";

/// Le due forme in cui arrivano i muri di un rilievo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    /// Linee sulle facce dei muri.
    Continua,
    /// Una polilinea chiusa per muro: i lati dei muri vicini si sovrappongono.
    ChiusaPerMuro,
}

fn at(unit: DrawingUnit, p: [f64; 2]) -> [f64; 2] {
    [unit.metres(p[0]), unit.metres(p[1])]
}

fn line(unit: DrawingUnit, layer: &str, a: [f64; 2], b: [f64; 2]) -> EntityType {
    let (a, b) = (at(unit, a), at(unit, b));
    let mut line = Line::from_points(Vector3::new(a[0], a[1], 0.0), Vector3::new(b[0], b[1], 0.0));
    line.common.layer = layer.to_string();
    EntityType::Line(line)
}

/// Il contorno chiuso `corners` come `LINE` separate.
fn lines(unit: DrawingUnit, layer: &str, corners: &[[f64; 2]]) -> Vec<EntityType> {
    corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .map(|(&a, &b)| line(unit, layer, a, b))
        .collect()
}

/// Il contorno chiuso `corners` come una `LWPOLYLINE` chiusa.
fn closed_polyline(unit: DrawingUnit, layer: &str, corners: &[[f64; 2]]) -> EntityType {
    let points = corners
        .iter()
        .map(|&c| {
            let c = at(unit, c);
            Vector2::new(c[0], c[1])
        })
        .collect();
    let mut polyline = LwPolyline::from_points(points);
    polyline.is_closed = true;
    polyline.common.layer = layer.to_string();
    EntityType::LwPolyline(polyline)
}

/// Il caso A, muri sul layer `MURI`.
pub(crate) fn case_a(form: Form, unit: DrawingUnit) -> Vec<EntityType> {
    match form {
        Form::Continua => [
            // Contorno esterno.
            [[-0.30, -0.30], [8.30, -0.30], [8.30, 3.30], [-0.30, 3.30]],
            // Locale A.
            [[0.0, 0.0], [4.00, 0.0], [4.00, 3.00], [0.0, 3.00]],
            // Locale B.
            [[4.10, 0.0], [8.00, 0.0], [8.00, 3.00], [4.10, 3.00]],
        ]
        .iter()
        .flat_map(|corners| lines(unit, LAYER_MURI, corners))
        .collect(),
        Form::ChiusaPerMuro => [
            // Sud.
            [[-0.30, -0.30], [8.30, -0.30], [8.30, 0.0], [-0.30, 0.0]],
            // Nord.
            [[-0.30, 3.00], [8.30, 3.00], [8.30, 3.30], [-0.30, 3.30]],
            // Ovest.
            [[-0.30, 0.0], [0.0, 0.0], [0.0, 3.00], [-0.30, 3.00]],
            // Est.
            [[8.00, 0.0], [8.30, 0.0], [8.30, 3.00], [8.00, 3.00]],
            // Tramezzo.
            [[4.00, 0.0], [4.10, 0.0], [4.10, 3.00], [4.00, 3.00]],
        ]
        .iter()
        .map(|corners| closed_polyline(unit, LAYER_MURI, corners))
        .collect(),
    }
}

/// Il pilastro del caso D: 0,30 × 0,30 m dentro il locale A, layer `PILASTRI`.
pub(crate) fn pilastro(unit: DrawingUnit) -> EntityType {
    closed_polyline(
        unit,
        LAYER_PILASTRI,
        &[[1.00, 1.00], [1.30, 1.00], [1.30, 1.30], [1.00, 1.30]],
    )
}

/// Il disturbo del caso F, tutto dentro il locale A: un `TEXT` e un `MTEXT`,
/// una campitura solida su `ARREDI` e una polilinea chiusa
/// [0,5 – 3,5] × [0,5 – 2,5] su `AL-LOCALI` (come il contorno di un locale già
/// scritto).
pub(crate) fn disturbi(unit: DrawingUnit) -> Vec<EntityType> {
    let point = |p: [f64; 2]| {
        let p = at(unit, p);
        Vector3::new(p[0], p[1], 0.0)
    };
    let height = unit.metres(0.25);

    let mut text = Text::with_value("Soggiorno", point([1.0, 2.0])).with_height(height);
    text.common.layer = LAYER_TESTI.to_string();

    let mut mtext = MText::with_value("A\\PS = 12,00 m²", point([2.0, 1.0])).with_height(height);
    mtext.common.layer = LAYER_TESTI.to_string();

    let rug: Vec<Vector2> = [[2.60, 1.80], [3.60, 1.80], [3.60, 2.60], [2.60, 2.60]]
        .iter()
        .map(|&c| {
            let c = at(unit, c);
            Vector2::new(c[0], c[1])
        })
        .collect();
    let mut path = BoundaryPath::with_flags(BoundaryPathFlags::from_bits(
        BoundaryPathFlags::OUTERMOST.bits() | BoundaryPathFlags::EXTERNAL.bits(),
    ));
    path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(rug, true)));
    let mut hatch = Hatch::new();
    hatch.paths = vec![path];
    hatch.common.layer = LAYER_ARREDI.to_string();

    vec![
        EntityType::Text(text),
        EntityType::MText(Box::new(mtext)),
        EntityType::Hatch(Box::new(hatch)),
        closed_polyline(
            unit,
            LAYER_LOCALI,
            &[[0.5, 0.5], [3.5, 0.5], [3.5, 2.5], [0.5, 2.5]],
        ),
    ]
}

/// Il tramezzo del caso A spostato di `dx` metri verso est, con le facce dei
/// locali che lo seguono, come una correzione del rilievo fatta a mano: ogni
/// vertice delle entità su `MURI` a x = 4,00 o 4,10 m (le due facce del
/// tramezzo e i capi delle facce sud e nord di A e B a linee). Con `dx` =
/// 0,20 il locale A passa da 12,00 a 12,60 m² e B da 11,70 a 11,10 m², nelle
/// due forme. I locali già scritti (su altri layer) non si toccano.
pub(crate) fn shift_tramezzo(doc: &mut Document, unit: DrawingUnit, dx: f64) {
    let faces = [unit.metres(4.00), unit.metres(4.10)];
    let on_face = |x: f64| {
        faces
            .iter()
            .any(|face| (x - face).abs() < 1e-9 * unit.per_metre())
    };
    let dx = unit.metres(dx);
    let handles: Vec<Handle> = doc
        .entities()
        .filter(|entity| entity.common().layer == LAYER_MURI)
        .map(|entity| entity.common().handle)
        .collect();
    for handle in handles {
        match doc.get_entity_mut(handle) {
            Some(EntityType::Line(line)) => {
                for p in [&mut line.start, &mut line.end] {
                    if on_face(p.x) {
                        p.x += dx;
                    }
                }
            }
            Some(EntityType::LwPolyline(polyline)) => {
                for vertex in &mut polyline.vertices {
                    if on_face(vertex.location.x) {
                        vertex.location.x += dx;
                    }
                }
            }
            _ => {}
        }
    }
}

/// L'entità passa sul layer `layer` (creato se manca), come un cambio di
/// layer fatto a mano in AutoCAD: l'XDATA resta.
pub(crate) fn move_to_layer(doc: &mut Document, handle: Handle, layer: &str) {
    if !doc.layers.contains(layer) {
        let mut new = Layer::new(layer);
        new.handle = doc.allocate_handle();
        doc.layers.add(new).unwrap();
    }
    let layer_handle = doc.layers.get(layer).map(|l| l.handle);
    let common = doc.get_entity_mut(handle).expect("entità").common_mut();
    common.layer = layer.into();
    common.layer_handle = layer_handle;
}

/// Una scena con le entità nello spazio modello e `INSUNITS` = `insunits`. I
/// layer nominati dalle entità si creano accesi, scongelati e sbloccati.
pub(crate) fn scene_with(entities: Vec<EntityType>, insunits: i16) -> crate::scene::Scene {
    let mut scene = crate::scene::Scene::new();
    scene.document.header.insertion_units = insunits;
    for entity in entities {
        scene.add_entity(entity);
    }
    scene
}
