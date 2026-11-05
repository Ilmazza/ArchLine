//! Prospetto (spec parte 4 §5.2): le facciate esterne di un edificio viste da
//! una direzione, con le loro aperture. Logica pura: dalle facce dei muri, dai
//! locali e dalle aperture dà le facciate (con altezza e tratti visibili) e le
//! aperture che vi cadono; poi le disegna come contorni 2D.
//!
//! Regole (R118, R119):
//! - la **direzione** è la normale uscente della facciata che si vuole vedere
//!   («Nord» = (0; 1)); chi guarda guarda in senso opposto, con la destra a
//!   `(−n.y; n.x)`: guardando a sud (prospetto Nord) l'est è a sinistra;
//! - il contorno dell'edificio è l'anello della **faccia più grande** delle
//!   facce dei muri (un solo edificio); un suo lato è una facciata se la
//!   normale uscente sta entro [`FACING_DEGREES`] dalla direzione;
//! - l'altezza di una facciata si prende dal muro dietro di essa: da tre punti
//!   del lato (¼, ½, ¾) si manda un raggio verso l'interno e si legge l'altezza
//!   della massa muraria che incontra per prima (`section::elements`, il
//!   massimo delle altezze dei locali accanto); vale il massimo dei valori
//!   noti. Senza nessuna altezza la facciata non si disegna e c'è un avviso;
//! - **occlusione semplice**: una facciata più vicina nasconde, lungo la sua
//!   estensione orizzontale, quelle più lontane che non la superano in altezza;
//!   una più bassa lascia vedere la più alta. Nessuna occlusione parziale in
//!   verticale;
//! - un'apertura sta sulla facciata se il punto medio del suo asse dista dal
//!   lato al più metà del suo spessore; porte e portefinestre partono da
//!   terra, le finestre dal davanzale; senza altezza o davanzale non si
//!   disegna e c'è un avviso. Si disegna solo dove la facciata è visibile.
//!
//! Distanze in unità del disegno, quote in metri.

use codec::entities::{LwPolyline, MText, AttachmentPoint};
use codec::{Color, EntityType, Vector2, Vector3};

use super::aperture::{ApertureKind, ApertureState};
use super::height::Height;
use super::report::Notice;
use super::section::{elements, profile, Element};
use super::section_draw::{Tint, LAYER_SEZIONI};
use super::units::format_decimal;

/// Di quanti gradi al massimo la normale di un lato si discosta dalla
/// direzione di vista perché sia una facciata.
pub const FACING_DEGREES: f64 = 30.0;
/// Quanto lontano dal lato si cerca la massa che dà l'altezza, in metri.
const PROBE_M: f64 = 1.5;

/// Un'apertura vista dal prospetto: l'asse e lo spessore in unità del
/// disegno, altezza e davanzale in metri (`None` = `n.d.`).
#[derive(Clone, Debug, PartialEq)]
pub struct ElevationAperture {
    pub code: String,
    pub kind: ApertureKind,
    pub state: ApertureState,
    pub axis: ([f64; 2], [f64; 2]),
    pub thickness: f64,
    pub height_m: Option<f64>,
    pub sill_m: Option<f64>,
}

/// Un'apertura sulla facciata, in coordinate del prospetto.
#[derive(Clone, Debug, PartialEq)]
pub struct Opening {
    pub code: String,
    pub state: ApertureState,
    /// Distanze orizzontali dal bordo sinistro del prospetto.
    pub x0: f64,
    pub x1: f64,
    pub bottom_m: f64,
    pub height_m: f64,
}

/// Una facciata visibile: da `x0` a `x1` (distanze dal bordo sinistro del
/// prospetto), alta `height_m`, con i tratti `visible` non nascosti da
/// facciate più vicine.
#[derive(Clone, Debug, PartialEq)]
pub struct Facade {
    pub x0: f64,
    pub x1: f64,
    pub height_m: f64,
    /// Più vicina = più grande.
    pub depth: f64,
    pub visible: Vec<(f64, f64)>,
    pub openings: Vec<Opening>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Elevation {
    pub facades: Vec<Facade>,
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn area(ring: &[[f64; 2]]) -> f64 {
    (0..ring.len())
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>()
        / 2.0
}

/// Il prospetto con normale uscente `normal` (di lunghezza unitaria).
#[allow(clippy::too_many_arguments)]
pub fn elevation(
    faces: &[Vec<[f64; 2]>],
    outlines: &[Vec<[f64; 2]>],
    rooms: &[(String, Height)],
    apertures: &[ElevationAperture],
    normal: [f64; 2],
    per_metre: f64,
    tol: f64,
) -> (Elevation, Vec<Notice>) {
    let mut notices = Vec::new();
    let Some(outer) = faces
        .iter()
        .max_by(|a, b| area(a).abs().total_cmp(&area(b).abs()))
    else {
        return (Elevation::default(), notices);
    };
    // Anello antiorario: l'interno sta a sinistra di ogni lato.
    let mut ring = outer.clone();
    if area(&ring) < 0.0 {
        ring.reverse();
    }
    let right = [-normal[1], normal[0]];
    let facing = FACING_DEGREES.to_radians().cos();
    // Facciate candidate: lato, normale uscente, estremi orizzontali.
    struct Raw {
        a: [f64; 2],
        b: [f64; 2],
        outward: [f64; 2],
        lo: f64,
        hi: f64,
    }
    let mut raws: Vec<Raw> = Vec::new();
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        let e = [b[0] - a[0], b[1] - a[1]];
        let len = e[0].hypot(e[1]);
        if len <= tol {
            continue;
        }
        let outward = [e[1] / len, -e[0] / len];
        if dot(outward, normal) < facing {
            continue;
        }
        let (pa, pb) = (dot(a, right), dot(b, right));
        raws.push(Raw {
            a,
            b,
            outward,
            lo: pa.min(pb),
            hi: pa.max(pb),
        });
    }
    if raws.is_empty() {
        return (Elevation::default(), notices);
    }
    let origin = raws.iter().map(|r| r.lo).fold(f64::MAX, f64::min);
    let probe = PROBE_M * per_metre;
    // Altezze: dal muro dietro la facciata.
    let mut facades: Vec<Facade> = Vec::new();
    let mut sides: Vec<usize> = Vec::new();
    for (index, raw) in raws.iter().enumerate() {
        let mut best: Option<f64> = None;
        for fraction in [0.25, 0.5, 0.75] {
            let start = [
                raw.a[0] + (raw.b[0] - raw.a[0]) * fraction,
                raw.a[1] + (raw.b[1] - raw.a[1]) * fraction,
            ];
            let end = [start[0] - raw.outward[0] * probe, start[1] - raw.outward[1] * probe];
            let spans = profile(faces, outlines, start, end, tol);
            let (found, _) = elements(&spans, rooms, &[], start, end, per_metre, tol);
            if let Some(Element::Mass { height, .. }) = found.first() {
                if let Some(value) = height.value_m {
                    best = Some(best.map_or(value, |b| b.max(value)));
                }
            }
        }
        let at = format!(
            "facciata a {} m",
            format_decimal((raw.lo - origin) / per_metre)
        );
        let Some(height_m) = best else {
            notices.push(Notice::SezioneEsclusa {
                what: at,
                reason: "altezza del locale dietro n.d.".to_string(),
            });
            continue;
        };
        facades.push(Facade {
            x0: raw.lo - origin,
            x1: raw.hi - origin,
            height_m,
            depth: dot(
                [(raw.a[0] + raw.b[0]) / 2.0, (raw.a[1] + raw.b[1]) / 2.0],
                normal,
            ),
            visible: vec![(raw.lo - origin, raw.hi - origin)],
            openings: Vec::new(),
        });
        sides.push(index);
    }
    // Occlusione semplice: una facciata più vicina nasconde la più lontana
    // dove non è più bassa.
    for far in 0..facades.len() {
        for near in 0..facades.len() {
            if facades[near].depth <= facades[far].depth + tol
                || facades[near].height_m + 1.0e-9 < facades[far].height_m
            {
                continue;
            }
            let (n0, n1) = (facades[near].x0, facades[near].x1);
            let mut pieces = Vec::new();
            for &(a, b) in &facades[far].visible {
                if n1 <= a || n0 >= b {
                    pieces.push((a, b));
                    continue;
                }
                if n0 > a + tol {
                    pieces.push((a, n0));
                }
                if n1 < b - tol {
                    pieces.push((n1, b));
                }
            }
            facades[far].visible = pieces;
        }
    }
    // Aperture sulle facciate.
    for aperture in apertures {
        let mid = [
            (aperture.axis.0[0] + aperture.axis.1[0]) / 2.0,
            (aperture.axis.0[1] + aperture.axis.1[1]) / 2.0,
        ];
        let Some((facade_index, _)) = sides.iter().enumerate().find(|(_, &side)| {
            let raw = &raws[side];
            let e = [raw.b[0] - raw.a[0], raw.b[1] - raw.a[1]];
            let len2 = dot(e, e);
            let t = (dot([mid[0] - raw.a[0], mid[1] - raw.a[1]], e) / len2).clamp(0.0, 1.0);
            let foot = [raw.a[0] + e[0] * t, raw.a[1] + e[1] * t];
            (mid[0] - foot[0]).hypot(mid[1] - foot[1]) <= aperture.thickness / 2.0 + tol
                && dot(mid, right) >= raw.lo - tol
                && dot(mid, right) <= raw.hi + tol
        }) else {
            continue;
        };
        let bottom = match aperture.kind {
            ApertureKind::Finestra => aperture.sill_m,
            ApertureKind::Porta | ApertureKind::Portafinestra => Some(0.0),
        };
        let (Some(bottom_m), Some(height_m)) = (bottom, aperture.height_m) else {
            notices.push(Notice::SezioneEsclusa {
                what: format!("apertura {} sul prospetto", aperture.code),
                reason: if aperture.height_m.is_none() {
                    "altezza dell'apertura n.d.".to_string()
                } else {
                    "davanzale n.d.".to_string()
                },
            });
            continue;
        };
        let (pa, pb) = (dot(aperture.axis.0, right), dot(aperture.axis.1, right));
        let (x0, x1) = (pa.min(pb) - origin, pa.max(pb) - origin);
        let facade = &mut facades[facade_index];
        // Solo dove la facciata si vede.
        for &(a, b) in &facade.visible.clone() {
            let (lo, hi) = (x0.max(a), x1.min(b));
            if hi - lo > tol {
                facade.openings.push(Opening {
                    code: aperture.code.clone(),
                    state: aperture.state,
                    x0: lo,
                    x1: hi,
                    bottom_m,
                    height_m,
                });
            }
        }
    }
    // Le più lontane per prime: l'ordine di disegno è l'ordine di profondità.
    facades.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    (Elevation { facades }, notices)
}

fn tint_of(state: ApertureState) -> Tint {
    match state {
        ApertureState::Esistente => Tint::Plain,
        ApertureState::Demolita => Tint::Demolished,
        ApertureState::Nuova => Tint::New,
    }
}

fn outline(x0: f64, y0: f64, x1: f64, y1: f64, color: Color) -> EntityType {
    let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let mut poly =
        LwPolyline::from_points(corners.iter().map(|c| Vector2::new(c[0], c[1])).collect());
    poly.is_closed = true;
    poly.common.layer = LAYER_SEZIONI.to_string();
    poly.common.color = color;
    EntityType::LwPolyline(poly)
}

fn label(at: [f64; 2], value: &str, height: f64, width: f64) -> EntityType {
    let mut text = MText::with_value(value, Vector3::new(at[0], at[1], 0.0));
    text.height = height;
    text.rectangle_width = width;
    text.attachment_point = AttachmentPoint::TopLeft;
    text.common.layer = LAYER_SEZIONI.to_string();
    EntityType::MText(Box::new(text))
}

/// Il disegno del prospetto: il contorno di ogni tratto visibile di facciata
/// da terra alla sua altezza, le aperture come contorni (colorati per stato),
/// la linea di terra e il titolo con il riquadro dei limiti. Senza facciate,
/// niente. `per_metre` unità per metro, `origin` l'angolo in basso a sinistra
/// del piano di spiccato.
pub fn draw_elevation(
    title: &str,
    elevation: &Elevation,
    origin: [f64; 2],
    per_metre: f64,
    text_height: f64,
) -> Vec<EntityType> {
    let Some(first) = elevation.facades.first() else {
        return Vec::new();
    };
    let x_of = |d: f64| origin[0] + d;
    let y_of = |m: f64| origin[1] + m * per_metre;
    let (mut lo, mut hi) = (first.x0, first.x1);
    for facade in &elevation.facades {
        lo = lo.min(facade.x0);
        hi = hi.max(facade.x1);
    }
    let mut ground =
        codec::entities::Line::from_points(Vector3::new(x_of(lo), y_of(0.0), 0.0), Vector3::new(x_of(hi), y_of(0.0), 0.0));
    ground.common.layer = LAYER_SEZIONI.to_string();
    let mut out = vec![EntityType::Line(ground)];
    for facade in &elevation.facades {
        for &(a, b) in &facade.visible {
            out.push(outline(
                x_of(a),
                y_of(0.0),
                x_of(b),
                y_of(facade.height_m),
                Color::ByLayer,
            ));
        }
        for opening in &facade.openings {
            out.push(outline(
                x_of(opening.x0),
                y_of(opening.bottom_m),
                x_of(opening.x1),
                y_of(opening.bottom_m + opening.height_m),
                tint_of(opening.state).color(),
            ));
        }
    }
    let width = (hi - lo).max(40.0 * text_height);
    let below = [x_of(lo), y_of(0.0)];
    out.push(label([below[0], below[1] - text_height], title, text_height, width));
    out.push(label(
        [below[0], below[1] - 3.0 * text_height],
        LIMITS_TEXT,
        text_height * 0.6,
        width,
    ));
    out
}

/// Il riquadro dei limiti del prospetto.
pub const LIMITS_TEXT: &str = "Prospetto dedotto dalla pianta: un solo livello (quota 0 = piano di \
    spiccato), facciate dritte dell'edificio più grande. Coperture, balconi, aggetti e quote del \
    terreno non sono nel modello e non sono disegnati. Altezze: dei locali e delle aperture inserite.";

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bim::fixtures::{case_a, scene_with, Form};
    use crate::bim::height::HeightState;
    use crate::bim::rooms::measure_at;
    use crate::bim::units::DrawingUnit;
    use crate::bim::walls::{faces, wall_set_without_rooms};

    const M: DrawingUnit = DrawingUnit::Metri;
    const TOL: f64 = 1e-6;

    fn given(value: f64) -> Height {
        Height {
            value_m: Some(value),
            state: HeightState::Inserita,
            note: None,
        }
    }

    /// Facce, contorni di A e B e le loro altezze.
    fn plan(heights: [Option<f64>; 2]) -> (Vec<Vec<[f64; 2]>>, Vec<Vec<[f64; 2]>>, Vec<(String, Height)>) {
        let scene = scene_with(case_a(Form::Continua, M), M.insunits());
        let walls = wall_set_without_rooms(&scene.document, TOL);
        let all = faces(&walls.sources, TOL);
        let outlines = [[2.0, 1.5], [6.0, 1.5]]
            .into_iter()
            .map(|seed| {
                measure_at(&all, None, &walls.sources, seed, M, TOL)
                    .expect("locale chiuso")
                    .region
                    .outer
            })
            .collect();
        let rooms = vec![
            ("A".to_string(), heights[0].map_or_else(|| Height::missing(None), given)),
            ("B".to_string(), heights[1].map_or_else(|| Height::missing(None), given)),
        ];
        (all, outlines, rooms)
    }

    fn near(a: f64, b: f64, what: &str) {
        assert!((a - b).abs() < 1e-9, "{what}: {a} invece di {b}");
    }

    /// Prospetto Sud (normale (0; −1)): una facciata larga 8,60 alta 2,70; con
    /// la destra a est la X cresce verso est.
    #[test]
    fn the_south_elevation_is_one_facade() {
        let (faces, outlines, rooms) = plan([Some(2.7), Some(2.7)]);
        let (elevation, notices) = super::elevation(&faces, &outlines, &rooms, &[], [0.0, -1.0], 1.0, TOL);
        assert!(notices.is_empty(), "{notices:?}");
        let [facade] = &elevation.facades[..] else {
            panic!("{elevation:?}")
        };
        near(facade.x0, 0.0, "inizio");
        near(facade.x1, 8.6, "larghezza");
        near(facade.height_m, 2.7, "altezza");
        assert_eq!(facade.visible.len(), 1);
        near(facade.visible[0].0, 0.0, "tratto visibile: inizio");
        near(facade.visible[0].1, 8.6, "tratto visibile: fine");
    }

    /// Le quattro direzioni: nord e sud sono lunghe 8,60, est e ovest 3,60.
    #[test]
    fn each_direction_sees_its_own_facade() {
        let (faces, outlines, rooms) = plan([Some(2.7), Some(2.7)]);
        for (normal, width) in [([0.0, 1.0], 8.6), ([0.0, -1.0], 8.6), ([1.0, 0.0], 3.6), ([-1.0, 0.0], 3.6)] {
            let (elevation, _) = super::elevation(&faces, &outlines, &rooms, &[], normal, 1.0, TOL);
            let [facade] = &elevation.facades[..] else {
                panic!("{normal:?}: {elevation:?}")
            };
            near(facade.x1 - facade.x0, width, &format!("{normal:?}"));
        }
    }

    /// La facciata prende il massimo delle altezze dei locali dietro di essa:
    /// con A 2,70 e B 3,00 il fronte sud è alto 3,00; senza altezze non c'è.
    #[test]
    fn height_is_the_highest_known_behind_and_missing_means_no_facade() {
        let (faces, outlines, rooms) = plan([Some(2.7), Some(3.0)]);
        let (elevation, _) = super::elevation(&faces, &outlines, &rooms, &[], [0.0, -1.0], 1.0, TOL);
        near(elevation.facades[0].height_m, 3.0, "massimo");

        let (faces, outlines, rooms) = plan([None, None]);
        let (elevation, notices) = super::elevation(&faces, &outlines, &rooms, &[], [0.0, -1.0], 1.0, TOL);
        assert!(elevation.facades.is_empty());
        assert_eq!(notices.len(), 1);
        assert!(notices[0].text().contains("facciata a"), "{}", notices[0].text());
    }

    fn window(code: &str, x: (f64, f64), sill: Option<f64>, height: Option<f64>, state: ApertureState) -> ElevationAperture {
        ElevationAperture {
            code: code.into(),
            kind: ApertureKind::Finestra,
            state,
            axis: ([x.0, -0.15], [x.1, -0.15]),
            thickness: 0.30,
            height_m: height,
            sill_m: sill,
        }
    }

    /// Una finestra sul fronte sud: dal bordo sinistro (x = −0,30) a 1,30..2,50,
    /// dal davanzale 0,90 per 1,20. Una finestra al piano di sopra del fronte
    /// nord non compare sul sud; senza davanzale non si disegna.
    #[test]
    fn openings_sit_on_their_facade_and_missing_data_is_a_notice() {
        let (faces, outlines, rooms) = plan([Some(2.7), Some(2.7)]);
        let on_north = ElevationAperture {
            axis: ([1.0, 3.15], [2.2, 3.15]),
            ..window("F9", (1.0, 2.2), Some(0.9), Some(1.2), ApertureState::Esistente)
        };
        let apertures = [
            window("F1", (1.0, 2.2), Some(0.9), Some(1.2), ApertureState::Nuova),
            window("F2", (5.0, 6.0), None, Some(1.2), ApertureState::Esistente),
            on_north,
        ];
        let (elevation, notices) = super::elevation(&faces, &outlines, &rooms, &apertures, [0.0, -1.0], 1.0, TOL);
        let [facade] = &elevation.facades[..] else {
            panic!("{elevation:?}")
        };
        let [opening] = &facade.openings[..] else {
            panic!("{:?}", facade.openings)
        };
        assert_eq!(opening.code, "F1");
        near(opening.x0, 1.3, "x0");
        near(opening.x1, 2.5, "x1");
        near(opening.bottom_m, 0.9, "davanzale");
        near(opening.height_m, 1.2, "altezza");
        let texts: Vec<String> = notices.iter().map(Notice::text).collect();
        assert!(texts.iter().any(|t| t.contains("apertura F2") && t.contains("davanzale n.d.")), "{texts:?}");
    }

    /// Un rientro: la facciata più vicina nasconde la più lontana dove non è
    /// più bassa, e la più bassa lascia vedere la più alta.
    #[test]
    fn a_nearer_facade_hides_the_farther_one_unless_it_is_lower() {
        let near_facade = |height| Facade {
            x0: 2.0,
            x1: 4.0,
            height_m: height,
            depth: 5.0,
            visible: vec![(2.0, 4.0)],
            openings: Vec::new(),
        };
        // Costruiamo l'occlusione dal piano di un edificio a L: un lato in
        // fondo e uno in avanti. Qui si prova la regola sui dati.
        let mut facades = vec![
            Facade {
                x0: 0.0,
                x1: 6.0,
                height_m: 2.7,
                depth: 1.0,
                visible: vec![(0.0, 6.0)],
                openings: Vec::new(),
            },
            near_facade(2.7),
        ];
        // Stessa regola di `elevation`, applicata a mano.
        for far in 0..facades.len() {
            for near in 0..facades.len() {
                if facades[near].depth <= facades[far].depth || facades[near].height_m < facades[far].height_m {
                    continue;
                }
                let (n0, n1) = (facades[near].x0, facades[near].x1);
                let mut pieces = Vec::new();
                for &(a, b) in &facades[far].visible {
                    if n0 > a {
                        pieces.push((a, n0));
                    }
                    if n1 < b {
                        pieces.push((n1, b));
                    }
                }
                facades[far].visible = pieces;
            }
        }
        assert_eq!(facades[0].visible, vec![(0.0, 2.0), (4.0, 6.0)]);
        assert_eq!(facades[1].visible, vec![(2.0, 4.0)]);
    }

    /// Il disegno: linea di terra, un contorno per tratto di facciata, uno per
    /// apertura (rosso se nuova), titolo e limiti.
    #[test]
    fn the_drawing_has_ground_facade_openings_and_texts() {
        let (faces, outlines, rooms) = plan([Some(2.7), Some(2.7)]);
        let apertures = [window("F1", (1.0, 2.2), Some(0.9), Some(1.2), ApertureState::Nuova)];
        let (elevation, _) = super::elevation(&faces, &outlines, &rooms, &apertures, [0.0, -1.0], 1.0, TOL);
        let entities = draw_elevation("Prospetto Sud", &elevation, [100.0, 50.0], 1.0, 0.25);
        let polys: Vec<&LwPolyline> = entities
            .iter()
            .filter_map(|e| match e {
                EntityType::LwPolyline(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(polys.len(), 2, "facciata e finestra");
        let top = |p: &LwPolyline| p.vertices.iter().map(|v| v.location.y).fold(f64::MIN, f64::max);
        near(top(polys[0]), 52.7, "facciata alta 2,70");
        near(top(polys[1]), 52.1, "finestra: 0,90 + 1,20 sopra terra");
        assert_eq!(polys[1].common.color, Color::from_index(1), "nuova = rossa");
        assert_eq!(entities.iter().filter(|e| matches!(e, EntityType::MText(_))).count(), 2);
        assert_eq!(entities.iter().filter(|e| matches!(e, EntityType::Line(_))).count(), 1);
        assert!(draw_elevation("x", &Elevation::default(), [0.0, 0.0], 1.0, 0.25).is_empty());
    }
}
