//! Comando zona (spec §5.1): `AL_DEMOLISCI` (zona gialla, demolita) e
//! `AL_NUOVO` (zona rossa, muro nuovo). In fondo il comando locale
//! [`RoomCommand`] (`AL_LOCALE`, spec §5.2) e il ricalcolo dei locali di fatto
//! [`refresh_rooms`] (`AL_AGGIORNA`, spec §4.2).
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

use std::borrow::Cow;
use std::iter;

use codec::entities::hatch::BoundaryPathFlags;
use codec::entities::{
    BoundaryEdge, BoundaryPath, Hatch, HatchStyleType, LwPolyline, LwVertex, MText, PolylineEdge,
};
use codec::{CadDocument, EntityType, Handle, Vector2};
use glam::DVec3;
use kernel::geom2d::{
    contains, distance_to, segment_crossing, signed_area, Curve, Line, SegmentCrossing, Tolerance,
};
use rustc_hash::FxHashMap;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, WorkingPlane};
use crate::entities::hatch::boundary_area;
use crate::scene::model::wire_model::WireModel;
use crate::scene::{exact_hatch_paths, BoundarySource};

use super::project::project_geometry;
use super::report::Notice;
use super::rooms::{
    label, measure_at, quality_notices, room_entities, rooms_in, seed_entity, Computed, Measured,
    Phase, Room, RoomPart,
};
use super::units::{area_m2, format_m2, DrawingUnit};
use super::walls::{
    faces as wall_faces, net_area, rect_from_axis, rect_from_axis_thickness, region_at, sources_of,
    wall_set, AxisMiss, Region, WallRect, FACE_PARALLEL_DEGREES,
};
use super::zone::{
    read_zone, zone_hatch, zone_hatch_from_paths, zones_in, Zone, ZoneGeom, ZoneOrigin, ZoneState,
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
///   zona ricalca un muro esistente non demolito ([`sits_on_a_wall`]), oppure
///   se una zona rettangolare sta stretta tra le due facce di un muro
///   esistente non demolito, anche più sottile di lui ([`inside_a_wall`]). Le
///   facce su cui il muro nuovo soltanto si appoggia stanno sul bordo e non
///   contano. Falsi allarmi che restano (costano solo una conferma): un muro
///   nuovo lungo un corridoio largo fino a 1,50 m, il riempimento di una
///   nicchia, la chiusura di un vano porta con le linee di soglia disegnate.
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
                || inside_a_wall(candidate, snap, max_thickness)
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

/// La zona, se è un rettangolo senza buchi, sta tra le due facce di un muro
/// esistente non demolito (decisione R20, «tenaglia»)? Si cerca il muro con
/// `rect_from_axis` sulla mezzeria parallela ai lati lunghi (dal punto medio
/// di un lato corto a quello dell'altro), entro lo spessore massimo, tra le
/// sorgenti dei muri senza i tratti dentro le zone `Demolito` (bordo
/// compreso). Prende il muro nuovo più sottile del vecchio (facce fuori
/// dalla zona) o allineato a una sola delle sue facce, che né l'interno
/// stretto né [`sits_on_a_wall`] vedono.
fn inside_a_wall(candidate: &[Vec<[f64; 2]>], snap: &ZoneSnapshot, max_thickness: f64) -> bool {
    let [outer] = candidate else {
        return false;
    };
    let Some((p1, p2)) = rectangle_midline(outer, snap.tol) else {
        return false;
    };
    // Le facce si cercano fino allo spessore massimo dalla zona: valgono le
    // zone demolite fin lì.
    let reach = bounds(candidate);
    let demolished: Vec<&[Vec<[f64; 2]>]> = snap
        .zones
        .iter()
        .filter(|z| z.zone.state == ZoneState::Demolito)
        .filter(|z| meets(&bounds(&z.rings), &reach, snap.tol + max_thickness))
        .map(|z| z.rings.as_slice())
        .collect();
    let standing = if demolished.is_empty() {
        Cow::Borrowed(&snap.walls)
    } else {
        Cow::Owned(standing_walls(&snap.walls, &demolished, snap.tol))
    };
    rect_from_axis(&standing, p1, p2, max_thickness, snap.tol).is_ok()
}

/// La mezzeria lungo i lati lunghi di un anello rettangolare (quattro
/// vertici, angoli retti), da un lato corto all'altro; `None` per ogni altra
/// forma.
fn rectangle_midline(ring: &[[f64; 2]], tol: f64) -> Option<([f64; 2], [f64; 2])> {
    let [c0, c1, c2, c3] = ring else {
        return None;
    };
    let vector = |a: &[f64; 2], b: &[f64; 2]| [b[0] - a[0], b[1] - a[1]];
    let sides = [
        vector(c0, c1),
        vector(c1, c2),
        vector(c2, c3),
        vector(c3, c0),
    ];
    let lengths = sides.map(|v| v[0].hypot(v[1]));
    if lengths.iter().any(|&length| !(length > tol)) {
        return None;
    }
    let right_angles = (0..4).all(|i| {
        let (u, v) = (sides[i], sides[(i + 1) % 4]);
        (u[0] * v[0] + u[1] * v[1]).abs() <= 1e-9 * lengths[i] * lengths[(i + 1) % 4]
    });
    if !right_angles {
        return None;
    }
    let middle = |a: &[f64; 2], b: &[f64; 2]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    Some(if lengths[0] >= lengths[1] {
        // Lati lunghi c0-c1 e c2-c3: i corti sono c3-c0 e c1-c2.
        (middle(c3, c0), middle(c1, c2))
    } else {
        (middle(c0, c1), middle(c2, c3))
    })
}

/// Le sorgenti dei muri con i soli lati dritti (gli unici che
/// `rect_from_axis` prende come facce), senza i tratti dentro le zone
/// demolite, bordo compreso: ogni lato si spezza sui lati delle zone e ogni
/// pezzo più lungo di `tol` decide dal suo punto medio.
fn standing_walls(
    walls: &FxHashMap<Handle, BoundarySource>,
    demolished: &[&[Vec<[f64; 2]>]],
    tol: f64,
) -> FxHashMap<Handle, BoundarySource> {
    let tolerance = Tolerance::new(tol);
    let pieces = |line: Line| -> Vec<Line> {
        let length = distance(line.start, line.end);
        let mut cuts = vec![0.0, 1.0];
        for edge in demolished.iter().flat_map(|zone| edges(zone)) {
            match segment_crossing(line, edge, tolerance) {
                SegmentCrossing::Point { a, .. } => cuts.push(a.clamp(0.0, 1.0)),
                SegmentCrossing::Overlap { a, .. } => cuts.extend(a.map(|t| t.clamp(0.0, 1.0))),
                SegmentCrossing::None => {}
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.windows(2)
            .filter(|pair| (pair[1] - pair[0]) * length > tol)
            .filter(|pair| {
                let p = lerp(line, (pair[0] + pair[1]) / 2.0);
                demolished
                    .iter()
                    .all(|zone| side(zone, p, tol) == Side::Outside)
            })
            .map(|pair| Line {
                start: lerp(line, pair[0]),
                end: lerp(line, pair[1]),
            })
            .collect()
    };
    walls
        .iter()
        .map(|(&handle, source)| {
            let kept: Vec<Line> = source
                .curves
                .iter()
                .filter_map(|curve| match curve {
                    Curve::Line(line) => Some(*line),
                    _ => None,
                })
                .flat_map(&pieces)
                .collect();
            let curves = kept.iter().copied().map(Curve::Line).collect();
            (
                handle,
                BoundarySource {
                    segments: kept,
                    curves,
                },
            )
        })
        .collect()
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

// ---------------------------------------------------------------------------
// Locali: `AL_LOCALE` e `AL_AGGIORNA` (spec §4.2, §5.2)
// ---------------------------------------------------------------------------

/// L'altezza dei testi dei locali, in metri, quando il disegno non ne dà una
/// (`TEXTSIZE` non positiva): un parametro di aspetto, non una misura.
pub const DEFAULT_ROOM_TEXT_M: f64 = 0.25;

/// La geometria che [`RoomCommand`] legge, preparata all'avvio
/// ([`room_snapshot`]). In unità del disegno, sul piano XY del mondo.
#[derive(Clone)]
pub struct RoomSnapshot {
    /// Le facce (`walls::faces` a `tol`) e i muri del rilievo.
    pub fatto: (Vec<Vec<[f64; 2]>>, FxHashMap<Handle, BoundarySource>),
    /// Le stesse sulla geometria di progetto (`project::project_geometry`, con
    /// le soglie dei varchi).
    pub progetto: (Vec<Vec<[f64; 2]>>, FxHashMap<Handle, BoundarySource>),
    /// Gli anelli delle zone nuove, contorni e buchi insieme, senza doppioni:
    /// un punto sta in un muro nuovo se è sul bordo di un anello o dentro un
    /// numero dispari di anelli. Due zone nuove sovrapposte solo in parte (la
    /// convalida lo segnala) si annullano nella parte comune.
    pub new_zone_rings: Vec<Vec<[f64; 2]>>,
    pub unit: DrawingUnit,
    /// La tolleranza dei locali: `max(unit.tolerance(), HPGAPTOL)`.
    pub tol: f64,
    /// L'altezza dei testi nuovi ([`room_text_height`]).
    pub text_height: f64,
    /// I locali di fatto già nel disegno, uno per locale: un locale di fatto
    /// la cui regione contiene il seme di un altro li fonde (R54).
    pub fatto_rooms: Vec<Room>,
}

/// L'istantanea di `AL_LOCALE` per `doc` alla tolleranza dei locali `tol`, e
/// quante entità muro restano fuori dal piano XY (ignorate). La geometria di
/// progetto si taglia alla tolleranza dell'unità, come nell'estrazione; le
/// facce si saldano a `tol`.
pub fn room_snapshot(doc: &CadDocument, unit: DrawingUnit, tol: f64) -> (RoomSnapshot, usize) {
    let base = unit.tolerance();
    let walls = wall_set(doc, base);
    let zones = zones_in(doc);
    // Il layer dei muri nuovi non cambia la geometria dei locali.
    let pg = project_geometry(doc, &walls, &zones, "0", base);
    let (pg_sources, _) = sources_of(&pg.room_entities, base);
    let pg_faces = wall_faces(&pg_sources, tol);
    let fatto_faces = wall_faces(&walls.sources, tol);
    // Una zona nuova data due volte (lo stesso muro) conta una volta: per la
    // regola pari-dispari due anelli uguali si annullerebbero.
    let mut new_zone_rings: Vec<Vec<[f64; 2]>> = Vec::new();
    for ring in zones
        .iter()
        .filter(|zone| zone.zone.state == ZoneState::Nuovo)
        .flat_map(|zone| zone.rings.iter())
    {
        if !new_zone_rings.iter().any(|seen| same_ring(seen, ring, tol)) {
            new_zone_rings.push(ring.clone());
        }
    }
    let snap = RoomSnapshot {
        fatto: (fatto_faces, walls.sources),
        progetto: (pg_faces, pg_sources),
        new_zone_rings,
        unit,
        tol,
        text_height: room_text_height(doc, unit).0,
        fatto_rooms: fatto_rooms(doc, tol),
    };
    (snap, walls.off_plane)
}

/// Lo stesso anello entro `tol`, da qualunque vertice e in tutti e due i versi.
fn same_ring(a: &[[f64; 2]], b: &[[f64; 2]], tol: f64) -> bool {
    let n = a.len();
    let rotated = |b: &[[f64; 2]]| {
        (0..n).any(|shift| (0..n).all(|k| distance(a[(k + shift) % n], b[k]) <= tol))
    };
    let reversed: Vec<[f64; 2]> = b.iter().rev().copied().collect();
    n == b.len() && (n == 0 || rotated(b) || rotated(&reversed))
}

/// L'altezza dei testi dei locali: `TEXTSIZE` del disegno se positiva,
/// altrimenti [`DEFAULT_ROOM_TEXT_M`] in unità del disegno, con `true`.
pub fn room_text_height(doc: &CadDocument, unit: DrawingUnit) -> (f64, bool) {
    let height = doc.header.text_height;
    if height.is_finite() && height > 0.0 {
        (height, false)
    } else {
        (unit.metres(DEFAULT_ROOM_TEXT_M), true)
    }
}

/// I locali di fatto del disegno, uno per locale: contorno e testo dello
/// stesso locale (stesso nome, seme entro `tol`) contano una volta.
fn fatto_rooms(doc: &CadDocument, tol: f64) -> Vec<Room> {
    let mut rooms: Vec<Room> = Vec::new();
    for (_, room, _) in rooms_in(doc) {
        let twin = rooms
            .iter()
            .any(|seen| seen.name == room.name && distance(seen.seed, room.seed) <= tol);
        if room.phase == Phase::Fatto && !twin {
            rooms.push(room);
        }
    }
    rooms
}

/// Perché il punto di prima non ha dato un locale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RoomMiss {
    /// Nessuna faccia chiusa attorno, o muri dentro (R50).
    NotClosed,
    /// Un seme di progetto dentro una zona nuova.
    InNewWall,
}

/// Il locale trovato attorno al punto.
struct FoundRoom {
    seed: [f64; 2],
    measured: Measured,
    /// I locali di fatto già scritti con il seme in questa regione (R54).
    others: Vec<String>,
}

/// I passi di [`RoomCommand`].
enum RoomStep {
    /// Punto interno, o la parola chiave della fase; `miss` se il punto di
    /// prima non ha dato un locale.
    Point {
        miss: Option<RoomMiss>,
    },
    /// Nome; `empty` dopo un Invio a vuoto.
    Name {
        found: FoundRoom,
        empty: bool,
    },
    Number {
        found: FoundRoom,
        name: String,
    },
    /// Locale consegnato.
    Done,
}

/// `AL_LOCALE` (spec §5.2): punto interno, nome e numero. Nella fase di fatto
/// consegna contorno e testo misurati sul rilievo; in quella di progetto
/// mostra la superficie sulla geometria di progetto e consegna il solo seme
/// (spec §4.2). Parte nella fase di fatto; `F` la cambia.
pub struct RoomCommand {
    snap: RoomSnapshot,
    phase: Phase,
    step: RoomStep,
}

impl RoomCommand {
    pub fn new(snap: RoomSnapshot) -> Self {
        Self {
            snap,
            phase: Phase::Fatto,
            step: RoomStep::Point { miss: None },
        }
    }

    fn point_prompt(&self) -> String {
        let phase = match self.phase {
            Phase::Fatto => "Fatto",
            Phase::Progetto => "Progetto",
        };
        format!("AL_LOCALE ({phase})  Punto interno del locale o [Fase]:")
    }

    /// Le facce e i muri della fase.
    fn geometry(&self) -> (&[Vec<[f64; 2]>], &FxHashMap<Handle, BoundarySource>) {
        let (faces, sources) = match self.phase {
            Phase::Fatto => &self.snap.fatto,
            Phase::Progetto => &self.snap.progetto,
        };
        (faces, sources)
    }

    /// Il locale attorno a `p`, con l'area intera (archi veri dove si può,
    /// avvisi della tolleranza: `rooms::measure_at`).
    fn pick(&mut self, p: [f64; 2]) -> CmdResult {
        let tol = self.snap.tol;
        if self.phase == Phase::Progetto && inside_new_walls(&self.snap.new_zone_rings, p, tol) {
            self.step = RoomStep::Point {
                miss: Some(RoomMiss::InNewWall),
            };
            return CmdResult::NeedPoint;
        }
        let (faces, sources) = self.geometry();
        let Some(measured) = measure_at(faces, None, sources, p, self.snap.unit, tol) else {
            self.step = RoomStep::Point {
                miss: Some(RoomMiss::NotClosed),
            };
            return CmdResult::NeedPoint;
        };
        // R54: un vuoto con i due lati attaccati altrove fonde due locali senza
        // che la regola dei muri dentro lo veda. Nel progetto i semi si
        // raggruppano all'estrazione (`rooms::project_rooms`).
        let others = match self.phase {
            Phase::Fatto => self
                .snap
                .fatto_rooms
                .iter()
                .filter(|room| region_holds(&measured.region, room.seed, tol))
                .map(|room| room.name.clone())
                .collect(),
            Phase::Progetto => Vec::new(),
        };
        self.step = RoomStep::Name {
            found: FoundRoom {
                seed: p,
                measured,
                others,
            },
            empty: false,
        };
        CmdResult::NeedPoint
    }

    /// Il nome, senza spazi attorno; vuoto, lo si richiede col motivo.
    fn name_given(&mut self, text: &str) -> CmdResult {
        let name = text.trim();
        match std::mem::replace(&mut self.step, RoomStep::Done) {
            RoomStep::Name { found, .. } if !name.is_empty() => {
                self.step = RoomStep::Number {
                    found,
                    name: name.to_string(),
                };
            }
            RoomStep::Name { found, .. } => self.step = RoomStep::Name { found, empty: true },
            other => self.step = other,
        }
        CmdResult::NeedPoint
    }

    /// Il numero (anche vuoto) chiude il comando: contorno e testo nel fatto,
    /// il seme nel progetto.
    fn finish(&mut self, number: &str) -> CmdResult {
        let RoomStep::Number { found, name } = std::mem::replace(&mut self.step, RoomStep::Done)
        else {
            return CmdResult::NeedPoint;
        };
        let number = number.trim().to_string();
        let FoundRoom { seed, measured, .. } = found;
        let entities = match self.phase {
            Phase::Fatto => room_entities(
                &Computed {
                    name,
                    number,
                    area_m2: measured.area_m2,
                    region: measured.region,
                    confirm: false,
                    exact: measured.exact,
                    islands: measured.islands,
                    seed,
                },
                Phase::Fatto,
                &self.snap.fatto.1,
                self.snap.text_height,
                self.snap.tol,
            ),
            Phase::Progetto => vec![seed_entity(
                &Room {
                    phase: Phase::Progetto,
                    name,
                    number,
                    seed,
                },
                self.snap.text_height,
            )],
        };
        CmdResult::CommitEntitiesAndExit(entities)
    }
}

/// Gli avvisi del locale trovato, col nome dato: prima la fusione con altri
/// locali di fatto (R54), poi quelli di qualità (isole, tolleranza, contorno).
fn found_notices(found: &FoundRoom, name: &str) -> Vec<Notice> {
    let m = &found.measured;
    let mut notices = Vec::new();
    if !found.others.is_empty() {
        notices.push(Notice::LocaleFuso {
            names: iter::once(name.to_string())
                .chain(found.others.iter().cloned())
                .collect(),
            area_m2: m.area_m2,
        });
    }
    notices.extend(quality_notices(name, m.exact, m.by_tolerance, m.islands));
    notices
}

/// ` (1 isola)`, ` (3 isole)`; niente senza isole.
fn islands_text(islands: usize) -> String {
    match islands {
        0 => String::new(),
        1 => " (1 isola)".into(),
        n => format!(" ({n} isole)"),
    }
}

fn ring_curves(ring: &[[f64; 2]]) -> Vec<Curve> {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .filter(|(start, end)| distance(**start, **end) > 0.0)
        .map(|(&start, &end)| Curve::Line(Line { start, end }))
        .collect()
}

/// `p` sta nella regione: dentro il contorno esterno, bordo compreso, e non
/// strettamente dentro un buco.
fn region_holds(region: &Region, p: [f64; 2], tol: f64) -> bool {
    let tolerance = Tolerance::new(tol);
    contains(&ring_curves(&region.outer), p, tolerance)
        && !region.holes.iter().any(|hole| {
            let edges = ring_curves(hole);
            edges.iter().all(|edge| distance_to(edge, p) > tol) && contains(&edges, p, tolerance)
        })
}

/// `p` sta in un muro nuovo: sul bordo di un anello delle zone nuove, o
/// dentro un numero dispari di anelli (contorni e buchi insieme, vedi
/// [`RoomSnapshot::new_zone_rings`]). È la regola di `rooms::project_rooms`
/// (bordo compreso, buchi esclusi) per zone che non si sovrappongono.
fn inside_new_walls(rings: &[Vec<[f64; 2]>], p: [f64; 2], tol: f64) -> bool {
    let tolerance = Tolerance::new(tol);
    let mut inside = false;
    for ring in rings.iter().filter(|ring| ring.len() >= 3) {
        let edges = ring_curves(ring);
        if edges.iter().any(|edge| distance_to(edge, p) <= tol) {
            return true;
        }
        if contains(&edges, p, tolerance) {
            inside = !inside;
        }
    }
    inside
}

impl CadCommand for RoomCommand {
    /// Contorno e testo nascono sui layer `AL-LOCALI*`, `ByLayer`.
    fn preserve_commit_layer(&self) -> bool {
        true
    }

    fn preserve_commit_style(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "AL_LOCALE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            RoomStep::Point { miss: None } => self.point_prompt(),
            RoomStep::Point {
                miss: Some(RoomMiss::NotClosed),
            } => format!(
                "Nessun locale chiuso attorno al punto (chiudi i vani con una linea di soglia o usa HPGAPTOL). {}",
                self.point_prompt()
            ),
            RoomStep::Point {
                miss: Some(RoomMiss::InNewWall),
            } => format!("Il punto cade dentro un muro nuovo. {}", self.point_prompt()),
            RoomStep::Name { found, empty } => {
                let ask = format!(
                    "Superficie netta {}{}. Nome del locale:",
                    format_m2(found.measured.area_m2),
                    islands_text(found.measured.islands)
                );
                if *empty {
                    format!("Il nome del locale non può essere vuoto. {ask}")
                } else {
                    ask
                }
            }
            // Gli avvisi dopo la riga della superficie, quando il nome c'è.
            RoomStep::Number { found, name } => {
                let texts: Vec<String> = found_notices(found, name).iter().map(Notice::text).collect();
                if texts.is_empty() {
                    "Numero <nessuno>:".into()
                } else {
                    format!("{} Numero <nessuno>:", texts.join(" "))
                }
            }
            RoomStep::Done => String::new(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            RoomStep::Point { .. } => vec![CmdOption::new("Fase", "F")],
            _ => Vec::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            RoomStep::Point { .. } => self.pick([pt.x, pt.y]),
            // Un clic non è un nome né un numero.
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            RoomStep::Point { .. } | RoomStep::Done => CmdResult::Cancel,
            RoomStep::Name { .. } => self.name_given(""),
            RoomStep::Number { .. } => self.finish(""),
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match self.step {
            RoomStep::Point { .. } => {
                if !matches!(text.trim().to_uppercase().as_str(), "F" | "FASE") {
                    return None;
                }
                self.phase = match self.phase {
                    Phase::Fatto => Phase::Progetto,
                    Phase::Progetto => Phase::Fatto,
                };
                self.step = RoomStep::Point { miss: None };
                Some(CmdResult::NeedPoint)
            }
            RoomStep::Name { .. } => Some(self.name_given(text)),
            RoomStep::Number { .. } => Some(self.finish(text)),
            RoomStep::Done => None,
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            RoomStep::Name { .. } | RoomStep::Number { .. } => InputKind::FreeText,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, RoomStep::Point { .. })
    }
}

/// Una modifica di `AL_AGGIORNA` a un'entità di un locale di fatto, che resta
/// con il suo handle.
#[derive(Clone, Debug, PartialEq)]
pub enum RoomEdit {
    /// I vertici (con i bulge) e la chiusura del contorno.
    Contorno {
        vertices: Vec<LwVertex>,
        closed: bool,
    },
    /// Il testo dell'etichetta.
    Testo { value: String },
}

impl RoomEdit {
    /// Scrive la modifica nell'entità, se è del tipo giusto (`LWPOLYLINE` per
    /// il contorno, `MTEXT` per il testo); un altro tipo resta com'è.
    pub fn apply_to(&self, entity: &mut EntityType) {
        match (self, entity) {
            (Self::Contorno { vertices, closed }, EntityType::LwPolyline(polyline)) => {
                polyline.vertices = vertices.clone();
                polyline.is_closed = *closed;
            }
            (Self::Testo { value }, EntityType::MText(text)) => text.value = value.clone(),
            _ => {}
        }
    }
}

/// Il ricalcolo dei locali di fatto di un disegno ([`refresh_rooms`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoomsRefresh {
    /// Per ogni locale in cui qualcosa cambia, le modifiche alle sue entità.
    pub changed: Vec<Vec<(Handle, RoomEdit)>>,
    /// Gli avvisi, locale per locale, nell'ordine di disegno.
    pub notices: Vec<Notice>,
}

/// Un locale di fatto nel disegno: il contorno e il testo accoppiati per seme.
struct FattoParts<'a> {
    room: Room,
    outline: Option<(Handle, &'a LwPolyline)>,
    text: Option<(Handle, &'a MText)>,
}

/// Le parti dei locali di fatto dello spazio modello, accoppiate per seme
/// entro `tol` nell'ordine di disegno: ogni parte va al primo locale con il
/// seme vicino che non ne ha ancora una di quel tipo. Un contorno che non è
/// una `LWPOLYLINE`, o un testo che non è un `MTEXT`, non conta.
fn fatto_parts(doc: &CadDocument, tol: f64) -> Vec<FattoParts<'_>> {
    let mut rooms: Vec<FattoParts> = Vec::new();
    for (handle, room, part) in rooms_in(doc) {
        if room.phase != Phase::Fatto {
            continue;
        }
        let seed = room.seed;
        let near = |parts: &FattoParts| distance(parts.room.seed, seed) <= tol;
        match (part, doc.get_entity(handle)) {
            (RoomPart::Contorno, Some(EntityType::LwPolyline(polyline))) => {
                match rooms
                    .iter_mut()
                    .find(|parts| parts.outline.is_none() && near(parts))
                {
                    Some(parts) => parts.outline = Some((handle, polyline)),
                    None => rooms.push(FattoParts {
                        room,
                        outline: Some((handle, polyline)),
                        text: None,
                    }),
                }
            }
            (RoomPart::Testo, Some(EntityType::MText(text))) => {
                match rooms
                    .iter_mut()
                    .find(|parts| parts.text.is_none() && near(parts))
                {
                    Some(parts) => parts.text = Some((handle, text)),
                    None => rooms.push(FattoParts {
                        room,
                        outline: None,
                        text: Some((handle, text)),
                    }),
                }
            }
            _ => {}
        }
    }
    rooms
}

/// `AL_AGGIORNA` (spec §4.2): ogni locale di fatto si rimisura sui muri
/// attuali attorno al suo seme, alla tolleranza dei locali `tol`
/// (`rooms::measure_at`). Se il locale c'è, il contorno prende i vertici
/// nuovi e il testo l'etichetta nuova (solo dove cambiano: un contorno uguale
/// partendo da un altro vertice non cambia); se non è più chiuso,
/// [`Notice::LocaleNonChiuso`] e niente cambia. Un locale senza contorno o
/// senza testo lo dice ([`Notice::ContornoMancante`],
/// [`Notice::TestoMancante`]) e aggiorna la parte che ha. Una regione con il
/// seme di un altro locale dà [`Notice::LocaleFuso`], una volta per gruppo
/// (R54); poi gli avvisi di qualità (`rooms::quality_notices`). In testa,
/// [`Notice::FuoriPiano`] se ci sono muri fuori dal piano XY. Senza locali di
/// fatto non c'è niente da dire. Il disegno non si tocca: le modifiche le
/// applica chi chiama ([`RoomEdit::apply_to`]).
pub fn refresh_rooms(doc: &CadDocument, unit: DrawingUnit, tol: f64) -> RoomsRefresh {
    let mut out = RoomsRefresh::default();
    let rooms = fatto_parts(doc, tol);
    if rooms.is_empty() {
        return out;
    }
    let base = unit.tolerance();
    let walls = wall_set(doc, base);
    let faces = wall_faces(&walls.sources, tol);
    // R47: le facce alla tolleranza di base, una volta per tutti i locali.
    let base_faces = (tol > base * (1.0 + 1.0e-9)).then(|| wall_faces(&walls.sources, base));
    if walls.off_plane > 0 {
        out.notices.push(Notice::FuoriPiano {
            count: walls.off_plane,
        });
    }
    let found: Vec<Option<Measured>> = rooms
        .iter()
        .map(|parts| {
            measure_at(
                &faces,
                base_faces.as_deref(),
                &walls.sources,
                parts.room.seed,
                unit,
                tol,
            )
        })
        .collect();
    let mut fused = vec![false; rooms.len()];
    for (index, parts) in rooms.iter().enumerate() {
        let name = &parts.room.name;
        if parts.outline.is_none() {
            out.notices
                .push(Notice::ContornoMancante { name: name.clone() });
        }
        if parts.text.is_none() {
            out.notices
                .push(Notice::TestoMancante { name: name.clone() });
        }
        let Some(measured) = &found[index] else {
            out.notices
                .push(Notice::LocaleNonChiuso { name: name.clone() });
            continue;
        };
        if !fused[index] {
            let others: Vec<usize> = (0..rooms.len())
                .filter(|&other| {
                    other != index && region_holds(&measured.region, rooms[other].room.seed, tol)
                })
                .collect();
            if !others.is_empty() {
                for &other in iter::once(&index).chain(&others) {
                    fused[other] = true;
                }
                out.notices.push(Notice::LocaleFuso {
                    names: iter::once(name.clone())
                        .chain(others.iter().map(|&other| rooms[other].room.name.clone()))
                        .collect(),
                    area_m2: measured.area_m2,
                });
            }
        }
        out.notices.extend(quality_notices(
            name,
            measured.exact,
            measured.by_tolerance,
            measured.islands,
        ));
        let mut edits = Vec::new();
        if let Some((handle, old)) = parts.outline {
            let computed = Computed {
                name: name.clone(),
                number: parts.room.number.clone(),
                area_m2: measured.area_m2,
                region: measured.region.clone(),
                confirm: false,
                exact: measured.exact,
                islands: measured.islands,
                seed: parts.room.seed,
            };
            let fresh = room_entities(&computed, Phase::Fatto, &walls.sources, 0.0, tol);
            if let Some(EntityType::LwPolyline(new)) = fresh.into_iter().next() {
                if !same_outline(old, &new, tol) {
                    edits.push((
                        handle,
                        RoomEdit::Contorno {
                            vertices: new.vertices,
                            closed: new.is_closed,
                        },
                    ));
                }
            }
        }
        if let Some((handle, old)) = parts.text {
            let value = label(name, measured.area_m2, false);
            if old.value != value {
                edits.push((handle, RoomEdit::Testo { value }));
            }
        }
        if !edits.is_empty() {
            out.changed.push(edits);
        }
    }
    out
}

/// Lo stesso contorno: stessa chiusura, stessi vertici entro `tol` e stessi
/// bulge, anche partendo da un altro vertice. Le larghezze non contano.
fn same_outline(old: &LwPolyline, new: &LwPolyline, tol: f64) -> bool {
    let n = new.vertices.len();
    if old.is_closed != new.is_closed || old.vertices.len() != n {
        return false;
    }
    let same = |a: &LwVertex, b: &LwVertex| {
        (a.location.x - b.location.x).hypot(a.location.y - b.location.y) <= tol
            && (a.bulge - b.bulge).abs() <= 1.0e-9
    };
    n == 0
        || (0..n)
            .any(|shift| (0..n).all(|k| same(&old.vertices[(k + shift) % n], &new.vertices[k])))
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
        assert!(validate(&[rect(5.0, 0.0, 5.12, 1.1)], n, &corridor()).is_empty());
    }

    /// Un corridoio largo 1,10 m tra due linee lunghe 10 m (y = 0 e y = 1,10).
    fn corridor() -> ZoneSnapshot {
        let wall = |a: [f64; 2], b: [f64; 2]| {
            let mut line =
                Line::from_points(Vector3::new(a[0], a[1], 0.0), Vector3::new(b[0], b[1], 0.0));
            line.common.layer = LAYER_MURI.to_string();
            EntityType::Line(line)
        };
        snap(vec![
            wall([0.0, 0.0], [10.0, 0.0]),
            wall([0.0, 1.1], [10.0, 1.1]),
        ])
    }

    #[test]
    fn new_wall_thinner_than_the_existing_one_is_noticed() {
        // R20: AL_NUOVO sull'asse del tramezzo con spessore 0,06 (zona
        // 4,02–4,08): le facce restano fuori dalla zona, nelle due forme.
        let on_wall = "La zona nuova si sovrappone a un muro esistente non demolito. Confermi la zona? [Sì/No] <Sì>:";
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let mut c = ZoneCommand::new(ZoneState::Nuovo, case(form));
            need_point(c.on_point(pt(4.05, 0.0)));
            need_point(c.on_point(pt(4.05, 3.0)));
            need_point_text(c.on_text_input("0,06"));
            assert_eq!(c.prompt(), on_wall, "{form:?}");
        }
        // Muro sottile allineato a una faccia (4,00–4,06), dal rettangolo.
        let mut c = ZoneCommand::new(ZoneState::Nuovo, case(Form::Continua));
        need_point_text(c.on_text_input("R"));
        need_point(c.on_point(pt(4.0, 0.0)));
        need_point(c.on_point(pt(4.06, 3.0)));
        assert_eq!(c.prompt(), on_wall);

        let n = ZoneState::Nuovo;
        let thin = [rect(4.02, 0.0, 4.08, 3.0)];
        let aligned = [rect(4.0, 0.0, 4.06, 3.0)];
        let s = case(Form::Continua);
        assert_eq!(validate(&thin, n, &s), [Notice::NuovoSuEsistente]);
        assert_eq!(validate(&aligned, n, &s), [Notice::NuovoSuEsistente]);
        // Un pezzo corto: la mezzeria corre lungo i lati lunghi (verticali).
        assert_eq!(
            validate(&[rect(4.02, 1.0, 4.08, 1.5)], n, &s),
            [Notice::NuovoSuEsistente]
        );

        // Con il tramezzo demolito è una ricostruzione: niente.
        let mut s = case(Form::Continua);
        s.zones = vec![zone_geom(
            ZoneState::Demolito,
            vec![rect(4.0, 0.0, 4.1, 3.0)],
        )];
        assert!(validate(&thin, n, &s).is_empty());
        assert!(validate(&aligned, n, &s).is_empty());
        // Demolito solo fino a 1,50: le facce che restano bastano.
        s.zones = vec![zone_geom(
            ZoneState::Demolito,
            vec![rect(4.0, 0.0, 4.1, 1.5)],
        )];
        assert_eq!(validate(&thin, n, &s), [Notice::NuovoSuEsistente]);

        // Restano senza avviso: (2;0)-(2;3) 0,12 in A (facce a 2 m) e il muro
        // trasversale a un corridoio stretto.
        let s = case(Form::Continua);
        assert!(validate(&[rect(1.94, 0.0, 2.06, 3.0)], n, &s).is_empty());
        assert!(validate(&[rect(5.0, 0.0, 5.12, 1.1)], n, &corridor()).is_empty());
        // Falso allarme dichiarato: un muro nuovo lungo un corridoio stretto.
        assert_eq!(
            validate(&[rect(2.0, 0.5, 8.0, 0.62)], n, &corridor()),
            [Notice::NuovoSuEsistente]
        );
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

#[cfg(test)]
mod room_tests {
    //! `AL_LOCALE` ([`RoomCommand`]) e il ricalcolo di `AL_AGGIORNA`
    //! ([`refresh_rooms`]) sui casi di prova, senza l'app.
    use super::*;
    use crate::bim::fixtures::{case_a, pilastro, scene_with, Form};
    use crate::bim::rooms::{decode, room_entities, Computed, Phase, Room, RoomPart};
    use crate::bim::zone::{zone_hatch, Zone, ZoneOrigin};
    use crate::bim::{APP_LOCALE, LAYER_LOCALI, LAYER_LOCALI_PROGETTO};
    use crate::command::InputKind;
    use crate::scene::Scene;
    use codec::entities::{LwPolyline, LwVertex};
    use codec::{CadDocument, Vector3};

    const TOL: f64 = 1e-6;
    const M: DrawingUnit = DrawingUnit::Metri;

    fn scene_of(entities: Vec<EntityType>) -> Scene {
        scene_with(entities, 6)
    }

    fn snap_of(scene: &Scene) -> RoomSnapshot {
        room_snapshot(&scene.document, M, TOL).0
    }

    fn case_snap(form: Form) -> RoomSnapshot {
        snap_of(&scene_of(case_a(form, M)))
    }

    fn pt(x: f64, y: f64) -> DVec3 {
        DVec3::new(x, y, 0.0)
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    #[track_caller]
    fn need(result: CmdResult) {
        assert!(matches!(result, CmdResult::NeedPoint));
    }

    #[track_caller]
    fn need_text(result: Option<CmdResult>) {
        assert!(matches!(result, Some(CmdResult::NeedPoint)));
    }

    #[track_caller]
    fn committed(result: CmdResult) -> Vec<EntityType> {
        let CmdResult::CommitEntitiesAndExit(entities) = result else {
            panic!("nessun commit")
        };
        entities
    }

    fn near(p: Vector3, q: [f64; 2]) -> bool {
        (p.x - q[0]).abs() < 1e-9 && (p.y - q[1]).abs() < 1e-9
    }

    /// L'area dei vertici di una polilinea (lati dritti).
    fn polyline_area(vertices: &[LwVertex]) -> f64 {
        let ring: Vec<[f64; 2]> = vertices
            .iter()
            .map(|v| [v.location.x, v.location.y])
            .collect();
        signed_area(&ring).abs()
    }

    /// Le entità che `AL_LOCALE` (fase di fatto, numero vuoto) consegna per
    /// il locale attorno a `seed`.
    fn room_of(scene: &Scene, name: &str, seed: [f64; 2]) -> Vec<EntityType> {
        let mut c = RoomCommand::new(snap_of(scene));
        need(c.on_point(pt(seed[0], seed[1])));
        need_text(c.on_text_input(name));
        committed(c.on_enter())
    }

    /// Il locale scritto nella scena: gli handle di contorno e testo.
    fn write_room(scene: &mut Scene, name: &str, seed: [f64; 2]) -> (Handle, Handle) {
        let handles: Vec<Handle> = room_of(scene, name, seed)
            .into_iter()
            .map(|entity| scene.add_entity(entity))
            .collect();
        (handles[0], handles[1])
    }

    /// L'handle della `LINE` da `a` a `b`.
    fn line_handle(doc: &CadDocument, a: [f64; 2], b: [f64; 2]) -> Handle {
        doc.entities()
            .find(|e| matches!(e, EntityType::Line(l) if near(l.start, a) && near(l.end, b)))
            .map(|e| e.common().handle)
            .expect("linea del rilievo")
    }

    /// La faccia ovest di A, (0;3)-(0;0), si ferma a (0;0,002): un vuoto
    /// d'angolo di 2 mm (Review Focus 3).
    fn open_west_face(entities: &mut [EntityType]) {
        let west = entities
            .iter_mut()
            .find_map(|e| match e {
                EntityType::Line(l) if near(l.start, [0.0, 3.0]) && near(l.end, [0.0, 0.0]) => {
                    Some(l)
                }
                _ => None,
            })
            .expect("faccia ovest di A");
        west.end = Vector3::new(0.0, 0.002, 0.0);
    }

    /// Il tramezzo del caso A a polilinee si ferma 2 mm prima del muro nord.
    fn open_tramezzo(polyline: &mut LwPolyline) {
        for vertex in &mut polyline.vertices {
            if (vertex.location.y - 3.0).abs() < 1e-9 {
                vertex.location.y = 2.998;
            }
        }
    }

    fn is_tramezzo(entity: &EntityType) -> bool {
        matches!(entity, EntityType::LwPolyline(p) if p.vertices.iter().any(|v| {
            (v.location.x - 4.1).abs() < 1e-9 && (v.location.y - 3.0).abs() < 1e-9
        }))
    }

    /// Una zona demolita «muro tra due punti» sulle facce del caso A, o nuova
    /// «linea + spessore» (`thickness`), come campitura nel disegno.
    fn zone_entity(
        scene: &Scene,
        state: ZoneState,
        p1: [f64; 2],
        p2: [f64; 2],
        thickness: f64,
    ) -> EntityType {
        let rect = match state {
            ZoneState::Demolito => {
                let walls = wall_set(&scene.document, TOL);
                rect_from_axis(&walls.sources, p1, p2, MAX_WALL_THICKNESS_M, TOL).expect("muro")
            }
            ZoneState::Nuovo => rect_from_axis_thickness(p1, p2, thickness).expect("rettangolo"),
        };
        let zone = Zone {
            state,
            origin: match state {
                ZoneState::Demolito => ZoneOrigin::MuroDuePunti,
                ZoneState::Nuovo => ZoneOrigin::LineaSpessore,
            },
            axis: Some(rect.axis),
            thickness: Some(rect.thickness),
            height: None,
            varco: false,
        };
        zone_hatch(&zone, &[rect.ring()])
    }

    #[test]
    fn room_prompts_phase_and_input_kinds() {
        let mut c = RoomCommand::new(case_snap(Form::Continua));
        assert_eq!(c.name(), "AL_LOCALE");
        assert_eq!(
            c.prompt(),
            "AL_LOCALE (Fatto)  Punto interno del locale o [Fase]:"
        );
        assert_eq!(c.input_kind(), InputKind::Point);
        assert!(c.point_step_accepts_keywords());
        let keywords: Vec<String> = c.options().into_iter().map(|o| o.keyword).collect();
        assert_eq!(keywords, ["F"]);
        assert!(c.on_text_input("X").is_none());

        // `F` alterna la fase.
        need_text(c.on_text_input("f"));
        assert_eq!(
            c.prompt(),
            "AL_LOCALE (Progetto)  Punto interno del locale o [Fase]:"
        );
        need_text(c.on_text_input("Fase"));
        assert_eq!(
            c.prompt(),
            "AL_LOCALE (Fatto)  Punto interno del locale o [Fase]:"
        );

        // Fuori da ogni locale: si richiede il punto con il motivo.
        need(c.on_point(pt(20.0, 20.0)));
        assert_eq!(
            c.prompt(),
            "Nessun locale chiuso attorno al punto (chiudi i vani con una linea di soglia o usa HPGAPTOL). AL_LOCALE (Fatto)  Punto interno del locale o [Fase]:"
        );
        need_text(c.on_text_input("F"));
        need(c.on_point(pt(20.0, 20.0)));
        assert_eq!(
            c.prompt(),
            "Nessun locale chiuso attorno al punto (chiudi i vani con una linea di soglia o usa HPGAPTOL). AL_LOCALE (Progetto)  Punto interno del locale o [Fase]:"
        );
        // Cambiando fase il motivo cade.
        need_text(c.on_text_input("F"));
        assert_eq!(
            c.prompt(),
            "AL_LOCALE (Fatto)  Punto interno del locale o [Fase]:"
        );

        // Il locale A: la superficie, poi il nome, in testo libero.
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 12,00 m². Nome del locale:");
        assert_eq!(c.input_kind(), InputKind::FreeText);
        assert!(!c.point_step_accepts_keywords());
        assert!(c.options().is_empty());
        // Un clic non è un nome.
        need(c.on_point(pt(6.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 12,00 m². Nome del locale:");
        // Il nome non può essere vuoto.
        need(c.on_enter());
        let empty =
            "Il nome del locale non può essere vuoto. Superficie netta 12,00 m². Nome del locale:";
        assert_eq!(c.prompt(), empty);
        need_text(c.on_text_input("   "));
        assert_eq!(c.prompt(), empty);
        // Spazi interni e maiuscole del nome restano; poi il numero.
        need_text(c.on_text_input("  Camera da letto "));
        assert_eq!(c.prompt(), "Numero <nessuno>:");
        assert_eq!(c.input_kind(), InputKind::FreeText);
        let entities = committed(c.on_enter());
        let rec = entities[1]
            .common()
            .extended_data
            .get_record(APP_LOCALE)
            .expect("XDATA");
        assert_eq!(decode(rec).unwrap().0.name, "Camera da letto");

        // Invio al primo passo chiude il comando.
        let mut c = RoomCommand::new(case_snap(Form::Continua));
        assert!(matches!(c.on_enter(), CmdResult::Cancel));
    }

    #[test]
    fn fatto_commits_outline_and_label() {
        let snap = case_snap(Form::Continua);
        assert_eq!(snap.text_height, 2.5, "TEXTSIZE del disegno");
        let mut c = RoomCommand::new(snap);
        need(c.on_point(pt(2.0, 1.5)));
        need_text(c.on_text_input("A"));
        assert_eq!(c.prompt(), "Numero <nessuno>:");
        let entities = committed(c.on_enter());
        assert_eq!(entities.len(), 2, "contorno e testo");
        let EntityType::LwPolyline(outline) = &entities[0] else {
            panic!("contorno: {:?}", entities[0])
        };
        assert_eq!(outline.common.layer, LAYER_LOCALI);
        assert!(outline.is_closed);
        assert!((polyline_area(&outline.vertices) - 12.0).abs() < 1e-9);
        let EntityType::MText(text) = &entities[1] else {
            panic!("testo: {:?}", entities[1])
        };
        assert_eq!(text.value, "A\\PS = 12,00 m²");
        assert_eq!(text.common.layer, LAYER_LOCALI);
        assert_eq!(text.height, 2.5);
        assert!(near(text.insertion_point, [2.0, 1.5]));
        let room = Room {
            phase: Phase::Fatto,
            name: "A".into(),
            number: String::new(),
            seed: [2.0, 1.5],
        };
        for (entity, part) in entities.iter().zip([RoomPart::Contorno, RoomPart::Testo]) {
            let rec = entity
                .common()
                .extended_data
                .get_record(APP_LOCALE)
                .expect("XDATA");
            assert_eq!(decode(rec), Some((room.clone(), part)));
        }
        assert!(c.preserve_commit_layer() && c.preserve_commit_style());
    }

    #[test]
    fn progetto_commits_only_the_seed() {
        let mut c = RoomCommand::new(case_snap(Form::Continua));
        need_text(c.on_text_input("F"));
        need(c.on_point(pt(3.0, 1.0)));
        assert_eq!(c.prompt(), "Superficie netta 12,00 m². Nome del locale:");
        need_text(c.on_text_input("Soggiorno"));
        let entities = committed(c.on_text_input(" 12 ").expect("numero"));
        assert_eq!(entities.len(), 1, "solo il seme");
        let EntityType::MText(seed) = &entities[0] else {
            panic!("seme: {:?}", entities[0])
        };
        assert_eq!(seed.value, "Soggiorno");
        assert_eq!(seed.common.layer, LAYER_LOCALI_PROGETTO);
        let rec = seed
            .common
            .extended_data
            .get_record(APP_LOCALE)
            .expect("XDATA");
        let want = Room {
            phase: Phase::Progetto,
            name: "Soggiorno".into(),
            number: "12".into(),
            seed: [3.0, 1.0],
        };
        assert_eq!(decode(rec), Some((want, RoomPart::Seme)));
    }

    /// Nel progetto la superficie viene dalla geometria di progetto e un seme
    /// dentro un muro nuovo si rifiuta; nel fatto le zone non contano.
    #[test]
    fn progetto_measures_the_project_and_refuses_new_walls() {
        // Tramezzo demolito: nel progetto A e B sono un locale solo.
        let mut scene = scene_of(case_a(Form::Continua, M));
        let zone = zone_entity(&scene, ZoneState::Demolito, [4.05, 0.0], [4.05, 3.0], 0.0);
        scene.add_entity(zone);
        let snap = snap_of(&scene);
        let mut c = RoomCommand::new(snap.clone());
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(
            c.prompt(),
            "Superficie netta 12,00 m². Nome del locale:",
            "il fatto non vede le zone"
        );
        let mut c = RoomCommand::new(snap);
        need_text(c.on_text_input("F"));
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 24,00 m². Nome del locale:");

        // Muro nuovo da 0,12 sull'asse (2;0)-(2;3), dato due volte: per la
        // regola pari-dispari conta una volta sola.
        let mut scene = scene_of(case_a(Form::Continua, M));
        let zone = zone_entity(&scene, ZoneState::Nuovo, [2.0, 0.0], [2.0, 3.0], 0.12);
        scene.add_entity(zone.clone());
        scene.add_entity(zone);
        let snap = snap_of(&scene);
        assert_eq!(snap.new_zone_rings.len(), 1, "doppione tolto");
        let mut c = RoomCommand::new(snap.clone());
        need_text(c.on_text_input("F"));
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(
            c.prompt(),
            "Il punto cade dentro un muro nuovo. AL_LOCALE (Progetto)  Punto interno del locale o [Fase]:"
        );
        assert_eq!(c.input_kind(), InputKind::Point);
        // Sul bordo del muro nuovo è lo stesso.
        need(c.on_point(pt(1.94, 1.5)));
        assert!(c
            .prompt()
            .starts_with("Il punto cade dentro un muro nuovo."));
        need(c.on_point(pt(1.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 5,82 m². Nome del locale:");
        need_text(c.on_text_input("Studio"));
        assert_eq!(committed(c.on_enter()).len(), 1);
        // Nel fatto il muro nuovo non c'è ancora.
        let mut c = RoomCommand::new(snap);
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 12,00 m². Nome del locale:");
    }

    #[test]
    fn islands_and_tolerance_in_the_prompts() {
        // Il pilastro del caso D: l'isola nella superficie e nell'avviso.
        let mut entities = case_a(Form::Continua, M);
        entities.push(pilastro(M));
        let mut c = RoomCommand::new(snap_of(&scene_of(entities)));
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(
            c.prompt(),
            "Superficie netta 11,91 m² (1 isola). Nome del locale:"
        );
        need_text(c.on_text_input("A"));
        assert_eq!(c.prompt(), "Locale A: 1 isole sottratte. Numero <nessuno>:");

        // Review Focus 3: vuoto di 2 mm sulla faccia ovest di A.
        let mut entities = case_a(Form::Continua, M);
        open_west_face(&mut entities);
        let scene = scene_of(entities);
        let mut c = RoomCommand::new(room_snapshot(&scene.document, M, TOL).0);
        need(c.on_point(pt(2.0, 1.5)));
        assert!(
            c.prompt()
                .starts_with("Nessun locale chiuso attorno al punto"),
            "{}",
            c.prompt()
        );
        // Con HPGAPTOL a 5 mm si chiude: 12,00, ma solo per la tolleranza.
        let mut c = RoomCommand::new(room_snapshot(&scene.document, M, 0.005).0);
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 12,00 m². Nome del locale:");
        need_text(c.on_text_input("A"));
        assert_eq!(
            c.prompt(),
            "Locale A: chiuso solo dalla tolleranza HPGAPTOL, superficie da verificare. Numero <nessuno>:"
        );
        let entities = committed(c.on_enter());
        let EntityType::LwPolyline(outline) = &entities[0] else {
            panic!("contorno")
        };
        assert!(
            outline.vertices.iter().all(|v| v.bulge == 0.0),
            "l'anello delle facce"
        );
        assert!((polyline_area(&outline.vertices) - 12.0).abs() < 1e-3);
        let EntityType::MText(text) = &entities[1] else {
            panic!("testo")
        };
        assert_eq!(text.value, "A\\PS = 12,00 m²");
    }

    /// R54: un vuoto con i due lati attaccati altrove fonde due locali senza
    /// che la regola dei muri dentro lo veda; il seme di un altro locale di
    /// fatto nella regione lo dice prima del numero.
    #[test]
    fn fused_room_is_noticed_before_the_number() {
        let mut entities = case_a(Form::ChiusaPerMuro, M);
        let Some(EntityType::LwPolyline(tramezzo)) =
            entities.iter_mut().find(|entity| is_tramezzo(entity))
        else {
            panic!("tramezzo")
        };
        open_tramezzo(tramezzo);
        let mut scene = scene_of(entities);
        // B scritto prima del vuoto: contorno e testo, un locale solo.
        let b = Computed {
            name: "B".into(),
            number: String::new(),
            area_m2: 11.7,
            region: Region {
                outer: rect(4.1, 0.0, 8.0, 3.0),
                holes: Vec::new(),
            },
            confirm: false,
            exact: false,
            islands: 0,
            seed: [6.0, 1.5],
        };
        for entity in room_entities(&b, Phase::Fatto, &FxHashMap::default(), 0.25, TOL) {
            scene.add_entity(entity);
        }
        let snap = snap_of(&scene);
        let b_room = Room {
            phase: Phase::Fatto,
            name: "B".into(),
            number: String::new(),
            seed: [6.0, 1.5],
        };
        assert_eq!(snap.fatto_rooms, [b_room], "uno per locale");

        let mut c = RoomCommand::new(snap.clone());
        need(c.on_point(pt(2.0, 1.5)));
        assert_eq!(c.prompt(), "Superficie netta 23,70 m². Nome del locale:");
        need_text(c.on_text_input("A"));
        assert_eq!(
            c.prompt(),
            "Locali A + B fusi in uno (23,70 m²): nome da confermare. Numero <nessuno>:"
        );
        // Nel progetto i semi si raggruppano all'estrazione.
        let mut c = RoomCommand::new(snap);
        need_text(c.on_text_input("F"));
        need(c.on_point(pt(2.0, 1.5)));
        need_text(c.on_text_input("A"));
        assert_eq!(c.prompt(), "Numero <nessuno>:");
    }

    #[test]
    fn refresh_rooms_edits_only_what_changes() {
        let mut scene = scene_of(case_a(Form::Continua, M));
        let (outline, text) = write_room(&mut scene, "A", [2.0, 1.5]);
        assert_eq!(
            refresh_rooms(&scene.document, M, TOL),
            RoomsRefresh::default(),
            "niente da cambiare"
        );

        // Il tramezzo da x = 4,00 a x = 3,90.
        let east = line_handle(&scene.document, [4.0, 0.0], [4.0, 3.0]);
        if let Some(EntityType::Line(line)) = scene.document.get_entity_mut(east) {
            line.start.x = 3.9;
            line.end.x = 3.9;
        }
        let refresh = refresh_rooms(&scene.document, M, TOL);
        assert!(refresh.notices.is_empty(), "{:?}", refresh.notices);
        let [edits] = &refresh.changed[..] else {
            panic!("{:?}", refresh.changed)
        };
        assert_eq!(edits.len(), 2, "{edits:?}");
        assert_eq!(edits[0].0, outline);
        let RoomEdit::Contorno { vertices, closed } = &edits[0].1 else {
            panic!("{:?}", edits[0])
        };
        assert!(*closed);
        assert!((polyline_area(vertices) - 11.7).abs() < 1e-9);
        assert_eq!(
            edits[1],
            (
                text,
                RoomEdit::Testo {
                    value: "A\\PS = 11,70 m²".into()
                }
            )
        );
        // Applicate, non resta niente.
        for (handle, edit) in edits {
            edit.apply_to(scene.document.get_entity_mut(*handle).unwrap());
        }
        assert_eq!(
            refresh_rooms(&scene.document, M, TOL),
            RoomsRefresh::default()
        );

        // Senza la faccia sud di A il locale non è più chiuso: niente cambia.
        let south = line_handle(&scene.document, [0.0, 0.0], [4.0, 0.0]);
        scene.erase_entities(&[south]);
        let refresh = refresh_rooms(&scene.document, M, TOL);
        assert!(refresh.changed.is_empty(), "{:?}", refresh.changed);
        assert_eq!(
            refresh.notices,
            [Notice::LocaleNonChiuso { name: "A".into() }]
        );
    }

    #[test]
    fn refresh_rooms_pairs_by_seed_and_reports_lone_parts() {
        let mut scene = scene_of(case_a(Form::Continua, M));
        // Le parti di A e B in ordine sparso: contorno di A, di B, testo di
        // B, di A.
        let a = room_of(&scene, "A", [2.0, 1.5]);
        let b = room_of(&scene, "B", [6.0, 1.5]);
        let a_outline = scene.add_entity(a[0].clone());
        scene.add_entity(b[0].clone());
        let b_text = scene.add_entity(b[1].clone());
        let a_text = scene.add_entity(a[1].clone());
        assert_eq!(
            refresh_rooms(&scene.document, M, TOL),
            RoomsRefresh::default(),
            "accoppiati per seme"
        );

        // Senza il contorno di A e il testo di B, con il testo di A vecchio.
        scene.erase_entities(&[a_outline, b_text]);
        if let Some(EntityType::MText(text)) = scene.document.get_entity_mut(a_text) {
            text.value = "A\\PS = 99,00 m²".into();
        }
        let refresh = refresh_rooms(&scene.document, M, TOL);
        assert_eq!(
            refresh.notices,
            [
                Notice::TestoMancante { name: "B".into() },
                Notice::ContornoMancante { name: "A".into() },
            ]
        );
        assert_eq!(
            refresh.changed,
            [vec![(
                a_text,
                RoomEdit::Testo {
                    value: "A\\PS = 12,00 m²".into()
                }
            )]]
        );
    }

    /// R54 in `AL_AGGIORNA`: una volta per gruppo; i locali si aggiornano
    /// comunque sulla regione trovata (l'avviso è l'unica differenza).
    #[test]
    fn refresh_rooms_reports_fused_rooms_once() {
        let mut scene = scene_of(case_a(Form::ChiusaPerMuro, M));
        write_room(&mut scene, "A", [2.0, 1.5]);
        write_room(&mut scene, "B", [6.0, 1.5]);
        assert_eq!(
            refresh_rooms(&scene.document, M, TOL),
            RoomsRefresh::default()
        );
        let tramezzo = scene
            .document
            .entities()
            .find(|entity| is_tramezzo(entity))
            .map(|entity| entity.common().handle)
            .expect("tramezzo");
        if let Some(EntityType::LwPolyline(polyline)) = scene.document.get_entity_mut(tramezzo) {
            open_tramezzo(polyline);
        }
        let refresh = refresh_rooms(&scene.document, M, TOL);
        let [Notice::LocaleFuso { names, area_m2 }] = &refresh.notices[..] else {
            panic!("{:?}", refresh.notices)
        };
        assert_eq!(names, &["A", "B"]);
        assert!((area_m2 - 23.7002).abs() < 1e-9, "{area_m2}");
        assert_eq!(refresh.changed.len(), 2);
    }

    #[test]
    fn new_walls_and_outline_rules() {
        // Una zona nuova con un buco: contorno e buco insieme, pari e dispari.
        let rings = vec![rect(0.0, 0.0, 10.0, 10.0), rect(2.0, 2.0, 8.0, 8.0)];
        assert!(inside_new_walls(&rings, [1.0, 5.0], TOL), "nel muro");
        assert!(!inside_new_walls(&rings, [5.0, 5.0], TOL), "nel buco");
        assert!(
            inside_new_walls(&rings, [2.0, 5.0], TOL),
            "sul bordo del buco"
        );
        assert!(
            inside_new_walls(&rings, [0.0, 5.0], TOL),
            "sul bordo esterno"
        );
        assert!(!inside_new_walls(&rings, [11.0, 5.0], TOL), "fuori");
        assert!(!inside_new_walls(&[], [1.0, 1.0], TOL));
        // I doppioni si riconoscono da qualunque vertice e in tutti e due i versi.
        let square = rect(0.0, 0.0, 1.0, 1.0);
        let mut turned: Vec<[f64; 2]> = square.iter().rev().copied().collect();
        turned.rotate_left(2);
        assert!(same_ring(&square, &turned, TOL));
        assert!(!same_ring(&square, &rect(0.0, 0.0, 1.0, 2.0), TOL));
        assert!(!same_ring(&square, &square[..3], TOL));

        // Lo stesso contorno da un altro vertice non è una modifica.
        let polyline = |ring: &[[f64; 2]], closed: bool| {
            let mut p = LwPolyline::new();
            for &[x, y] in ring {
                p.add_vertex(LwVertex::new(Vector2::new(x, y)));
            }
            p.is_closed = closed;
            p
        };
        let ring = rect(0.0, 0.0, 4.0, 3.0);
        let mut rotated = ring.clone();
        rotated.rotate_left(1);
        let a = polyline(&ring, true);
        assert!(same_outline(&a, &polyline(&rotated, true), TOL));
        let nudged = [[0.0, 5e-7], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]];
        assert!(same_outline(&a, &polyline(&nudged, true), TOL), "entro tol");
        assert!(!same_outline(
            &a,
            &polyline(&rect(0.0, 0.0, 3.9, 3.0), true),
            TOL
        ));
        assert!(!same_outline(&a, &polyline(&ring, false), TOL));
        assert!(!same_outline(&a, &polyline(&ring[..3], true), TOL));
        let mut arc = polyline(&ring, true);
        arc.vertices[1].bulge = 0.2;
        assert!(!same_outline(&a, &arc, TOL));
    }
}
