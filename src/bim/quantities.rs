//! Quantità delle zone (spec parte 2, §4.2–4.3): una riga per zona, con le
//! grandezze in metri e l'altezza risolta con il suo stato. Logica pura: legge
//! solo geometria e XDATA di [`ZoneGeom`].
//!
//! Regole:
//! - area in pianta = anello esterno meno i buchi; lunghezza dall'asse;
//!   spessore effettivo = area ÷ lunghezza. La geometria vince sul metadato
//!   `spessore` (se diverge oltre `tol`, [`Notice::SpessoreDiverso`]), così
//!   `L × W × H` è il volume anche con buchi o con l'asse accorciato;
//! - demolizione: volume = area × H, anche senza asse; parete = L × H solo con
//!   l'asse. Varco: volume = L × W × H, altezza solo propria
//!   (`resolve_varco`). Nuova: superficie = L × H solo con l'asse, volume
//!   secondario = area × H;
//! - una zona degenere (area nulla, asse di lunghezza nulla, coordinate non
//!   finite) dà una riga incompleta con la nota: mai una divisione per zero;
//! - sovrapposizioni tra zone dello stesso stato (un varco è demolito): avviso
//!   e nota sulle due righe solo se l'area comune supera il 25% della più
//!   piccola. Angoli e incroci a T non si correggono e non avvisano.

use codec::Handle;
use kernel::geom2d::{signed_area, triangulate_rings};

use super::height::{resolve, resolve_varco, Height};
use super::report::Notice;
use super::units::{area_m2, DrawingUnit};
use super::zone::{Zone, ZoneGeom, ZoneState};

/// Oltre questa frazione dell'area della zona più piccola una sovrapposizione
/// è sostanziale (spec §4.3, soglia scelta).
const SUBSTANTIAL_OVERLAP: f64 = 0.25;

const NOTE_NO_AXIS: &str = "zona senza asse";
const NOTE_NULL_AXIS: &str = "asse di lunghezza nulla";
const NOTE_NULL_AREA: &str = "area in pianta nulla";

/// Il tipo di riga: un varco è una zona demolita con `varco` = sì.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ZoneKind {
    Demolizione,
    Varco,
    Nuova,
}

impl ZoneKind {
    fn of(zone: &Zone) -> Self {
        match (zone.state, zone.varco) {
            (ZoneState::Demolito, false) => Self::Demolizione,
            (ZoneState::Demolito, true) => Self::Varco,
            (ZoneState::Nuovo, _) => Self::Nuova,
        }
    }

    /// Prefisso del codice di riga.
    fn prefix(self) -> &'static str {
        match self {
            Self::Demolizione => "D",
            Self::Varco => "V",
            Self::Nuova => "N",
        }
    }
}

/// Una riga di quantità di una zona. Grandezze in m, m², m³; `None` = `n.d.`.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneRow {
    pub handle: Handle,
    pub kind: ZoneKind,
    /// `D1`, `V1`, `N1`…: progressivo per tipo, in ordine di handle.
    pub code: String,
    pub plan_area_m2: f64,
    pub length_m: Option<f64>,
    /// Effettivo = area in pianta ÷ lunghezza dell'asse.
    pub thickness_m: Option<f64>,
    /// Il metadato `spessore` dell'XDATA.
    pub declared_thickness_m: Option<f64>,
    pub height: Height,
    /// Demolizione: lunghezza × H.
    pub wall_area_m2: Option<f64>,
    pub volume_m3: Option<f64>,
    /// Nuova: lunghezza × H.
    pub surface_m2: Option<f64>,
    /// Note di geometria e sovrapposizioni, separate da «; ». Il motivo di
    /// un'altezza `n.d.` sta in `height.note`.
    pub note: Option<String>,
}

impl ZoneRow {
    /// La quantità principale: volume per demolizioni e varchi, superficie
    /// per le nuove murature.
    pub fn main(&self) -> Option<f64> {
        match self.kind {
            ZoneKind::Demolizione | ZoneKind::Varco => self.volume_m3,
            ZoneKind::Nuova => self.surface_m2,
        }
    }

    /// Solo le righe complete entrano nei totali e nel CSV.
    pub fn is_complete(&self) -> bool {
        self.main().is_some()
    }

    fn add_note(&mut self, text: &str) {
        self.note = Some(match self.note.take() {
            Some(note) => format!("{note}; {text}"),
            None => text.to_string(),
        });
    }
}

/// Le nuove murature di uno spessore, al millimetro (`None` = «spessore
/// n.d.»). `rows` sono indici nelle righe date; i totali sommano solo le
/// righe complete.
#[derive(Clone, Debug, PartialEq)]
pub struct ThicknessGroup {
    pub thickness_mm: Option<i64>,
    pub rows: Vec<usize>,
    pub total_surface_m2: f64,
    pub total_length_m: f64,
}

/// Le righe delle zone, in ordine di tipo (demolizioni, varchi, nuove) e,
/// dentro un tipo, di handle; `default_height` e `tol` in unità del disegno.
/// Gli avvisi: spessori dichiarati diversi da quelli misurati, poi le
/// sovrapposizioni sostanziali.
pub fn zone_rows(
    zones: &[ZoneGeom],
    default_height: Option<f64>,
    unit: DrawingUnit,
    tol: f64,
) -> (Vec<ZoneRow>, Vec<Notice>) {
    let mut order: Vec<&ZoneGeom> = zones.iter().collect();
    order.sort_by_key(|geom| (ZoneKind::of(&geom.zone), geom.handle.value()));
    let mut counts = [0_usize; 3];
    let mut rows = Vec::with_capacity(order.len());
    let mut areas = Vec::with_capacity(order.len());
    let mut notices = Vec::new();
    for geom in &order {
        let kind = ZoneKind::of(&geom.zone);
        let count = &mut counts[kind as usize];
        *count += 1;
        let code = format!("{}{count}", kind.prefix());
        let (row, area, notice) = zone_row(geom, kind, code, default_height, unit, tol);
        rows.push(row);
        areas.push(area);
        notices.extend(notice);
    }
    flag_overlaps(&order, &areas, &mut rows, &mut notices);
    (rows, notices)
}

/// Area in pianta in unità del disegno al quadrato: anello esterno meno i
/// buchi. `signed_area` somma attorno al primo punto dell'anello, quindi
/// regge le coordinate georiferite.
fn plan_area(rings: &[Vec<[f64; 2]>]) -> f64 {
    let Some((outer, holes)) = rings.split_first() else {
        return 0.0;
    };
    signed_area(outer).abs()
        - holes
            .iter()
            .map(|hole| signed_area(hole).abs())
            .sum::<f64>()
}

/// La riga di una zona, con l'area in unità del disegno (0 se degenere, così
/// la zona non entra nel controllo delle sovrapposizioni) e l'eventuale avviso
/// sullo spessore.
fn zone_row(
    geom: &ZoneGeom,
    kind: ZoneKind,
    code: String,
    default_height: Option<f64>,
    unit: DrawingUnit,
    tol: f64,
) -> (ZoneRow, f64, Option<Notice>) {
    let zone = &geom.zone;
    let per_metre = unit.per_metre();
    let raw_area = plan_area(&geom.rings);
    let area = (raw_area.is_finite() && raw_area > tol * tol).then_some(raw_area);
    let length = zone
        .axis
        .map(|(a, b)| (b[0] - a[0]).hypot(b[1] - a[1]))
        .filter(|length| length.is_finite() && *length > tol);
    let thickness = area.zip(length).map(|(area, length)| area / length);
    let declared = zone.thickness.filter(|t| t.is_finite() && *t > 0.0);
    let height = match kind {
        ZoneKind::Varco => resolve_varco(zone.height, unit),
        ZoneKind::Demolizione | ZoneKind::Nuova => resolve(zone.height, default_height, unit),
    };

    let area_m = area.map(|area| area_m2(area, unit));
    let length_m = length.map(|length| length / per_metre);
    let thickness_m = thickness.map(|t| t / per_metre);
    // Nessuna quantità da una zona di area nulla, neanche con l'asse.
    let h = height.value_m.filter(|_| area.is_some());
    let (wall_area_m2, volume_m3, surface_m2) = match kind {
        ZoneKind::Demolizione => (
            length_m.zip(h).map(|(l, h)| l * h),
            area_m.zip(h).map(|(a, h)| a * h),
            None,
        ),
        ZoneKind::Varco => (
            None,
            length_m
                .zip(thickness_m)
                .zip(h)
                .map(|((l, w), h)| l * w * h),
            None,
        ),
        ZoneKind::Nuova => (
            None,
            area_m.zip(h).map(|(a, h)| a * h),
            length_m.zip(h).map(|(l, h)| l * h),
        ),
    };

    let notice = declared
        .zip(thickness)
        .filter(|(declared, measured)| (declared - measured).abs() > tol)
        .map(|(declared, measured)| Notice::SpessoreDiverso {
            code: code.clone(),
            declared_m: declared / per_metre,
            measured_m: measured / per_metre,
        });
    let mut row = ZoneRow {
        handle: geom.handle,
        kind,
        code,
        plan_area_m2: area_m.unwrap_or(0.0),
        length_m,
        thickness_m,
        declared_thickness_m: declared.map(|t| t / per_metre),
        height,
        wall_area_m2,
        volume_m3,
        surface_m2,
        note: None,
    };
    if area.is_none() {
        row.add_note(NOTE_NULL_AREA);
    }
    match (zone.axis, length) {
        (None, _) => row.add_note(NOTE_NO_AXIS),
        (Some(_), None) => row.add_note(NOTE_NULL_AXIS),
        (Some(_), Some(_)) => {}
    }
    (row, area.unwrap_or(0.0), notice)
}

/// Avviso e nota per ogni coppia di zone dello stesso stato con un'area
/// comune oltre la soglia; `zones`, `areas` e `rows` vanno di pari passo.
fn flag_overlaps(
    zones: &[&ZoneGeom],
    areas: &[f64],
    rows: &mut [ZoneRow],
    notices: &mut Vec<Notice>,
) {
    for i in 0..zones.len() {
        for j in i + 1..zones.len() {
            let smaller = areas[i].min(areas[j]);
            if zones[i].zone.state != zones[j].zone.state || smaller <= 0.0 {
                continue;
            }
            let common = overlap_area(&zones[i].rings, &zones[j].rings);
            if common <= SUBSTANTIAL_OVERLAP * smaller {
                continue;
            }
            let (a, b) = (rows[i].code.clone(), rows[j].code.clone());
            rows[i].add_note(&format!("sovrapposta in modo sostanziale a {b}"));
            rows[j].add_note(&format!("sovrapposta in modo sostanziale a {a}"));
            notices.push(Notice::ZoneSovrapposteSostanzialmente { codes: vec![a, b] });
        }
    }
}

/// Le nuove murature per spessore effettivo al millimetro, dal più sottile;
/// il gruppo «spessore n.d.» in fondo.
pub fn group_new_by_thickness(rows: &[ZoneRow]) -> Vec<ThicknessGroup> {
    let mut groups: Vec<ThicknessGroup> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if row.kind != ZoneKind::Nuova {
            continue;
        }
        let thickness_mm = row.thickness_m.map(|t| (t * 1000.0).round() as i64);
        let at = match groups.iter().position(|g| g.thickness_mm == thickness_mm) {
            Some(at) => at,
            None => {
                groups.push(ThicknessGroup {
                    thickness_mm,
                    rows: Vec::new(),
                    total_surface_m2: 0.0,
                    total_length_m: 0.0,
                });
                groups.len() - 1
            }
        };
        let group = &mut groups[at];
        group.rows.push(index);
        if let (Some(surface), Some(length)) = (row.surface_m2, row.length_m) {
            group.total_surface_m2 += surface;
            group.total_length_m += length;
        }
    }
    groups.sort_by_key(|g| (g.thickness_mm.is_none(), g.thickness_mm));
    groups
}

/// Un riquadro allineato agli assi.
#[derive(Clone, Copy)]
struct Bounds {
    min: [f64; 2],
    max: [f64; 2],
}

impl Bounds {
    fn of<'a>(points: impl IntoIterator<Item = &'a [f64; 2]>) -> Option<Bounds> {
        let mut points = points.into_iter();
        let first = *points.next()?;
        let mut bounds = Bounds {
            min: first,
            max: first,
        };
        for p in points {
            for k in 0..2 {
                bounds.min[k] = bounds.min[k].min(p[k]);
                bounds.max[k] = bounds.max[k].max(p[k]);
            }
        }
        Some(bounds)
    }

    fn overlaps(&self, other: &Bounds) -> bool {
        (0..2).all(|k| self.min[k] <= other.max[k] && other.min[k] <= self.max[k])
    }
}

/// Area comune di due zone (anelli in unità del disegno: il primo è
/// l'esterno, gli altri i buchi), in unità del disegno al quadrato.
///
/// Il kernel non ha operazioni booleane tra poligoni: si triangolano le due
/// zone e si somma, per ogni coppia di triangoli i cui riquadri si toccano,
/// l'area del ritaglio di uno con l'altro (Sutherland–Hodgman: il ritagliante
/// è convesso). I triangoli di una zona non si sovrappongono tra loro, quindi
/// la somma è l'area comune. Zone lontane costano un confronto di riquadri.
/// Tutto si calcola attorno a un'origine locale (come R11): a coordinate
/// georiferite i prodotti perderebbero le cifre che contano.
fn overlap_area(a: &[Vec<[f64; 2]>], b: &[Vec<[f64; 2]>]) -> f64 {
    let (Some(box_a), Some(box_b)) = (
        a.first().and_then(|outer| Bounds::of(outer)),
        b.first().and_then(|outer| Bounds::of(outer)),
    ) else {
        return 0.0;
    };
    if !box_a.overlaps(&box_b) {
        return 0.0;
    }
    let origin = box_a.min;
    let (triangles_a, triangles_b) = (triangles(a, origin), triangles(b, origin));
    let mut common = 0.0;
    for (ta, ba) in &triangles_a {
        for (tb, bb) in &triangles_b {
            if ba.overlaps(bb) {
                common += clipped_area(ta, tb);
            }
        }
    }
    common
}

/// I triangoli della zona spostati in `origin`, in senso antiorario, con il
/// loro riquadro; quelli piatti si saltano.
fn triangles(rings: &[Vec<[f64; 2]>], origin: [f64; 2]) -> Vec<([[f64; 2]; 3], Bounds)> {
    let local: Vec<Vec<[f64; 2]>> = rings
        .iter()
        .map(|ring| {
            ring.iter()
                .map(|p| [p[0] - origin[0], p[1] - origin[1]])
                .collect()
        })
        .collect();
    let (points, indices) = triangulate_rings(&local);
    indices
        .iter()
        .filter(|triangle| triangle.iter().all(|&index| index < points.len()))
        .filter_map(|triangle| {
            let mut t = triangle.map(|index| points[index]);
            let area = signed_area(&t);
            if !area.is_finite() || area == 0.0 {
                return None;
            }
            if area < 0.0 {
                t.swap(1, 2);
            }
            Some((t, Bounds::of(&t)?))
        })
        .collect()
}

/// Area del triangolo `subject` ritagliato dal triangolo antiorario
/// `clipper` (Sutherland–Hodgman).
fn clipped_area(subject: &[[f64; 2]; 3], clipper: &[[f64; 2]; 3]) -> f64 {
    let mut polygon: Vec<[f64; 2]> = subject.to_vec();
    for k in 0..3 {
        if polygon.is_empty() {
            return 0.0;
        }
        let (p, q) = (clipper[k], clipper[(k + 1) % 3]);
        // > 0 a sinistra del lato, cioè dentro.
        let side = |v: [f64; 2]| (q[0] - p[0]) * (v[1] - p[1]) - (q[1] - p[1]) * (v[0] - p[0]);
        let input = std::mem::take(&mut polygon);
        let mut previous = input[input.len() - 1];
        for &current in &input {
            let (sp, sc) = (side(previous), side(current));
            if (sp >= 0.0) != (sc >= 0.0) {
                // Segni opposti: `sp - sc` non è mai zero.
                let t = sp / (sp - sc);
                polygon.push([
                    previous[0] + t * (current[0] - previous[0]),
                    previous[1] + t * (current[1] - previous[1]),
                ]);
            }
            if sc >= 0.0 {
                polygon.push(current);
            }
            previous = current;
        }
    }
    signed_area(&polygon).abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bim::height::HeightState;
    use crate::bim::zone::{read_zone, zone_hatch, ZoneOrigin};
    use DrawingUnit::{Centimetri, Metri};

    const EPS: f64 = 1e-9;

    fn tol() -> f64 {
        Metri.tolerance()
    }

    fn close(got: Option<f64>, want: f64) -> bool {
        got.is_some_and(|v| (v - want).abs() < EPS)
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    fn zone(state: ZoneState, axis: Option<([f64; 2], [f64; 2])>) -> Zone {
        Zone {
            state,
            origin: if axis.is_some() {
                ZoneOrigin::MuroDuePunti
            } else {
                ZoneOrigin::Rettangolo
            },
            axis,
            thickness: None,
            height: None,
            varco: false,
        }
    }

    /// La zona come la legge `zones_in`: campitura scritta e riletta.
    fn geom(handle: u64, zone: Zone, rings: Vec<Vec<[f64; 2]>>) -> ZoneGeom {
        read_zone(Handle::new(handle), &zone_hatch(&zone, &rings)).expect("zona leggibile")
    }

    /// Caso A: il tramezzo 0,10 × 3,00 con l'asse sulla mezzeria.
    fn tramezzo(handle: u64, height: Option<f64>) -> ZoneGeom {
        let z = Zone {
            thickness: Some(0.10),
            height,
            ..zone(ZoneState::Demolito, Some(([4.05, 0.0], [4.05, 3.0])))
        };
        geom(handle, z, vec![rect(4.0, 0.0, 4.1, 3.0)])
    }

    /// Caso B: il varco 0,10 × 0,90 nello stesso muro.
    fn varco(handle: u64, height: Option<f64>) -> ZoneGeom {
        let z = Zone {
            thickness: Some(0.10),
            height,
            varco: true,
            ..zone(ZoneState::Demolito, Some(([4.05, 1.0], [4.05, 1.9])))
        };
        geom(handle, z, vec![rect(4.0, 1.0, 4.1, 1.9)])
    }

    /// Caso G: una nuova muratura con asse 2,00 e spessore 0,08, a quota `y`.
    fn nuova(handle: u64, y: f64) -> ZoneGeom {
        let z = Zone {
            thickness: Some(0.08),
            ..zone(ZoneState::Nuovo, Some(([0.0, y + 0.04], [2.0, y + 0.04])))
        };
        geom(handle, z, vec![rect(0.0, y, 2.0, y + 0.08)])
    }

    #[test]
    fn demolition_without_height_default_is_not_available() {
        let (rows, notices) = zone_rows(&[tramezzo(0x10, None)], None, Metri, tol());
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.kind, ZoneKind::Demolizione);
        assert_eq!(row.volume_m3, None);
        assert_eq!(row.wall_area_m2, None);
        assert_eq!(row.height.state, HeightState::NonDisponibile);
        assert!(!row.is_complete());
        assert_eq!(row.main(), None);
        // La geometria resta: area, lunghezza e spessore non dipendono da H.
        assert!(close(Some(row.plan_area_m2), 0.30));
        assert!(close(row.length_m, 3.0));
        assert!(close(row.thickness_m, 0.10));
        assert!(notices.is_empty(), "{notices:?}");
    }

    #[test]
    fn demolition_inherits_default() {
        let (rows, notices) = zone_rows(&[tramezzo(0x10, None)], Some(2.70), Metri, tol());
        let row = &rows[0];
        assert_eq!(row.code, "D1");
        assert_eq!(row.handle, Handle::new(0x10));
        assert!(close(Some(row.plan_area_m2), 0.30));
        assert!(close(row.length_m, 3.00));
        assert!(close(row.thickness_m, 0.10));
        assert!(close(row.declared_thickness_m, 0.10));
        assert!(close(row.volume_m3, 0.81));
        assert!(close(row.wall_area_m2, 8.10));
        assert_eq!(row.surface_m2, None);
        assert_eq!(row.height.state, HeightState::Ereditata);
        assert!(close(row.height.value_m, 2.70));
        assert!(row.is_complete());
        assert!(close(row.main(), 0.81));
        assert_eq!(row.note, None);
        assert!(notices.is_empty(), "{notices:?}");
    }

    #[test]
    fn demolition_own_height() {
        let (rows, _) = zone_rows(&[tramezzo(0x10, Some(3.0))], Some(2.70), Metri, tol());
        let row = &rows[0];
        assert!(close(row.volume_m3, 0.90));
        assert!(close(row.wall_area_m2, 9.00));
        assert_eq!(row.height.state, HeightState::Inserita);
    }

    #[test]
    fn demolition_without_axis_still_has_volume() {
        let g = geom(
            0x10,
            zone(ZoneState::Demolito, None),
            vec![rect(4.0, 0.0, 4.1, 3.0)],
        );
        let (rows, _) = zone_rows(&[g], Some(2.70), Metri, tol());
        let row = &rows[0];
        assert!(close(row.volume_m3, 0.81));
        assert_eq!(row.wall_area_m2, None);
        assert_eq!(row.length_m, None);
        assert_eq!(row.thickness_m, None);
        assert!(row.is_complete());
        assert_eq!(row.note.as_deref(), Some("zona senza asse"));
    }

    #[test]
    fn varco_requires_own_height() {
        let (rows, _) = zone_rows(&[varco(0x10, None)], Some(2.70), Metri, tol());
        let row = &rows[0];
        assert_eq!(row.kind, ZoneKind::Varco);
        assert_eq!(row.code, "V1");
        assert_eq!(row.height.state, HeightState::NonDisponibile);
        assert_eq!(
            row.height.note.as_deref(),
            Some("altezza del varco da inserire")
        );
        assert_eq!(row.volume_m3, None);
        assert!(!row.is_complete());

        let (rows, _) = zone_rows(&[varco(0x10, Some(2.10))], Some(2.70), Metri, tol());
        let row = &rows[0];
        assert_eq!(row.height.state, HeightState::Inserita);
        assert!(close(row.length_m, 0.90));
        assert!(close(row.thickness_m, 0.10));
        assert!(close(row.volume_m3, 0.189));
        assert_eq!(row.wall_area_m2, None);
        assert!(row.is_complete());
    }

    #[test]
    fn new_wall_surface_and_group() {
        let (rows, notices) = zone_rows(&[nuova(0x10, 0.0)], Some(2.70), Metri, tol());
        let row = &rows[0];
        assert_eq!(row.kind, ZoneKind::Nuova);
        assert_eq!(row.code, "N1");
        assert!(close(Some(row.plan_area_m2), 0.16));
        assert!(close(row.thickness_m, 0.08));
        assert!(close(row.surface_m2, 5.40));
        assert!(close(row.volume_m3, 0.432));
        assert!(close(row.main(), 5.40));
        assert!(notices.is_empty(), "{notices:?}");
        let groups = group_new_by_thickness(&rows);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].thickness_mm, Some(80));
        assert_eq!(groups[0].rows, vec![0]);

        // Due zone dello stesso spessore: un gruppo, somma esatta.
        let (rows, _) = zone_rows(
            &[nuova(0x10, 0.0), nuova(0x11, 1.0)],
            Some(2.70),
            Metri,
            tol(),
        );
        let groups = group_new_by_thickness(&rows);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].thickness_mm, Some(80));
        assert_eq!(groups[0].rows, vec![0, 1]);
        assert!((groups[0].total_surface_m2 - 10.80).abs() < EPS);
        assert!((groups[0].total_length_m - 4.00).abs() < EPS);
    }

    #[test]
    fn new_wall_without_axis_has_no_surface() {
        let bare = geom(
            0x20,
            zone(ZoneState::Nuovo, None),
            vec![rect(0.0, 5.0, 2.0, 5.08)],
        );
        let (rows, _) = zone_rows(&[nuova(0x10, 0.0), bare], Some(2.70), Metri, tol());
        let row = &rows[1];
        assert_eq!(row.code, "N2");
        assert_eq!(row.surface_m2, None);
        assert!(!row.is_complete());
        assert_eq!(row.note.as_deref(), Some("zona senza asse"));
        assert!(close(Some(row.plan_area_m2), 0.16));
        assert!(close(row.volume_m3, 0.432));
        // Senza spessore: gruppo «spessore n.d.», in fondo, fuori dai totali.
        let groups = group_new_by_thickness(&rows);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].thickness_mm, Some(80));
        assert_eq!(groups[1].thickness_mm, None);
        assert_eq!(groups[1].rows, vec![1]);
        assert_eq!(groups[1].total_surface_m2, 0.0);
        assert_eq!(groups[1].total_length_m, 0.0);
    }

    #[test]
    fn declared_thickness_differs_from_measured() {
        let z = Zone {
            thickness: Some(0.12),
            ..zone(ZoneState::Demolito, Some(([4.05, 0.0], [4.05, 3.0])))
        };
        let g = geom(0x10, z, vec![rect(4.0, 0.0, 4.1, 3.0)]);
        let (rows, notices) = zone_rows(&[g], Some(2.70), Metri, tol());
        let row = &rows[0];
        assert!(close(row.thickness_m, 0.10));
        assert!(close(row.declared_thickness_m, 0.12));
        assert!(row.is_complete(), "lo spessore diverso non toglie la riga");
        assert_eq!(notices.len(), 1);
        match &notices[0] {
            Notice::SpessoreDiverso {
                code,
                declared_m,
                measured_m,
            } => {
                assert_eq!(code, "D1");
                assert!((declared_m - 0.12).abs() < EPS);
                assert!((measured_m - 0.10).abs() < EPS);
            }
            other => panic!("atteso SpessoreDiverso, trovato {other:?}"),
        }
        // `L × W × H` è il volume (CSV del libretto delle misure).
        let product =
            row.length_m.unwrap() * row.thickness_m.unwrap() * row.height.value_m.unwrap();
        assert!(close(row.volume_m3, product));
    }

    #[test]
    fn degenerate_zone_does_not_panic() {
        // Area nulla (anello piatto), asse di lunghezza nulla, altezza NaN.
        let flat = Zone {
            height: Some(f64::NAN),
            thickness: Some(f64::NAN),
            ..zone(ZoneState::Demolito, Some(([1.0, 1.0], [1.0, 1.0])))
        };
        let flat_ring = vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]];
        let zones = [
            ZoneGeom {
                handle: Handle::new(1),
                zone: flat.clone(),
                rings: vec![flat_ring.clone()],
            },
            ZoneGeom {
                handle: Handle::new(2),
                zone: Zone {
                    varco: true,
                    ..flat.clone()
                },
                rings: vec![flat_ring.clone()],
            },
            ZoneGeom {
                handle: Handle::new(3),
                zone: Zone {
                    state: ZoneState::Nuovo,
                    ..flat.clone()
                },
                rings: vec![flat_ring],
            },
            // Nessun anello, coordinate non finite.
            ZoneGeom {
                handle: Handle::new(4),
                zone: Zone {
                    state: ZoneState::Nuovo,
                    ..flat.clone()
                },
                rings: vec![],
            },
            ZoneGeom {
                handle: Handle::new(5),
                zone: Zone {
                    state: ZoneState::Nuovo,
                    axis: Some(([0.0, 0.0], [f64::NAN, 1.0])),
                    ..flat.clone()
                },
                rings: vec![vec![[0.0, 0.0], [f64::NAN, 0.0], [1.0, 1.0], [0.0, 1.0]]],
            },
        ];
        for default in [None, Some(2.70)] {
            let (rows, _) = zone_rows(&zones, default, Metri, tol());
            assert_eq!(rows.len(), zones.len());
            for row in &rows {
                assert!(!row.is_complete(), "{row:?}");
                assert_eq!(row.main(), None, "{row:?}");
                assert_eq!(row.thickness_m, None, "{row:?}");
                assert_eq!(row.length_m, None, "{row:?}");
                assert_eq!(row.plan_area_m2, 0.0, "{row:?}");
                assert!(row.note.is_some(), "{row:?}");
                assert_eq!(row.declared_thickness_m, None, "{row:?}");
            }
            let groups = group_new_by_thickness(&rows);
            assert!(groups.iter().all(|g| g.total_surface_m2 == 0.0));
        }
        let (rows, _) = zone_rows(&zones[..1], None, Metri, tol());
        assert_eq!(rows[0].height.state, HeightState::NonDisponibile);
    }

    #[test]
    fn corner_overlap_gives_no_notice() {
        // Angolo: orizzontale e verticale si coprono per 0,10 × 0,10.
        let h = Zone {
            thickness: Some(0.10),
            ..zone(ZoneState::Nuovo, Some(([0.0, 0.05], [2.0, 0.05])))
        };
        let v = Zone {
            thickness: Some(0.10),
            ..zone(ZoneState::Nuovo, Some(([0.05, 0.0], [0.05, 2.0])))
        };
        let zones = [
            geom(0x10, h, vec![rect(0.0, 0.0, 2.0, 0.1)]),
            geom(0x11, v, vec![rect(0.0, 0.0, 0.1, 2.0)]),
        ];
        let (rows, notices) = zone_rows(&zones, Some(2.70), Metri, tol());
        assert!(notices.is_empty(), "{notices:?}");
        assert!(rows.iter().all(|row| row.note.is_none()), "{rows:?}");
        assert!((overlap_area(&zones[0].rings, &zones[1].rings) - 0.01).abs() < EPS);
    }

    #[test]
    fn substantial_overlap_gives_notice() {
        // Metà della più piccola in comune.
        let a = Zone {
            thickness: Some(0.10),
            ..zone(ZoneState::Nuovo, Some(([0.0, 0.05], [2.0, 0.05])))
        };
        let b = Zone {
            thickness: Some(0.10),
            ..zone(ZoneState::Nuovo, Some(([1.0, 0.05], [3.0, 0.05])))
        };
        let zones = [
            geom(0x10, a, vec![rect(0.0, 0.0, 2.0, 0.1)]),
            geom(0x11, b, vec![rect(1.0, 0.0, 3.0, 0.1)]),
        ];
        let (rows, notices) = zone_rows(&zones, Some(2.70), Metri, tol());
        assert_eq!(
            notices,
            vec![Notice::ZoneSovrapposteSostanzialmente {
                codes: vec!["N1".into(), "N2".into()],
            }]
        );
        assert!(rows[0].note.as_deref().is_some_and(|n| n.contains("N2")));
        assert!(rows[1].note.as_deref().is_some_and(|n| n.contains("N1")));
        // Il numero non si corregge da solo.
        assert!(close(rows[0].surface_m2, 5.40));
        assert!(close(rows[1].surface_m2, 5.40));

        // Una ricostruzione (nuova sopra una demolita) non è una sovrapposizione.
        let rebuilt = Zone {
            thickness: Some(0.10),
            ..zone(ZoneState::Nuovo, Some(([4.05, 0.0], [4.05, 3.0])))
        };
        let rebuilt = geom(0x11, rebuilt, vec![rect(4.0, 0.0, 4.1, 3.0)]);
        let (rows, notices) = zone_rows(&[tramezzo(0x10, None), rebuilt], Some(2.70), Metri, tol());
        assert!(notices.is_empty(), "{notices:?}");
        assert!(rows.iter().all(|row| row.note.is_none()), "{rows:?}");
    }

    #[test]
    fn varco_inside_demolition_gives_notice() {
        let (rows, notices) = zone_rows(
            &[tramezzo(0x10, None), varco(0x11, Some(2.10))],
            Some(2.70),
            Metri,
            tol(),
        );
        assert_eq!(
            notices,
            vec![Notice::ZoneSovrapposteSostanzialmente {
                codes: vec!["D1".into(), "V1".into()],
            }]
        );
        assert!(rows.iter().all(|row| row.note.is_some()), "{rows:?}");
    }

    #[test]
    fn codes_follow_handle_order() {
        // 0x9 < 0x10 come numeri (come testo «10» verrebbe prima di «9»).
        let zones = [
            tramezzo(0x30, None),
            nuova(0x40, 4.0),
            tramezzo(0x10, None),
            varco(0x5, None),
            nuova(0x9, 2.0),
            nuova(0x20, 0.0),
        ];
        let (rows, _) = zone_rows(&zones, Some(2.70), Metri, tol());
        let got: Vec<(String, u64)> = rows
            .iter()
            .map(|row| (row.code.clone(), row.handle.value()))
            .collect();
        let want: Vec<(String, u64)> = [
            ("D1", 0x10),
            ("D2", 0x30),
            ("V1", 0x5),
            ("N1", 0x9),
            ("N2", 0x20),
            ("N3", 0x40),
        ]
        .into_iter()
        .map(|(code, handle)| (code.to_string(), handle))
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn centimetre_zone_gives_the_same_quantities() {
        let z = Zone {
            thickness: Some(10.0),
            height: Some(270.0),
            ..zone(ZoneState::Demolito, Some(([405.0, 0.0], [405.0, 300.0])))
        };
        let g = geom(0x10, z, vec![rect(400.0, 0.0, 410.0, 300.0)]);
        let (rows, notices) = zone_rows(&[g], None, Centimetri, Centimetri.tolerance());
        let row = &rows[0];
        assert!(close(Some(row.plan_area_m2), 0.30));
        assert!(close(row.length_m, 3.00));
        assert!(close(row.thickness_m, 0.10));
        assert!(close(row.declared_thickness_m, 0.10));
        assert!(close(row.volume_m3, 0.81));
        assert!(close(row.wall_area_m2, 8.10));
        assert!(notices.is_empty(), "{notices:?}");
    }

    #[test]
    fn overlap_area_handles_holes_and_survey_coordinates() {
        for offset in [[0.0, 0.0], [2_300_000.0, 4_600_000.0]] {
            let shift = |ring: Vec<[f64; 2]>| -> Vec<[f64; 2]> {
                ring.into_iter()
                    .map(|p| [p[0] + offset[0], p[1] + offset[1]])
                    .collect()
            };
            // 4 × 4 con un buco 2 × 2; l'altra copre la metà destra.
            let holed = vec![
                shift(rect(0.0, 0.0, 4.0, 4.0)),
                shift(rect(1.0, 1.0, 3.0, 3.0)),
            ];
            let right = vec![shift(rect(2.0, 0.0, 6.0, 4.0))];
            assert!(
                (overlap_area(&holed, &right) - 6.0).abs() < 1e-6,
                "{offset:?}"
            );
            assert!(
                (overlap_area(&right, &holed) - 6.0).abs() < 1e-6,
                "{offset:?}"
            );
            // Lontane, o che si toccano su un lato: niente in comune.
            let far = vec![shift(rect(10.0, 0.0, 11.0, 1.0))];
            assert_eq!(overlap_area(&holed, &far), 0.0);
            let beside = vec![shift(rect(4.0, 0.0, 5.0, 4.0))];
            assert!(overlap_area(&holed, &beside).abs() < 1e-6, "{offset:?}");
        }
    }
}
