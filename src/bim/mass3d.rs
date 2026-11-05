//! Modello 3D, primo passo (spec parte 5 §4.1): le facce di una fase come celle
//! di tre tipi — **locale**, **vano** di un'apertura e **massa muraria** — con la
//! regione di ognuna (anello esterno e figli diretti) e l'altezza della massa.
//! Logica pura, nessun disegno: i prismi vengono dopo.
//!
//! Regole (R123):
//! - le facce sono i cicli del grafo planare dei muri (`walls::faces`); al grafo
//!   si aggiungono **tutti e quattro i lati** del rettangolo di ogni apertura,
//!   così il vano è una faccia chiusa in entrambe le forme del rilievo (linee
//!   continue o polilinee chiuse);
//! - una faccia è un **locale** se coincide con il contorno di un locale
//!   misurato, un **vano** se coincide con il rettangolo di un'apertura, altrimenti
//!   **massa**; «coincide» = stessa area (entro `tol²` e 1e-6 relativo) e ogni
//!   vertice dell'altra sul suo bordo (entro `tol`);
//! - i **figli diretti** di una faccia sono le facce che le stanno dentro (tutti i
//!   vertici dentro o sul bordo, area minore) e non dentro un'altra faccia più
//!   piccola che la contiene: sono i buchi del suo prisma;
//! - l'altezza di una massa è il **massimo** delle altezze dei locali il cui
//!   contorno tocca il suo anello o uno dei suoi buchi (un vertice dell'uno sul
//!   bordo dell'altro); `inserita`
//!   solo se tutti lo sono, altrimenti `ereditata`. Se uno di quei locali non ha
//!   altezza, o nessun locale la tocca, la massa è `n.d.`, **non si estrude** e
//!   c'è un avviso [`Notice::ModelloEscluso`]: mai un'altezza di ripiego.
//!
//! Distanze in unità del disegno, quote in metri.

use codec::{EntityType, Handle};
use kernel::geom2d::{contains, distance_to, signed_area, Tolerance};
use rustc_hash::FxHashMap;

use super::height::{Height, HeightState};
use super::project::FIRST_SYNTHETIC_HANDLE;
use super::report::Notice;
use super::rooms::ring_curves;
use super::units::format_decimal;
use super::walls::{faces, sources_of};
use crate::scene::BoundarySource;

/// Il rettangolo di un'apertura, con il suo codice.
#[derive(Clone, Debug, PartialEq)]
pub struct VanoRing {
    pub code: String,
    pub ring: Vec<[f64; 2]>,
    /// I due lati che il vano attraversa (le facce del muro), paralleli
    /// all'asse dell'apertura: lungo questi il muro si apre.
    pub through: Vec<([f64; 2], [f64; 2])>,
    /// Quota del bordo basso in metri: 0 per porte e portefinestre, il davanzale
    /// per le finestre; `None` se manca.
    pub bottom_m: Option<f64>,
    /// Altezza del vano in metri; `None` se manca.
    pub height_m: Option<f64>,
}

/// Di che cosa è fatta una faccia.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    /// Il locale di indice dato (nell'elenco dei contorni passati).
    Room(usize),
    /// Il vano dell'apertura di indice dato (nell'elenco dei vani passati).
    Vano(usize),
    Mass,
}

/// Una faccia classificata, con la sua regione.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub kind: CellKind,
    /// L'anello esterno, antiorario.
    pub ring: Vec<[f64; 2]>,
    /// I figli diretti: i buchi del prisma.
    pub holes: Vec<Vec<[f64; 2]>>,
    /// L'altezza della massa; per locali e vani non si usa (`n.d.`).
    pub height: Height,
}

impl Cell {
    /// Area in pianta della regione (anello meno buchi), in unità del disegno al
    /// quadrato.
    pub fn plan_area(&self) -> f64 {
        signed_area(&self.ring).abs() - self.holes.iter().map(|h| signed_area(h).abs()).sum::<f64>()
    }
}

fn near_boundary(ring: &[[f64; 2]], p: [f64; 2], tol: f64) -> bool {
    ring_curves(ring)
        .iter()
        .any(|curve| distance_to(curve, p) <= tol)
}

/// Le due facce hanno lo stesso contorno entro `tol`.
fn same_outline(a: &[[f64; 2]], b: &[[f64; 2]], tol: f64) -> bool {
    let (area_a, area_b) = (signed_area(a).abs(), signed_area(b).abs());
    (area_a - area_b).abs() <= tol * tol + 1.0e-6 * area_a.max(area_b)
        && a.iter().all(|&p| near_boundary(b, p, tol))
        && b.iter().all(|&p| near_boundary(a, p, tol))
}

/// Il contorno `inner` sta dentro `outer` (vertici dentro o sul bordo).
fn is_inside(outer: &[[f64; 2]], inner: &[[f64; 2]], tol: f64) -> bool {
    let tolerance = Tolerance::new(tol);
    let curves = ring_curves(outer);
    signed_area(inner).abs() < signed_area(outer).abs() - tol * tol
        && inner.iter().all(|&p| contains(&curves, p, tolerance))
}

/// I due contorni si toccano: un vertice dell'uno sul bordo dell'altro.
fn touches(a: &[[f64; 2]], b: &[[f64; 2]], tol: f64) -> bool {
    a.iter().any(|&p| near_boundary(b, p, tol)) || b.iter().any(|&p| near_boundary(a, p, tol))
}

/// Aggiunge ai muri (`sources`) i lati dei rettangoli dei vani come linee
/// sintetiche, con handle sopra gli esistenti.
fn with_vano_sides(
    sources: &FxHashMap<Handle, BoundarySource>,
    vani: &[VanoRing],
    tol: f64,
) -> FxHashMap<Handle, BoundarySource> {
    let mut all = sources.clone();
    let first = all
        .keys()
        .map(|handle| handle.value().saturating_add(1))
        .fold(FIRST_SYNTHETIC_HANDLE, u64::max);
    let mut lines: Vec<(Handle, EntityType)> = Vec::new();
    for vano in vani {
        for i in 0..vano.ring.len() {
            let (a, b) = (vano.ring[i], vano.ring[(i + 1) % vano.ring.len()]);
            let handle = Handle::new(first.saturating_add(lines.len() as u64));
            let mut line = codec::entities::Line::from_points(
                codec::Vector3::new(a[0], a[1], 0.0),
                codec::Vector3::new(b[0], b[1], 0.0),
            );
            line.common.layer = "0".to_string();
            line.common.handle = handle;
            lines.push((handle, EntityType::Line(line)));
        }
    }
    all.extend(sources_of(&lines, tol).0);
    all
}

/// Le celle di una fase: locali (`outlines`, con nome e altezza in `rooms`, nello
/// stesso ordine), vani delle aperture e masse, con gli avvisi per le masse che
/// non si possono estrudere. `per_metre` converte in metri per i messaggi.
pub fn classify(
    sources: &FxHashMap<Handle, BoundarySource>,
    vani: &[VanoRing],
    outlines: &[Vec<[f64; 2]>],
    rooms: &[(String, Height)],
    per_metre: f64,
    tol: f64,
) -> (Vec<Cell>, Vec<Notice>) {
    let graph = with_vano_sides(sources, vani, tol);
    let all = faces(&graph, tol);
    let areas: Vec<f64> = all.iter().map(|f| signed_area(f).abs()).collect();
    // Il contenitore più piccolo di ogni faccia.
    let parent: Vec<Option<usize>> = (0..all.len())
        .map(|c| {
            (0..all.len())
                .filter(|&f| f != c && is_inside(&all[f], &all[c], tol))
                .min_by(|&a, &b| areas[a].total_cmp(&areas[b]))
        })
        .collect();
    let mut notices = Vec::new();
    let mut cells = Vec::new();
    for (f, ring) in all.iter().enumerate() {
        let kind = if let Some(i) = outlines.iter().position(|o| same_outline(ring, o, tol)) {
            CellKind::Room(i)
        } else if let Some(k) = vani.iter().position(|v| same_outline(ring, &v.ring, tol)) {
            CellKind::Vano(k)
        } else {
            CellKind::Mass
        };
        let holes: Vec<Vec<[f64; 2]>> = (0..all.len())
            .filter(|&c| parent[c] == Some(f))
            .map(|c| all[c].clone())
            .collect();
        let height = if kind == CellKind::Mass {
            let touching: Vec<&(String, Height)> = outlines
                .iter()
                .zip(rooms)
                .filter(|(outline, _)| {
                    // La regione è l'anello e i suoi buchi: un locale può essere uno dei buchi.
                    touches(ring, outline, tol) || holes.iter().any(|hole| touches(hole, outline, tol))
                })
                .map(|(_, room)| room)
                .collect();
            let at = format!(
                "massa a ({}; {}) m",
                format_decimal(ring[0][0] / per_metre),
                format_decimal(ring[0][1] / per_metre)
            );
            if touching.is_empty() {
                notices.push(Notice::ModelloEscluso {
                    what: at,
                    reason: "nessun locale accanto".to_string(),
                });
                Height::missing(Some("nessun locale accanto"))
            } else if touching.iter().any(|(_, h)| h.value_m.is_none()) {
                notices.push(Notice::ModelloEscluso {
                    what: at,
                    reason: "altezza del locale accanto n.d.".to_string(),
                });
                Height::missing(Some("altezza del locale accanto n.d."))
            } else {
                let value = touching
                    .iter()
                    .filter_map(|(_, h)| h.value_m)
                    .fold(f64::MIN, f64::max);
                let all_given = touching.iter().all(|(_, h)| h.state == HeightState::Inserita);
                Height {
                    value_m: Some(value),
                    state: if all_given {
                        HeightState::Inserita
                    } else {
                        HeightState::Ereditata
                    },
                    note: None,
                }
            }
        } else {
            Height::missing(None)
        };
        cells.push(Cell {
            kind,
            ring: ring.clone(),
            holes,
            height,
        });
    }
    (cells, notices)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bim::fixtures::{case_a, case_a_with_door, scene_with, Form};
    use crate::bim::rooms::measure_at;
    use crate::bim::units::DrawingUnit;
    use crate::bim::walls::wall_set_without_rooms;

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

    /// I muri del caso, i contorni di A e B (con la porta se `door`, chiusa dalle
    /// sue soglie) e i vani.
    fn setup(
        form: Form,
        door: bool,
    ) -> (FxHashMap<Handle, BoundarySource>, Vec<VanoRing>, Vec<Vec<[f64; 2]>>) {
        use crate::bim::aperture::{aperture_polyline, ensure_layer, Aperture, ApertureKind, ApertureState};
        let entities = if door { case_a_with_door(form, M) } else { case_a(form, M) };
        let scene = scene_with(entities, M.insunits());
        let mut sources = wall_set_without_rooms(&scene.document, TOL).sources;
        let mut vani = Vec::new();
        if door {
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
            let ring = crate::bim::aperture::apertures_in(&doc)[0].ring.clone();
            vani.push(VanoRing {
                code: "P1".into(),
                ring,
                through: Vec::new(),
                bottom_m: Some(0.0),
                height_m: Some(2.1),
            });
        }
        let all = faces(&sources, TOL);
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

    fn rooms(a: Option<f64>, b: Option<f64>) -> Vec<(String, Height)> {
        vec![
            ("A".into(), a.map_or_else(|| Height::missing(None), given)),
            ("B".into(), b.map_or_else(|| Height::missing(None), given)),
        ]
    }

    fn mass_area(cells: &[Cell]) -> f64 {
        cells
            .iter()
            .filter(|c| c.kind == CellKind::Mass)
            .map(Cell::plan_area)
            .sum()
    }

    fn count(cells: &[Cell], want: impl Fn(CellKind) -> bool) -> usize {
        cells.iter().filter(|c| want(c.kind)).count()
    }

    /// Caso A senza porta: due locali, e la massa vale 30,96 − 12,00 − 11,70 =
    /// 7,26 m²; la regione ha i due locali per buchi. Alta 2,70.
    #[test]
    fn case_a_is_two_rooms_and_the_wall_mass() {
        for form in FORMS {
            let (sources, vani, outlines) = setup(form, false);
            let (cells, notices) = classify(&sources, &vani, &outlines, &rooms(Some(2.7), Some(2.7)), 1.0, TOL);
            assert!(notices.is_empty(), "{form:?}: {notices:?}");
            assert_eq!(count(&cells, |k| matches!(k, CellKind::Room(_))), 2, "{form:?}");
            assert_eq!(count(&cells, |k| matches!(k, CellKind::Vano(_))), 0, "{form:?}");
            let area = mass_area(&cells);
            assert!((area - 7.26).abs() < 1e-9, "{form:?}: massa {area}");
            for cell in cells.iter().filter(|c| c.kind == CellKind::Mass) {
                assert_eq!(cell.height.value_m, Some(2.7), "{form:?}");
                assert_eq!(cell.height.state, HeightState::Inserita);
            }
        }
    }

    /// Con la porta il vano è una faccia sua in entrambe le forme del rilievo e
    /// la massa perde i suoi 0,09 m²: 7,26 − 0,09 = 7,17.
    #[test]
    fn the_door_vano_is_its_own_face_and_leaves_the_mass() {
        for form in FORMS {
            let (sources, vani, outlines) = setup(form, true);
            let (cells, notices) = classify(&sources, &vani, &outlines, &rooms(Some(2.7), Some(2.7)), 1.0, TOL);
            assert!(notices.is_empty(), "{form:?}: {notices:?}");
            assert_eq!(count(&cells, |k| k == CellKind::Vano(0)), 1, "{form:?}");
            assert_eq!(count(&cells, |k| matches!(k, CellKind::Room(_))), 2, "{form:?}");
            let area = mass_area(&cells);
            assert!((area - 7.17).abs() < 1e-9, "{form:?}: massa {area}");
        }
    }

    /// La massa prende il massimo dei locali che tocca (il muro d'ambito tocca
    /// A e B); `ereditata` se uno dei due lo è.
    #[test]
    fn mass_height_is_the_highest_of_the_rooms_it_touches() {
        let (sources, vani, outlines) = setup(Form::Continua, false);
        let mut list = rooms(Some(2.7), Some(3.0));
        list[1].1.state = HeightState::Ereditata;
        let (cells, _) = classify(&sources, &vani, &outlines, &list, 1.0, TOL);
        let mass: Vec<&Cell> = cells.iter().filter(|c| c.kind == CellKind::Mass).collect();
        assert!(!mass.is_empty());
        for cell in mass {
            assert_eq!(cell.height.value_m, Some(3.0));
            assert_eq!(cell.height.state, HeightState::Ereditata);
        }
    }

    /// Un locale senza altezza: la massa che lo tocca è `n.d.` con l'avviso;
    /// senza nessun locale accanto lo stesso. Mai un ripiego.
    #[test]
    fn missing_heights_mean_no_extrusion_and_a_notice() {
        let (sources, vani, outlines) = setup(Form::Continua, false);
        let (cells, notices) = classify(&sources, &vani, &outlines, &rooms(Some(2.7), None), 1.0, TOL);
        assert!(cells
            .iter()
            .filter(|c| c.kind == CellKind::Mass)
            .all(|c| c.height.value_m.is_none()));
        assert!(!notices.is_empty());
        assert!(notices[0].text().contains("non estruso"), "{}", notices[0].text());

        let (cells, notices) = classify(&sources, &vani, &[], &[], 1.0, TOL);
        assert!(cells
            .iter()
            .all(|c| c.kind == CellKind::Mass && c.height.value_m.is_none()));
        assert!(notices.iter().any(|n| n.text().contains("nessun locale accanto")));
    }

    /// I figli diretti sono i buchi: la regione di massa del caso A ha 2 buchi
    /// (i locali) e area = anello − buchi.
    #[test]
    fn holes_are_the_direct_children() {
        let (sources, vani, outlines) = setup(Form::Continua, false);
        let (cells, _) = classify(&sources, &vani, &outlines, &rooms(Some(2.7), Some(2.7)), 1.0, TOL);
        let outer = cells
            .iter()
            .find(|c| c.kind == CellKind::Mass)
            .expect("massa");
        assert_eq!(outer.holes.len(), 2);
        assert!((outer.plan_area() - 7.26).abs() < 1e-9);
    }
}
