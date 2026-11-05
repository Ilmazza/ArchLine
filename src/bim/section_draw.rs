//! Il disegno di una sezione (spec parte 4 §5.1): da [`Element`] a entità 2D
//! normali (`LINE`, `LWPOLYLINE`, `HATCH` solide, `MTEXT`) sul layer
//! `AL-SEZIONI`, in un piano che parte da un punto d'inserimento dato. Logica
//! pura, nessun XDATA: è un'uscita, non un modello.
//!
//! Sul foglio la distanza lungo il taglio è la X e la quota (m × `per_metre`)
//! la Y, a partire da `origin`. Il piano di spiccato è a quota 0 e vale per
//! tutta la sezione. Ogni elemento disegnato ha altezza nota (R111): le masse
//! senza altezza non ci sono e le elencano gli avvisi di [`super::section`].
//!
//! - **massa muraria** (poché): un rettangolo da terra alla sua altezza, con il
//!   contorno e una campitura solida; dove un'apertura la buca, la massa si
//!   divide in pezzi (sopra il vano, e sotto se il vano parte da un davanzale),
//!   mai un riempimento che copre il vano. Un vano che non si disegna
//!   (`n.d.`) lascia la massa intera;
//! - **locale**: la linea del soffitto alla sua altezza, se nota;
//! - la linea del pavimento a quota 0 su tutta la sezione;
//! - il titolo («Sezione A-A») e il riquadro dei **limiti**: un solo livello,
//!   nessun solaio, tetto, scala né quota del terreno nel modello (spec §2).
//!
//! Colori (decisione di Mauro, parte 1): le parti demolite in giallo (ACI 2),
//! le nuove in rosso (ACI 1), le altre `ByLayer`.

use codec::entities::hatch::BoundaryPathFlags;
use codec::entities::{
    AttachmentPoint, BoundaryEdge, BoundaryPath, Hatch, Line, LwPolyline, MText, PolylineEdge,
};
use codec::{Color, EntityType, Vector2, Vector3};

use super::section::{Element, Hole, Span};
use super::{ACI_GIALLO, ACI_ROSSO};

/// Il layer delle sezioni.
pub const LAYER_SEZIONI: &str = "AL-SEZIONI";

/// Il riquadro dei limiti, sotto il disegno.
pub const LIMITS_TEXT: &str = "Sezione dedotta dalla pianta: un solo livello (quota 0 = piano di \
    spiccato). Solai, coperture, scale e quote del terreno non sono nel modello e non sono \
    disegnati. Altezze: dei locali e delle aperture inserite.";

/// Lo stato di un elemento nella tavola di demolizioni e costruzioni.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    Plain,
    Demolished,
    New,
}

impl Tint {
    fn color(self) -> Color {
        match self {
            Self::Plain => Color::ByLayer,
            Self::Demolished => Color::from_index(ACI_GIALLO),
            Self::New => Color::from_index(ACI_ROSSO),
        }
    }
}

struct Frame {
    origin: [f64; 2],
    per_metre: f64,
}

impl Frame {
    fn at(&self, distance: f64, metres: f64) -> [f64; 2] {
        [
            self.origin[0] + distance,
            self.origin[1] + metres * self.per_metre,
        ]
    }
}

fn line(a: [f64; 2], b: [f64; 2], color: Color) -> EntityType {
    let mut line = Line::from_points(Vector3::new(a[0], a[1], 0.0), Vector3::new(b[0], b[1], 0.0));
    line.common.layer = LAYER_SEZIONI.to_string();
    line.common.color = color;
    EntityType::Line(line)
}

fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64, tint: Tint) -> [EntityType; 2] {
    let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let mut outline =
        LwPolyline::from_points(corners.iter().map(|c| Vector2::new(c[0], c[1])).collect());
    outline.is_closed = true;
    outline.common.layer = LAYER_SEZIONI.to_string();
    outline.common.color = tint.color();
    let mut path = BoundaryPath::with_flags(BoundaryPathFlags::from_bits(
        BoundaryPathFlags::OUTERMOST.bits() | BoundaryPathFlags::EXTERNAL.bits(),
    ));
    path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
        corners.iter().map(|c| Vector2::new(c[0], c[1])).collect(),
        true,
    )));
    let mut hatch = Hatch::new();
    hatch.paths = vec![path];
    hatch.common.layer = LAYER_SEZIONI.to_string();
    hatch.common.color = tint.color();
    [
        EntityType::LwPolyline(outline),
        EntityType::Hatch(Box::new(hatch)),
    ]
}

fn text(at: [f64; 2], value: &str, height: f64, width: f64, top_left: bool) -> EntityType {
    let mut text = MText::with_value(value, Vector3::new(at[0], at[1], 0.0));
    text.height = height;
    text.rectangle_width = width;
    text.attachment_point = if top_left {
        AttachmentPoint::TopLeft
    } else {
        AttachmentPoint::BottomLeft
    };
    text.common.layer = LAYER_SEZIONI.to_string();
    EntityType::MText(Box::new(text))
}

/// I pezzi di una massa tra `span.from` e `span.to`, alta `height` m, bucata
/// da `holes`: rettangoli `(x0, x1, y0, y1)` in metri di quota, distanze
/// invariate. Solo i vani disegnabili dividono la massa.
fn mass_pieces(span: &Span, height: f64, holes: &[Hole]) -> Vec<(f64, f64, f64, f64)> {
    let mut pieces = Vec::new();
    let mut cursor = span.from;
    let mut drawable: Vec<&Hole> = holes.iter().filter(|h| h.is_drawable()).collect();
    drawable.sort_by(|a, b| a.from.total_cmp(&b.from));
    for hole in drawable {
        let (bottom, top) = (
            hole.bottom_m.unwrap_or(0.0),
            hole.bottom_m.unwrap_or(0.0) + hole.height_m.unwrap_or(0.0),
        );
        if hole.from > cursor {
            pieces.push((cursor, hole.from, 0.0, height));
        }
        if bottom > 0.0 {
            pieces.push((hole.from, hole.to, 0.0, bottom.min(height)));
        }
        if top < height {
            pieces.push((hole.from, hole.to, top.max(0.0), height));
        }
        cursor = cursor.max(hole.to);
    }
    if cursor < span.to {
        pieces.push((cursor, span.to, 0.0, height));
    }
    pieces
}

/// Le entità della sezione `title` («Sezione A-A»): `elements` con il loro
/// colore, a partire da `origin` (in unità del disegno), `per_metre` unità per
/// metro, testo alto `text_height`. Senza elementi, niente.
pub fn draw_section(
    title: &str,
    elements: &[(Element, Tint)],
    origin: [f64; 2],
    per_metre: f64,
    text_height: f64,
) -> Vec<EntityType> {
    let Some(first) = elements.first() else {
        return Vec::new();
    };
    let frame = Frame { origin, per_metre };
    let start = first.0.span().from;
    let end = elements.last().map_or(start, |e| e.0.span().to);
    let mut out = vec![line(
        frame.at(start, 0.0),
        frame.at(end, 0.0),
        Color::ByLayer,
    )];
    for (element, tint) in elements {
        let Some(height) = element.height().value_m else {
            continue;
        };
        match element {
            Element::Room { span, .. } => {
                out.push(line(
                    frame.at(span.from, height),
                    frame.at(span.to, height),
                    Color::ByLayer,
                ));
            }
            Element::Mass { span, holes, .. } => {
                for (x0, x1, y0, y1) in mass_pieces(span, height, holes) {
                    let (a, b) = (frame.at(x0, y0), frame.at(x1, y1));
                    out.extend(rectangle(a[0], a[1], b[0], b[1], *tint));
                }
            }
        }
    }
    let below = frame.at(start, 0.0);
    // I testi vanno a capo a questa larghezza: quella della sezione, ma mai
    // meno di quaranta altezze di testo (i limiti restano su tre righe).
    let width = (end - start).max(40.0 * text_height);
    out.push(text(
        [below[0], below[1] - text_height],
        title,
        text_height,
        width,
        true,
    ));
    out.push(text(
        [below[0], below[1] - 3.0 * text_height],
        LIMITS_TEXT,
        text_height * 0.6,
        width,
        true,
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bim::height::{Height, HeightState};
    use crate::bim::section::SpanKind;

    fn given(value: f64) -> Height {
        Height {
            value_m: Some(value),
            state: HeightState::Inserita,
            note: None,
        }
    }

    fn span(from: f64, to: f64, kind: SpanKind) -> Span {
        Span { from, to, kind }
    }

    fn mass(from: f64, to: f64, height: Option<f64>, holes: Vec<Hole>) -> Element {
        Element::Mass {
            span: span(from, to, SpanKind::Mass),
            height: height.map_or_else(|| Height::missing(None), given),
            holes,
        }
    }

    fn room(from: f64, to: f64, index: usize) -> Element {
        Element::Room {
            span: span(from, to, SpanKind::Room(index)),
            index,
            height: given(2.7),
        }
    }

    /// Il caso A a y = 1,5: i tre muri, i due locali, nessuna apertura.
    fn case_a_elements() -> Vec<(Element, Tint)> {
        vec![
            (mass(0.7, 1.0, Some(2.7), vec![]), Tint::Plain),
            (room(1.0, 5.0, 0), Tint::Plain),
            (mass(5.0, 5.1, Some(2.7), vec![]), Tint::Plain),
            (room(5.1, 9.0, 1), Tint::Plain),
            (mass(9.0, 9.3, Some(2.7), vec![]), Tint::Plain),
        ]
    }

    fn boxes(entities: &[EntityType]) -> Vec<[f64; 4]> {
        entities
            .iter()
            .filter_map(|e| match e {
                EntityType::LwPolyline(p) => {
                    let xs: Vec<f64> = p.vertices.iter().map(|v| v.location.x).collect();
                    let ys: Vec<f64> = p.vertices.iter().map(|v| v.location.y).collect();
                    Some([
                        xs.iter().cloned().fold(f64::MAX, f64::min),
                        ys.iter().cloned().fold(f64::MAX, f64::min),
                        xs.iter().cloned().fold(f64::MIN, f64::max),
                        ys.iter().cloned().fold(f64::MIN, f64::max),
                    ])
                }
                _ => None,
            })
            .collect()
    }

    fn count(entities: &[EntityType], kind: &str) -> usize {
        entities
            .iter()
            .filter(|e| match (kind, e) {
                ("line", EntityType::Line(_)) => true,
                ("poly", EntityType::LwPolyline(_)) => true,
                ("hatch", EntityType::Hatch(_)) => true,
                ("text", EntityType::MText(_)) => true,
                _ => false,
            })
            .count()
    }

    /// Tre muri pieni da terra a 2,70 m (contorno e campitura), due soffitti,
    /// la linea del pavimento, il titolo e i limiti; sul foglio dall'origine.
    #[test]
    fn case_a_section_draws_walls_ceilings_floor_and_texts() {
        let entities = draw_section("Sezione A-A", &case_a_elements(), [100.0, 50.0], 1.0, 0.25);
        assert_eq!(count(&entities, "poly"), 3);
        assert_eq!(count(&entities, "hatch"), 3);
        assert_eq!(count(&entities, "line"), 3, "pavimento e due soffitti");
        assert_eq!(count(&entities, "text"), 2, "titolo e limiti");
        let mut b = boxes(&entities);
        b.sort_by(|x, y| x[0].total_cmp(&y[0]));
        let want = [[100.7, 50.0, 101.0, 52.7], [105.0, 50.0, 105.1, 52.7], [109.0, 50.0, 109.3, 52.7]];
        for (got, want) in b.iter().zip(want) {
            for (g, w) in got.iter().zip(want) {
                assert!((g - w).abs() < 1e-9, "{got:?} contro {want:?}");
            }
        }
        for e in &entities {
            assert_eq!(e.common().layer, LAYER_SEZIONI);
            assert!(e.common().extended_data.records().is_empty(), "nessun XDATA");
        }
    }

    /// Una massa senza altezza non si disegna (e i locali sì).
    #[test]
    fn a_mass_without_height_is_not_drawn() {
        let elements = vec![
            (mass(0.0, 0.3, None, vec![]), Tint::Plain),
            (room(0.3, 4.0, 0), Tint::Plain),
        ];
        let entities = draw_section("S", &elements, [0.0, 0.0], 1.0, 0.25);
        assert_eq!(count(&entities, "poly"), 0);
        assert_eq!(count(&entities, "hatch"), 0);
        assert_eq!(count(&entities, "line"), 2, "pavimento e soffitto");
    }

    /// La porta: il tramezzo è la sola parte sopra il vano, da 2,10 a 2,70.
    #[test]
    fn a_door_leaves_the_wall_only_above_the_vano() {
        let hole = Hole {
            code: "P1".into(),
            from: 5.0,
            to: 5.1,
            bottom_m: Some(0.0),
            height_m: Some(2.1),
        };
        let elements = vec![(mass(5.0, 5.1, Some(2.7), vec![hole]), Tint::Plain)];
        let entities = draw_section("S", &elements, [0.0, 0.0], 1.0, 0.25);
        let b = boxes(&entities);
        assert_eq!(b.len(), 1, "{b:?}");
        for (g, w) in b[0].iter().zip([5.0, 2.1, 5.1, 2.7]) {
            assert!((g - w).abs() < 1e-9, "{:?}", b[0]);
        }
    }

    /// Una finestra: pezzo sotto il davanzale e pezzo sopra l'architrave; un
    /// vano a metà massa lascia anche i tratti prima e dopo.
    #[test]
    fn a_window_leaves_pieces_below_and_above_and_beside() {
        let hole = Hole {
            code: "F1".into(),
            from: 1.0,
            to: 2.0,
            bottom_m: Some(0.9),
            height_m: Some(1.2),
        };
        let pieces = mass_pieces(&span(0.0, 3.0, SpanKind::Mass), 2.7, &[hole]);
        assert_eq!(
            pieces,
            vec![
                (0.0, 1.0, 0.0, 2.7),
                (1.0, 2.0, 0.0, 0.9),
                (1.0, 2.0, 2.1, 2.7),
                (2.0, 3.0, 0.0, 2.7)
            ]
        );
    }

    /// Un vano che non si disegna (davanzale n.d.) lascia la massa intera.
    #[test]
    fn an_undrawable_hole_leaves_the_mass_whole() {
        let hole = Hole {
            code: "F1".into(),
            from: 1.0,
            to: 2.0,
            bottom_m: None,
            height_m: Some(1.2),
        };
        let pieces = mass_pieces(&span(0.0, 3.0, SpanKind::Mass), 2.7, &[hole]);
        assert_eq!(pieces, vec![(0.0, 3.0, 0.0, 2.7)]);
    }

    /// Giallo per il demolito, rosso per il nuovo, `ByLayer` per il resto.
    #[test]
    fn tints_colour_the_poche() {
        let elements = vec![
            (mass(0.0, 0.3, Some(2.7), vec![]), Tint::Demolished),
            (mass(1.0, 1.3, Some(2.7), vec![]), Tint::New),
            (mass(2.0, 2.3, Some(2.7), vec![]), Tint::Plain),
        ];
        let entities = draw_section("S", &elements, [0.0, 0.0], 1.0, 0.25);
        let mut hatches: Vec<(f64, Color)> = entities
            .iter()
            .filter_map(|e| match e {
                EntityType::Hatch(h) => {
                    let EntityType::Hatch(_) = e else { return None };
                    let x = match &h.paths[0].edges[0] {
                        BoundaryEdge::Polyline(p) => p.vertices[0].x,
                        _ => return None,
                    };
                    Some((x, h.common.color.clone()))
                }
                _ => None,
            })
            .collect();
        hatches.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(hatches[0].1, Color::from_index(2));
        assert_eq!(hatches[1].1, Color::from_index(1));
        assert_eq!(hatches[2].1, Color::ByLayer);
    }

    #[test]
    fn nothing_to_draw_gives_nothing() {
        assert!(draw_section("S", &[], [0.0, 0.0], 1.0, 0.25).is_empty());
    }

    /// In centimetri la quota è in centimetri: 2,70 m sono 270.
    #[test]
    fn heights_follow_the_drawing_unit() {
        let elements = vec![(mass(0.0, 30.0, Some(2.7), vec![]), Tint::Plain)];
        let entities = draw_section("S", &elements, [0.0, 0.0], 100.0, 25.0);
        let b = boxes(&entities);
        assert!((b[0][3] - 270.0).abs() < 1e-9, "{:?}", b[0]);
    }
}
