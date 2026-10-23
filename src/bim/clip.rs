//! Taglio delle entità muro (spec §5.4, punto 2): i pezzi che restano.
//!
//! Per disegnare lo stato di progetto, le linee dei muri dentro una zona
//! demolita si tolgono. Qui c'è solo la primitiva pura: data un'entità muro
//! (`LINE`, `ARC`, `LWPOLYLINE`, `POLYLINE` 2D) e gli intervalli di parametro
//! da togliere dai suoi segmenti, restituisce i pezzi che restano, dello
//! stesso tipo e con gli attributi dell'originale. Quali intervalli togliere
//! lo calcola il chiamante (Task 11).
//!
//! # Come si numerano e si parametrizzano i segmenti
//!
//! * `LINE` e `ARC`: un solo segmento, l'indice 0.
//! * Polilinee: un segmento per vertice, nell'ordine dei vertici; il segmento
//!   `i` va dal vertice `i` al vertice `i + 1` e prende bulge e larghezze dal
//!   vertice `i`. In una polilinea chiusa l'ultimo segmento, quello di
//!   chiusura, torna al vertice 0 (indice = numero di vertici − 1), con il
//!   bulge dell'ultimo vertice. È lo stesso ordine delle curve di
//!   `BoundarySource.curves`.
//! * Il parametro `t` corre da 0 a 1 **nel verso dell'entità**: `LINE` da
//!   `start` a `end`, `ARC` dall'angolo iniziale a quello finale in senso
//!   antiorario, segmento di polilinea dal suo vertice di partenza a quello
//!   di arrivo. Su un segmento dritto è lineare nella lunghezza; su un `ARC`
//!   e su un segmento con bulge è lineare nell'**angolo** (lo sweep), cioè
//!   `punto(t)` sta all'angolo `inizio + sweep · t`. Non è lineare nella
//!   corda.
//! * Attenzione per chi parte da `BoundarySource.curves`: il kernel
//!   rappresenta un segmento con bulge **negativo** come un arco antiorario
//!   con gli estremi scambiati (`Curve::segments`), quindi il suo parametro
//!   corre al contrario. Per quei segmenti l'intervallo da passare qui è
//!   `[1 − t1, 1 − t0]`. Vale per entità sul piano di default (normale +Z);
//!   le coordinate dei pezzi sono quelle proprie dell'entità (OCS).
//!
//! # Cosa viene restituito
//!
//! * Ogni pezzo copia l'originale (layer, colore, tipo di linea e relativa
//!   scala, spessore di linea, trasparenza, XDATA di altre applicazioni,
//!   proprietario; per le polilinee anche `constant_width`, quota, normale,
//!   `thickness`, `plinegen`, e per `POLYLINE` 2D anche flag e larghezze
//!   predefinite). Non passano le identità dell'originale: l'handle è
//!   `NULL`, e dizionario di estensione, reattori e byte grezzi del DWG si
//!   azzerano (un dizionario appartiene a un solo oggetto; i byte grezzi
//!   descriverebbero la geometria vecchia).
//! * I segmenti contigui che restano formano **una** polilinea aperta, dello
//!   stesso tipo dell'originale (una `POLYLINE` 2D resta `POLYLINE` 2D, non si
//!   converte in `LWPOLYLINE`), anche quando il pezzo è un solo segmento. Una
//!   polilinea chiusa tagliata si apre partendo dal punto di taglio: se il
//!   pezzo attraversa la chiusura, prosegue dal vertice 0. I pezzi sono in
//!   ordine di partenza lungo l'entità.
//! * Un segmento con bulge tagliato a metà diventa un arco con il bulge
//!   `tan(θ'/4)`, dove `θ'` è lo sweep del tratto che resta, con il segno
//!   dell'originale. Le larghezze di vertice si interpolano lungo il tratto
//!   (`t` lineare); i vertici veri conservano id e flag (e le larghezze, se il
//!   loro segmento resta intero), quelli nati dal taglio hanno id 0 e flag
//!   vuoti.
//! * **Senza tagli** (nessun intervallo, indici fuori dall'entità, intervalli
//!   vuoti, non finiti o trascurabili) si restituisce l'entità intera, cioè
//!   una copia identica con le stesse regole di identità dei pezzi (handle
//!   `NULL`): il risultato è sempre fatto di oggetti nuovi, da aggiungere a un
//!   disegno, e il chiamante che vuole riconoscere un'entità intatta guarda
//!   `removed`. Lo stesso vale per i tipi che non si tagliano (qualunque
//!   entità diversa dalle quattro sopra): tornano intere, mai perse.
//! * Estremi invertiti si riordinano, estremi fuori da 0..1 si portano a 0..1,
//!   intervalli sovrapposti o che si toccano si fondono, l'ordine non conta.
//!   Un intervallo con un estremo non finito si ignora.
//!
//! # Lunghezze trascurabili
//!
//! Un tratto più corto di [`MIN_PIECE_REL`] volte la lunghezza dell'intera
//! entità non è un pezzo: né un intervallo da togliere (non spezza), né un
//! tratto che resta (si scarta). Le unghie di rumore numerico (`t` calcolato
//! con un errore di 1e-12) non diventano `LINE` di lunghezza nulla. Un
//! segmento già degenerato (vertice doppio) in mezzo a un pezzo non lo
//! spezza.
//!
//! Limite noto: una `POLYLINE` 2D con adattamento a curva o spline si taglia
//! sui vertici memorizzati, gli stessi che vedono i muri (`entity_curve`); i
//! flag di adattamento restano quelli dell'originale.

use codec::entities::{Arc, Line, LwPolyline, LwVertex, Polyline2D, Vertex2D};
use codec::{EntityType, Handle, Vector2, Vector3};
use kernel::geom2d::{arc_span, normalize_angle, BulgeArc};

/// Lunghezza minima di un tratto, relativa alla lunghezza dell'intera
/// entità: sotto, il tratto è rumore e non conta (vedi «Lunghezze
/// trascurabili» nella documentazione del modulo). 1e-9 della lunghezza: 10 nm
/// su un muro di 10 m, ben sotto la tolleranza geometrica di 1 µm.
pub const MIN_PIECE_REL: f64 = 1.0e-9;

/// I pezzi che restano di `entity` togliendo gli intervalli `removed`.
///
/// `removed` è un elenco di `(indice del segmento, [t0, t1])` con `t` in 0..1
/// nel verso dell'entità; la numerazione dei segmenti e la parametrizzazione
/// sono nella documentazione del modulo. Con tutto tolto restituisce un
/// elenco vuoto.
pub fn kept_pieces(entity: &EntityType, removed: &[(usize, [f64; 2])]) -> Vec<EntityType> {
    let pieces = match entity {
        EntityType::Line(line) => cut_line(entity, line, removed),
        EntityType::Arc(arc) => cut_arc(entity, arc, removed),
        EntityType::LwPolyline(poly) => {
            let vertices = poly.vertices.iter().map(lw_vertex).collect();
            cut_polyline(entity, vertices, poly.is_closed, removed, |nodes| {
                EntityType::LwPolyline(lw_piece(poly, nodes))
            })
        }
        EntityType::Polyline2D(poly) => {
            let vertices = poly.vertices.iter().map(heavy_vertex).collect();
            cut_polyline(entity, vertices, poly.is_closed(), removed, |nodes| {
                EntityType::Polyline2D(heavy_piece(poly, nodes))
            })
        }
        other => vec![other.clone()],
    };
    pieces.into_iter().map(as_new_object).collect()
}

/// Un pezzo è un oggetto nuovo: senza l'identità dell'originale.
fn as_new_object(mut entity: EntityType) -> EntityType {
    let common = entity.common_mut();
    common.handle = Handle::NULL;
    common.xdictionary_handle = None;
    common.reactors.clear();
    common.raw_record = None;
    common.graphic_data = None;
    entity
}

// ---------------------------------------------------------------------------
// Intervalli
// ---------------------------------------------------------------------------

/// Interpolazione lineare esatta agli estremi.
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    if t <= 0.0 {
        a
    } else if t >= 1.0 {
        b
    } else {
        a + (b - a) * t
    }
}

/// Gli intervalli da togliere da un segmento lungo `length`: dentro 0..1,
/// ordinati, fusi, senza quelli non finiti, vuoti o più corti di `eps`.
fn merged_removed(raw: &[[f64; 2]], length: f64, eps: f64) -> Vec<[f64; 2]> {
    let mut intervals: Vec<[f64; 2]> = raw
        .iter()
        .filter_map(|&[a, b]| {
            if !a.is_finite() || !b.is_finite() {
                return None;
            }
            let (lo, hi) = (a.min(b).clamp(0.0, 1.0), a.max(b).clamp(0.0, 1.0));
            (hi > lo).then_some([lo, hi])
        })
        .collect();
    intervals.sort_by(|x, y| x[0].total_cmp(&y[0]));
    let mut merged: Vec<[f64; 2]> = Vec::with_capacity(intervals.len());
    for interval in intervals {
        match merged.last_mut() {
            Some(last) if interval[0] <= last[1] => last[1] = last[1].max(interval[1]),
            _ => merged.push(interval),
        }
    }
    merged.retain(|interval| (interval[1] - interval[0]) * length > eps);
    merged
}

/// Il complemento di `removed` (ordinati e fusi) in 0..1.
fn kept_of(removed: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut kept = Vec::with_capacity(removed.len() + 1);
    let mut cursor = 0.0;
    for &[lo, hi] in removed {
        if lo > cursor {
            kept.push([cursor, lo]);
        }
        cursor = hi;
    }
    if cursor < 1.0 {
        kept.push([cursor, 1.0]);
    }
    kept
}

/// Gli intervalli di `removed` che riguardano il segmento `index`.
fn raw_for(removed: &[(usize, [f64; 2])], index: usize) -> Vec<[f64; 2]> {
    removed
        .iter()
        .filter(|(segment, _)| *segment == index)
        .map(|(_, interval)| *interval)
        .collect()
}

// ---------------------------------------------------------------------------
// LINE e ARC
// ---------------------------------------------------------------------------

fn lerp3(a: Vector3, b: Vector3, t: f64) -> Vector3 {
    Vector3::new(lerp(a.x, b.x, t), lerp(a.y, b.y, t), lerp(a.z, b.z, t))
}

fn cut_line(entity: &EntityType, line: &Line, removed: &[(usize, [f64; 2])]) -> Vec<EntityType> {
    let length = line.start.distance(&line.end);
    let eps = MIN_PIECE_REL * length;
    let cuts = merged_removed(&raw_for(removed, 0), length, eps);
    if cuts.is_empty() {
        return vec![entity.clone()];
    }
    kept_of(&cuts)
        .into_iter()
        .filter(|[a, b]| (b - a) * length > eps)
        .map(|[a, b]| {
            let mut piece = line.clone();
            piece.start = lerp3(line.start, line.end, a);
            piece.end = lerp3(line.start, line.end, b);
            EntityType::Line(piece)
        })
        .collect()
}

fn cut_arc(entity: &EntityType, arc: &Arc, removed: &[(usize, [f64; 2])]) -> Vec<EntityType> {
    let span = arc_span(arc.start_angle, arc.end_angle);
    let length = arc.radius.abs() * span;
    let eps = MIN_PIECE_REL * length;
    let cuts = merged_removed(&raw_for(removed, 0), length, eps);
    if cuts.is_empty() {
        return vec![entity.clone()];
    }
    let first = normalize_angle(arc.start_angle);
    kept_of(&cuts)
        .into_iter()
        .filter(|[a, b]| (b - a) * length > eps)
        .map(|[a, b]| {
            let mut piece = arc.clone();
            if a > 0.0 {
                piece.start_angle = normalize_angle(first + span * a);
            }
            if b < 1.0 {
                piece.end_angle = normalize_angle(first + span * b);
            }
            EntityType::Arc(piece)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Polilinee
// ---------------------------------------------------------------------------

/// Un vertice di polilinea, quanto serve al taglio.
struct Vtx {
    pos: [f64; 2],
    z: f64,
    /// Bulge del segmento in uscita.
    bulge: f64,
    /// Larghezze all'inizio e alla fine del segmento in uscita.
    start_width: f64,
    end_width: f64,
}

fn lw_vertex(v: &LwVertex) -> Vtx {
    Vtx {
        pos: [v.location.x, v.location.y],
        z: 0.0,
        bulge: v.bulge,
        start_width: v.start_width,
        end_width: v.end_width,
    }
}

fn heavy_vertex(v: &Vertex2D) -> Vtx {
    Vtx {
        pos: [v.location.x, v.location.y],
        z: v.location.z,
        bulge: v.bulge,
        start_width: v.start_width,
        end_width: v.end_width,
    }
}

/// Un segmento di polilinea: dal vertice `from` al vertice `to`.
struct Seg {
    from: usize,
    to: usize,
    p0: [f64; 2],
    p1: [f64; 2],
    z0: f64,
    z1: f64,
    bulge: f64,
    w0: f64,
    w1: f64,
    arc: Option<BulgeArc>,
    length: f64,
}

impl Seg {
    fn new(vertices: &[Vtx], from: usize, to: usize) -> Self {
        let (a, b) = (&vertices[from], &vertices[to]);
        let arc = BulgeArc::from_bulge(a.pos, b.pos, a.bulge);
        let length = match &arc {
            Some(arc) => arc.radius * arc.sweep.abs(),
            None => (b.pos[0] - a.pos[0]).hypot(b.pos[1] - a.pos[1]),
        };
        Seg {
            from,
            to,
            p0: a.pos,
            p1: b.pos,
            z0: a.z,
            z1: b.z,
            bulge: a.bulge,
            w0: a.start_width,
            w1: a.end_width,
            arc,
            length,
        }
    }

    /// Il punto al parametro `t` (lineare nell'angolo per un arco).
    fn point(&self, t: f64) -> [f64; 2] {
        if t <= 0.0 {
            self.p0
        } else if t >= 1.0 {
            self.p1
        } else if let Some(arc) = &self.arc {
            arc.sample(t)
        } else {
            [
                lerp(self.p0[0], self.p1[0], t),
                lerp(self.p0[1], self.p1[1], t),
            ]
        }
    }

    /// Il bulge del tratto `[a, b]`: `tan(θ'/4)` con il segno dello sweep.
    fn bulge_between(&self, a: f64, b: f64) -> f64 {
        if a <= 0.0 && b >= 1.0 {
            return self.bulge;
        }
        match &self.arc {
            Some(arc) => (arc.sweep * (b - a) / 4.0).tan(),
            None => 0.0,
        }
    }

    /// Le larghezze all'inizio e alla fine del tratto `[a, b]`.
    fn widths_between(&self, a: f64, b: f64) -> (f64, f64) {
        if self.w0 == self.w1 {
            (self.w0, self.w1)
        } else {
            (lerp(self.w0, self.w1, a), lerp(self.w0, self.w1, b))
        }
    }
}

/// Un tratto di segmento che resta: segmento `seg`, parametri `[a, b]`.
struct Sub {
    seg: usize,
    a: f64,
    b: f64,
}

/// Un vertice del pezzo. `origin` è il vertice dell'originale che coincide
/// con questo, se il punto non è nato dal taglio.
struct Node {
    pos: [f64; 2],
    z: f64,
    bulge: f64,
    start_width: f64,
    end_width: f64,
    origin: Option<usize>,
}

fn cut_polyline(
    entity: &EntityType,
    vertices: Vec<Vtx>,
    closed: bool,
    removed: &[(usize, [f64; 2])],
    build: impl Fn(&[Node]) -> EntityType,
) -> Vec<EntityType> {
    let count = vertices.len();
    let segment_count = match count {
        0 | 1 => 0,
        n if closed => n,
        n => n - 1,
    };
    let segments: Vec<Seg> = (0..segment_count)
        .map(|i| Seg::new(&vertices, i, (i + 1) % count))
        .collect();
    let eps = MIN_PIECE_REL * segments.iter().map(|s| s.length).sum::<f64>();

    let cuts: Vec<Vec<[f64; 2]>> = segments
        .iter()
        .enumerate()
        .map(|(i, seg)| merged_removed(&raw_for(removed, i), seg.length, eps))
        .collect();
    if cuts.iter().all(Vec::is_empty) {
        return vec![entity.clone()];
    }

    let mut runs = contiguous_runs(&cuts);
    // Un pezzo che attraversa la chiusura: la fine dell'ultimo e l'inizio del
    // primo sono lo stesso pezzo, che parte dove inizia l'ultimo.
    if closed && runs.len() > 1 {
        let wraps = matches!(
            (runs.first().and_then(|r| r.first()), runs.last().and_then(|r| r.last())),
            (Some(first), Some(last))
                if first.seg == 0 && first.a <= 0.0
                    && last.seg == segment_count - 1 && last.b >= 1.0
        );
        if wraps {
            let head = runs.remove(0);
            if let Some(tail) = runs.last_mut() {
                tail.extend(head);
            }
        }
    }

    runs.iter()
        .filter_map(|run| run_nodes(run, &segments, eps))
        .map(|nodes| build(&nodes))
        .collect()
}

/// I tratti che restano, raggruppati in sequenze contigue (un tratto che
/// arriva a 1 e uno che riparte da 0 sul segmento dopo sono contigui).
fn contiguous_runs(cuts: &[Vec<[f64; 2]>]) -> Vec<Vec<Sub>> {
    let mut runs: Vec<Vec<Sub>> = Vec::new();
    for (seg, removed) in cuts.iter().enumerate() {
        for [a, b] in kept_of(removed) {
            let linked = runs
                .last()
                .and_then(|run| run.last())
                .is_some_and(|prev| prev.seg + 1 == seg && prev.b >= 1.0 && a <= 0.0);
            match runs.last_mut() {
                Some(run) if linked => run.push(Sub { seg, a, b }),
                _ => runs.push(vec![Sub { seg, a, b }]),
            }
        }
    }
    runs
}

/// I vertici del pezzo per una sequenza di tratti; `None` se non resta nulla
/// di più lungo di `eps`.
fn run_nodes(run: &[Sub], segments: &[Seg], eps: f64) -> Option<Vec<Node>> {
    let subs: Vec<&Sub> = run
        .iter()
        .filter(|s| (s.b - s.a) * segments[s.seg].length > eps)
        .collect();
    let last = subs.last()?;
    let mut nodes: Vec<Node> = subs
        .iter()
        .map(|s| {
            let seg = &segments[s.seg];
            let (start_width, end_width) = seg.widths_between(s.a, s.b);
            Node {
                pos: seg.point(s.a),
                z: lerp(seg.z0, seg.z1, s.a),
                bulge: seg.bulge_between(s.a, s.b),
                start_width,
                end_width,
                origin: (s.a <= 0.0).then_some(seg.from),
            }
        })
        .collect();
    let seg = &segments[last.seg];
    nodes.push(Node {
        pos: seg.point(last.b),
        z: lerp(seg.z0, seg.z1, last.b),
        bulge: 0.0,
        start_width: 0.0,
        end_width: 0.0,
        origin: (last.b >= 1.0).then_some(seg.to),
    });
    Some(nodes)
}

fn lw_piece(original: &LwPolyline, nodes: &[Node]) -> LwPolyline {
    let mut piece = original.clone();
    piece.is_closed = false;
    piece.vertices = nodes
        .iter()
        .map(|node| LwVertex {
            location: Vector2::new(node.pos[0], node.pos[1]),
            bulge: node.bulge,
            start_width: node.start_width,
            end_width: node.end_width,
            vertex_id: node.origin.map_or(0, |i| original.vertices[i].vertex_id),
        })
        .collect();
    piece
}

fn heavy_piece(original: &Polyline2D, nodes: &[Node]) -> Polyline2D {
    let mut piece = original.clone();
    piece.flags.set_closed(false);
    piece.vertices = nodes
        .iter()
        .map(|node| {
            let mut vertex = match node.origin {
                Some(i) => original.vertices[i].clone(),
                None => Vertex2D::new(Vector3::ZERO),
            };
            vertex.location = Vector3::new(node.pos[0], node.pos[1], node.z);
            vertex.bulge = node.bulge;
            vertex.start_width = node.start_width;
            vertex.end_width = node.end_width;
            vertex
        })
        .collect();
    piece
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::polyline::VertexFlags;
    use codec::entities::{Arc, EntityCommon, Line, LwPolyline, LwVertex, Polyline2D, Vertex2D};
    use codec::types::{Color, LineWeight, Transparency};
    use codec::{Handle, Vector2, Vector3};
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    const TOL: f64 = 1.0e-9;

    #[track_caller]
    fn near(got: f64, want: f64, what: &str) {
        assert!((got - want).abs() < TOL, "{what}: {got} invece di {want}");
    }

    #[track_caller]
    fn near_angle(got: f64, want: f64, what: &str) {
        let d = (got - want).rem_euclid(TAU);
        assert!(d.min(TAU - d) < TOL, "{what}: {got} invece di {want}");
    }

    #[track_caller]
    fn assert_xy(got: [f64; 2], want: [f64; 2], what: &str) {
        near(got[0], want[0], &format!("{what} x"));
        near(got[1], want[1], &format!("{what} y"));
    }

    /// Attributi comuni e identità "decorati" come in un rilievo vero.
    fn decorate(common: &mut EntityCommon) {
        common.layer = "MURI".to_string();
        common.color = Color::from_index(3);
        common.linetype = "DASHED".to_string();
        common.linetype_scale = 2.5;
        common.line_weight = LineWeight::Value(35);
        common.transparency = Transparency::new(77);
        common.color_name = Some("VERDE MURI".to_string());
        common.owner_handle = Handle::new(0x1F);
        common.handle = Handle::new(0x2A);
        common.reactors = vec![Handle::new(0x30)];
        common.xdictionary_handle = Some(Handle::new(0x31));
        let mut record = codec::xdata::ExtendedDataRecord::new("ALTRA_APP");
        record.add_value(codec::xdata::XDataValue::Integer16(7));
        common.extended_data.add_record(record);
    }

    /// Gli attributi dell'originale tornano uguali, campo per campo; le
    /// identità dell'originale (handle, dizionario, reattori, byte grezzi)
    /// non passano ai pezzi.
    #[track_caller]
    fn assert_attributes(got: &EntityCommon, want: &EntityCommon) {
        assert_eq!(got.layer, want.layer, "layer");
        assert_eq!(got.color, want.color, "colore");
        assert_eq!(got.linetype, want.linetype, "tipo di linea");
        assert_eq!(
            got.linetype_scale, want.linetype_scale,
            "scala del tipo di linea"
        );
        assert_eq!(got.line_weight, want.line_weight, "spessore di linea");
        assert_eq!(got.transparency, want.transparency, "trasparenza");
        assert_eq!(got.color_name, want.color_name, "nome del colore");
        assert_eq!(got.invisible, want.invisible, "visibilità");
        assert_eq!(got.owner_handle, want.owner_handle, "proprietario");
        assert_eq!(got.extended_data, want.extended_data, "XDATA");
        assert_eq!(got.handle, Handle::NULL, "handle");
        assert_eq!(got.xdictionary_handle, None, "dizionario di estensione");
        assert!(got.reactors.is_empty(), "reattori");
        assert!(got.raw_record.is_none(), "byte grezzi del DWG");
        assert!(got.graphic_data.is_none(), "grafica grezza");
    }

    fn decorated_line(a: [f64; 2], b: [f64; 2]) -> Line {
        let mut line =
            Line::from_points(Vector3::new(a[0], a[1], 0.0), Vector3::new(b[0], b[1], 0.0));
        decorate(&mut line.common);
        line.thickness = 0.5;
        line
    }

    fn decorated_lw(points: &[([f64; 2], f64)], closed: bool) -> LwPolyline {
        let mut poly = LwPolyline::new();
        for (p, bulge) in points {
            poly.add_vertex(LwVertex::with_bulge(Vector2::new(p[0], p[1]), *bulge));
        }
        poly.is_closed = closed;
        poly.constant_width = 0.02;
        poly.elevation = 1.5;
        poly.thickness = 0.25;
        poly.plinegen = true;
        decorate(&mut poly.common);
        poly
    }

    fn square(closed: bool) -> LwPolyline {
        decorated_lw(
            &[
                ([0.0, 0.0], 0.0),
                ([10.0, 0.0], 0.0),
                ([10.0, 10.0], 0.0),
                ([0.0, 10.0], 0.0),
            ],
            closed,
        )
    }

    fn lw(piece: &EntityType) -> &LwPolyline {
        match piece {
            EntityType::LwPolyline(p) => p,
            other => panic!("attesa una LWPOLYLINE, trovata {other:?}"),
        }
    }

    fn line_of(piece: &EntityType) -> ([f64; 3], [f64; 3]) {
        match piece {
            EntityType::Line(l) => (
                [l.start.x, l.start.y, l.start.z],
                [l.end.x, l.end.y, l.end.z],
            ),
            other => panic!("attesa una LINE, trovata {other:?}"),
        }
    }

    fn xy(v: &LwVertex) -> [f64; 2] {
        [v.location.x, v.location.y]
    }

    /// I vertici di una LWPOLYLINE confrontati con quelli attesi.
    #[track_caller]
    fn assert_vertices(piece: &EntityType, want: &[[f64; 2]]) {
        let poly = lw(piece);
        assert_eq!(poly.vertices.len(), want.len(), "vertici di {poly:?}");
        for (i, (v, w)) in poly.vertices.iter().zip(want).enumerate() {
            assert_xy(xy(v), *w, &format!("vertice {i}"));
        }
    }

    #[test]
    fn line_middle_removed() {
        let line = decorated_line([0.0, 0.0], [10.0, 0.0]);
        let pieces = kept_pieces(&EntityType::Line(line.clone()), &[(0, [0.4, 0.6])]);
        assert_eq!(pieces.len(), 2);
        let (a0, a1) = line_of(&pieces[0]);
        let (b0, b1) = line_of(&pieces[1]);
        assert_xy([a0[0], a0[1]], [0.0, 0.0], "primo inizio");
        assert_xy([a1[0], a1[1]], [4.0, 0.0], "prima fine");
        assert_xy([b0[0], b0[1]], [6.0, 0.0], "secondo inizio");
        assert_xy([b1[0], b1[1]], [10.0, 0.0], "seconda fine");
        for piece in &pieces {
            let EntityType::Line(got) = piece else {
                panic!("attesa una LINE")
            };
            assert_attributes(&got.common, &line.common);
            assert_eq!(got.thickness, 0.5, "thickness");
            assert_eq!(got.normal, line.normal, "normale");
        }
    }

    #[test]
    fn line_keeps_z_by_interpolation() {
        let mut line = decorated_line([0.0, 0.0], [10.0, 0.0]);
        line.end.z = 2.0;
        let pieces = kept_pieces(&EntityType::Line(line), &[(0, [0.0, 0.5])]);
        assert_eq!(pieces.len(), 1);
        let (a, b) = line_of(&pieces[0]);
        assert_xy([a[0], a[1]], [5.0, 0.0], "inizio");
        near(a[2], 1.0, "z dell'inizio");
        assert_xy([b[0], b[1]], [10.0, 0.0], "fine");
        near(b[2], 2.0, "z della fine");
    }

    #[test]
    fn closed_square_opens_at_cut_keeping_attributes() {
        let poly = square(true);
        let pieces = kept_pieces(&EntityType::LwPolyline(poly.clone()), &[(1, [0.4, 0.6])]);
        assert_eq!(pieces.len(), 1);
        assert_vertices(
            &pieces[0],
            &[
                [10.0, 6.0],
                [10.0, 10.0],
                [0.0, 10.0],
                [0.0, 0.0],
                [10.0, 0.0],
                [10.0, 4.0],
            ],
        );
        let got = lw(&pieces[0]);
        assert!(!got.is_closed, "il pezzo è aperto");
        assert_attributes(&got.common, &poly.common);
        assert_eq!(got.common.layer, "MURI");
        assert_eq!(got.common.color, Color::from_index(3));
        assert_eq!(got.common.linetype, "DASHED");
        assert_eq!(got.constant_width, 0.02);
        assert_eq!(got.elevation, 1.5);
        assert_eq!(got.thickness, 0.25);
        assert_eq!(got.normal, poly.normal);
        assert!(got.plinegen);
        assert!(got.vertices.iter().all(|v| v.bulge == 0.0));
    }

    #[test]
    fn arc_half_removed() {
        let mut arc = Arc::from_center_radius_angles(Vector3::ZERO, 1.0, 0.0, FRAC_PI_2);
        decorate(&mut arc.common);
        arc.thickness = 0.5;
        let pieces = kept_pieces(&EntityType::Arc(arc.clone()), &[(0, [0.5, 1.0])]);
        assert_eq!(pieces.len(), 1);
        let EntityType::Arc(got) = &pieces[0] else {
            panic!("attesto un ARC")
        };
        near_angle(got.start_angle, 0.0, "angolo iniziale");
        near_angle(got.end_angle, FRAC_PI_2 / 2.0, "angolo finale");
        assert_eq!(got.center, arc.center);
        assert_eq!(got.radius, 1.0);
        assert_eq!(got.thickness, 0.5);
        assert_eq!(got.normal, arc.normal);
        assert_attributes(&got.common, &arc.common);
    }

    #[test]
    fn arc_across_zero_keeps_its_direction() {
        // Da 350° a 10° (venti gradi): il parametro corre sull'angolo.
        let arc = Arc::from_center_radius_angles(
            Vector3::new(5.0, 5.0, 0.0),
            2.0,
            350f64.to_radians(),
            10f64.to_radians(),
        );
        let pieces = kept_pieces(&EntityType::Arc(arc), &[(0, [0.25, 0.5])]);
        assert_eq!(pieces.len(), 2);
        let EntityType::Arc(a) = &pieces[0] else {
            panic!("attesto un ARC")
        };
        let EntityType::Arc(b) = &pieces[1] else {
            panic!("attesto un ARC")
        };
        near_angle(a.start_angle, 350f64.to_radians(), "a: inizio");
        near_angle(a.end_angle, 355f64.to_radians(), "a: fine");
        near_angle(b.start_angle, 0.0, "b: inizio");
        near_angle(b.end_angle, 10f64.to_radians(), "b: fine");
    }

    #[test]
    fn bulge_segment_half_removed() {
        let bulge = (FRAC_PI_2 / 4.0).tan();
        let poly = decorated_lw(&[([1.0, 0.0], bulge), ([0.0, 1.0], 0.0)], false);
        let pieces = kept_pieces(&EntityType::LwPolyline(poly.clone()), &[(0, [0.5, 1.0])]);
        assert_eq!(pieces.len(), 1);
        let got = lw(&pieces[0]);
        assert_eq!(got.vertices.len(), 2);
        let half = FRAC_PI_2 / 2.0;
        assert_xy(xy(&got.vertices[0]), [1.0, 0.0], "inizio");
        assert_xy(
            xy(&got.vertices[1]),
            [half.cos(), half.sin()],
            "fine sull'arco",
        );
        near(got.vertices[0].bulge, (half / 4.0).tan(), "nuovo bulge");
        assert_eq!(got.vertices[1].bulge, 0.0);
        assert_attributes(&got.common, &poly.common);
    }

    #[test]
    fn negative_bulge_keeps_its_sign_and_t_follows_the_polyline_direction() {
        // (1;0) -> (0;-1) in senso orario attorno all'origine: bulge negativo.
        let bulge = -(FRAC_PI_2 / 4.0).tan();
        let poly = decorated_lw(&[([1.0, 0.0], bulge), ([0.0, -1.0], 0.0)], false);
        // t = 0 è il vertice 0: tolgo il primo quarto, resta [0,25; 1].
        let pieces = kept_pieces(&EntityType::LwPolyline(poly), &[(0, [0.0, 0.25])]);
        assert_eq!(pieces.len(), 1);
        let got = lw(&pieces[0]);
        let a = -(FRAC_PI_2 * 0.25);
        assert_xy(xy(&got.vertices[0]), [a.cos(), a.sin()], "inizio");
        assert_xy(xy(&got.vertices[1]), [0.0, -1.0], "fine");
        near(
            got.vertices[0].bulge,
            -((FRAC_PI_2 * 0.75) / 4.0).tan(),
            "bulge negativo",
        );
    }

    #[test]
    fn bulge_segment_middle_removed_splits_the_arc() {
        // Semicerchio antiorario da (2;0) a (-2;0), centro (0;0), raggio 2.
        let poly = decorated_lw(&[([2.0, 0.0], 1.0), ([-2.0, 0.0], 0.0)], false);
        let pieces = kept_pieces(&EntityType::LwPolyline(poly), &[(0, [0.25, 0.5])]);
        assert_eq!(pieces.len(), 2);
        let first = lw(&pieces[0]);
        let second = lw(&pieces[1]);
        // Primo pezzo: da 0° a 45°.
        let p = 45f64.to_radians();
        assert_xy(xy(&first.vertices[0]), [2.0, 0.0], "a0");
        assert_xy(xy(&first.vertices[1]), [2.0 * p.cos(), 2.0 * p.sin()], "a1");
        near(first.vertices[0].bulge, (p / 4.0).tan(), "bulge a");
        // Secondo pezzo: da 90° a 180°.
        assert_xy(xy(&second.vertices[0]), [0.0, 2.0], "b0");
        assert_xy(xy(&second.vertices[1]), [-2.0, 0.0], "b1");
        near(second.vertices[0].bulge, (FRAC_PI_2 / 4.0).tan(), "bulge b");
    }

    #[test]
    fn two_intervals_on_one_segment() {
        let line = decorated_line([0.0, 0.0], [10.0, 0.0]);
        let pieces = kept_pieces(&EntityType::Line(line), &[(0, [0.6, 0.7]), (0, [0.1, 0.2])]);
        assert_eq!(pieces.len(), 3);
        let want = [(0.0, 1.0), (2.0, 6.0), (7.0, 10.0)];
        for (piece, (x0, x1)) in pieces.iter().zip(want) {
            let (a, b) = line_of(piece);
            near(a[0], x0, "inizio");
            near(b[0], x1, "fine");
        }
    }

    #[test]
    fn overlapping_unsorted_and_touching_intervals_merge() {
        let line = decorated_line([0.0, 0.0], [10.0, 0.0]);
        let removed = [
            (0, [0.5, 0.7]),
            (0, [0.2, 0.4]),
            (0, [0.35, 0.55]),
            (0, [0.7, 0.7]),
        ];
        let pieces = kept_pieces(&EntityType::Line(line.clone()), &removed);
        assert_eq!(pieces.len(), 2);
        near(line_of(&pieces[0]).1[0], 2.0, "prima fine");
        near(line_of(&pieces[1]).0[0], 7.0, "seconda partenza");

        // Intervalli che si toccano soltanto: nessun pezzo tra i due.
        let touching = [(0, [0.2, 0.4]), (0, [0.4, 0.6])];
        let pieces = kept_pieces(&EntityType::Line(line.clone()), &touching);
        assert_eq!(pieces.len(), 2);
        near(line_of(&pieces[0]).1[0], 2.0, "fine");
        near(line_of(&pieces[1]).0[0], 6.0, "inizio");

        // Estremi invertiti e fuori da 0..1 si riconducono a 0..1.
        let odd = [(0, [0.3, -0.5])];
        let pieces = kept_pieces(&EntityType::Line(line), &odd);
        assert_eq!(pieces.len(), 1);
        near(line_of(&pieces[0]).0[0], 3.0, "inizio");
        near(line_of(&pieces[0]).1[0], 10.0, "fine");
    }

    #[test]
    fn intervals_touching_the_ends_and_whole_segment() {
        let line = decorated_line([0.0, 0.0], [10.0, 0.0]);
        let head = kept_pieces(&EntityType::Line(line.clone()), &[(0, [0.0, 0.3])]);
        assert_eq!(head.len(), 1);
        near(line_of(&head[0]).0[0], 3.0, "tolta la testa");
        let tail = kept_pieces(&EntityType::Line(line.clone()), &[(0, [0.7, 1.0])]);
        assert_eq!(tail.len(), 1);
        near(line_of(&tail[0]).1[0], 7.0, "tolta la coda");
        let all = kept_pieces(&EntityType::Line(line), &[(0, [0.0, 1.0])]);
        assert!(all.is_empty(), "segmento tolto per intero: {all:?}");
    }

    #[test]
    fn whole_segment_removed_in_the_middle_of_an_open_polyline_splits_it() {
        // (0;0)-(10;0)-(10;10)-(0;10), tolgo il segmento 1 per intero.
        let poly = square(false);
        let pieces = kept_pieces(&EntityType::LwPolyline(poly), &[(1, [0.0, 1.0])]);
        assert_eq!(pieces.len(), 2);
        assert_vertices(&pieces[0], &[[0.0, 0.0], [10.0, 0.0]]);
        assert_vertices(&pieces[1], &[[10.0, 10.0], [0.0, 10.0]]);
    }

    #[test]
    fn open_polyline_pieces_are_not_wrapped() {
        let poly = decorated_lw(
            &[([0.0, 0.0], 0.0), ([10.0, 0.0], 0.0), ([10.0, 10.0], 0.0)],
            false,
        );
        let pieces = kept_pieces(&EntityType::LwPolyline(poly), &[(0, [0.4, 0.6])]);
        assert_eq!(pieces.len(), 2);
        assert_vertices(&pieces[0], &[[0.0, 0.0], [4.0, 0.0]]);
        assert_vertices(&pieces[1], &[[6.0, 0.0], [10.0, 0.0], [10.0, 10.0]]);
    }

    #[test]
    fn closed_polyline_cut_twice_gives_two_open_pieces() {
        let poly = square(true);
        let pieces = kept_pieces(
            &EntityType::LwPolyline(poly.clone()),
            &[(3, [0.4, 0.6]), (1, [0.4, 0.6])],
        );
        assert_eq!(pieces.len(), 2);
        // In ordine di partenza lungo la polilinea: prima quello che parte sul
        // lato 1, poi quello che parte sul lato 3 e prosegue oltre la chiusura.
        assert_vertices(
            &pieces[0],
            &[[10.0, 6.0], [10.0, 10.0], [0.0, 10.0], [0.0, 6.0]],
        );
        assert_vertices(
            &pieces[1],
            &[[0.0, 4.0], [0.0, 0.0], [10.0, 0.0], [10.0, 4.0]],
        );
        for piece in &pieces {
            let got = lw(piece);
            assert!(!got.is_closed);
            assert_attributes(&got.common, &poly.common);
            assert_eq!(got.constant_width, 0.02);
        }
    }

    #[test]
    fn interval_reaching_t1_at_the_closing_segment() {
        // Il lato di chiusura è il 3 ((0;10) -> (0;0)). Tolgo la sua seconda
        // metà: ciò che resta è unico, parte da (0;0) e arriva a (0;5).
        let poly = square(true);
        let pieces = kept_pieces(&EntityType::LwPolyline(poly.clone()), &[(3, [0.5, 1.0])]);
        assert_eq!(pieces.len(), 1);
        assert_vertices(
            &pieces[0],
            &[
                [0.0, 0.0],
                [10.0, 0.0],
                [10.0, 10.0],
                [0.0, 10.0],
                [0.0, 5.0],
            ],
        );
        assert!(!lw(&pieces[0]).is_closed);

        // Tolgo la prima metà del lato di chiusura: il pezzo che resta parte
        // da (0;5), attraversa la chiusura e prosegue dal vertice 0.
        let pieces = kept_pieces(&EntityType::LwPolyline(poly), &[(3, [0.0, 0.5])]);
        assert_eq!(pieces.len(), 1);
        assert_vertices(
            &pieces[0],
            &[
                [0.0, 5.0],
                [0.0, 0.0],
                [10.0, 0.0],
                [10.0, 10.0],
                [0.0, 10.0],
            ],
        );
    }

    #[test]
    fn closing_segment_bulge_belongs_to_the_last_vertex() {
        // Triangolo con il lato di chiusura (da (0;10) a (0;0)) curvo.
        let bulge = 0.5;
        let poly = decorated_lw(
            &[([0.0, 0.0], 0.0), ([10.0, 0.0], 0.0), ([0.0, 10.0], bulge)],
            true,
        );
        let pieces = kept_pieces(&EntityType::LwPolyline(poly), &[(0, [0.0, 1.0])]);
        assert_eq!(pieces.len(), 1);
        let got = lw(&pieces[0]);
        assert_eq!(got.vertices.len(), 3);
        assert_xy(xy(&got.vertices[0]), [10.0, 0.0], "v0");
        assert_xy(xy(&got.vertices[1]), [0.0, 10.0], "v1");
        assert_xy(xy(&got.vertices[2]), [0.0, 0.0], "v2");
        near(got.vertices[0].bulge, 0.0, "bulge del lato 1");
        near(got.vertices[1].bulge, bulge, "bulge del lato di chiusura");
        near(
            got.vertices[2].bulge,
            0.0,
            "l'ultimo vertice non ha lato in uscita",
        );
    }

    #[test]
    fn per_vertex_widths_are_interpolated_along_the_cut_segment() {
        let mut poly = decorated_lw(&[([0.0, 0.0], 0.0), ([10.0, 0.0], 0.0)], false);
        poly.vertices[0].start_width = 1.0;
        poly.vertices[0].end_width = 3.0;
        poly.vertices[0].vertex_id = 11;
        poly.vertices[1].vertex_id = 12;
        let pieces = kept_pieces(
            &EntityType::LwPolyline(poly),
            &[(0, [0.0, 0.25]), (0, [0.75, 1.0])],
        );
        assert_eq!(pieces.len(), 1);
        let got = lw(&pieces[0]);
        near(got.vertices[0].start_width, 1.5, "larghezza iniziale");
        near(got.vertices[0].end_width, 2.5, "larghezza finale");
        // Un vertice nato dal taglio non eredita l'identità di un vertice vero.
        assert_eq!(got.vertices[0].vertex_id, 0);
    }

    #[test]
    fn original_vertices_keep_their_id_and_width() {
        let mut poly = square(true);
        for (i, v) in poly.vertices.iter_mut().enumerate() {
            v.vertex_id = 100 + i as i32;
            v.start_width = 0.5;
            v.end_width = 0.5;
        }
        let pieces = kept_pieces(&EntityType::LwPolyline(poly), &[(1, [0.4, 0.6])]);
        let got = lw(&pieces[0]);
        // (10;6) nasce dal taglio; (10;10) (0;10) (0;0) (10;0) sono veri.
        assert_eq!(got.vertices[0].vertex_id, 0);
        assert_eq!(got.vertices[1].vertex_id, 102);
        assert_eq!(got.vertices[2].vertex_id, 103);
        assert_eq!(got.vertices[3].vertex_id, 100);
        assert_eq!(got.vertices[4].vertex_id, 101);
        for v in &got.vertices[..5] {
            assert_eq!((v.start_width, v.end_width), (0.5, 0.5));
        }
    }

    #[test]
    fn polyline2d_keeps_its_type_flags_and_vertex_data() {
        let mut heavy = Polyline2D::new();
        decorate(&mut heavy.common);
        heavy.start_width = 0.1;
        heavy.end_width = 0.1;
        heavy.elevation = 3.0;
        heavy.thickness = 0.5;
        heavy.close();
        let corners = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        for (i, c) in corners.iter().enumerate() {
            let mut v = Vertex2D::new(Vector3::new(c[0], c[1], 3.0));
            v.start_width = 0.3;
            v.end_width = 0.3;
            v.id = 40 + i as i32;
            v.flags = VertexFlags::from_bits(0);
            heavy.add_vertex(v);
        }
        let pieces = kept_pieces(&EntityType::Polyline2D(heavy.clone()), &[(1, [0.4, 0.6])]);
        assert_eq!(pieces.len(), 1);
        let EntityType::Polyline2D(got) = &pieces[0] else {
            panic!("il tipo deve restare POLYLINE 2D: {:?}", pieces[0])
        };
        assert!(!got.is_closed(), "il pezzo è aperto");
        assert_attributes(&got.common, &heavy.common);
        assert_eq!(got.start_width, 0.1);
        assert_eq!(got.end_width, 0.1);
        assert_eq!(got.elevation, 3.0);
        assert_eq!(got.thickness, 0.5);
        assert_eq!(got.normal, heavy.normal);
        let want = [
            [10.0, 6.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 4.0],
        ];
        assert_eq!(got.vertices.len(), want.len());
        for (v, w) in got.vertices.iter().zip(want) {
            assert_xy([v.location.x, v.location.y], w, "vertice");
            near(v.location.z, 3.0, "quota del vertice");
        }
        // Larghezze: i vertici veri e quelli nati dal taglio (costanti) valgono 0,3.
        assert!(got.vertices[..5]
            .iter()
            .all(|v| v.start_width == 0.3 && v.end_width == 0.3));
        assert_eq!(got.vertices[1].id, 42);
        assert_eq!(got.vertices[0].id, 0, "vertice nato dal taglio");
    }

    #[test]
    fn polyline2d_bulge_is_recomputed() {
        let mut heavy = Polyline2D::new();
        heavy.add_vertex(
            Vertex2D::new(Vector3::new(1.0, 0.0, 0.0)).with_bulge((FRAC_PI_2 / 4.0).tan()),
        );
        heavy.add_vertex(Vertex2D::new(Vector3::new(0.0, 1.0, 0.0)));
        let pieces = kept_pieces(&EntityType::Polyline2D(heavy), &[(0, [0.5, 1.0])]);
        assert_eq!(pieces.len(), 1);
        let EntityType::Polyline2D(got) = &pieces[0] else {
            panic!("attesa POLYLINE 2D")
        };
        near(
            got.vertices[0].bulge,
            (45f64.to_radians() / 4.0).tan(),
            "bulge",
        );
        assert_eq!(got.vertices[1].bulge, 0.0);
    }

    #[test]
    fn no_cuts_returns_the_whole_entity_without_identity() {
        let poly = square(true);
        for removed in [
            vec![],
            // Indice fuori dalla polilinea, intervallo vuoto, non finito, trascurabile.
            vec![
                (9, [0.0, 1.0]),
                (0, [0.5, 0.5]),
                (1, [f64::NAN, 0.5]),
                (2, [0.3, 0.3 + 1.0e-14]),
            ],
        ] {
            let pieces = kept_pieces(&EntityType::LwPolyline(poly.clone()), &removed);
            assert_eq!(pieces.len(), 1);
            let got = lw(&pieces[0]);
            assert!(got.is_closed, "senza tagli la polilinea resta chiusa");
            assert_eq!(got.vertices, poly.vertices);
            assert_attributes(&got.common, &poly.common);
        }
        let line = decorated_line([0.0, 0.0], [10.0, 0.0]);
        let pieces = kept_pieces(&EntityType::Line(line.clone()), &[]);
        assert_eq!(pieces.len(), 1);
        assert_eq!(line_of(&pieces[0]), line_of(&EntityType::Line(line)));
    }

    #[test]
    fn slivers_and_degenerate_segments_do_not_make_pieces_or_splits() {
        let line = decorated_line([0.0, 0.0], [10.0, 0.0]);
        // Resta solo un'unghia di 1e-11: niente pezzo.
        let pieces = kept_pieces(&EntityType::Line(line), &[(0, [0.0, 1.0 - 1.0e-12])]);
        assert!(pieces.is_empty(), "{pieces:?}");

        // Un vertice doppio (segmento di lunghezza nulla) non spezza la polilinea.
        let poly = decorated_lw(
            &[
                ([0.0, 0.0], 0.0),
                ([10.0, 0.0], 0.0),
                ([10.0, 0.0], 0.0),
                ([10.0, 10.0], 0.0),
                ([0.0, 10.0], 0.0),
            ],
            false,
        );
        let pieces = kept_pieces(&EntityType::LwPolyline(poly), &[(3, [0.5, 1.0])]);
        assert_eq!(pieces.len(), 1);
        assert_vertices(
            &pieces[0],
            &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [5.0, 10.0]],
        );
    }

    #[test]
    fn everything_removed_leaves_nothing() {
        let poly = square(true);
        let all: Vec<_> = (0..4).map(|i| (i, [0.0, 1.0])).collect();
        assert!(kept_pieces(&EntityType::LwPolyline(poly), &all).is_empty());
    }

    #[test]
    fn pieces_never_keep_the_original_identity() {
        // La stessa decorazione su un ARC: handle, dizionario e reattori non passano.
        let mut arc = Arc::from_center_radius_angles(Vector3::ZERO, 3.0, 0.0, PI);
        decorate(&mut arc.common);
        for piece in kept_pieces(&EntityType::Arc(arc.clone()), &[(0, [0.4, 0.6])]) {
            let EntityType::Arc(got) = piece else {
                panic!("attesto un ARC")
            };
            assert_attributes(&got.common, &arc.common);
        }
        assert!(!kept_pieces(&EntityType::Arc(arc), &[(0, [0.4, 0.6])]).is_empty());
    }

    /// Il contratto sul parametro, verificato contro le curve che il chiamante
    /// avrà davvero (`BoundarySource.curves`): per `LINE`, `ARC` e segmenti con
    /// bulge positivo `t` coincide con quello della curva; per un bulge
    /// negativo la curva del kernel corre al contrario e vale `1 - t`.
    #[test]
    fn parameter_contract_against_boundary_source_curves() {
        use crate::command::WorkingPlane;
        use crate::scene::entity_boundary_source_on_plane;

        let curve_point = |entity: &EntityType, t: f64| {
            let source =
                entity_boundary_source_on_plane(entity, WorkingPlane::default(), 1.0e-6, None)
                    .expect("curve del muro");
            source.curves[0].point_at(t)
        };
        let t = 0.3;
        let first_vertex = |entity: &EntityType| match &kept_pieces(entity, &[(0, [0.0, t])])[0] {
            EntityType::LwPolyline(p) => xy(&p.vertices[0]),
            EntityType::Line(l) => [l.start.x, l.start.y],
            EntityType::Arc(a) => [
                a.center.x + a.radius * a.start_angle.cos(),
                a.center.y + a.radius * a.start_angle.sin(),
            ],
            other => panic!("tipo inatteso {other:?}"),
        };

        let straight = EntityType::Line(decorated_line([1.0, 2.0], [9.0, 5.0]));
        assert_xy(first_vertex(&straight), curve_point(&straight, t), "LINE");

        let arc = EntityType::Arc(Arc::from_center_radius_angles(
            Vector3::new(1.0, 1.0, 0.0),
            3.0,
            0.2,
            2.5,
        ));
        assert_xy(first_vertex(&arc), curve_point(&arc, t), "ARC");

        // Quota 0: una polilinea in quota non è sul piano XY dei muri.
        let flat = |bulge: f64| {
            let mut poly = decorated_lw(&[([0.0, 0.0], bulge), ([4.0, 0.0], 0.0)], false);
            poly.elevation = 0.0;
            EntityType::LwPolyline(poly)
        };
        let positive = flat(0.5);
        assert_xy(
            first_vertex(&positive),
            curve_point(&positive, t),
            "bulge positivo",
        );

        let negative = flat(-0.5);
        assert_xy(
            first_vertex(&negative),
            curve_point(&negative, 1.0 - t),
            "bulge negativo",
        );
    }
}
