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

use super::aperture::ApertureKind;
use super::height::{Height, HeightState};
use super::report::Notice;
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


// ---------------------------------------------------------------------------
// Altezze e aperture sul profilo
// ---------------------------------------------------------------------------

/// Un'apertura vista dal taglio: il rettangolo del vano in pianta e le misure
/// inserite, in metri (`None` = `n.d.`).
#[derive(Clone, Debug, PartialEq)]
pub struct SectionAperture {
    pub code: String,
    pub kind: ApertureKind,
    pub ring: Vec<[f64; 2]>,
    pub height_m: Option<f64>,
    pub sill_m: Option<f64>,
}

/// Il vano di un'apertura nella massa muraria: tra due distanze dal primo
/// punto del taglio, dalla quota `bottom_m` per `height_m`.
#[derive(Clone, Debug, PartialEq)]
pub struct Hole {
    pub code: String,
    pub from: f64,
    pub to: f64,
    /// Quota del bordo basso: 0 per porte e portefinestre, il davanzale per le
    /// finestre; `None` se il davanzale manca.
    pub bottom_m: Option<f64>,
    pub height_m: Option<f64>,
}

impl Hole {
    /// Il vano si può disegnare: ha quota e altezza.
    pub fn is_drawable(&self) -> bool {
        self.bottom_m.is_some() && self.height_m.is_some()
    }
}

/// Un elemento della sezione, con la sua altezza e lo stato di questa.
#[derive(Clone, Debug, PartialEq)]
pub enum Element {
    /// Un locale: vuoto da terra al soffitto (quota = altezza del locale).
    Room {
        span: Span,
        index: usize,
        height: Height,
    },
    /// La massa muraria (poché) con i vani delle aperture che il taglio
    /// attraversa.
    Mass {
        span: Span,
        height: Height,
        holes: Vec<Hole>,
    },
}

impl Element {
    pub fn span(&self) -> Span {
        match self {
            Self::Room { span, .. } | Self::Mass { span, .. } => *span,
        }
    }

    pub fn height(&self) -> &Height {
        match self {
            Self::Room { height, .. } | Self::Mass { height, .. } => height,
        }
    }
}

/// I parametri `t` (0..1 lungo il taglio) in cui il taglio entra ed esce dal
/// rettangolo (convesso) `ring`: Cyrus-Beck. `None` se non lo attraversa.
fn clip_to_convex(from: [f64; 2], to: [f64; 2], ring: &[[f64; 2]]) -> Option<(f64, f64)> {
    if ring.len() < 3 {
        return None;
    }
    let d = sub(to, from);
    let area: f64 = (0..ring.len())
        .map(|i| cross(ring[i], ring[(i + 1) % ring.len()]))
        .sum();
    let orientation = if area >= 0.0 { 1.0 } else { -1.0 };
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        let edge = sub(b, a);
        // Dentro = a sinistra del lato (per un anello antiorario).
        let side = |p: [f64; 2]| orientation * cross(edge, sub(p, a));
        let (start, along) = (side(from), orientation * cross(edge, d));
        if along.abs() < 1.0e-15 {
            if start < 0.0 {
                return None;
            }
        } else {
            let t = -start / along;
            if along > 0.0 {
                lo = lo.max(t);
            } else {
                hi = hi.min(t);
            }
        }
    }
    (lo < hi).then_some((lo, hi))
}

/// Gli elementi della sezione: i tratti `Room` e `Mass` del profilo con le
/// altezze risolte (R111), i vani delle aperture dentro la massa e gli avvisi
/// per ciò che non si può disegnare.
///
/// - un locale ha l'altezza della sua riga (`rooms`: nome e altezza, nell'ordine
///   dei contorni passati a [`profile`]); senza, l'elemento c'è con l'altezza
///   `n.d.`;
/// - la massa ha il **massimo** delle altezze dei locali che la toccano
///   (il tratto `Room` che la precede e quello che la segue, se ci sono); se uno
///   di quei locali non ha altezza, o la massa non tocca nessun locale, è `n.d.`
///   e un avviso la elenca: mai un'altezza di ripiego;
/// - un'apertura fa un vano nella massa dove il taglio la attraversa (più
///   corto di `tol` non conta): porte e portefinestre da terra, finestre dal
///   davanzale; con l'altezza o il davanzale che mancano il vano è `n.d.`, non si
///   disegna e un avviso lo dice.
///
/// `per_metre` converte le distanze del disegno in metri per i messaggi.
pub fn elements(
    spans: &[Span],
    rooms: &[(String, Height)],
    apertures: &[SectionAperture],
    from: [f64; 2],
    to: [f64; 2],
    per_metre: f64,
    tol: f64,
) -> (Vec<Element>, Vec<Notice>) {
    let d = sub(to, from);
    let len = d[0].hypot(d[1]);
    let mut out = Vec::new();
    let mut notices = Vec::new();
    for (position, span) in spans.iter().enumerate() {
        match span.kind {
            SpanKind::Outside => {}
            SpanKind::Room(index) => {
                let (name, height) = rooms
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| (String::new(), Height::missing(Some("locale sconosciuto"))));
                if height.value_m.is_none() {
                    notices.push(Notice::SezioneEsclusa {
                        what: format!("altezza del locale {name}"),
                        reason: "altezza del locale n.d.".to_string(),
                    });
                }
                out.push(Element::Room {
                    span: *span,
                    index,
                    height,
                });
            }
            SpanKind::Mass => {
                let neighbour = |other: Option<&Span>| match other.map(|s| s.kind) {
                    Some(SpanKind::Room(index)) => rooms.get(index),
                    _ => None,
                };
                let touching: Vec<&(String, Height)> = [
                    position.checked_sub(1).and_then(|i| spans.get(i)),
                    spans.get(position + 1),
                ]
                .into_iter()
                .filter_map(neighbour)
                .collect();
                let at = format!("muro a {} m", super::units::format_decimal(span.from / per_metre));
                let height = if touching.is_empty() {
                    notices.push(Notice::SezioneEsclusa {
                        what: at,
                        reason: "nessun locale accanto".to_string(),
                    });
                    Height::missing(Some("nessun locale accanto"))
                } else if touching.iter().any(|(_, h)| h.value_m.is_none()) {
                    notices.push(Notice::SezioneEsclusa {
                        what: at,
                        reason: "altezza del locale accanto n.d.".to_string(),
                    });
                    Height::missing(Some("altezza del locale accanto n.d."))
                } else {
                    let value = touching
                        .iter()
                        .filter_map(|(_, h)| h.value_m)
                        .fold(f64::MIN, f64::max);
                    let all_given = touching
                        .iter()
                        .all(|(_, h)| h.state == HeightState::Inserita);
                    Height {
                        value_m: Some(value),
                        state: if all_given {
                            HeightState::Inserita
                        } else {
                            HeightState::Ereditata
                        },
                        note: None,
                    }
                };
                let holes = if height.value_m.is_some() {
                    holes_in(span, apertures, from, to, len, per_metre, tol, &mut notices)
                } else {
                    Vec::new()
                };
                out.push(Element::Mass {
                    span: *span,
                    height,
                    holes,
                });
            }
        }
    }
    (out, notices)
}

#[allow(clippy::too_many_arguments)]
fn holes_in(
    span: &Span,
    apertures: &[SectionAperture],
    from: [f64; 2],
    to: [f64; 2],
    len: f64,
    per_metre: f64,
    tol: f64,
    notices: &mut Vec<Notice>,
) -> Vec<Hole> {
    let _ = per_metre;
    let mut holes = Vec::new();
    for aperture in apertures {
        let Some((t0, t1)) = clip_to_convex(from, to, &aperture.ring) else {
            continue;
        };
        let (start, end) = ((t0 * len).max(span.from), (t1 * len).min(span.to));
        if end - start <= tol {
            continue;
        }
        let bottom_m = match aperture.kind {
            ApertureKind::Finestra => aperture.sill_m,
            ApertureKind::Porta | ApertureKind::Portafinestra => Some(0.0),
        };
        let hole = Hole {
            code: aperture.code.clone(),
            from: start,
            to: end,
            bottom_m,
            height_m: aperture.height_m,
        };
        if !hole.is_drawable() {
            let reason = if aperture.height_m.is_none() {
                "altezza dell'apertura n.d."
            } else {
                "davanzale n.d."
            };
            notices.push(Notice::SezioneEsclusa {
                what: format!("vano di {}", aperture.code),
                reason: reason.to_string(),
            });
        }
        holes.push(hole);
    }
    holes.sort_by(|a, b| a.from.total_cmp(&b.from));
    holes
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

    // ------------------------------------------------ altezze e aperture

    use crate::bim::aperture::{aperture_polyline, Aperture, ApertureState};
    use crate::bim::height::Height;

    fn given(value: f64) -> Height {
        Height {
            value_m: Some(value),
            state: HeightState::Inserita,
            note: None,
        }
    }

    fn inherited(value: f64) -> Height {
        Height {
            value_m: Some(value),
            state: HeightState::Ereditata,
            note: None,
        }
    }

    fn ring_of(a: &Aperture) -> Vec<[f64; 2]> {
        let EntityType::LwPolyline(p) = aperture_polyline(a).unwrap() else {
            panic!("polilinea")
        };
        p.vertices.iter().map(|v| [v.location.x, v.location.y]).collect()
    }

    fn door_aperture(height: Option<f64>) -> SectionAperture {
        let a = Aperture {
            kind: ApertureKind::Porta,
            state: ApertureState::Esistente,
            axis: ([4.05, 1.0], [4.05, 1.9]),
            thickness: 0.10,
            height,
            sill: None,
            threshold: true,
            lintel: false,
            bearing: None,
        };
        SectionAperture {
            code: "P1".into(),
            kind: a.kind,
            ring: ring_of(&a),
            height_m: height,
            sill_m: None,
        }
    }

    fn case_a_profile() -> Vec<Span> {
        let (faces, rooms) = plan(case_a(Form::Continua, M));
        profile(&faces, &rooms, [-1.0, 1.5], [9.0, 1.5], TOL)
    }

    use codec::EntityType;

    /// Il poché ha il massimo delle altezze dei locali che tocca: il muro
    /// d'ambito quella del suo locale, il tramezzo la maggiore dei due; un
    /// locale ereditato rende la massa ereditata. I locali hanno la loro.
    #[test]
    fn mass_takes_the_highest_of_the_rooms_it_touches() {
        let spans = case_a_profile();
        let rooms = [("A".to_string(), given(2.7)), ("B".to_string(), inherited(3.0))];
        let (elements, notices) = elements(&spans, &rooms, &[], [-1.0, 1.5], [9.0, 1.5], 1.0, TOL);
        assert!(notices.is_empty(), "{notices:?}");
        let heights: Vec<(f64, HeightState)> = elements
            .iter()
            .map(|e| (e.height().value_m.unwrap(), e.height().state))
            .collect();
        assert_eq!(
            heights,
            [
                (2.7, HeightState::Inserita),   // muro ovest: solo A
                (2.7, HeightState::Inserita),   // A
                (3.0, HeightState::Ereditata),  // tramezzo: max(2,7; 3,0), B ereditata
                (3.0, HeightState::Ereditata),  // B
                (3.0, HeightState::Ereditata),  // muro est: solo B
            ]
        );
    }

    /// Un locale senza altezza: l'elemento c'è con `n.d.`, e i muri che lo
    /// toccano non si disegnano; ogni cosa esclusa è un avviso. Mai un ripiego.
    #[test]
    fn a_room_without_height_excludes_the_walls_around_it() {
        let spans = case_a_profile();
        let rooms = [
            ("A".to_string(), given(2.7)),
            ("B".to_string(), Height::missing(None)),
        ];
        let (elements, notices) = elements(&spans, &rooms, &[], [-1.0, 1.5], [9.0, 1.5], 1.0, TOL);
        let missing: Vec<bool> = elements.iter().map(|e| e.height().value_m.is_none()).collect();
        assert_eq!(missing, [false, false, true, true, true]);
        let texts: Vec<String> = notices.iter().map(Notice::text).collect();
        assert!(texts.iter().any(|t| t.contains("altezza del locale B")), "{texts:?}");
        assert_eq!(
            texts.iter().filter(|t| t.contains("muro a")).count(),
            2,
            "il tramezzo e il muro est: {texts:?}"
        );
    }

    /// La porta del tramezzo buca il poché: dal pavimento a 2,10 m, larga 0,10
    /// lungo il taglio (lo spessore del muro).
    #[test]
    fn a_door_makes_a_hole_in_the_mass() {
        let (faces, rooms) = plan(case_a(Form::Continua, M));
        let spans = profile(&faces, &rooms, [2.0, 1.5], [6.0, 1.5], TOL);
        let heights = [("A".to_string(), given(2.7)), ("B".to_string(), given(2.7))];
        let (elements, notices) = elements(
            &spans,
            &heights,
            &[door_aperture(Some(2.1))],
            [2.0, 1.5],
            [6.0, 1.5],
            1.0,
            TOL,
        );
        assert!(notices.is_empty(), "{notices:?}");
        let Element::Mass { holes, span, .. } = &elements[1] else {
            panic!("{elements:?}")
        };
        let [hole] = &holes[..] else {
            panic!("{holes:?}")
        };
        assert_eq!(hole.code, "P1");
        near(hole.from, span.from, "inizio");
        near(hole.to - hole.from, 0.1, "spessore");
        assert_eq!((hole.bottom_m, hole.height_m), (Some(0.0), Some(2.1)));
        assert!(hole.is_drawable());
    }

    /// Una finestra parte dal davanzale; senza altezza o davanzale il vano
    /// non si disegna e c'è l'avviso. Fuori dal taglio non buca nulla.
    #[test]
    fn windows_start_at_the_sill_and_missing_data_is_a_notice() {
        let (faces, rooms) = plan(case_a(Form::Continua, M));
        let spans = profile(&faces, &rooms, [2.0, 1.5], [6.0, 1.5], TOL);
        let heights = [("A".to_string(), given(2.7)), ("B".to_string(), given(2.7))];
        let window = |height, sill| SectionAperture {
            kind: ApertureKind::Finestra,
            height_m: height,
            sill_m: sill,
            ..door_aperture(Some(1.0))
        };
        let run = |aperture: SectionAperture| {
            elements(&spans, &heights, &[aperture], [2.0, 1.5], [6.0, 1.5], 1.0, TOL)
        };
        let (elements_ok, notices) = run(window(Some(1.4), Some(0.9)));
        assert!(notices.is_empty());
        let Element::Mass { holes, .. } = &elements_ok[1] else { panic!() };
        assert_eq!((holes[0].bottom_m, holes[0].height_m), (Some(0.9), Some(1.4)));

        for (height, sill, why) in [
            (None, Some(0.9), "altezza dell'apertura n.d."),
            (Some(1.4), None, "davanzale n.d."),
        ] {
            let (elements_nd, notices) = run(window(height, sill));
            let Element::Mass { holes, .. } = &elements_nd[1] else { panic!() };
            assert!(!holes[0].is_drawable());
            let texts: Vec<String> = notices.iter().map(Notice::text).collect();
            assert!(texts.iter().any(|t| t.contains("vano di P1") && t.contains(why)), "{texts:?}");
        }

        // Un taglio che non attraversa il vano (y = 0,5) non ha buchi.
        let low = profile(&faces, &rooms, [2.0, 0.5], [6.0, 0.5], TOL);
        let (elements_low, _) = elements(
            &low,
            &heights,
            &[door_aperture(Some(2.1))],
            [2.0, 0.5],
            [6.0, 0.5],
            1.0,
            TOL,
        );
        let Element::Mass { holes, .. } = &elements_low[1] else { panic!() };
        assert!(holes.is_empty());
    }

    /// Un muro che non tocca nessun locale non ha altezza.
    #[test]
    fn a_mass_touching_no_room_has_no_height() {
        let spans = vec![Span {
            from: 0.0,
            to: 0.3,
            kind: SpanKind::Mass,
        }];
        let (elements, notices) = elements(&spans, &[], &[], [0.0, 0.0], [0.3, 0.0], 1.0, TOL);
        assert_eq!(elements[0].height().value_m, None);
        assert_eq!(notices.len(), 1);
        assert!(notices[0].text().contains("nessun locale accanto"));
    }
}
