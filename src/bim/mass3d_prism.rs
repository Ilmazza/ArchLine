//! Modello 3D, secondo passo (spec parte 5 §4.1, §4.2): dalle celle di
//! [`super::mass3d`] ai prismi, come facce piane. Logica pura, nessun disegno.
//!
//! - una **massa** con altezza nota è un prisma da 0 a `H`: il **coperchio** a `H`
//!   (triangolazione dell'anello meno i buchi) e per ogni lato di ogni anello un
//!   **quadrilatero verticale**; niente fondo;
//! - un **vano** apre il muro nei suoi due **lati passanti** (le facce del muro
//!   che l'apertura attraversa, `VanoRing::through`): lungo quei lati nessuna
//!   faccia verticale della massa tranne, dal vano, la **testa** (da `bottom +
//!   h` a `H`) e il **davanzale** (da 0 a `bottom`). La testa ha anche il suo
//!   intradosso a `bottom + h` e il suo coperchio a `H`; il davanzale il suo
//!   piano a `bottom`. Le spallette sono i lati di estremità del vano, che
//!   restano lati della massa. Una testa che arriva a `H` o più lascia il
//!   vano aperto fino in cima;
//! - un vano senza altezza (o una finestra senza davanzale) **non si apre**: la
//!   massa resta piena lungo i suoi lati passanti, il vano ha il suo coperchio a `H`
//!   e un avviso [`Notice::ModelloEscluso`] dice perché (mai una quota di
//!   ripiego); senza massa accanto, che dia l'altezza, il vano non c'è.
//!
//! Le quote sono in metri × `per_metre`; i punti in unità del disegno.

use kernel::geom2d::{distance_to, triangulate_rings, Curve, Line};

use super::mass3d::{Cell, CellKind, VanoRing};
use super::report::Notice;

/// Una faccia piana di quattro punti; un triangolo ripete l'ultimo.
pub type Quad = [[f64; 3]; 4];

/// Che cos'è una faccia, per il colore.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Mass,
    Floor,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face3 {
    pub quad: Quad,
    pub tone: Tone,
}

fn p3(p: [f64; 2], z: f64) -> [f64; 3] {
    [p[0], p[1], z]
}

fn wall(a: [f64; 2], b: [f64; 2], z0: f64, z1: f64) -> Face3 {
    Face3 {
        quad: [p3(a, z0), p3(b, z0), p3(b, z1), p3(a, z1)],
        tone: Tone::Mass,
    }
}

/// I triangoli di una regione (anello esterno e buchi) a quota `z`.
pub fn cover(ring: &[[f64; 2]], holes: &[Vec<[f64; 2]>], z: f64, tone: Tone) -> Vec<Face3> {
    let rings: Vec<Vec<[f64; 2]>> = std::iter::once(ring.to_vec())
        .chain(holes.iter().cloned())
        .collect();
    let (points, triangles) = triangulate_rings(&rings);
    triangles
        .iter()
        .filter(|t| t.iter().all(|&i| i < points.len()))
        .map(|t| Face3 {
            quad: [
                p3(points[t[0]], z),
                p3(points[t[1]], z),
                p3(points[t[2]], z),
                p3(points[t[2]], z),
            ],
            tone,
        })
        .collect()
}

fn line(a: [f64; 2], b: [f64; 2]) -> Curve {
    Curve::Line(Line { start: a, end: b })
}

/// I parametri `[t0, t1]` (0..1 lungo `a → b`) del tratto di `side` che sta
/// sulla retta di `a → b` e lo sovrappone per più di `tol`.
fn overlap(a: [f64; 2], b: [f64; 2], side: ([f64; 2], [f64; 2]), tol: f64) -> Option<(f64, f64)> {
    let e = [b[0] - a[0], b[1] - a[1]];
    let len = e[0].hypot(e[1]);
    if len <= tol {
        return None;
    }
    let edge = line(a, b);
    if distance_to(&edge, side.0) > tol || distance_to(&edge, side.1) > tol {
        return None;
    }
    let t = |p: [f64; 2]| ((p[0] - a[0]) * e[0] + (p[1] - a[1]) * e[1]) / (len * len);
    let (t0, t1) = (t(side.0).min(t(side.1)).max(0.0), t(side.0).max(t(side.1)).min(1.0));
    ((t1 - t0) * len > tol).then_some((t0, t1))
}

/// I tratti di `a → b` fuori dagli intervalli dei lati passanti.
fn free_parts(
    a: [f64; 2],
    b: [f64; 2],
    through: &[([f64; 2], [f64; 2])],
    tol: f64,
) -> Vec<([f64; 2], [f64; 2])> {
    let mut gaps: Vec<(f64, f64)> = through
        .iter()
        .filter_map(|side| overlap(a, b, *side, tol))
        .collect();
    gaps.sort_by(|x, y| x.0.total_cmp(&y.0));
    let at = |t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let mut parts = Vec::new();
    let mut cursor = 0.0_f64;
    for (t0, t1) in gaps {
        if (t0 - cursor) * len > tol {
            parts.push((at(cursor), at(t0)));
        }
        cursor = cursor.max(t1);
    }
    if (1.0 - cursor) * len > tol {
        parts.push((at(cursor), b));
    }
    parts
}

fn touches(a: &[[f64; 2]], b: &[[f64; 2]], tol: f64) -> bool {
    let near = |ring: &[[f64; 2]], p: [f64; 2]| {
        (0..ring.len()).any(|i| {
            distance_to(&line(ring[i], ring[(i + 1) % ring.len()]), p) <= tol
        })
    };
    a.iter().any(|&p| near(b, p)) || b.iter().any(|&p| near(a, p))
}

/// Il lato `a → b` alto `top` senza le parti in comune con altre masse
/// (`neighbours`: intervalli 0..1 sul lato e quota del vicino): dove il vicino
/// è alto quanto o più di questa massa non si disegna nulla, altrimenti solo
/// da lui in su.
fn visible_wall(
    a: [f64; 2],
    b: [f64; 2],
    top: f64,
    mut neighbours: Vec<(f64, f64, f64)>,
    tol: f64,
) -> Vec<Face3> {
    neighbours.sort_by(|x, y| x.0.total_cmp(&y.0));
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let at = |t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let mut out = Vec::new();
    let mut cursor = 0.0_f64;
    for (t0, t1, other_top) in neighbours {
        let lo = t0.max(cursor);
        if (lo - cursor) * len > tol {
            out.push(wall(at(cursor), at(lo), 0.0, top));
        }
        if t1 > lo && other_top < top - tol {
            out.push(wall(at(lo), at(t1), other_top, top));
        }
        cursor = cursor.max(t1);
    }
    if (1.0 - cursor) * len > tol {
        out.push(wall(at(cursor), b, 0.0, top));
    }
    out
}

/// Le facce del modello: prismi delle masse con altezza, vani aperti e gli
/// avvisi. `per_metre` converte le quote (in metri) in unità del disegno.
pub fn extrude(
    cells: &[Cell],
    vani: &[VanoRing],
    per_metre: f64,
    tol: f64,
) -> (Vec<Face3>, Vec<Notice>) {
    let mut faces = Vec::new();
    let mut notices = Vec::new();
    // Altezza di ogni vano: il massimo delle masse con altezza che lo toccano.
    let vano_height: Vec<Option<f64>> = cells
        .iter()
        .filter_map(|cell| match cell.kind {
            CellKind::Vano(k) => Some((k, &cell.ring)),
            _ => None,
        })
        .fold(vec![None; vani.len()], |mut heights, (k, ring)| {
            heights[k] = cells
                .iter()
                .filter(|c| c.kind == CellKind::Mass)
                .filter_map(|c| {
                    let h = c.height.value_m?;
                    let near = touches(&c.ring, ring, tol)
                        || c.holes.iter().any(|hole| touches(hole, ring, tol));
                    near.then_some(h)
                })
                .fold(None, |best: Option<f64>, h| Some(best.map_or(h, |b| b.max(h))));
            heights
        });
    // Vani che si aprono davvero: lati passanti da togliere alla massa.
    let drawable = |k: usize| -> bool {
        vano_height[k].is_some() && vani[k].bottom_m.is_some() && vani[k].height_m.is_some()
    };
    let through: Vec<([f64; 2], [f64; 2])> = (0..vani.len())
        .filter(|&k| drawable(k))
        .flat_map(|k| vani[k].through.iter().copied())
        .collect();
    // I lati delle altre masse (con la loro quota): il lato in comune tra due
    // masse è interno e non si disegna, tranne la parte che sporge sulla più bassa.
    let mass_edges: Vec<(usize, ([f64; 2], [f64; 2]), f64)> = cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.kind == CellKind::Mass)
        .filter_map(|(index, c)| Some((index, c.height.value_m? * per_metre, c)))
        .flat_map(|(index, top, c)| {
            std::iter::once(&c.ring)
                .chain(&c.holes)
                .flat_map(move |ring| {
                    (0..ring.len()).map(move |i| (index, (ring[i], ring[(i + 1) % ring.len()]), top))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    for (index, cell) in cells.iter().enumerate().filter(|(_, c)| c.kind == CellKind::Mass) {
        let Some(height) = cell.height.value_m else {
            continue;
        };
        let top = height * per_metre;
        faces.extend(cover(&cell.ring, &cell.holes, top, Tone::Mass));
        for ring in std::iter::once(&cell.ring).chain(&cell.holes) {
            for i in 0..ring.len() {
                let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                for (pa, pb) in free_parts(a, b, &through, tol) {
                    let neighbours: Vec<(f64, f64, f64)> = mass_edges
                        .iter()
                        .filter(|(other, _, _)| *other != index)
                        .filter_map(|(_, side, other_top)| {
                            overlap(pa, pb, *side, tol).map(|(t0, t1)| (t0, t1, *other_top))
                        })
                        .collect();
                    faces.extend(visible_wall(pa, pb, top, neighbours, tol));
                }
            }
        }
    }
    for cell in cells {
        let CellKind::Vano(k) = cell.kind else {
            continue;
        };
        let vano = &vani[k];
        let Some(height) = vano_height[k] else {
            notices.push(Notice::ModelloEscluso {
                what: format!("apertura {}", vano.code),
                reason: "nessuna massa con altezza accanto".to_string(),
            });
            continue;
        };
        let top = height * per_metre;
        let (Some(bottom_m), Some(h_m)) = (vano.bottom_m, vano.height_m) else {
            notices.push(Notice::ModelloEscluso {
                what: format!("apertura {}", vano.code),
                reason: if vano.height_m.is_none() {
                    "altezza dell'apertura n.d.: il muro resta pieno".to_string()
                } else {
                    "davanzale n.d.: il muro resta pieno".to_string()
                },
            });
            // Il vano pieno ha almeno il suo coperchio.
            faces.extend(cover(&cell.ring, &[], top, Tone::Mass));
            continue;
        };
        let (bottom, head) = (bottom_m * per_metre, (bottom_m + h_m) * per_metre);
        if bottom > tol {
            faces.extend(cover(&cell.ring, &[], bottom, Tone::Mass));
            for (a, b) in &vano.through {
                faces.push(wall(*a, *b, 0.0, bottom));
            }
        }
        if head < top - tol {
            faces.extend(cover(&cell.ring, &[], head, Tone::Mass));
            faces.extend(cover(&cell.ring, &[], top, Tone::Mass));
            for (a, b) in &vano.through {
                faces.push(wall(*a, *b, head, top));
            }
        }
    }
    (faces, notices)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use crate::bim::fixtures::{case_a, case_a_with_door, scene_with, Form};
    use crate::bim::height::{Height, HeightState};
    use crate::bim::mass3d::classify;
    use crate::bim::rooms::measure_at;
    use crate::bim::units::DrawingUnit;
    use crate::bim::walls::{faces as wall_faces, rect_from_axis_thickness, wall_set_without_rooms};
    use codec::Handle;
    use kernel::geom2d::signed_area;
    use rustc_hash::FxHashMap;

    use crate::scene::BoundarySource;

    const M: DrawingUnit = DrawingUnit::Metri;
    const TOL: f64 = 1e-6;
    const FORMS: [Form; 2] = [Form::Continua, Form::ChiusaPerMuro];

    fn given(value: f64) -> Height {
        Height {
            value_m: Some(value),
            state: HeightState::Inserita,
            note: None,
        }
    }

    type Setup = (
        FxHashMap<Handle, BoundarySource>,
        Vec<VanoRing>,
        Vec<Vec<[f64; 2]>>,
    );

    /// Come in `mass3d::tests`, con i lati passanti e le quote della porta.
    fn setup(form: Form, door: Option<(Option<f64>, Option<f64>)>) -> Setup {
        use crate::bim::aperture::{aperture_polyline, ensure_layer, Aperture, ApertureKind, ApertureState};
        let entities = if door.is_some() {
            case_a_with_door(form, M)
        } else {
            case_a(form, M)
        };
        let scene = scene_with(entities, M.insunits());
        let mut sources = wall_set_without_rooms(&scene.document, TOL).sources;
        let mut vani = Vec::new();
        if let Some((bottom, height)) = door {
            let a = Aperture {
                kind: ApertureKind::Porta,
                state: ApertureState::Esistente,
                axis: ([4.05, 1.0], [4.05, 1.9]),
                thickness: 0.10,
                height: Some(2.1),
                sill: None,
                threshold: true,
                lintel: false,
                bearing: None,
            };
            let mut doc = scene.document.clone();
            ensure_layer(&mut doc);
            doc.add_entity(aperture_polyline(&a).unwrap()).unwrap();
            crate::bim::aperture::add_fatto_thresholds(&doc, &mut sources, TOL);
            let rect = rect_from_axis_thickness(a.axis.0, a.axis.1, a.thickness).unwrap();
            vani.push(VanoRing {
                code: "P1".into(),
                ring: crate::bim::aperture::apertures_in(&doc)[0].ring.clone(),
                through: rect.long_sides().to_vec(),
                bottom_m: bottom,
                height_m: height,
            });
        }
        let all = wall_faces(&sources, TOL);
        let outlines = [[2.0, 1.5], [6.0, 1.5]]
            .into_iter()
            .map(|seed| {
                measure_at(&all, None, &sources, seed, M, TOL)
                    .expect("locale chiuso")
                    .region
                    .outer
            })
            .collect();
        (sources, vani, outlines)
    }

    fn run(setup: &Setup, heights: [Option<f64>; 2]) -> (Vec<Face3>, Vec<Notice>) {
        let rooms = vec![
            ("A".to_string(), heights[0].map_or_else(|| Height::missing(None), given)),
            ("B".to_string(), heights[1].map_or_else(|| Height::missing(None), given)),
        ];
        let (cells, _) = classify(&setup.0, &setup.1, &setup.2, &rooms, 1.0, TOL);
        extrude(&cells, &setup.1, 1.0, TOL)
    }

    fn area(face: &Face3) -> f64 {
        let ring: Vec<[f64; 2]> = face.quad[..3].iter().map(|p| [p[0], p[1]]).collect();
        signed_area(&ring).abs()
    }

    /// Area dei coperchi alla quota data.
    fn top_area(faces: &[Face3], z: f64) -> f64 {
        faces
            .iter()
            .filter(|f| f.quad.iter().all(|p| (p[2] - z).abs() < 1e-9))
            .map(area)
            .sum()
    }

    /// Caso A senza porta: i coperchi a 2,70 sommano 7,26 m², il volume
    /// 7,26 × 2,70 = 19,602 m³; ogni spigolo verticale è condiviso da due lati.
    #[test]
    fn case_a_walls_are_closed_prisms() {
        for form in FORMS {
            let (faces, notices) = run(&setup(form, None), [Some(2.7), Some(2.7)]);
            assert!(notices.is_empty(), "{form:?}: {notices:?}");
            let top = top_area(&faces, 2.7);
            assert!((top - 7.26).abs() < 1e-9, "{form:?}: coperchi {top}");
            assert!((top * 2.7 - 19.602).abs() < 1e-9);
            let mut edges: HashMap<(i64, i64, i64, i64), usize> = HashMap::new();
            let key = |p: [f64; 3]| (p[0] * 1e6).round() as i64 * 1_000_000_007 + (p[1] * 1e6).round() as i64;
            for f in faces.iter().filter(|f| f.quad[0][2] == 0.0 && f.quad[3][2] > 0.0) {
                for (a, b) in [(f.quad[0], f.quad[3]), (f.quad[1], f.quad[2])] {
                    *edges.entry((key(a), 0, (a[2] * 1e6) as i64, (b[2] * 1e6) as i64)).or_default() += 1;
                }
            }
            assert!(!edges.is_empty());
            assert!(edges.values().all(|&n| n == 2), "{form:?}: {edges:?}");
        }
    }

    /// Con la porta (0,90 × 2,10): niente faccia verticale sulle facce del
    /// muro nel vano sotto la testa; la testa è alta 0,60 (da 2,10 a 2,70), con
    /// l'intradosso e il coperchio di 0,09 m²; i coperchi della massa 7,17 m².
    #[test]
    fn the_door_opens_the_wall_and_leaves_a_head_0_60_high() {
        for form in FORMS {
            let (faces, notices) = run(&setup(form, Some((Some(0.0), Some(2.1)))), [Some(2.7), Some(2.7)]);
            assert!(notices.is_empty(), "{form:?}: {notices:?}");
            let top = top_area(&faces, 2.7);
            assert!((top - 7.26).abs() < 1e-9, "{form:?}: coperchi massa + testa = {top}");
            assert!((top_area(&faces, 2.1) - 0.09).abs() < 1e-9, "{form:?}: intradosso");
            // Facce verticali sul piano x = 4,00 e x = 4,10 con y in 1,00..1,90.
            for x in [4.0, 4.1] {
                let across: Vec<&Face3> = faces
                    .iter()
                    .filter(|f| f.quad.iter().all(|p| (p[0] - x).abs() < 1e-9))
                    .filter(|f| f.quad.iter().all(|p| p[1] > 1.0 - 1e-9 && p[1] < 1.9 + 1e-9))
                    .collect();
                assert_eq!(across.len(), 1, "{form:?} x={x}: solo la testa, {across:?}");
                let zs: Vec<f64> = across[0].quad.iter().map(|p| p[2]).collect();
                assert!(zs.iter().all(|z| (*z - 2.1).abs() < 1e-9 || (*z - 2.7).abs() < 1e-9), "{zs:?}");
            }
        }
    }

    /// Una finestra: davanzale 0,90 e altezza 1,20: sotto un piano a 0,90, la
    /// testa da 2,10 a 2,70.
    #[test]
    fn a_window_has_a_sill_and_a_head() {
        let (faces, notices) = run(&setup(Form::Continua, Some((Some(0.9), Some(1.2)))), [Some(2.7), Some(2.7)]);
        assert!(notices.is_empty(), "{notices:?}");
        assert!((top_area(&faces, 0.9) - 0.09).abs() < 1e-9, "piano del davanzale");
        assert!((top_area(&faces, 2.1) - 0.09).abs() < 1e-9, "intradosso della testa");
        let under: Vec<&Face3> = faces
            .iter()
            .filter(|f| (f.quad[0][0] - 4.0).abs() < 1e-9 && f.quad.iter().all(|p| (p[0] - 4.0).abs() < 1e-9))
            .filter(|f| f.quad.iter().all(|p| p[1] > 1.0 - 1e-9 && p[1] < 1.9 + 1e-9))
            .collect();
        assert_eq!(under.len(), 2, "sotto il davanzale e sopra la testa: {under:?}");
    }

    /// Senza altezza (o senza davanzale) il vano non si apre: il muro resta
    /// pieno, c'è l'avviso e un coperchio per il vano.
    #[test]
    fn missing_data_leaves_the_wall_full_with_a_notice() {
        for (bottom, height, why) in [
            (Some(0.0), None, "altezza dell'apertura n.d."),
            (None, Some(1.2), "davanzale n.d."),
        ] {
            let (faces, notices) = run(&setup(Form::Continua, Some((bottom, height))), [Some(2.7), Some(2.7)]);
            let texts: Vec<String> = notices.iter().map(Notice::text).collect();
            assert!(texts.iter().any(|t| t.contains("apertura P1") && t.contains(why)), "{texts:?}");
            // Il muro è pieno: il piano x = 4,00 ha le sue facce intere.
            let full = faces
                .iter()
                .filter(|f| f.quad.iter().all(|p| (p[0] - 4.0).abs() < 1e-9))
                .filter(|f| f.quad.iter().any(|p| p[1] > 1.0 && p[1] < 1.9) || f.quad.iter().all(|p| p[1] >= 1.0 && p[1] <= 1.9))
                .any(|f| f.quad.iter().any(|p| p[2] < 1e-9) && f.quad.iter().any(|p| (p[2] - 2.7).abs() < 1e-9));
            assert!(full, "il muro resta pieno");
        }
    }

    /// Una massa senza altezza non si estrude: nessuna faccia.
    #[test]
    fn a_mass_without_height_gives_no_faces() {
        let (faces, notices) = run(&setup(Form::Continua, None), [None, None]);
        assert!(faces.is_empty());
        assert!(!notices.is_empty() || faces.is_empty());
    }
}
