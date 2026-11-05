//! Comando apertura (spec parte 3 §5.1): `AL_PORTA`, `AL_FINESTRA` e
//! `AL_PORTAFINESTRA`.
//!
//! Chiede i **due punti sull'asse del muro alle spalle del vano**: la
//! larghezza è la loro distanza (`dal disegno`). Lo spessore si ricava dalle
//! facce del muro sui due lati del vano (il vano stesso non ha facce, quindi
//! l'asse si estende di [`REACH_M`] oltre le spalle per toccare le facce che lo
//! fiancheggiano); se non si trovano, **lo chiede**, mai indovinato. Poi chiede
//! lo stato, l'altezza (Invio = `n.d.`, nessun valore di ripiego) e, per le
//! finestre, il davanzale. Con un valore fuori dagli intervalli attesi chiede
//! conferma (solo un avviso, come le zone). Consegna una sola `LWPOLYLINE` con
//! `CommitEntitiesAndExit`.
//!
//! Lavora su un'istantanea dei muri ([`ApertureSnapshot`]) che prepara il lato
//! app: qui niente documento e niente undo. Tutto sul piano XY del mondo, in
//! unità del disegno.

use codec::{EntityType, Handle};
use glam::DVec3;
use kernel::geom2d::{distance_to, Curve};
use rustc_hash::FxHashMap;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::BoundarySource;

use super::aperture::{aperture_polyline, Aperture, ApertureKind, ApertureState};
use super::commands::{format_number, parse_number, yes_no};
use super::units::DrawingUnit;
use super::walls::{rect_from_axis, rect_from_axis_thickness, AxisMiss};
use super::MAX_WALL_THICKNESS_M;

/// Di quanto si estende l'asse oltre le spalle per toccare le facce del muro
/// che fiancheggiano il vano, in metri.
pub const REACH_M: f64 = 0.05;
/// Di quanti gradi al massimo la direzione dei clic si allinea alla retta di
/// muro più vicina.
pub const SNAP_DEGREES: f64 = 15.0;
/// Intervallo di larghezza atteso, in metri (solo avviso).
pub const WIDTH_RANGE_M: (f64, f64) = (0.60, 3.00);
/// Intervallo di altezza atteso, in metri (solo avviso).
pub const HEIGHT_RANGE_M: (f64, f64) = (0.50, 3.50);

/// La geometria che il comando legge, preparata dal lato app all'avvio.
#[derive(Clone)]
pub struct ApertureSnapshot {
    /// I muri per handle (`walls::wall_set_without_rooms(..).sources`: le
    /// parti dei locali non sono muri, R67).
    pub walls: FxHashMap<Handle, BoundarySource>,
    pub unit: DrawingUnit,
    /// Tolleranza geometrica, in unità del disegno (positiva).
    pub tol: f64,
}

enum Step {
    /// Primo punto alla spalla del vano.
    Start,
    /// Secondo punto alla spalla opposta.
    AxisEnd { p1: [f64; 2] },
    /// Spessore digitato, quando le facce non si trovano.
    Thickness { p1: [f64; 2], p2: [f64; 2] },
    State,
    Height,
    Sill,
    Confirm { notices: Vec<String> },
    Done,
}

pub struct ApertureCommand {
    kind: ApertureKind,
    snap: ApertureSnapshot,
    step: Step,
    /// L'apertura che si va componendo; l'asse e lo spessore si scrivono
    /// quando il vano è trovato.
    draft: Aperture,
}

impl ApertureCommand {
    pub fn new(kind: ApertureKind, snap: ApertureSnapshot) -> Self {
        Self {
            kind,
            snap,
            step: Step::Start,
            draft: Aperture {
                kind,
                state: ApertureState::Esistente,
                axis: ([0.0; 2], [0.0; 2]),
                thickness: 0.0,
                height: None,
                sill: None,
                threshold: kind.is_door(),
                lintel: false,
                bearing: None,
            },
        }
    }

    fn per_metre(&self) -> f64 {
        self.snap.unit.per_metre()
    }

    /// Dopo i due punti: lo spessore dalle facce, o lo si chiede.
    fn axis_end(&mut self, p1: [f64; 2], p2: [f64; 2]) -> CmdResult {
        let tol = self.snap.tol;
        let length = distance(p1, p2);
        if length <= tol {
            // Punti coincidenti: si richiede il secondo.
            return CmdResult::NeedPoint;
        }
        let clicked = [(p2[0] - p1[0]) / length, (p2[1] - p1[1]) / length];
        let mid = [(p1[0] + p2[0]) / 2.0, (p1[1] + p2[1]) / 2.0];
        let u = self.wall_direction(mid, clicked);
        // La lunghezza del vano lungo il muro, non lungo il clic inclinato.
        let along = ((p2[0] - p1[0]) * u[0] + (p2[1] - p1[1]) * u[1]).abs();
        let reach = self.snap.unit.metres(REACH_M);
        let half = along / 2.0 + reach;
        let from = [mid[0] - u[0] * half, mid[1] - u[1] * half];
        let to = [mid[0] + u[0] * half, mid[1] + u[1] * half];
        let max = self.snap.unit.metres(MAX_WALL_THICKNESS_M);
        match rect_from_axis(&self.snap.walls, from, to, max, tol) {
            Ok(rect) => {
                // L'asse del vano giace sulla mezzeria tra le facce.
                let (a, b) = rect.axis;
                self.draft.axis = (project(p1, a, b), project(p2, a, b));
                self.draft.thickness = rect.thickness;
                self.step = Step::State;
                CmdResult::NeedPoint
            }
            Err(AxisMiss::Degenerate) => CmdResult::NeedPoint,
            Err(AxisMiss::NoFaces | AxisMiss::OneFace) => {
                self.step = Step::Thickness { p1, p2 };
                CmdResult::NeedPoint
            }
        }
    }

    /// La direzione del muro attorno al vano: quella della retta di muro più
    /// vicina al centro del vano, se si discosta di non più di
    /// [`SNAP_DEGREES`] da quella dei due clic (due spalle cliccate su facce
    /// diverse danno un asse un poco storto); altrimenti quella dei clic.
    fn wall_direction(&self, mid: [f64; 2], clicked: [f64; 2]) -> [f64; 2] {
        let reach = self.snap.unit.metres(MAX_WALL_THICKNESS_M);
        let mut best: Option<(f64, [f64; 2])> = None;
        for source in self.snap.walls.values() {
            for curve in &source.curves {
                let Curve::Line(line) = curve else { continue };
                let (dx, dy) = (line.end[0] - line.start[0], line.end[1] - line.start[1]);
                let len = dx.hypot(dy);
                if len <= self.snap.tol {
                    continue;
                }
                let dir = [dx / len, dy / len];
                let cos = (dir[0] * clicked[0] + dir[1] * clicked[1]).abs();
                if cos < SNAP_DEGREES.to_radians().cos() {
                    continue;
                }
                let near = distance_to(curve, mid);
                if near <= reach && best.map_or(true, |(d, _)| near < d) {
                    // Il verso di quello dei clic.
                    let sign = if dir[0] * clicked[0] + dir[1] * clicked[1] < 0.0 {
                        -1.0
                    } else {
                        1.0
                    };
                    best = Some((near, [dir[0] * sign, dir[1] * sign]));
                }
            }
        }
        best.map_or(clicked, |(_, dir)| dir)
    }

    fn with_thickness(&mut self, thickness: f64) -> CmdResult {
        let Step::Thickness { p1, p2 } = self.step else {
            return CmdResult::NeedPoint;
        };
        match rect_from_axis_thickness(p1, p2, thickness) {
            Some(rect) => {
                self.draft.axis = (p1, p2);
                self.draft.thickness = rect.thickness;
                self.step = Step::State;
                CmdResult::NeedPoint
            }
            None => CmdResult::ReportError("Spessore non valido.".into()),
        }
    }

    fn answer_state(&mut self, state: ApertureState) -> CmdResult {
        self.draft.state = state;
        self.step = Step::Height;
        CmdResult::NeedPoint
    }

    /// `None` = Invio = `n.d.`.
    fn answer_height(&mut self, height: Option<f64>) -> CmdResult {
        self.draft.height = height;
        if self.kind == ApertureKind::Finestra {
            self.step = Step::Sill;
            CmdResult::NeedPoint
        } else {
            self.check()
        }
    }

    fn answer_sill(&mut self, sill: Option<f64>) -> CmdResult {
        self.draft.sill = sill;
        self.check()
    }

    /// Gli avvisi di intervallo, in metri; senza, l'apertura va nel disegno.
    fn notices(&self) -> Vec<String> {
        let per_metre = self.per_metre();
        let mut notices = Vec::new();
        let width = self.draft.width() / per_metre;
        if width < WIDTH_RANGE_M.0 || width > WIDTH_RANGE_M.1 {
            notices.push(format!(
                "Larghezza {} m fuori da {}–{} m.",
                format_number(round2(width)),
                format_number(WIDTH_RANGE_M.0),
                format_number(WIDTH_RANGE_M.1)
            ));
        }
        if let Some(height) = self.draft.height.map(|h| h / per_metre) {
            if height < HEIGHT_RANGE_M.0 || height > HEIGHT_RANGE_M.1 {
                notices.push(format!(
                    "Altezza {} m fuori da {}–{} m.",
                    format_number(round2(height)),
                    format_number(HEIGHT_RANGE_M.0),
                    format_number(HEIGHT_RANGE_M.1)
                ));
            }
        }
        notices
    }

    fn check(&mut self) -> CmdResult {
        let notices = self.notices();
        if notices.is_empty() {
            self.finish()
        } else {
            self.step = Step::Confirm { notices };
            CmdResult::NeedPoint
        }
    }

    fn finish(&mut self) -> CmdResult {
        self.step = Step::Done;
        match aperture_polyline(&self.draft) {
            Some(entity) => CmdResult::CommitEntitiesAndExit(vec![entity]),
            None => CmdResult::ReportError("Apertura non valida.".into()),
        }
    }

    fn answer_confirm(&mut self, yes: bool) -> CmdResult {
        if yes {
            self.finish()
        } else {
            self.step = Step::Done;
            CmdResult::Cancel
        }
    }
}

/// Il punto `p` proiettato sulla retta per `a` e `b`.
fn project(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    if len2 <= 0.0 {
        return p;
    }
    let t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2;
    [a[0] + d[0] * t, a[1] + d[1] * t]
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

impl CadCommand for ApertureCommand {
    /// L'apertura ha già layer `AL-APERTURE` e colore ACI dello stato.
    fn preserve_commit_layer(&self) -> bool {
        true
    }

    fn preserve_commit_style(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        match self.kind {
            ApertureKind::Porta => "AL_PORTA",
            ApertureKind::Finestra => "AL_FINESTRA",
            ApertureKind::Portafinestra => "AL_PORTAFINESTRA",
        }
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Start => format!(
                "{}  Primo punto alla spalla del vano, sull'asse del muro:",
                self.name()
            ),
            Step::AxisEnd { .. } => "Punto alla spalla opposta:".into(),
            Step::Thickness { .. } => "Facce del muro non trovate. Spessore del muro:".into(),
            Step::State => "Stato [Esistente/Demolita/Nuova] <Esistente>:".into(),
            Step::Height => "Altezza del vano (Invio = n.d.):".into(),
            Step::Sill => "Altezza del davanzale da terra (Invio = n.d.):".into(),
            Step::Confirm { notices } => {
                format!("{} Confermi l'apertura? [Sì/No] <Sì>:", notices.join(" "))
            }
            Step::Done => String::new(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match &self.step {
            Step::State => vec![
                CmdOption::new("Esistente", "E"),
                CmdOption::new("Demolita", "D"),
                CmdOption::new("Nuova", "N"),
            ],
            Step::Confirm { .. } => vec![CmdOption::new("Sì", "S"), CmdOption::new("No", "N")],
            _ => Vec::new(),
        }
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
            // uno spessore né un'altezza.
            Step::Thickness { .. } | Step::Height | Step::Sill => CmdResult::ReportError(
                "Digita il valore, per esempio 0,12 o 0.12.".into(),
            ),
            Step::State | Step::Confirm { .. } | Step::Done => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Thickness { .. } => CmdResult::NeedPoint,
            Step::State => self.answer_state(ApertureState::Esistente),
            Step::Height => self.answer_height(None),
            Step::Sill => self.answer_sill(None),
            Step::Confirm { .. } => self.answer_confirm(true),
            _ => CmdResult::Cancel,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let word = text.trim().to_uppercase();
        match self.step {
            Step::Thickness { .. } => {
                let thickness = parse_number(text)?;
                Some(if thickness > 0.0 {
                    self.with_thickness(thickness)
                } else {
                    CmdResult::ReportError("Lo spessore deve essere maggiore di zero.".into())
                })
            }
            Step::State => {
                let state = match word.as_str() {
                    "E" | "ESISTENTE" => ApertureState::Esistente,
                    "D" | "DEMOLITA" => ApertureState::Demolita,
                    "N" | "NUOVA" => ApertureState::Nuova,
                    _ => return None,
                };
                Some(self.answer_state(state))
            }
            Step::Height => {
                let height = parse_number(text)?;
                Some(if height > 0.0 {
                    self.answer_height(Some(height))
                } else {
                    CmdResult::ReportError("L'altezza deve essere maggiore di zero.".into())
                })
            }
            Step::Sill => {
                let sill = parse_number(text)?;
                Some(if sill >= 0.0 {
                    self.answer_sill(Some(sill))
                } else {
                    CmdResult::ReportError("Il davanzale non può essere negativo.".into())
                })
            }
            Step::Confirm { .. } => yes_no(&word).map(|yes| self.answer_confirm(yes)),
            _ => None,
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Thickness { .. }
            | Step::State
            | Step::Height
            | Step::Sill
            | Step::Confirm { .. } => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bim::fixtures::{case_a, case_a_with_door, scene_with, Form};
    use crate::bim::walls::wall_set_without_rooms;

    const M: DrawingUnit = DrawingUnit::Metri;
    const TOL: f64 = 1e-6;
    const FORMS: [Form; 2] = [Form::Continua, Form::ChiusaPerMuro];

    fn snapshot(entities: Vec<EntityType>) -> ApertureSnapshot {
        let scene = scene_with(entities, M.insunits());
        ApertureSnapshot {
            walls: wall_set_without_rooms(&scene.document, TOL).sources,
            unit: M,
            tol: TOL,
        }
    }

    fn point(cmd: &mut ApertureCommand, x: f64, y: f64) -> CmdResult {
        cmd.on_point(DVec3::new(x, y, 0.0))
    }

    fn text(cmd: &mut ApertureCommand, value: &str) -> CmdResult {
        cmd.on_text_input(value)
            .unwrap_or_else(|| panic!("«{value}» non accettato: {}", cmd.prompt()))
    }

    fn committed(result: CmdResult) -> Aperture {
        let CmdResult::CommitEntitiesAndExit(mut entities) = result else {
            panic!("non consegnata: {:?}", std::mem::discriminant(&result))
        };
        assert_eq!(entities.len(), 1);
        let entity = entities.remove(0);
        super::super::aperture::read_aperture(Handle::NULL, &entity)
            .expect("apertura leggibile")
            .aperture
    }

    /// La porta del caso A+P: due punti alle spalle, spessore dalle facce ai
    /// due lati del vano, stato di default, altezza 2,10.
    #[test]
    fn door_width_comes_from_the_drawing_and_thickness_from_the_flanking_faces() {
        for form in FORMS {
            let mut cmd = ApertureCommand::new(
                ApertureKind::Porta,
                snapshot(case_a_with_door(form, M)),
            );
            assert!(matches!(point(&mut cmd, 4.05, 1.0), CmdResult::NeedPoint));
            assert!(matches!(point(&mut cmd, 4.05, 1.9), CmdResult::NeedPoint));
            assert!(cmd.prompt().starts_with("Stato"), "{form:?}: {}", cmd.prompt());
            assert!(matches!(cmd.on_enter(), CmdResult::NeedPoint));
            assert!(cmd.prompt().starts_with("Altezza"), "{form:?}");
            let result = text(&mut cmd, "2.1");
            let a = committed(result);
            assert_eq!(a.kind, ApertureKind::Porta, "{form:?}");
            assert_eq!(a.state, ApertureState::Esistente, "{form:?}");
            assert!((a.width() - 0.9).abs() < 1e-9, "{form:?}: {}", a.width());
            assert!((a.thickness - 0.10).abs() < 1e-9, "{form:?}: {}", a.thickness);
            assert!((a.axis.0[0] - 4.05).abs() < 1e-9, "{form:?}: {:?}", a.axis);
            assert_eq!(a.height, Some(2.1), "{form:?}");
            assert_eq!(a.sill, None, "{form:?}");
            assert!(a.threshold, "una porta chiude il vano");
            assert!(!a.lintel);
        }
    }

    /// Un clic fuori dalla mezzeria si proietta sulla mezzeria del muro.
    #[test]
    fn axis_points_are_projected_on_the_wall_midline() {
        let mut cmd = ApertureCommand::new(
            ApertureKind::Porta,
            snapshot(case_a_with_door(Form::Continua, M)),
        );
        point(&mut cmd, 4.02, 1.0);
        point(&mut cmd, 4.08, 1.9);
        cmd.on_enter();
        let a = committed(text(&mut cmd, "2.1"));
        assert!((a.axis.0[0] - 4.05).abs() < 1e-9, "{:?}", a.axis);
        assert!((a.axis.1[0] - 4.05).abs() < 1e-9, "{:?}", a.axis);
        assert!((a.width() - 0.9).abs() < 1e-9, "{}", a.width());
    }

    /// Senza facce (nessun muro) lo spessore si chiede, mai indovinato.
    #[test]
    fn thickness_is_asked_when_the_faces_are_missing() {
        let mut cmd = ApertureCommand::new(ApertureKind::Porta, snapshot(Vec::new()));
        point(&mut cmd, 0.0, 0.0);
        point(&mut cmd, 0.9, 0.0);
        assert!(cmd.prompt().contains("Spessore"), "{}", cmd.prompt());
        assert!(matches!(
            point(&mut cmd, 1.0, 1.0),
            CmdResult::ReportError(_)
        ));
        assert!(matches!(cmd.on_enter(), CmdResult::NeedPoint), "Invio non basta");
        assert!(matches!(text(&mut cmd, "0"), CmdResult::ReportError(_)));
        text(&mut cmd, "0,12");
        cmd.on_enter();
        let a = committed(text(&mut cmd, "2,1"));
        assert!((a.thickness - 0.12).abs() < 1e-9);
        assert!((a.width() - 0.9).abs() < 1e-9);
    }

    /// Invio all'altezza lascia `n.d.`: nessun valore di ripiego.
    #[test]
    fn enter_at_the_height_leaves_it_missing() {
        let mut cmd = ApertureCommand::new(
            ApertureKind::Porta,
            snapshot(case_a_with_door(Form::Continua, M)),
        );
        point(&mut cmd, 4.05, 1.0);
        point(&mut cmd, 4.05, 1.9);
        cmd.on_enter();
        let a = committed(cmd.on_enter());
        assert_eq!(a.height, None);
    }

    /// Una finestra chiede il davanzale, non chiude il vano; stato e valori
    /// da testo.
    #[test]
    fn window_asks_for_the_sill_and_does_not_close_the_room() {
        let mut cmd = ApertureCommand::new(
            ApertureKind::Finestra,
            snapshot(case_a_with_door(Form::Continua, M)),
        );
        point(&mut cmd, 4.05, 1.0);
        point(&mut cmd, 4.05, 1.9);
        text(&mut cmd, "n");
        text(&mut cmd, "1,4");
        assert!(cmd.prompt().contains("davanzale"), "{}", cmd.prompt());
        assert!(matches!(text(&mut cmd, "-1"), CmdResult::ReportError(_)));
        let a = committed(text(&mut cmd, "0.9"));
        assert_eq!(a.kind, ApertureKind::Finestra);
        assert_eq!(a.state, ApertureState::Nuova);
        assert_eq!(a.height, Some(1.4));
        assert_eq!(a.sill, Some(0.9));
        assert!(!a.threshold);
    }

    /// Fuori dagli intervalli attesi chiede conferma; «No» annulla.
    #[test]
    fn out_of_range_values_ask_for_confirmation() {
        let make = || {
            let mut cmd = ApertureCommand::new(
                ApertureKind::Porta,
                snapshot(case_a_with_door(Form::Continua, M)),
            );
            point(&mut cmd, 4.05, 1.0);
            point(&mut cmd, 4.05, 1.9);
            cmd.on_enter();
            cmd
        };
        let mut cmd = make();
        assert!(matches!(text(&mut cmd, "15"), CmdResult::NeedPoint));
        assert!(cmd.prompt().contains("Altezza 15 m fuori"), "{}", cmd.prompt());
        assert!(matches!(text(&mut cmd, "no"), CmdResult::Cancel));

        let mut cmd = make();
        text(&mut cmd, "15");
        let a = committed(cmd.on_enter());
        assert_eq!(a.height, Some(15.0), "Invio conferma");
    }

    /// Una porta di 4 m è fuori dall'intervallo di larghezza: lo dice.
    #[test]
    fn a_very_wide_opening_warns() {
        let mut cmd = ApertureCommand::new(ApertureKind::Porta, snapshot(Vec::new()));
        point(&mut cmd, 0.0, 0.0);
        point(&mut cmd, 4.0, 0.0);
        text(&mut cmd, "0.3");
        cmd.on_enter();
        text(&mut cmd, "2.1");
        assert!(cmd.prompt().contains("Larghezza 4 m fuori"), "{}", cmd.prompt());
    }

    #[test]
    fn coincident_points_ask_for_the_second_again() {
        let mut cmd = ApertureCommand::new(
            ApertureKind::Porta,
            snapshot(case_a(Form::Continua, M)),
        );
        point(&mut cmd, 2.0, 1.0);
        assert!(matches!(point(&mut cmd, 2.0, 1.0), CmdResult::NeedPoint));
        assert!(cmd.prompt().contains("spalla opposta"), "{}", cmd.prompt());
    }

    #[test]
    fn names_and_defaults() {
        for (kind, name, door) in [
            (ApertureKind::Porta, "AL_PORTA", true),
            (ApertureKind::Finestra, "AL_FINESTRA", false),
            (ApertureKind::Portafinestra, "AL_PORTAFINESTRA", true),
        ] {
            let cmd = ApertureCommand::new(kind, snapshot(Vec::new()));
            assert_eq!(cmd.name(), name);
            assert_eq!(cmd.draft.threshold, door);
            assert!(cmd.preserve_commit_layer() && cmd.preserve_commit_style());
        }
    }
}
