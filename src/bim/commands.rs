//! Comando zona (spec §5.1): `AL_DEMOLISCI` (zona gialla, demolita) e
//! `AL_NUOVO` (zona rossa, muro nuovo).
//!
//! Chiede come indicare l'area: due punti sull'asse del muro (modo
//! principale), punto interno, rettangolo, poligono o polilinea chiusa
//! esistente. Chiede lo spessore quando le facce non bastano (sempre per un
//! muro nuovo), il varco per le demolizioni, convalida e consegna una sola
//! `HATCH` di zona con `CommitEntitiesAndExit`.
//!
//! Lavora su un'istantanea della geometria ([`ZoneSnapshot`]) che prepara il
//! lato app: qui niente documento e niente undo (il passo lo apre l'host al
//! commit). Tutto sta sul piano XY del mondo, in unità del disegno.

use std::iter;

use codec::entities::hatch::BoundaryPathFlags;
use codec::entities::{BoundaryEdge, BoundaryPath, Hatch, HatchStyleType, PolylineEdge};
use codec::{EntityType, Handle, Vector2};
use glam::DVec3;
use kernel::geom2d::{
    distance_to, segment_crossing, signed_area, Curve, Line, SegmentCrossing, Tolerance,
};
use rustc_hash::FxHashMap;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, WorkingPlane};
use crate::entities::hatch::boundary_area;
use crate::scene::model::wire_model::WireModel;
use crate::scene::{exact_hatch_paths, BoundarySource};

use super::report::Notice;
use super::units::{area_m2, format_m2, DrawingUnit};
use super::walls::{
    net_area, rect_from_axis, rect_from_axis_thickness, region_at, AxisMiss, Region, WallRect,
    FACE_PARALLEL_DEGREES,
};
use super::zone::{
    read_zone, zone_hatch, zone_hatch_from_paths, Zone, ZoneGeom, ZoneOrigin, ZoneState,
};
use super::MAX_WALL_THICKNESS_M;

/// Colore dell'anteprima delle zone demolite (ACI 2, giallo).
const PREVIEW_GIALLO: [f32; 4] = [1.0, 1.0, 0.0, 1.0];
/// Colore dell'anteprima delle zone nuove (ACI 1, rosso).
const PREVIEW_ROSSO: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

/// La geometria che il comando legge, preparata dal lato app all'avvio.
#[derive(Clone)]
pub struct ZoneSnapshot {
    /// La geometria dei muri per handle (`walls::wall_set(..).sources`).
    pub walls: FxHashMap<Handle, BoundarySource>,
    /// Le facce chiuse dai muri (`walls::faces`), per il punto interno.
    pub faces: Vec<Vec<[f64; 2]>>,
    /// Le zone già nel disegno, per la convalida.
    pub zones: Vec<ZoneGeom>,
    pub unit: DrawingUnit,
    /// Tolleranza geometrica, in unità del disegno (positiva).
    pub tol: f64,
    /// Lo spessore proposto con Invio (l'ultima zona nuova), in unità del
    /// disegno.
    pub last_thickness: Option<f64>,
}

/// La zona proposta, in attesa del varco e della conferma.
struct Candidate {
    /// `varco` resta no finché non si risponde.
    zone: Zone,
    /// `rings[0]` è il contorno esterno, gli altri buchi: servono alla
    /// convalida, all'anteprima e, senza `paths`, alla campitura.
    rings: Vec<Vec<[f64; 2]>>,
    /// I percorsi della campitura con gli archi veri, esterno per primo.
    paths: Option<Vec<BoundaryPath>>,
    /// In unità del disegno al quadrato.
    area: f64,
}

impl Candidate {
    fn hatch(self) -> EntityType {
        match self.paths {
            Some(paths) => zone_hatch_from_paths(&self.zone, paths),
            None => zone_hatch(&self.zone, &self.rings),
        }
    }
}

/// I passi del comando.
enum Step {
    /// Primo punto sull'asse, o una parola chiave.
    Start,
    /// Secondo punto sull'asse.
    AxisEnd {
        p1: [f64; 2],
    },
    /// Spessore digitato; `faces_missing` quando `AL_DEMOLISCI` non ha
    /// trovato le due facce.
    Thickness {
        p1: [f64; 2],
        p2: [f64; 2],
        faces_missing: bool,
    },
    /// Punto dentro un'area chiusa; `missed` se il precedente non ne aveva.
    Interior {
        missed: bool,
    },
    RectFirst,
    RectSecond {
        first: [f64; 2],
    },
    Polygon {
        points: Vec<[f64; 2]>,
    },
    /// Polilinea chiusa da scegliere; `rejected` se l'oggetto scelto prima
    /// non lo era.
    Select {
        rejected: bool,
    },
    Varco(Candidate),
    Confirm {
        candidate: Candidate,
        notices: Vec<Notice>,
    },
    /// Zona consegnata o comando annullato.
    Done,
}

pub struct ZoneCommand {
    state: ZoneState,
    snap: ZoneSnapshot,
    step: Step,
    /// Il piano di lavoro: il rettangolo segue il suo asse X.
    plane: WorkingPlane,
    /// L'entità sotto il clic, iniettata dall'host prima di `on_entity_pick`.
    picked: Option<EntityType>,
}

impl ZoneCommand {
    pub fn new(state: ZoneState, snap: ZoneSnapshot) -> Self {
        Self {
            state,
            snap,
            step: Step::Start,
            plane: WorkingPlane::default(),
            picked: None,
        }
    }

    fn tol(&self) -> f64 {
        self.snap.tol
    }

    /// Lo spessore proposto, se è un numero positivo.
    fn last_thickness(&self) -> Option<f64> {
        self.snap
            .last_thickness
            .filter(|t| t.is_finite() && *t > 0.0)
    }

    fn zone(&self, origin: ZoneOrigin) -> Zone {
        Zone {
            state: self.state,
            origin,
            axis: None,
            thickness: None,
            height: None,
            varco: false,
        }
    }

    /// Una zona dall'asse è `muro-due-punti` per le demolizioni (anche con lo
    /// spessore digitato) e `linea-spessore` per i muri nuovi.
    fn axis_origin(&self) -> ZoneOrigin {
        match self.state {
            ZoneState::Demolito => ZoneOrigin::MuroDuePunti,
            ZoneState::Nuovo => ZoneOrigin::LineaSpessore,
        }
    }

    /// La zona del muro: asse (la mezzeria tra le facce) e spessore scritti.
    fn wall_candidate(&self, rect: &WallRect) -> Candidate {
        let ring = rect.ring();
        Candidate {
            zone: Zone {
                axis: Some(rect.axis),
                thickness: Some(rect.thickness),
                ..self.zone(self.axis_origin())
            },
            area: signed_area(&ring).abs(),
            rings: vec![ring],
            paths: None,
        }
    }

    /// Una zona su un solo anello di lati dritti; `None` se non ha area.
    fn ring_candidate(&self, ring: Vec<[f64; 2]>, origin: ZoneOrigin) -> Option<Candidate> {
        let area = signed_area(&ring).abs();
        (area > self.tol() * self.tol()).then(|| Candidate {
            zone: self.zone(origin),
            rings: vec![ring],
            paths: None,
            area,
        })
    }

    /// La zona del locale attorno al punto: area da `net_area` e, quando è
    /// esatta, la campitura sui percorsi con gli archi veri. Se `net_area`
    /// non si fida dei percorsi esatti (lato attribuibile a due curve, scarto
    /// dalle facce), la campitura va sugli anelli, come l'area mostrata.
    fn interior_candidate(&self, region: Region) -> Candidate {
        let (area, exact) = net_area(&region, &self.snap.walls, self.tol());
        let rings: Vec<Vec<[f64; 2]>> = iter::once(region.outer).chain(region.holes).collect();
        let paths = exact
            .then(|| {
                let exterior: Vec<bool> = (0..rings.len()).map(|index| index == 0).collect();
                exact_hatch_paths(&rings, &exterior, &self.snap.walls, self.tol())
            })
            .filter(|paths| paths.len() == rings.len());
        Candidate {
            zone: self.zone(ZoneOrigin::PuntoInterno),
            rings,
            paths,
            area,
        }
    }

    /// La zona sulla polilinea scelta, archi compresi; `None` se non è una
    /// `LWPOLYLINE` chiusa in pianta e con area.
    fn polyline_candidate(&self, entity: &EntityType) -> Option<Candidate> {
        let vertices = closed_outline(entity, self.tol())?;
        let area = outline_area(&vertices);
        if !(area > self.tol() * self.tol()) {
            return None;
        }
        let mut edge = PolylineEdge::new(Vec::new(), true);
        for &(p, bulge) in &vertices {
            edge.add_vertex(Vector2::new(p[0], p[1]), bulge);
        }
        let path = outer_path(edge);
        let zone = self.zone(ZoneOrigin::PolilineaEsistente);
        let rings = read_zone(
            Handle::NULL,
            &zone_hatch_from_paths(&zone, vec![path.clone()]),
        )?
        .rings;
        Some(Candidate {
            zone,
            rings,
            paths: Some(vec![path]),
            area,
        })
    }

    /// Il rettangolo con gli angoli opposti `a` e `b`, lati lungo l'asse X del
    /// piano di lavoro (ruotato attorno a Z) o del mondo; antiorario. `None` se
    /// un lato non supera la tolleranza.
    fn rectangle(&self, a: [f64; 2], b: [f64; 2]) -> Option<Vec<[f64; 2]>> {
        let (x, z) = (self.plane.x, self.plane.z);
        let length = x.x.hypot(x.y);
        let u = if z.x.abs() <= 1e-9 && z.y.abs() <= 1e-9 && length > 1e-9 {
            [x.x / length, x.y / length]
        } else {
            [1.0, 0.0]
        };
        let v = [-u[1], u[0]];
        let d = [b[0] - a[0], b[1] - a[1]];
        let along = d[0] * u[0] + d[1] * u[1];
        let across = d[0] * v[0] + d[1] * v[1];
        if !(along.abs() > self.tol() && across.abs() > self.tol()) {
            return None;
        }
        let mut ring = vec![
            a,
            [a[0] + u[0] * along, a[1] + u[1] * along],
            b,
            [a[0] + v[0] * across, a[1] + v[1] * across],
        ];
        if signed_area(&ring) < 0.0 {
            ring.reverse();
        }
        Some(ring)
    }

    /// Fine del secondo punto sull'asse.
    fn axis_end(&mut self, p1: [f64; 2], p2: [f64; 2]) -> CmdResult {
        match self.state {
            ZoneState::Demolito => {
                let max = self.snap.unit.metres(MAX_WALL_THICKNESS_M);
                match rect_from_axis(&self.snap.walls, p1, p2, max, self.tol()) {
                    Ok(rect) => {
                        let candidate = self.wall_candidate(&rect);
                        self.propose(candidate)
                    }
                    // Punti coincidenti: si richiede il secondo.
                    Err(AxisMiss::Degenerate) => CmdResult::NeedPoint,
                    Err(AxisMiss::NoFaces | AxisMiss::OneFace) => {
                        self.step = Step::Thickness {
                            p1,
                            p2,
                            faces_missing: true,
                        };
                        CmdResult::NeedPoint
                    }
                }
            }
            ZoneState::Nuovo => {
                if distance(p1, p2) > self.tol() {
                    self.step = Step::Thickness {
                        p1,
                        p2,
                        faces_missing: false,
                    };
                }
                CmdResult::NeedPoint
            }
        }
    }

    fn with_thickness(&mut self, thickness: f64) -> CmdResult {
        let Step::Thickness { p1, p2, .. } = self.step else {
            return CmdResult::NeedPoint;
        };
        match rect_from_axis_thickness(p1, p2, thickness) {
            Some(rect) => {
                let candidate = self.wall_candidate(&rect);
                self.propose(candidate)
            }
            None => CmdResult::ReportError("Spessore non valido.".into()),
        }
    }

    fn polygon_point(&mut self, p: [f64; 2]) -> CmdResult {
        let tol = self.tol();
        let Step::Polygon { points } = &mut self.step else {
            return CmdResult::NeedPoint;
        };
        if points.len() >= 3 && distance(points[0], p) <= tol {
            return self.close_polygon();
        }
        if !points.last().is_some_and(|&last| distance(last, p) <= tol) {
            points.push(p);
        }
        CmdResult::NeedPoint
    }

    /// Invio nel poligono: con tre punti almeno chiude, senza punti esce.
    fn close_polygon(&mut self) -> CmdResult {
        let Step::Polygon { points } = &self.step else {
            return CmdResult::NeedPoint;
        };
        if points.is_empty() {
            return CmdResult::Cancel;
        }
        if points.len() < 3 {
            return CmdResult::NeedPoint;
        }
        let mut ring = points.clone();
        if signed_area(&ring) < 0.0 {
            ring.reverse();
        }
        match self.ring_candidate(ring, ZoneOrigin::Poligono) {
            Some(candidate) => self.propose(candidate),
            None => CmdResult::NeedPoint,
        }
    }

    /// La zona c'è: le demolizioni chiedono del varco, poi si convalida.
    fn propose(&mut self, candidate: Candidate) -> CmdResult {
        match self.state {
            ZoneState::Demolito => {
                self.step = Step::Varco(candidate);
                CmdResult::NeedPoint
            }
            ZoneState::Nuovo => self.check(candidate),
        }
    }

    /// Con avvisi si chiede conferma, altrimenti la zona va nel disegno.
    fn check(&mut self, candidate: Candidate) -> CmdResult {
        let notices = validate(&candidate.rings, self.state, &self.snap);
        if notices.is_empty() {
            self.finish(candidate)
        } else {
            self.step = Step::Confirm { candidate, notices };
            CmdResult::NeedPoint
        }
    }

    fn finish(&mut self, candidate: Candidate) -> CmdResult {
        self.step = Step::Done;
        CmdResult::CommitEntitiesAndExit(vec![candidate.hatch()])
    }

    fn answer_varco(&mut self, varco: bool) -> CmdResult {
        match std::mem::replace(&mut self.step, Step::Done) {
            Step::Varco(mut candidate) => {
                candidate.zone.varco = varco;
                self.check(candidate)
            }
            other => {
                self.step = other;
                CmdResult::NeedPoint
            }
        }
    }

    fn answer_confirm(&mut self, yes: bool) -> CmdResult {
        match std::mem::replace(&mut self.step, Step::Done) {
            Step::Confirm { candidate, .. } if yes => self.finish(candidate),
            Step::Confirm { .. } => CmdResult::Cancel,
            other => {
                self.step = other;
                CmdResult::NeedPoint
            }
        }
    }

    fn preview_colour(&self) -> [f32; 4] {
        match self.state {
            ZoneState::Demolito => PREVIEW_GIALLO,
            ZoneState::Nuovo => PREVIEW_ROSSO,
        }
    }

    /// Il contorno che l'anteprima mostra ora, e se è chiuso.
    fn preview_outline(&self, cursor: [f64; 2]) -> Option<(Vec<[f64; 2]>, bool)> {
        let wall = |p1: [f64; 2], p2: [f64; 2], found: Option<WallRect>| match found {
            Some(rect) => Some((rect.ring(), true)),
            None => (distance(p1, p2) > self.tol()).then(|| (vec![p1, p2], false)),
        };
        match &self.step {
            Step::AxisEnd { p1 } => {
                let found = match self.state {
                    ZoneState::Demolito => {
                        let max = self.snap.unit.metres(MAX_WALL_THICKNESS_M);
                        rect_from_axis(&self.snap.walls, *p1, cursor, max, self.tol()).ok()
                    }
                    ZoneState::Nuovo => self
                        .last_thickness()
                        .and_then(|t| rect_from_axis_thickness(*p1, cursor, t)),
                };
                wall(*p1, cursor, found)
            }
            Step::Thickness { p1, p2, .. } => {
                let found = self
                    .last_thickness()
                    .and_then(|t| rect_from_axis_thickness(*p1, *p2, t));
                wall(*p1, *p2, found)
            }
            Step::RectSecond { first } => self.rectangle(*first, cursor).map(|ring| (ring, true)),
            Step::Polygon { points } if !points.is_empty() => {
                let mut outline = points.clone();
                outline.push(cursor);
                let closed = outline.len() >= 3;
                Some((outline, closed))
            }
            Step::Varco(candidate) | Step::Confirm { candidate, .. } => {
                Some((candidate.rings[0].clone(), true))
            }
            _ => None,
        }
    }
}

/// Il numero con la virgola e senza zeri in coda: `0,12`, `120`.
fn format_number(value: f64) -> String {
    let text = format!("{value:.6}");
    text.trim_end_matches('0')
        .trim_end_matches('.')
        .replace('.', ",")
}

/// Un numero con la virgola o il punto decimale (uno solo, una volta).
fn parse_number(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.chars().filter(|c| matches!(c, ',' | '.')).count() > 1 {
        return None;
    }
    text.replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

/// `S`/`SÌ`/`SI` o `N`/`NO`, già in maiuscolo.
fn yes_no(word: &str) -> Option<bool> {
    match word {
        "S" | "SI" | "SÌ" => Some(true),
        "N" | "NO" => Some(false),
        _ => None,
    }
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// I vertici (punto, bulge) di una `LWPOLYLINE` chiusa, nel piano XY del
/// mondo: chiusa dal flag o con l'ultimo vertice sul primo (che si toglie).
/// Una normale −Z (polilinea specchiata) rovescia la X dell'OCS e il verso
/// degli archi; una normale fuori asse non è in pianta. La quota si ignora.
fn closed_outline(entity: &EntityType, tol: f64) -> Option<Vec<([f64; 2], f64)>> {
    let EntityType::LwPolyline(polyline) = entity else {
        return None;
    };
    let normal = polyline.normal;
    if normal.x.abs() > 1e-9 || normal.y.abs() > 1e-9 || normal.z == 0.0 {
        return None;
    }
    let mirrored = normal.z < 0.0;
    let mut vertices: Vec<([f64; 2], f64)> = polyline
        .vertices
        .iter()
        .map(|vertex| {
            let (x, bulge) = if mirrored {
                (-vertex.location.x, -vertex.bulge)
            } else {
                (vertex.location.x, vertex.bulge)
            };
            ([x, vertex.location.y], bulge)
        })
        .collect();
    let ends_meet =
        vertices.len() > 2 && distance(vertices[0].0, vertices[vertices.len() - 1].0) <= tol;
    if ends_meet {
        vertices.pop();
    } else if !polyline.is_closed {
        return None;
    }
    (vertices.len() >= 2).then_some(vertices)
}

/// Il percorso esterno di una zona su un lato a polilinea.
fn outer_path(edge: PolylineEdge) -> BoundaryPath {
    let mut path = BoundaryPath::with_flags(BoundaryPathFlags::from_bits(
        BoundaryPathFlags::OUTERMOST.bits() | BoundaryPathFlags::EXTERNAL.bits(),
    ));
    path.add_edge(BoundaryEdge::Polyline(edge));
    path
}

/// L'area del contorno con gli archi veri, misurata attorno al primo
/// vertice: a coordinate georiferite `boundary_area` perderebbe le cifre
/// (vedi `walls::net_area`).
fn outline_area(vertices: &[([f64; 2], f64)]) -> f64 {
    let origin = vertices[0].0;
    let mut edge = PolylineEdge::new(Vec::new(), true);
    for &(p, bulge) in vertices {
        edge.add_vertex(Vector2::new(p[0] - origin[0], p[1] - origin[1]), bulge);
    }
    let mut hatch = Hatch::new();
    hatch.style = HatchStyleType::Normal;
    hatch.paths = vec![outer_path(edge)];
    boundary_area(&hatch)
}

/// Gli avvisi su una zona proposta (spec §5.1). `candidate[0]` è il contorno
/// esterno, gli altri anelli sono buchi; coordinate in unità del disegno.
///
/// - `Demolito`: [`Notice::ZonaSenzaMuri`] se nessun segmento dei muri tocca
///   la zona chiusa, bordo compreso (le facce del muro tolto stanno sul bordo
///   della zona ad asse).
/// - `Nuovo`: [`Notice::NuovoSuEsistente`] se un tratto di un segmento dei
///   muri ha il punto medio strettamente dentro la zona (oltre `tol` dal
///   bordo) e fuori da ogni zona `Demolito` (bordo compreso), oppure se la
///   zona ricalca un muro esistente non demolito ([`sits_on_a_wall`]). Le
///   facce su cui il muro nuovo soltanto si appoggia stanno sul bordo e non
///   contano.
/// - [`Notice::StessoStatoSovrapposte`] se la zona si sovrappone a una zona
///   dello stesso stato senza coincidere (anche quando una sta dentro
///   l'altra). Le zone di stato opposto non danno avviso (ricostruzione).
pub fn validate(candidate: &[Vec<[f64; 2]>], state: ZoneState, snap: &ZoneSnapshot) -> Vec<Notice> {
    let tol = snap.tol;
    let mut notices = Vec::new();
    if candidate.first().map_or(true, |outer| outer.len() < 3) {
        return notices;
    }
    let area = bounds(candidate);
    let segments = || {
        snap.walls
            .values()
            .flat_map(|source| source.segments.iter().copied())
            .filter(|segment| meets(&line_bounds(segment), &area, tol))
    };
    match state {
        ZoneState::Demolito => {
            if !segments().any(|segment| touches(candidate, segment, tol)) {
                notices.push(Notice::ZonaSenzaMuri);
            }
        }
        ZoneState::Nuovo => {
            let demolished: Vec<&[Vec<[f64; 2]>]> = snap
                .zones
                .iter()
                .filter(|z| z.zone.state == ZoneState::Demolito)
                .filter(|z| meets(&bounds(&z.rings), &area, tol))
                .map(|z| z.rings.as_slice())
                .collect();
            let walls: Vec<Line> = segments().collect();
            let max_thickness = snap.unit.metres(MAX_WALL_THICKNESS_M);
            if walls
                .iter()
                .any(|&segment| builds_over(candidate, segment, &demolished, tol))
                || sits_on_a_wall(&candidate[0], &walls, &demolished, max_thickness, tol)
            {
                notices.push(Notice::NuovoSuEsistente);
            }
        }
    }
    let overlapping = snap
        .zones
        .iter()
        .filter(|z| z.zone.state == state)
        .filter(|z| meets(&bounds(&z.rings), &area, tol))
        .any(|z| overlaps_in_part(candidate, &z.rings, tol));
    if overlapping {
        notices.push(Notice::StessoStatoSovrapposte);
    }
    notices
}

type Bounds = ([f64; 2], [f64; 2]);

fn bounds(rings: &[Vec<[f64; 2]>]) -> Bounds {
    rings.iter().flatten().fold(
        ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]),
        |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        },
    )
}

fn line_bounds(line: &Line) -> Bounds {
    let (a, b) = (line.start, line.end);
    (
        [a[0].min(b[0]), a[1].min(b[1])],
        [a[0].max(b[0]), a[1].max(b[1])],
    )
}

/// I due riquadri si toccano, con `tol` di margine?
fn meets(a: &Bounds, b: &Bounds, tol: f64) -> bool {
    (0..2).all(|axis| a.0[axis] <= b.1[axis] + tol && b.0[axis] <= a.1[axis] + tol)
}

/// I lati degli anelli.
fn edges(rings: &[Vec<[f64; 2]>]) -> impl Iterator<Item = Line> + '_ {
    rings.iter().flat_map(|ring| {
        ring.iter()
            .zip(ring.iter().cycle().skip(1))
            .map(|(&start, &end)| Line { start, end })
    })
}

fn lerp(line: Line, t: f64) -> [f64; 2] {
    [
        line.start[0] + t * (line.end[0] - line.start[0]),
        line.start[1] + t * (line.end[1] - line.start[1]),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Inside,
    Border,
    Outside,
}

/// Dove sta `p` rispetto alla zona (esterno meno buchi): sul bordo entro
/// `tol`, altrimenti dentro o fuori secondo la regola pari/dispari su tutti
/// gli anelli.
fn side(rings: &[Vec<[f64; 2]>], p: [f64; 2], tol: f64) -> Side {
    let mut inside = false;
    for edge in edges(rings) {
        if distance_to(&Curve::Line(edge), p) <= tol {
            return Side::Border;
        }
        let (a, b) = (edge.start, edge.end);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if x > p[0] {
                inside = !inside;
            }
        }
    }
    if inside {
        Side::Inside
    } else {
        Side::Outside
    }
}

/// Il segmento tocca la zona chiusa, bordo compreso?
fn touches(rings: &[Vec<[f64; 2]>], segment: Line, tol: f64) -> bool {
    let tolerance = Tolerance::new(tol);
    side(rings, segment.start, tol) != Side::Outside
        || side(rings, segment.end, tol) != Side::Outside
        || edges(rings)
            .any(|edge| segment_crossing(segment, edge, tolerance) != SegmentCrossing::None)
}

/// Un tratto del segmento passa strettamente dentro la zona, fuori dalle
/// zone demolite? Il segmento si divide dove incontra i lati della zona e
/// delle zone demolite; decide il punto medio di ogni tratto più lungo di
/// `tol`.
fn builds_over(
    rings: &[Vec<[f64; 2]>],
    segment: Line,
    demolished: &[&[Vec<[f64; 2]>]],
    tol: f64,
) -> bool {
    let length = distance(segment.start, segment.end);
    if !(length > tol) {
        return false;
    }
    let tolerance = Tolerance::new(tol);
    let mut cuts = vec![0.0, 1.0];
    for edge in edges(rings).chain(demolished.iter().flat_map(|zone| edges(zone))) {
        match segment_crossing(segment, edge, tolerance) {
            SegmentCrossing::Point { a, .. } => cuts.push(a.clamp(0.0, 1.0)),
            SegmentCrossing::Overlap { a, .. } => {
                cuts.extend(a.map(|t| t.clamp(0.0, 1.0)));
            }
            SegmentCrossing::None => {}
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.windows(2).any(|pair| {
        (pair[1] - pair[0]) * length > tol && {
            let p = lerp(segment, (pair[0] + pair[1]) / 2.0);
            side(rings, p, tol) == Side::Inside
                && demolished
                    .iter()
                    .all(|zone| side(zone, p, tol) == Side::Outside)
        }
    })
}

/// La zona ricalca un muro esistente (decisione R17)? Due lati opposti del
/// contorno esterno, paralleli entro [`FACE_PARALLEL_DEGREES`] e con le
/// proiezioni che si coprono, ciascuno coperto per più di metà della sua
/// lunghezza da segmenti dei muri fuori dalle zone demolite (bordo compreso).
/// La distanza tra i due lati deve superare `tol`, non superare lo spessore
/// massimo di un muro (come la ricerca delle facce dall'asse) ed essere minore
/// della lunghezza di entrambi: sono le facce del muro, non le estremità di
/// un muro nuovo appoggiato tra due muri (una tramezza in mezzo a un locale o
/// che chiude un corridoio) né i lati di una zona che riempie un locale.
fn sits_on_a_wall(
    outer: &[[f64; 2]],
    walls: &[Line],
    demolished: &[&[Vec<[f64; 2]>]],
    max_thickness: f64,
    tol: f64,
) -> bool {
    let sides: Vec<Line> = outer
        .iter()
        .zip(outer.iter().cycle().skip(1))
        .map(|(&start, &end)| Line { start, end })
        .filter(|&side| covered_by_walls(side, walls, demolished, tol))
        .collect();
    sides.iter().enumerate().any(|(index, &first)| {
        sides[index + 1..]
            .iter()
            .any(|&second| facing(first, second, max_thickness, tol))
    })
}

/// Il lato è coperto per più di metà da segmenti dei muri (sovrapposti entro
/// `tol`) fuori dalle zone demolite?
fn covered_by_walls(
    ring_side: Line,
    walls: &[Line],
    demolished: &[&[Vec<[f64; 2]>]],
    tol: f64,
) -> bool {
    let length = distance(ring_side.start, ring_side.end);
    if !(length > tol) {
        return false;
    }
    let tolerance = Tolerance::new(tol);
    let mut spans: Vec<[f64; 2]> = Vec::new();
    let mut cuts = vec![0.0, 1.0];
    for &wall in walls {
        if let SegmentCrossing::Overlap { a, .. } = segment_crossing(ring_side, wall, tolerance) {
            let span = [
                a[0].min(a[1]).clamp(0.0, 1.0),
                a[0].max(a[1]).clamp(0.0, 1.0),
            ];
            cuts.extend(span);
            spans.push(span);
        }
    }
    if spans.is_empty() {
        return false;
    }
    for edge in demolished.iter().flat_map(|zone| edges(zone)) {
        match segment_crossing(ring_side, edge, tolerance) {
            SegmentCrossing::Point { a, .. } => cuts.push(a.clamp(0.0, 1.0)),
            SegmentCrossing::Overlap { a, .. } => cuts.extend(a.map(|t| t.clamp(0.0, 1.0))),
            SegmentCrossing::None => {}
        }
    }
    cuts.sort_by(f64::total_cmp);
    let covered: f64 = cuts
        .windows(2)
        .filter(|pair| {
            let t = (pair[0] + pair[1]) / 2.0;
            let p = lerp(ring_side, t);
            spans.iter().any(|span| span[0] <= t && t <= span[1])
                && demolished
                    .iter()
                    .all(|zone| side(zone, p, tol) == Side::Outside)
        })
        .map(|pair| (pair[1] - pair[0]) * length)
        .sum();
    covered > length / 2.0
}

/// I due lati sono le due facce di uno stesso muro? Vedi [`sits_on_a_wall`].
fn facing(first: Line, second: Line, max_thickness: f64, tol: f64) -> bool {
    let (a, b) = (
        distance(first.start, first.end),
        distance(second.start, second.end),
    );
    let d = [
        (first.end[0] - first.start[0]) / a,
        (first.end[1] - first.start[1]) / a,
    ];
    let e = [
        (second.end[0] - second.start[0]) / b,
        (second.end[1] - second.start[1]) / b,
    ];
    let sin_limit = FACE_PARALLEL_DEGREES.to_radians().sin();
    // Opposti: in un anello chiuso i due lati di un muro corrono in versi
    // contrari.
    if (d[0] * e[1] - d[1] * e[0]).abs() > sin_limit || d[0] * e[0] + d[1] * e[1] >= 0.0 {
        return false;
    }
    let from = |p: [f64; 2]| [p[0] - first.start[0], p[1] - first.start[1]];
    let middle = from(lerp(second, 0.5));
    let gap = (middle[0] * d[1] - middle[1] * d[0]).abs();
    let along = |p: [f64; 2]| {
        let v = from(p);
        v[0] * d[0] + v[1] * d[1]
    };
    let (t0, t1) = (along(second.start), along(second.end));
    let shared = t0.max(t1).min(a) - t0.min(t1).max(0.0);
    gap > tol && gap <= max_thickness && gap < a.min(b) && shared > tol
}

/// Le due zone hanno interno in comune senza coincidere? I lati di entrambe
/// dividono il piano in celle; ogni cella sta tutta dentro o tutta fuori da
/// ciascuna zona. Si campiona ogni cella: su una retta orizzontale a metà tra
/// due quote consecutive di vertici e incroci (nella fascia i lati non si
/// incrociano, quindi la retta passa per tutte le celle della fascia), a metà
/// tra due intersezioni consecutive con i lati. I campioni a meno di `tol` da
/// un bordo non decidono.
fn overlaps_in_part(a: &[Vec<[f64; 2]>], b: &[Vec<[f64; 2]>], tol: f64) -> bool {
    let tolerance = Tolerance::new(tol);
    let lines: Vec<Line> = edges(a).chain(edges(b)).collect();
    let mut levels: Vec<f64> = a.iter().chain(b).flatten().map(|p| p[1]).collect();
    for first in edges(a) {
        for second in edges(b) {
            match segment_crossing(first, second, tolerance) {
                SegmentCrossing::Point { a: t, .. } => levels.push(lerp(first, t)[1]),
                SegmentCrossing::Overlap { a: [t0, t1], .. } => {
                    levels.extend([lerp(first, t0)[1], lerp(first, t1)[1]]);
                }
                SegmentCrossing::None => {}
            }
        }
    }
    levels.sort_by(f64::total_cmp);
    let (mut shared, mut apart) = (false, false);
    for band in levels.windows(2) {
        if !(band[1] - band[0] > tol) {
            continue;
        }
        let y = (band[0] + band[1]) / 2.0;
        let mut xs: Vec<f64> = lines
            .iter()
            .filter(|line| (line.start[1] > y) != (line.end[1] > y))
            .map(|line| {
                let (p, q) = (line.start, line.end);
                p[0] + (y - p[1]) * (q[0] - p[0]) / (q[1] - p[1])
            })
            .collect();
        xs.sort_by(f64::total_cmp);
        for span in xs.windows(2) {
            if !(span[1] - span[0] > tol) {
                continue;
            }
            let p = [(span[0] + span[1]) / 2.0, y];
            match (side(a, p, tol), side(b, p, tol)) {
                (Side::Inside, Side::Inside) => shared = true,
                (Side::Inside, Side::Outside) | (Side::Outside, Side::Inside) => apart = true,
                _ => {}
            }
            if shared && apart {
                return true;
            }
        }
    }
    false
}

impl CadCommand for ZoneCommand {
    /// Le zone hanno già layer `AL-*` e colore ACI dello stato.
    fn preserve_commit_layer(&self) -> bool {
        true
    }

    fn preserve_commit_style(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        match self.state {
            ZoneState::Demolito => "AL_DEMOLISCI",
            ZoneState::Nuovo => "AL_NUOVO",
        }
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Start => match self.state {
                ZoneState::Demolito => "AL_DEMOLISCI  Primo punto sull'asse del muro o [Punto interno/Rettangolo/Poligono/Selezione]:",
                ZoneState::Nuovo => "AL_NUOVO  Primo punto sull'asse del muro nuovo o [Punto interno/Rettangolo/Poligono/Selezione]:",
            }
            .into(),
            Step::AxisEnd { .. } => "Secondo punto sull'asse:".into(),
            Step::Thickness { faces_missing, .. } => {
                let ask = match self.last_thickness() {
                    Some(t) => format!("Spessore del muro <{}>:", format_number(t)),
                    None => "Spessore del muro:".into(),
                };
                if *faces_missing {
                    format!("Facce del muro non trovate. {ask}")
                } else {
                    ask
                }
            }
            Step::Interior { missed: false } => "Punto interno dell'area:".into(),
            Step::Interior { missed: true } => {
                "Nessuna area chiusa attorno al punto. Punto interno dell'area:".into()
            }
            Step::RectFirst => "Primo angolo:".into(),
            Step::RectSecond { .. } => "Angolo opposto:".into(),
            Step::Polygon { .. } => "Punto del contorno (Invio per chiudere):".into(),
            Step::Select { rejected: false } => "Seleziona una polilinea chiusa:".into(),
            Step::Select { rejected: true } => {
                "L'oggetto scelto non è una polilinea chiusa. Seleziona una polilinea chiusa:"
                    .into()
            }
            Step::Varco(candidate) => format!(
                "Zona {}. Varco (passaggio)? [Sì/No] <No>:",
                format_m2(area_m2(candidate.area, self.snap.unit))
            ),
            Step::Confirm { notices, .. } => {
                let texts: Vec<String> = notices.iter().map(Notice::text).collect();
                format!("{} Confermi la zona? [Sì/No] <Sì>:", texts.join(" "))
            }
            Step::Done => String::new(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match &self.step {
            Step::Start => vec![
                CmdOption::new("Punto interno", "I"),
                CmdOption::new("Rettangolo", "R"),
                CmdOption::new("Poligono", "P"),
                CmdOption::new("Selezione", "S"),
            ],
            Step::Polygon { points } if points.len() >= 3 => vec![CmdOption::enter("Chiudi")],
            Step::Varco(_) | Step::Confirm { .. } => {
                vec![CmdOption::new("Sì", "S"), CmdOption::new("No", "N")]
            }
            _ => Vec::new(),
        }
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let p = [pt.x, pt.y];
        match self.step {
            Step::Start => {
                self.step = Step::AxisEnd { p1: p };
                CmdResult::NeedPoint
            }
            Step::AxisEnd { p1 } => self.axis_end(p1, p),
            // Un clic (o «0,12» da uno script, letto come coordinate) non è
            // uno spessore.
            Step::Thickness { .. } => CmdResult::ReportError(
                "Digita lo spessore del muro, per esempio 0,12 o 0.12.".into(),
            ),
            Step::Interior { .. } => match region_at(&self.snap.faces, p, self.tol()) {
                Some(region) => {
                    let candidate = self.interior_candidate(region);
                    self.propose(candidate)
                }
                None => {
                    self.step = Step::Interior { missed: true };
                    CmdResult::NeedPoint
                }
            },
            Step::RectFirst => {
                self.step = Step::RectSecond { first: p };
                CmdResult::NeedPoint
            }
            Step::RectSecond { first } => {
                match self
                    .rectangle(first, p)
                    .and_then(|ring| self.ring_candidate(ring, ZoneOrigin::Rettangolo))
                {
                    Some(candidate) => self.propose(candidate),
                    None => CmdResult::NeedPoint,
                }
            }
            Step::Polygon { .. } => self.polygon_point(p),
            Step::Select { .. } | Step::Varco(_) | Step::Confirm { .. } | Step::Done => {
                CmdResult::NeedPoint
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            // Invio prende lo spessore proposto; senza, si chiede ancora.
            Step::Thickness { .. } => match self.last_thickness() {
                Some(t) => self.with_thickness(t),
                None => CmdResult::NeedPoint,
            },
            Step::Polygon { .. } => self.close_polygon(),
            Step::Varco(_) => self.answer_varco(false),
            Step::Confirm { .. } => self.answer_confirm(true),
            _ => CmdResult::Cancel,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let word = text.trim().to_uppercase();
        match self.step {
            Step::Start => {
                self.step = match word.as_str() {
                    "I" | "INTERNO" | "PUNTO" => Step::Interior { missed: false },
                    "R" | "RETTANGOLO" => Step::RectFirst,
                    "P" | "POLIGONO" => Step::Polygon { points: Vec::new() },
                    "S" | "SELEZIONE" => Step::Select { rejected: false },
                    _ => return None,
                };
                Some(CmdResult::NeedPoint)
            }
            Step::Thickness { .. } => {
                let thickness = parse_number(text)?;
                Some(if thickness > 0.0 {
                    self.with_thickness(thickness)
                } else {
                    CmdResult::ReportError("Lo spessore deve essere maggiore di zero.".into())
                })
            }
            Step::Varco(_) => yes_no(&word).map(|varco| self.answer_varco(varco)),
            Step::Confirm { .. } => yes_no(&word).map(|yes| self.answer_confirm(yes)),
            _ => None,
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Thickness { .. } | Step::Varco(_) | Step::Confirm { .. } => {
                InputKind::SingleToken
            }
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::Start)
    }

    /// Il secondo angolo del rettangolo è libero: l'ortogonale lo
    /// schiaccerebbe su una retta.
    fn window_corner_pick(&self) -> bool {
        matches!(self.step, Step::RectSecond { .. })
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::Select { .. })
    }

    fn inject_before_entity_pick(&self) -> bool {
        matches!(self.step, Step::Select { .. })
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        matches!(self.step, Step::Select { .. })
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    /// Solo una `LWPOLYLINE` chiusa: un altro oggetto ripete la domanda con il
    /// motivo (decisione R18), un clic a vuoto la ripete e basta.
    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if !matches!(self.step, Step::Select { .. }) {
            return CmdResult::NeedPoint;
        }
        let picked = self.picked.take();
        if handle.is_null() {
            self.step = Step::Select { rejected: false };
            return CmdResult::NeedPoint;
        }
        let candidate = picked
            .filter(|entity| entity.common().handle == handle)
            .and_then(|entity| self.polyline_candidate(&entity));
        match candidate {
            Some(candidate) => self.propose(candidate),
            None => {
                self.step = Step::Select { rejected: true };
                CmdResult::NeedPoint
            }
        }
    }

    /// Il contorno in corso nel colore dello stato (spec §6).
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let (mut outline, closed) = self.preview_outline([pt.x, pt.y])?;
        if closed {
            outline.push(outline[0]);
        }
        let mut wire = WireModel::solid_f64(
            "al_zona_preview".into(),
            outline.iter().map(|p| [p[0], p[1], 0.0]).collect(),
            self.preview_colour(),
            false,
        );
        wire.set_fixed_screen_width(2.0);
        Some(wire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bim::fixtures::{case_a, scene_with, Form, LAYER_MURI};
    use crate::bim::zone::{self, Zone, ZoneOrigin};
    use crate::bim::{walls, ACI_GIALLO, ACI_ROSSO, LAYER_DEMOLIZIONI, LAYER_NUOVE};
    use crate::command::InputKind;
    use crate::scene::model::wire_model::WireModel;
    use codec::entities::{Arc, BoundaryEdge, Line, LwPolyline, LwVertex};
    use codec::{Color, Vector2, Vector3};

    const TOL: f64 = 1e-6;

    /// L'istantanea di `entities` (coordinate in metri), tolleranza 1e-6,
    /// senza zone e senza spessore precedente.
    fn snap(entities: Vec<EntityType>) -> ZoneSnapshot {
        let scene = scene_with(entities, 6);
        let set = walls::wall_set(&scene.document, TOL);
        let faces = walls::faces(&set.sources, TOL);
        ZoneSnapshot {
            walls: set.sources,
            faces,
            zones: Vec::new(),
            unit: DrawingUnit::Metri,
            tol: TOL,
            last_thickness: None,
        }
    }

    fn case(form: Form) -> ZoneSnapshot {
        snap(case_a(form, DrawingUnit::Metri))
    }

    fn pt(x: f64, y: f64) -> DVec3 {
        DVec3::new(x, y, 0.0)
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    #[track_caller]
    fn need_point(result: CmdResult) {
        assert!(matches!(result, CmdResult::NeedPoint));
    }

    #[track_caller]
    fn need_point_text(result: Option<CmdResult>) {
        assert!(matches!(result, Some(CmdResult::NeedPoint)));
    }

    /// L'unica entità di un `CommitEntitiesAndExit`.
    #[track_caller]
    fn committed(result: CmdResult) -> EntityType {
        let CmdResult::CommitEntitiesAndExit(mut entities) = result else {
            panic!("nessun commit")
        };
        assert_eq!(entities.len(), 1, "una sola campitura");
        entities.remove(0)
    }

    #[track_caller]
    fn zone_of(entity: &EntityType) -> ZoneGeom {
        zone::read_zone(Handle::new(1), entity).expect("la campitura non è una zona")
    }

    #[track_caller]
    fn assert_close(got: [f64; 2], want: [f64; 2]) {
        assert!(
            (got[0] - want[0]).abs() < 1e-9 && (got[1] - want[1]).abs() < 1e-9,
            "{got:?} invece di {want:?}"
        );
    }

    /// Stessi vertici, a meno dell'ordine e del punto di partenza.
    #[track_caller]
    fn assert_ring(got: &[[f64; 2]], want: &[[f64; 2]]) {
        assert_eq!(got.len(), want.len(), "{got:?} invece di {want:?}");
        for w in want {
            assert!(
                got.iter()
                    .any(|g| (g[0] - w[0]).abs() < 1e-9 && (g[1] - w[1]).abs() < 1e-9),
                "manca {w:?} in {got:?}"
            );
        }
    }

    #[track_caller]
    fn assert_style(entity: &EntityType, layer: &str, aci: i16) {
        assert_eq!(entity.common().layer, layer);
        assert_eq!(entity.common().color, Color::from_index(aci));
    }

    fn keywords(command: &ZoneCommand) -> Vec<String> {
        command.options().into_iter().map(|o| o.keyword).collect()
    }

    fn zone_geom(state: ZoneState, rings: Vec<Vec<[f64; 2]>>) -> ZoneGeom {
        ZoneGeom {
            handle: Handle::new(0x77),
            zone: Zone {
                state,
                origin: ZoneOrigin::Rettangolo,
                axis: None,
                thickness: None,
                height: None,
                varco: false,
            },
            rings,
        }
    }

    #[test]
    fn demolish_by_axis_commits_yellow_zone() {
        let mut c = ZoneCommand::new(
            ZoneState::Demolito,
            snap(case_a(Form::Continua, DrawingUnit::Metri)),
        );
        assert!(matches!(
            c.on_point(DVec3::new(4.05, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        assert!(matches!(
            c.on_point(DVec3::new(4.05, 3.0, 0.0)),
            CmdResult::NeedPoint
        ));
        assert!(c.prompt().starts_with("Zona 0,30 m². Varco"));
        let CmdResult::CommitEntitiesAndExit(v) = c.on_text_input("N").unwrap() else {
            panic!()
        };
        let z = zone::read_zone(Handle::new(1), &v[0]).unwrap().zone;
        assert_eq!(
            (z.state, z.origin, z.varco),
            (ZoneState::Demolito, ZoneOrigin::MuroDuePunti, false)
        );
        assert!((z.thickness.unwrap() - 0.10).abs() < 1e-9);

        assert_eq!(v.len(), 1);
        let (a, b) = z.axis.expect("asse");
        assert_close(a, [4.05, 0.0]);
        assert_close(b, [4.05, 3.0]);
        assert_eq!(z.height, None, "l'altezza non si riempie mai");
        assert_style(&v[0], LAYER_DEMOLIZIONI, ACI_GIALLO);
        let g = zone_of(&v[0]);
        assert_eq!(g.rings.len(), 1);
        assert_ring(&g.rings[0], &rect(4.0, 0.0, 4.1, 3.0));
        assert!(c.preserve_commit_layer() && c.preserve_commit_style());
    }

    #[test]
    fn prompts_keywords_and_input_kinds() {
        let mut c = ZoneCommand::new(ZoneState::Demolito, case(Form::Continua));
        assert_eq!(c.name(), "AL_DEMOLISCI");
        assert_eq!(
            c.prompt(),
            "AL_DEMOLISCI  Primo punto sull'asse del muro o [Punto interno/Rettangolo/Poligono/Selezione]:"
        );
        assert_eq!(c.input_kind(), InputKind::Point);
        assert!(c.point_step_accepts_keywords());
        assert_eq!(keywords(&c), ["I", "R", "P", "S"]);
        assert!(c.on_text_input("X").is_none());
        need_point(c.on_point(pt(4.05, 0.0)));
        assert_eq!(c.prompt(), "Secondo punto sull'asse:");
        assert_eq!(c.input_kind(), InputKind::Point);
        assert!(!c.point_step_accepts_keywords());

        let n = ZoneCommand::new(ZoneState::Nuovo, case(Form::Continua));
        assert_eq!(n.name(), "AL_NUOVO");
        assert_eq!(
            n.prompt(),
            "AL_NUOVO  Primo punto sull'asse del muro nuovo o [Punto interno/Rettangolo/Poligono/Selezione]:"
        );

        for (keyword, prompt) in [
            ("I", "Punto interno dell'area:"),
            ("i", "Punto interno dell'area:"),
            ("R", "Primo angolo:"),
            ("P", "Punto del contorno (Invio per chiudere):"),
            ("S", "Seleziona una polilinea chiusa:"),
            ("selezione", "Seleziona una polilinea chiusa:"),
        ] {
            let mut c = ZoneCommand::new(ZoneState::Nuovo, case(Form::Continua));
            need_point_text(c.on_text_input(keyword));
            assert_eq!(c.prompt(), prompt, "{keyword}");
            assert_eq!(c.input_kind(), InputKind::Point, "{keyword}");
            assert!(!c.point_step_accepts_keywords(), "{keyword}");
            assert_eq!(c.needs_entity_pick(), prompt.starts_with("Seleziona"));
            assert_eq!(
                c.inject_before_entity_pick(),
                prompt.starts_with("Seleziona")
            );
        }

        // Invio al primo passo chiude il comando.
        let mut c = ZoneCommand::new(ZoneState::Nuovo, case(Form::Continua));
        assert!(matches!(c.on_enter(), CmdResult::Cancel));
    }

    #[test]
    fn new_wall_by_axis_and_thickness() {
        let mut c = ZoneCommand::new(ZoneState::Nuovo, case(Form::Continua));
        need_point(c.on_point(pt(2.0, 0.0)));
        need_point(c.on_point(pt(2.0, 3.0)));
        assert_eq!(c.prompt(), "Spessore del muro:");
        assert_eq!(c.input_kind(), InputKind::SingleToken);
        // Senza spessore precedente Invio non inventa nulla: chiede ancora.
        need_point(c.on_enter());
        assert_eq!(c.prompt(), "Spessore del muro:");
        // Nessun avviso: le due estremità stanno sulle facce di A.
        let entity = committed(c.on_text_input("0.12").unwrap());
        let g = zone_of(&entity);
        assert_eq!(
            (g.zone.state, g.zone.origin, g.zone.varco),
            (ZoneState::Nuovo, ZoneOrigin::LineaSpessore, false)
        );
        assert!((g.zone.thickness.unwrap() - 0.12).abs() < 1e-9);
        let (a, b) = g.zone.axis.expect("asse");
        assert_close(a, [2.0, 0.0]);
        assert_close(b, [2.0, 3.0]);
        assert_eq!(g.zone.height, None);
        assert_ring(&g.rings[0], &rect(1.94, 0.0, 2.06, 3.0));
        assert_style(&entity, LAYER_NUOVE, ACI_ROSSO);
    }

    #[test]
    fn thickness_default_comma_and_bad_values() {
        let mut s = case(Form::Continua);
        s.last_thickness = Some(0.12);
        let mut c = ZoneCommand::new(ZoneState::Nuovo, s);
        need_point(c.on_point(pt(6.0, 0.5)));
        need_point(c.on_point(pt(6.0, 2.5)));
        assert_eq!(c.prompt(), "Spessore del muro <0,12>:");
        assert!(keywords(&c).is_empty());
        // Valori non accettati: il passo resta.
        assert!(matches!(
            c.on_text_input("0"),
            Some(CmdResult::ReportError(_))
        ));
        assert!(matches!(
            c.on_text_input("-0,1"),
            Some(CmdResult::ReportError(_))
        ));
        assert!(c.on_text_input("ABC").is_none());
        assert!(c.on_text_input("0,1,2").is_none());
        // Un clic non è uno spessore.
        assert!(matches!(
            c.on_point(pt(1.0, 1.0)),
            CmdResult::ReportError(_)
        ));
        assert_eq!(c.prompt(), "Spessore del muro <0,12>:");
        // Invio prende lo spessore precedente.
        let g = zone_of(&committed(c.on_enter()));
        assert!((g.zone.thickness.unwrap() - 0.12).abs() < 1e-9);
        assert_ring(&g.rings[0], &rect(5.94, 0.5, 6.06, 2.5));

        // La virgola vale come separatore decimale.
        let mut c = ZoneCommand::new(ZoneState::Nuovo, case(Form::Continua));
        need_point(c.on_point(pt(6.0, 0.5)));
        need_point(c.on_point(pt(6.0, 2.5)));
        let g = zone_of(&committed(c.on_text_input("0,25").unwrap()));
        assert!((g.zone.thickness.unwrap() - 0.25).abs() < 1e-9);

        // Due punti coincidenti non sono un asse: si richiede il secondo.
        let mut c = ZoneCommand::new(ZoneState::Nuovo, case(Form::Continua));
        need_point(c.on_point(pt(6.0, 0.5)));
        need_point(c.on_point(pt(6.0, 0.5)));
        assert_eq!(c.prompt(), "Secondo punto sull'asse:");
    }

    #[test]
    fn demolish_asks_thickness_when_faces_missing() {
        let mut c = ZoneCommand::new(ZoneState::Demolito, case(Form::Continua));
        need_point(c.on_point(pt(2.0, 0.5)));
        need_point(c.on_point(pt(2.0, 2.5)));
        assert_eq!(c.prompt(), "Facce del muro non trovate. Spessore del muro:");
        assert_eq!(c.input_kind(), InputKind::SingleToken);
        need_point_text(c.on_text_input("0,12"));
        assert_eq!(c.prompt(), "Zona 0,24 m². Varco (passaggio)? [Sì/No] <No>:");
        assert_eq!(c.input_kind(), InputKind::SingleToken);
        assert_eq!(keywords(&c), ["S", "N"]);
        // Dentro il locale A nessuna linea: si chiede conferma.
        need_point(c.on_enter());
        assert_eq!(
            c.prompt(),
            "La zona demolita non incrocia nessuna linea del rilievo. Confermi la zona? [Sì/No] <Sì>:"
        );
        let entity = committed(c.on_enter());
        let g = zone_of(&entity);
        assert_eq!(
            (g.zone.state, g.zone.origin, g.zone.varco),
            (ZoneState::Demolito, ZoneOrigin::MuroDuePunti, false)
        );
        assert!((g.zone.thickness.unwrap() - 0.12).abs() < 1e-9);
        let (a, b) = g.zone.axis.expect("asse");
        assert_close(a, [2.0, 0.5]);
        assert_close(b, [2.0, 2.5]);
        assert_ring(&g.rings[0], &rect(1.94, 0.5, 2.06, 2.5));
        assert_style(&entity, LAYER_DEMOLIZIONI, ACI_GIALLO);
    }

    #[test]
    fn demolish_off_centre_axis_keeps_the_wall_midline() {
        let mut c = ZoneCommand::new(ZoneState::Demolito, case(Form::ChiusaPerMuro));
        need_point(c.on_point(pt(4.02, 0.0)));
        need_point(c.on_point(pt(4.02, 3.0)));
        let g = zone_of(&committed(c.on_text_input("N").unwrap()));
        let (a, b) = g.zone.axis.expect("asse");
        assert_close(a, [4.05, 0.0]);
        assert_close(b, [4.05, 3.0]);
        assert!((g.zone.thickness.unwrap() - 0.10).abs() < 1e-9);
        assert_ring(&g.rings[0], &rect(4.0, 0.0, 4.1, 3.0));
    }

    #[test]
    fn rectangle_mode_and_varco() {
        let mut c = ZoneCommand::new(ZoneState::Demolito, case(Form::Continua));
        need_point_text(c.on_text_input("R"));
        assert_eq!(c.prompt(), "Primo angolo:");
        need_point(c.on_point(pt(4.0, 1.0)));
        assert_eq!(c.prompt(), "Angolo opposto:");
        assert!(
            c.window_corner_pick(),
            "l'ortogonale non schiaccia il rettangolo"
        );
        need_point(c.on_point(pt(4.1, 1.9)));
        assert_eq!(c.prompt(), "Zona 0,09 m². Varco (passaggio)? [Sì/No] <No>:");
        // Il rettangolo sta sulle facce del tramezzo: nessun avviso.
        let entity = committed(c.on_text_input("S").unwrap());
        let g = zone_of(&entity);
        assert!(g.zone.varco);
        assert_eq!(g.zone.origin, ZoneOrigin::Rettangolo);
        assert_eq!((g.zone.axis, g.zone.thickness), (None, None));
        assert_ring(&g.rings[0], &rect(4.0, 1.0, 4.1, 1.9));
        assert_style(&entity, LAYER_DEMOLIZIONI, ACI_GIALLO);

        // Angoli dati nell'altro verso, «Sì» per esteso.
        let mut c = ZoneCommand::new(ZoneState::Demolito, case(Form::Continua));
        need_point_text(c.on_text_input("R"));
        need_point(c.on_point(pt(4.1, 1.9)));
        need_point(c.on_point(pt(4.0, 1.0)));
        let g = zone_of(&committed(c.on_text_input("SÌ").unwrap()));
        assert!(g.zone.varco);
        assert_ring(&g.rings[0], &rect(4.0, 1.0, 4.1, 1.9));

        // Rettangolo degenere: si richiede l'angolo.
        let mut c = ZoneCommand::new(ZoneState::Demolito, case(Form::Continua));
        need_point_text(c.on_text_input("R"));
        need_point(c.on_point(pt(4.0, 1.0)));
        need_point(c.on_point(pt(4.0, 1.9)));
        assert_eq!(c.prompt(), "Angolo opposto:");
    }

    #[test]
    fn validation_asks_confirmation() {
        let mut c = ZoneCommand::new(ZoneState::Demolito, case(Form::Continua));
        need_point_text(c.on_text_input("R"));
        need_point(c.on_point(pt(1.0, 1.0)));
        need_point(c.on_point(pt(2.0, 2.0)));
        assert_eq!(c.prompt(), "Zona 1,00 m². Varco (passaggio)? [Sì/No] <No>:");
        need_point_text(c.on_text_input("N"));
        let prompt = c.prompt();
        assert!(prompt.contains("non incrocia"), "{prompt}");
        assert!(prompt.contains("Confermi la zona?"), "{prompt}");
        assert!(
            prompt.ends_with("Confermi la zona? [Sì/No] <Sì>:"),
            "{prompt}"
        );
        assert_eq!(c.input_kind(), InputKind::SingleToken);
        assert_eq!(keywords(&c), ["S", "N"]);
        assert!(matches!(c.on_text_input("N"), Some(CmdResult::Cancel)));

        // «S» conferma.
        let mut c = ZoneCommand::new(ZoneState::Demolito, case(Form::Continua));
        need_point_text(c.on_text_input("R"));
        need_point(c.on_point(pt(1.0, 1.0)));
        need_point(c.on_point(pt(2.0, 2.0)));
        need_point_text(c.on_text_input("S"));
        let g = zone_of(&committed(c.on_text_input("S").unwrap()));
        assert!(g.zone.varco);
    }

    #[test]
    fn internal_point_on_closed_tramezzo() {
        let mut c = ZoneCommand::new(ZoneState::Demolito, case(Form::ChiusaPerMuro));
        need_point_text(c.on_text_input("I"));
        need_point(c.on_point(pt(4.05, 1.5)));
        assert_eq!(c.prompt(), "Zona 0,30 m². Varco (passaggio)? [Sì/No] <No>:");
        let entity = committed(c.on_text_input("N").unwrap());
        let g = zone_of(&entity);
        assert_eq!(g.zone.origin, ZoneOrigin::PuntoInterno);
        assert_eq!((g.zone.axis, g.zone.thickness), (None, None));
        assert_ring(&g.rings[0], &rect(4.0, 0.0, 4.1, 3.0));
        let EntityType::Hatch(hatch) = &entity else {
            panic!()
        };
        assert!((crate::entities::hatch::boundary_area(hatch) - 0.30).abs() < 1e-9);
        // Percorsi esatti di `exact_hatch_paths` (lati veri, non una polilinea).
        assert!(hatch.paths[0]
            .edges
            .iter()
            .all(|edge| matches!(edge, BoundaryEdge::Line(_))));
        assert_style(&entity, LAYER_DEMOLIZIONI, ACI_GIALLO);
    }

    #[test]
    fn internal_point_keeps_true_arcs_and_islands() {
        // Mezzo disco di raggio 2 chiuso dalla corda, con un pilastro dentro.
        let wall = |mut entity: EntityType| {
            entity.common_mut().layer = LAYER_MURI.to_string();
            entity
        };
        let chord = Line::from_points(Vector3::new(-2.0, 0.0, 0.0), Vector3::new(2.0, 0.0, 0.0));
        let arc = Arc::from_center_radius_angles(Vector3::ZERO, 2.0, 0.0, std::f64::consts::PI);
        let mut pillar = LwPolyline::from_points(
            [[-0.2, 0.5], [0.2, 0.5], [0.2, 0.9], [-0.2, 0.9]]
                .iter()
                .map(|p| Vector2::new(p[0], p[1]))
                .collect(),
        );
        pillar.is_closed = true;
        let s = snap(vec![
            wall(EntityType::Line(chord)),
            wall(EntityType::Arc(arc)),
            wall(EntityType::LwPolyline(pillar)),
        ]);
        let mut c = ZoneCommand::new(ZoneState::Nuovo, s);
        need_point_text(c.on_text_input("I"));
        // Muri e pilastro stanno sul bordo: nessun avviso, la zona nuova va.
        let entity = committed(c.on_point(pt(1.0, 0.3)));
        let EntityType::Hatch(hatch) = &entity else {
            panic!()
        };
        assert_eq!(hatch.paths.len(), 2, "contorno e pilastro");
        assert!(
            hatch.paths[0].flags.bits() & 16 != 0,
            "il contorno esterno è il primo"
        );
        assert!(hatch.paths[0]
            .edges
            .iter()
            .any(|edge| matches!(edge, BoundaryEdge::CircularArc(_))));
        let area = 2.0 * std::f64::consts::PI - 0.16;
        assert!((crate::entities::hatch::boundary_area(hatch) - area).abs() < 1e-9);
        let g = zone_of(&entity);
        assert_eq!(g.zone.origin, ZoneOrigin::PuntoInterno);
        assert_eq!(g.rings.len(), 2);
        assert_style(&entity, LAYER_NUOVE, ACI_ROSSO);
    }

    #[test]
    fn internal_point_without_a_closed_area_asks_again() {
        let mut c = ZoneCommand::new(ZoneState::Demolito, case(Form::Continua));
        need_point_text(c.on_text_input("I"));
        need_point(c.on_point(pt(20.0, 20.0)));
        assert_eq!(
            c.prompt(),
            "Nessuna area chiusa attorno al punto. Punto interno dell'area:"
        );
        need_point(c.on_point(pt(2.0, 1.5)));
        assert_eq!(
            c.prompt(),
            "Zona 12,00 m². Varco (passaggio)? [Sì/No] <No>:"
        );
    }

    #[test]
    fn polygon_mode() {
        let mut c = ZoneCommand::new(ZoneState::Nuovo, case(Form::Continua));
        need_point_text(c.on_text_input("P"));
        need_point(c.on_point(pt(5.0, 1.0)));
        need_point(c.on_point(pt(6.0, 1.0)));
        // Con meno di tre punti Invio non chiude.
        need_point(c.on_enter());
        assert_eq!(c.prompt(), "Punto del contorno (Invio per chiudere):");
        need_point(c.on_point(pt(6.0, 2.0)));
        need_point(c.on_point(pt(5.5, 2.5)));
        let entity = committed(c.on_enter());
        let g = zone_of(&entity);
        assert_eq!(g.zone.origin, ZoneOrigin::Poligono);
        assert_eq!((g.zone.axis, g.zone.thickness), (None, None));
        assert_ring(
            &g.rings[0],
            &[[5.0, 1.0], [6.0, 1.0], [6.0, 2.0], [5.5, 2.5]],
        );
        assert_style(&entity, LAYER_NUOVE, ACI_ROSSO);

        // Un clic sul primo punto chiude il contorno.
        let mut c = ZoneCommand::new(ZoneState::Nuovo, case(Form::Continua));
        need_point_text(c.on_text_input("P"));
        for p in [pt(5.0, 1.0), pt(6.0, 1.0), pt(6.0, 2.0)] {
            need_point(c.on_point(p));
        }
        let g = zone_of(&committed(c.on_point(pt(5.0, 1.0))));
        assert_ring(&g.rings[0], &[[5.0, 1.0], [6.0, 1.0], [6.0, 2.0]]);

        // Invio senza punti chiude il comando.
        let mut c = ZoneCommand::new(ZoneState::Nuovo, case(Form::Continua));
        need_point_text(c.on_text_input("P"));
        assert!(matches!(c.on_enter(), CmdResult::Cancel));
    }

    #[test]
    fn selection_mode_takes_closed_lwpolylines_only() {
        let mut c = ZoneCommand::new(ZoneState::Demolito, case(Form::Continua));
        need_point_text(c.on_text_input("S"));
        let prompt = "Seleziona una polilinea chiusa:";
        let rejected =
            "L'oggetto scelto non è una polilinea chiusa. Seleziona una polilinea chiusa:";
        assert_eq!(c.prompt(), prompt);
        assert!(c.needs_entity_pick() && c.inject_before_entity_pick());

        let picked = |mut entity: EntityType, handle: u64| {
            entity.common_mut().handle = Handle::new(handle);
            entity
        };
        // Una linea: no, e si dice perché.
        let line = Line::from_points(Vector3::new(4.0, 0.0, 0.0), Vector3::new(4.0, 3.0, 0.0));
        c.inject_picked_entity(picked(EntityType::Line(line), 0x10));
        need_point(c.on_entity_pick(Handle::new(0x10), pt(4.0, 1.0)));
        assert_eq!(c.prompt(), rejected);
        assert!(c.needs_entity_pick(), "si resta a scegliere");
        // Nessun oggetto sotto il cursore: la domanda e basta.
        need_point(c.on_entity_pick(Handle::NULL, pt(4.0, 1.0)));
        assert_eq!(c.prompt(), prompt);
        // Una polilinea aperta: no.
        let points: Vec<Vector2> = rect(4.0, 0.0, 4.1, 3.0)
            .iter()
            .map(|p| Vector2::new(p[0], p[1]))
            .collect();
        let open = LwPolyline::from_points(points.clone());
        c.inject_picked_entity(picked(EntityType::LwPolyline(open), 0x11));
        need_point(c.on_entity_pick(Handle::new(0x11), pt(4.0, 1.0)));
        assert_eq!(c.prompt(), rejected);
        // La polilinea chiusa del tramezzo: sì.
        let mut closed = LwPolyline::from_points(points);
        closed.is_closed = true;
        c.inject_picked_entity(picked(EntityType::LwPolyline(closed), 0x12));
        need_point(c.on_entity_pick(Handle::new(0x12), pt(4.0, 1.0)));
        assert_eq!(c.prompt(), "Zona 0,30 m². Varco (passaggio)? [Sì/No] <No>:");
        let entity = committed(c.on_text_input("N").unwrap());
        let g = zone_of(&entity);
        assert_eq!(g.zone.origin, ZoneOrigin::PolilineaEsistente);
        assert_eq!((g.zone.axis, g.zone.thickness), (None, None));
        assert_ring(&g.rings[0], &rect(4.0, 0.0, 4.1, 3.0));
    }

    #[test]
    fn selection_keeps_polyline_arcs() {
        // Lato curvo da (1;0) a (-1;0) (mezzo cerchio verso l'alto) più una
        // striscia 2 × 0,5: π/2 + 1.
        let mut bulged = LwPolyline::new();
        bulged.add_vertex(LwVertex::with_bulge(Vector2::new(1.0, 0.0), 1.0));
        bulged.add_vertex(LwVertex::new(Vector2::new(-1.0, 0.0)));
        bulged.add_vertex(LwVertex::new(Vector2::new(-1.0, -0.5)));
        bulged.add_vertex(LwVertex::new(Vector2::new(1.0, -0.5)));
        bulged.is_closed = true;
        bulged.common.handle = Handle::new(0x20);
        let mut c = ZoneCommand::new(ZoneState::Demolito, snap(Vec::new()));
        need_point_text(c.on_text_input("S"));
        c.inject_picked_entity(EntityType::LwPolyline(bulged));
        need_point(c.on_entity_pick(Handle::new(0x20), pt(0.0, 0.0)));
        // L'area mostrata è quella dell'arco vero, non delle sue corde.
        assert_eq!(c.prompt(), "Zona 2,57 m². Varco (passaggio)? [Sì/No] <No>:");
        need_point_text(c.on_text_input("N"));
        // Nessun muro nel disegno: si conferma.
        let entity = committed(c.on_text_input("S").unwrap());
        let EntityType::Hatch(hatch) = &entity else {
            panic!()
        };
        let want = std::f64::consts::FRAC_PI_2 + 1.0;
        assert!((crate::entities::hatch::boundary_area(hatch) - want).abs() < 1e-9);
        assert_eq!(zone_of(&entity).zone.origin, ZoneOrigin::PolilineaEsistente);
    }

    #[test]
    fn validate_demolished_needs_a_line_border_included() {
        let s = case(Form::Continua);
        let d = ZoneState::Demolito;
        // Dentro A, lontano dalle linee.
        assert_eq!(
            validate(&[rect(1.0, 1.0, 2.0, 2.0)], d, &s),
            [Notice::ZonaSenzaMuri]
        );
        // Sul tramezzo: le facce stanno sul bordo della zona.
        assert!(validate(&[rect(4.0, 0.0, 4.1, 3.0)], d, &s).is_empty());
        // Un solo lato appoggiato a una faccia.
        assert!(validate(&[rect(1.0, 0.0, 2.0, 1.0)], d, &s).is_empty());
        // Una linea che attraversa senza estremi dentro.
        assert!(validate(&[rect(3.5, 1.0, 4.5, 2.0)], d, &s).is_empty());
        // Nessun muro nel disegno.
        assert_eq!(
            validate(&[rect(1.0, 1.0, 2.0, 2.0)], d, &snap(Vec::new())),
            [Notice::ZonaSenzaMuri]
        );
    }

    #[test]
    fn validate_new_over_existing_walls() {
        let mut s = case(Form::Continua);
        let n = ZoneState::Nuovo;
        // Muro nuovo in A, appoggiato alle facce: nessun avviso.
        assert!(validate(&[rect(1.94, 0.0, 2.06, 3.0)], n, &s).is_empty());
        assert!(validate(&[rect(1.94, 0.5, 2.06, 2.5)], n, &s).is_empty());
        // Sopra il tramezzo, che attraversa la zona.
        let across = [rect(3.5, 1.0, 4.6, 2.0)];
        assert_eq!(validate(&across, n, &s), [Notice::NuovoSuEsistente]);
        // Con il tramezzo demolito non c'è più nulla da segnalare.
        s.zones = vec![zone_geom(
            ZoneState::Demolito,
            vec![rect(4.0, 0.0, 4.1, 3.0)],
        )];
        assert!(validate(&across, n, &s).is_empty());
        // Se la demolizione copre solo una parte del tramezzo, il resto conta.
        s.zones = vec![zone_geom(
            ZoneState::Demolito,
            vec![rect(4.0, 0.0, 4.1, 1.5)],
        )];
        assert_eq!(validate(&across, n, &s), [Notice::NuovoSuEsistente]);
    }

    #[test]
    fn new_wall_on_an_existing_wall_asks_confirmation() {
        // R17: AL_NUOVO sull'asse del tramezzo, spessore 0,10 digitato con la
        // virgola: la zona ricalca il muro esistente, nelle due forme.
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let mut c = ZoneCommand::new(ZoneState::Nuovo, case(form));
            need_point(c.on_point(pt(4.05, 0.0)));
            need_point(c.on_point(pt(4.05, 3.0)));
            need_point_text(c.on_text_input("0,10"));
            assert_eq!(
                c.prompt(),
                "La zona nuova si sovrappone a un muro esistente non demolito. Confermi la zona? [Sì/No] <Sì>:",
                "{form:?}"
            );
            let g = zone_of(&committed(c.on_text_input("S").unwrap()));
            assert_eq!(
                (g.zone.state, g.zone.origin),
                (ZoneState::Nuovo, ZoneOrigin::LineaSpessore)
            );
            assert!((g.zone.thickness.unwrap() - 0.10).abs() < 1e-9);
            assert_ring(&g.rings[0], &rect(4.0, 0.0, 4.1, 3.0));
        }
        // Con il tramezzo demolito la ricostruzione è consentita: niente.
        let mut s = case(Form::Continua);
        s.zones = vec![zone_geom(
            ZoneState::Demolito,
            vec![rect(4.0, 0.0, 4.1, 3.0)],
        )];
        let mut c = ZoneCommand::new(ZoneState::Nuovo, s);
        need_point(c.on_point(pt(4.05, 0.0)));
        need_point(c.on_point(pt(4.05, 3.0)));
        let g = zone_of(&committed(c.on_text_input("0,10").unwrap()));
        assert!((g.zone.thickness.unwrap() - 0.10).abs() < 1e-9);
    }

    #[test]
    fn validate_new_on_a_wall_only_between_its_faces() {
        let s = case(Form::Continua);
        let n = ZoneState::Nuovo;
        // Sul tramezzo, intero o in parte.
        assert_eq!(
            validate(&[rect(4.0, 0.0, 4.1, 3.0)], n, &s),
            [Notice::NuovoSuEsistente]
        );
        assert_eq!(
            validate(&[rect(4.0, 1.0, 4.1, 2.0)], n, &s),
            [Notice::NuovoSuEsistente]
        );
        // Accostato a una faccia sola (controparete in B): niente.
        assert!(validate(&[rect(4.1, 0.0, 4.3, 3.0)], n, &s).is_empty());
        // Tramezza nuova in A: le estremità stanno sulle facce, a 3 m.
        assert!(validate(&[rect(1.94, 0.0, 2.06, 3.0)], n, &s).is_empty());
        // Una zona che riempie il locale A.
        assert!(validate(&[rect(0.0, 0.0, 4.0, 3.0)], n, &s).is_empty());
        // Un muro nuovo che chiude un corridoio largo 1,10 m: le estremità
        // stanno sulle facce, più vicine dello spessore massimo, ma sono più
        // corte della distanza tra loro.
        let wall = |a: [f64; 2], b: [f64; 2]| {
            let mut line =
                Line::from_points(Vector3::new(a[0], a[1], 0.0), Vector3::new(b[0], b[1], 0.0));
            line.common.layer = LAYER_MURI.to_string();
            EntityType::Line(line)
        };
        let corridor = snap(vec![
            wall([0.0, 0.0], [10.0, 0.0]),
            wall([0.0, 1.1], [10.0, 1.1]),
        ]);
        assert!(validate(&[rect(5.0, 0.0, 5.12, 1.1)], n, &corridor).is_empty());
    }

    #[test]
    fn validate_same_state_overlaps() {
        let mut s = case(Form::Continua);
        s.zones = vec![zone_geom(
            ZoneState::Demolito,
            vec![rect(4.0, 0.0, 4.1, 3.0)],
        )];
        let d = ZoneState::Demolito;
        // In parte sovrapposta (tocca le facce del tramezzo: nessun altro avviso).
        assert_eq!(
            validate(&[rect(4.0, 2.0, 4.1, 3.2)], d, &s),
            [Notice::StessoStatoSovrapposte]
        );
        // Dentro l'altra: non coincidenti, quindi «in parte».
        assert_eq!(
            validate(&[rect(4.0, 1.0, 4.1, 2.0)], d, &s),
            [Notice::StessoStatoSovrapposte]
        );
        // Coincidente: niente.
        assert!(validate(&[rect(4.0, 0.0, 4.1, 3.0)], d, &s).is_empty());
        // Accostata, con un lato in comune: niente.
        assert!(validate(&[rect(4.1, 0.0, 4.2, 3.0)], d, &s).is_empty());
        // Stato opposto sovrapposto in parte: niente (ricostruzione).
        assert!(validate(&[rect(4.0, 2.0, 4.1, 3.0)], ZoneState::Nuovo, &s).is_empty());
        // Due avvisi insieme, nell'ordine: senza muri, poi sovrapposte.
        s.zones = vec![zone_geom(
            ZoneState::Demolito,
            vec![rect(1.0, 1.0, 2.0, 2.0)],
        )];
        assert_eq!(
            validate(&[rect(1.5, 1.5, 2.5, 2.5)], d, &s),
            [Notice::ZonaSenzaMuri, Notice::StessoStatoSovrapposte]
        );
    }

    #[test]
    fn preview_in_the_state_colour() {
        let yellow = [1.0, 1.0, 0.0, 1.0];
        let red = [1.0, 0.0, 0.0, 1.0];
        // Rettangolo in corso.
        for (state, colour) in [(ZoneState::Demolito, yellow), (ZoneState::Nuovo, red)] {
            let mut c = ZoneCommand::new(state, case(Form::Continua));
            assert!(
                c.on_mouse_move(pt(1.0, 1.0)).is_none(),
                "niente prima del primo punto"
            );
            need_point_text(c.on_text_input("R"));
            need_point(c.on_point(pt(1.0, 1.0)));
            let wire: WireModel = c.on_mouse_move(pt(2.0, 2.0)).expect("anteprima");
            assert_eq!(wire.color, colour);
            assert_eq!(wire.points.len(), 5, "contorno chiuso");
        }
        // Asse sul tramezzo: il rettangolo del muro trovato.
        let mut c = ZoneCommand::new(ZoneState::Demolito, case(Form::Continua));
        need_point(c.on_point(pt(4.05, 0.0)));
        let wire = c.on_mouse_move(pt(4.05, 3.0)).expect("anteprima");
        assert_eq!(wire.color, yellow);
        assert_eq!(wire.points.len(), 5);
        // Durante la domanda sul varco si vede la zona proposta.
        need_point(c.on_point(pt(4.05, 3.0)));
        let wire = c.on_mouse_move(pt(9.0, 9.0)).expect("anteprima");
        assert_eq!(wire.points.len(), 5);
    }
}
