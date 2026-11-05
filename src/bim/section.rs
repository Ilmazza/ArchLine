//! Sezione (spec parte 4): il profilo di un taglio sulla pianta. Logica pura:
//! dato il taglio (due punti), le facce dei muri e i contorni dei locali, dice
//! come è fatto il terreno lungo la retta — **fuori** dall'edificio, dentro un
//! **locale** o dentro la **massa muraria** (il poché) — in tratti di lunghezza
//! nota. Altezze, aperture e disegno vengono dopo (task successivi).
//!
//! Il profilo si ricava dalla pianta, non da «muri con altezza» (R111): nel
//! rilievo i muri sono linee sulle facce. Regole:
//! - i punti di cambio sono gli incroci del taglio con i lati delle facce
//!   (anelli di `walls::faces`), più i due estremi; un lato parallelo al
//!   taglio e sulla sua retta (entro `tol`) dà i suoi due estremi;
//! - un tratto è classificato dal suo punto medio: dentro il contorno di un
//!   locale (bordo compreso, il primo che lo contiene) è `Room(i)`; altrimenti
//!   dentro una faccia (`walls::region_at`) è `Mass`; altrimenti `Outside`;
//! - tratti vicini dello stesso tipo si uniscono; un tratto più corto di `tol`
//!   non esiste. Un taglio che corre lungo una faccia prende il tipo del lato
//!   dentro (bordo compreso): è un caso limite, non un errore.
//!
//! Distanze in unità del disegno, dal primo punto del taglio.

use kernel::geom2d::{contains, Tolerance};

use super::rooms::ring_curves;
use super::walls::region_at;

/// Di che cosa è fatto un tratto del taglio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind {
    /// Fuori dall'edificio: nessuna faccia.
    Outside,
    /// Dentro il locale di indice dato (nell'elenco dei contorni passati).
    Room(usize),
    /// Dentro la massa muraria: una faccia che non è un locale.
    Mass,
}

/// Un tratto del taglio, tra due distanze dal primo punto.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub from: f64,
    pub to: f64,
    pub kind: SpanKind,
}

impl Span {
    pub fn length(&self) -> f64 {
        self.to - self.from
    }
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

/// I parametri `t` (0..1 lungo il taglio) in cui la retta incontra i lati
/// degli anelli.
fn cut_params(from: [f64; 2], to: [f64; 2], rings: &[Vec<[f64; 2]>], tol: f64) -> Vec<f64> {
    let d = sub(to, from);
    let len = d[0].hypot(d[1]);
    let mut params = Vec::new();
    for ring in rings {
        for (index, &a) in ring.iter().enumerate() {
            let b = ring[(index + 1) % ring.len()];
            let e = sub(b, a);
            let e_len = e[0].hypot(e[1]);
            if e_len <= tol {
                continue;
            }
            let den = cross(d, e);
            if den.abs() > 1.0e-12 * len * e_len {
                let offset = sub(a, from);
                let t = cross(offset, e) / den;
                let s = cross(offset, d) / den;
                let slack = tol / len.max(tol);
                let on_edge = s >= -tol / e_len && s <= 1.0 + tol / e_len;
                if on_edge && t >= -slack && t <= 1.0 + slack {
                    params.push(t.clamp(0.0, 1.0));
                }
            } else {
                // Parallelo: conta solo se sta sulla retta del taglio.
                let off_line = cross(sub(a, from), d) / len;
                if off_line.abs() <= tol {
                    for p in [a, b] {
                        let t = ((p[0] - from[0]) * d[0] + (p[1] - from[1]) * d[1]) / (len * len);
                        if t > 0.0 && t < 1.0 {
                            params.push(t);
                        }
                    }
                }
            }
        }
    }
    params
}

/// Il profilo del taglio da `from` a `to`: i tratti in ordine, senza vuoti né
/// sovrapposizioni, dal primo all'ultimo punto. `faces` sono quelle dei muri
/// (`walls::faces`), `rooms` i contorni esterni dei locali. Un taglio di
/// lunghezza nulla non ha profilo.
pub fn profile(
    faces: &[Vec<[f64; 2]>],
    rooms: &[Vec<[f64; 2]>],
    from: [f64; 2],
    to: [f64; 2],
    tol: f64,
) -> Vec<Span> {
    let d = sub(to, from);
    let len = d[0].hypot(d[1]);
    if !(len > tol) {
        return Vec::new();
    }
    let mut ts = cut_params(from, to, faces, tol);
    ts.extend([0.0, 1.0]);
    ts.sort_by(f64::total_cmp);
    // Un tratto più corto di `tol` non esiste.
    let mut cuts: Vec<f64> = Vec::new();
    for t in ts {
        match cuts.last() {
            Some(&last) if (t - last) * len <= tol => {}
            _ => cuts.push(t),
        }
    }
    if let Some(last) = cuts.last_mut() {
        *last = 1.0;
    }
    let tolerance = Tolerance::new(tol);
    let room_curves: Vec<_> = rooms.iter().map(|ring| ring_curves(ring)).collect();
    let mut spans: Vec<Span> = Vec::new();
    for pair in cuts.windows(2) {
        let (t0, t1) = (pair[0], pair[1]);
        let tm = (t0 + t1) / 2.0;
        let p = [from[0] + d[0] * tm, from[1] + d[1] * tm];
        let kind = if let Some(index) = room_curves
            .iter()
            .position(|curves| contains(curves, p, tolerance))
        {
            SpanKind::Room(index)
        } else if region_at(faces, p, tol).is_some() {
            SpanKind::Mass
        } else {
            SpanKind::Outside
        };
        let span = Span {
            from: t0 * len,
            to: t1 * len,
            kind,
        };
        match spans.last_mut() {
            Some(last) if last.kind == kind => last.to = span.to,
            _ => spans.push(span),
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bim::fixtures::{case_a, case_a_with_door, scene_with, Form};
    use crate::bim::rooms::measure_at;
    use crate::bim::units::DrawingUnit;
    use crate::bim::walls::{faces, wall_set_without_rooms};

    const M: DrawingUnit = DrawingUnit::Metri;
    const TOL: f64 = 1e-6;
    const FORMS: [Form; 2] = [Form::Continua, Form::ChiusaPerMuro];

    /// Le facce dei muri e i contorni di A e B del caso dato.
    fn plan(entities: Vec<codec::EntityType>) -> (Vec<Vec<[f64; 2]>>, Vec<Vec<[f64; 2]>>) {
        let scene = scene_with(entities, M.insunits());
        let walls = wall_set_without_rooms(&scene.document, TOL);
        let all = faces(&walls.sources, TOL);
        let rooms = [[2.0, 1.5], [6.0, 1.5]]
            .into_iter()
            .map(|seed| {
                measure_at(&all, None, &walls.sources, seed, M, TOL)
                    .expect("locale chiuso")
                    .region
                    .outer
            })
            .collect();
        (all, rooms)
    }

    fn kinds(spans: &[Span]) -> Vec<SpanKind> {
        spans.iter().map(|s| s.kind).collect()
    }

    fn near(a: f64, b: f64, what: &str) {
        assert!((a - b).abs() < 1e-9, "{what}: {a} invece di {b}");
    }

    /// Caso A, taglio orizzontale a y = 1,5 da x = −1 a x = 9: fuori 0,70,
    /// muro 0,30, A 4,00, tramezzo 0,10, B 3,90, muro 0,30, fuori 0,70.
    #[test]
    fn horizontal_cut_through_case_a() {
        for form in FORMS {
            let (faces, rooms) = plan(case_a(form, M));
            let spans = profile(&faces, &rooms, [-1.0, 1.5], [9.0, 1.5], TOL);
            assert_eq!(
                kinds(&spans),
                [
                    SpanKind::Outside,
                    SpanKind::Mass,
                    SpanKind::Room(0),
                    SpanKind::Mass,
                    SpanKind::Room(1),
                    SpanKind::Mass,
                    SpanKind::Outside
                ],
                "{form:?}: {spans:?}"
            );
            let lengths: Vec<f64> = spans.iter().map(Span::length).collect();
            for (got, want) in lengths.iter().zip([0.7, 0.3, 4.0, 0.1, 3.9, 0.3, 0.7]) {
                near(*got, want, &format!("{form:?}"));
            }
            near(spans[0].from, 0.0, "inizio");
            near(spans.last().unwrap().to, 10.0, "fine");
            for pair in spans.windows(2) {
                near(pair[0].to, pair[1].from, "senza vuoti");
            }
        }
    }

    /// Un taglio che parte dentro un locale e finisce dentro l'altro.
    #[test]
    fn a_cut_can_start_and_end_inside_rooms() {
        let (faces, rooms) = plan(case_a(Form::Continua, M));
        let spans = profile(&faces, &rooms, [2.0, 1.5], [6.0, 1.5], TOL);
        assert_eq!(
            kinds(&spans),
            [SpanKind::Room(0), SpanKind::Mass, SpanKind::Room(1)],
            "{spans:?}"
        );
        near(spans[0].length(), 2.0, "A");
        near(spans[1].length(), 0.1, "tramezzo");
        near(spans[2].length(), 2.0 - 0.1, "B");
    }

    /// Un taglio verticale: nord-sud attraverso A.
    #[test]
    fn vertical_cut_through_a() {
        let (faces, rooms) = plan(case_a(Form::ChiusaPerMuro, M));
        let spans = profile(&faces, &rooms, [2.0, -1.0], [2.0, 4.0], TOL);
        assert_eq!(
            kinds(&spans),
            [
                SpanKind::Outside,
                SpanKind::Mass,
                SpanKind::Room(0),
                SpanKind::Mass,
                SpanKind::Outside
            ],
            "{spans:?}"
        );
        near(spans[2].length(), 3.0, "A in altezza di pianta");
    }

    /// Fuori dall'edificio non c'è nulla; un taglio degenere non ha profilo.
    #[test]
    fn a_cut_outside_has_one_outside_span_and_a_null_cut_has_none() {
        let (faces, rooms) = plan(case_a(Form::Continua, M));
        let spans = profile(&faces, &rooms, [-5.0, 10.0], [15.0, 10.0], TOL);
        assert_eq!(kinds(&spans), [SpanKind::Outside]);
        near(spans[0].length(), 20.0, "tutto fuori");
        assert!(profile(&faces, &rooms, [1.0, 1.0], [1.0, 1.0], TOL).is_empty());
    }

    /// Il vano porta del rilievo (senza spallette nella forma a linee): il
    /// taglio nel vano è massa muraria, poi le aperture lo bucheranno. I due
    /// locali restano due e il taglio alto non incontra il vano.
    #[test]
    fn the_door_vano_is_mass_for_the_profile() {
        let entities = case_a_with_door(Form::Continua, M);
        let scene = scene_with(entities, M.insunits());
        let mut sources = wall_set_without_rooms(&scene.document, TOL).sources;
        // La porta chiude A e B con le sue soglie.
        let a = crate::bim::aperture::Aperture {
            kind: crate::bim::aperture::ApertureKind::Porta,
            state: crate::bim::aperture::ApertureState::Esistente,
            axis: ([4.05, 1.0], [4.05, 1.9]),
            thickness: 0.10,
            height: Some(2.1),
            sill: None,
            threshold: true,
            lintel: false,
            bearing: None,
        };
        let mut doc = scene.document.clone();
        crate::bim::aperture::ensure_layer(&mut doc);
        doc.add_entity(crate::bim::aperture::aperture_polyline(&a).unwrap())
            .unwrap();
        crate::bim::aperture::add_fatto_thresholds(&doc, &mut sources, TOL);
        let all = faces(&sources, TOL);
        let rooms: Vec<Vec<[f64; 2]>> = [[2.0, 1.5], [6.0, 1.5]]
            .into_iter()
            .map(|seed| {
                measure_at(&all, None, &sources, seed, M, TOL)
                    .expect("locale chiuso")
                    .region
                    .outer
            })
            .collect();
        let through_door = profile(&all, &rooms, [2.0, 1.5], [6.0, 1.5], TOL);
        assert_eq!(
            kinds(&through_door),
            [SpanKind::Room(0), SpanKind::Mass, SpanKind::Room(1)],
            "{through_door:?}"
        );
        near(through_door[1].length(), 0.1, "il vano è spesso come il muro");
    }
}
