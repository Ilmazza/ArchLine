//! Comando `AL_TAGLIO` (spec parte 4 §5.1): due punti sul tracciato del taglio,
//! un punto dal lato in cui si guarda, il nome (Invio = quello proposto, la
//! prima lettera non ancora usata: «A-A»). Consegna una sola `LINE` con
//! `CommitEntitiesAndExit`. Un nome già usato si rifiuta: i disegni delle
//! sezioni si riconoscono dal nome (`AL_SEZIONE` li sostituisce a ogni lancio).
//!
//! Lavora su un'istantanea dei nomi già presi; niente documento né undo. Tutto
//! sul piano XY del mondo, in unità del disegno.

use glam::DVec3;

use crate::command::{CadCommand, CmdResult, InputKind};

use super::cut::{cut_line, suggested_name, Cut};

enum Step {
    Start,
    End { from: [f64; 2] },
    Side { from: [f64; 2], to: [f64; 2] },
    Name { from: [f64; 2], to: [f64; 2], view: [f64; 2] },
    Done,
}

pub struct CutCommand {
    taken: Vec<String>,
    step: Step,
}

impl CutCommand {
    /// `taken` sono i nomi dei tagli già nel disegno.
    pub fn new(taken: Vec<String>) -> Self {
        Self {
            taken,
            step: Step::Start,
        }
    }

    fn proposal(&self) -> Option<String> {
        suggested_name(&self.taken)
    }

    fn finish(&mut self, name: &str) -> CmdResult {
        let Step::Name { from, to, view } = self.step else {
            return CmdResult::NeedPoint;
        };
        let name = name.trim();
        if name.is_empty() {
            return CmdResult::ReportError("Il nome del taglio non può essere vuoto.".into());
        }
        if self.taken.iter().any(|used| used.eq_ignore_ascii_case(name)) {
            return CmdResult::ReportError(format!("Esiste già un taglio «{name}»: scegli un altro nome."));
        }
        let cut = Cut {
            name: name.to_string(),
            from,
            to,
            view,
        };
        self.step = Step::Done;
        match cut_line(&cut) {
            Some(line) => CmdResult::CommitEntitiesAndExit(vec![line]),
            None => CmdResult::ReportError("Taglio non valido.".into()),
        }
    }
}

impl CadCommand for CutCommand {
    /// Il taglio ha già il suo layer `AL-TAGLI`.
    fn preserve_commit_layer(&self) -> bool {
        true
    }

    fn preserve_commit_style(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "AL_TAGLIO"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Start => "AL_TAGLIO  Primo punto del taglio:".into(),
            Step::End { .. } => "Secondo punto del taglio:".into(),
            Step::Side { .. } => "Punto dal lato in cui si guarda:".into(),
            Step::Name { .. } => match self.proposal() {
                Some(name) => format!("Nome del taglio <{name}>:"),
                None => "Nome del taglio:".into(),
            },
            Step::Done => String::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let p = [pt.x, pt.y];
        match self.step {
            Step::Start => {
                self.step = Step::End { from: p };
                CmdResult::NeedPoint
            }
            Step::End { from } => {
                // Punti coincidenti: si richiede il secondo.
                if (p[0] - from[0]).hypot(p[1] - from[1]) > 0.0 {
                    self.step = Step::Side { from, to: p };
                }
                CmdResult::NeedPoint
            }
            Step::Side { from, to } => {
                let d = [to[0] - from[0], to[1] - from[1]];
                let v = [p[0] - from[0], p[1] - from[1]];
                // Un punto sul tracciato non dice da che parte si guarda.
                if (d[0] * v[1] - d[1] * v[0]).abs() > 0.0 {
                    self.step = Step::Name { from, to, view: p };
                }
                CmdResult::NeedPoint
            }
            Step::Name { .. } => {
                CmdResult::ReportError("Digita il nome del taglio, per esempio A-A.".into())
            }
            Step::Done => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Name { .. } => match self.proposal() {
                Some(name) => self.finish(&name),
                None => CmdResult::NeedPoint,
            },
            _ => CmdResult::Cancel,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match self.step {
            Step::Name { .. } => Some(self.finish(text)),
            _ => None,
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Name { .. } => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bim::cut::read_cut;

    fn point(cmd: &mut CutCommand, x: f64, y: f64) -> CmdResult {
        cmd.on_point(DVec3::new(x, y, 0.0))
    }

    fn committed(result: CmdResult) -> Cut {
        let CmdResult::CommitEntitiesAndExit(entities) = result else {
            panic!("non consegnato")
        };
        read_cut(codec::Handle::NULL, &entities[0]).expect("taglio").cut
    }

    /// Due punti, il lato di vista e Invio al nome proposto.
    #[test]
    fn a_cut_from_three_points_takes_the_proposed_name() {
        let mut cmd = CutCommand::new(Vec::new());
        assert!(matches!(point(&mut cmd, 2.0, -1.0), CmdResult::NeedPoint));
        assert!(matches!(point(&mut cmd, 2.0, 4.0), CmdResult::NeedPoint));
        assert!(cmd.prompt().contains("lato"), "{}", cmd.prompt());
        assert!(matches!(point(&mut cmd, 3.0, 1.5), CmdResult::NeedPoint));
        assert_eq!(cmd.prompt(), "Nome del taglio <A-A>:");
        let cut = committed(cmd.on_enter());
        assert_eq!(cut.name, "A-A");
        assert_eq!((cut.from, cut.to, cut.view), ([2.0, -1.0], [2.0, 4.0], [3.0, 1.5]));
    }

    /// Il nome si può digitare; uno già preso, o vuoto, si rifiuta; il
    /// suggerimento salta quelli usati.
    #[test]
    fn names_are_typed_checked_and_proposed_in_order() {
        let mut cmd = CutCommand::new(vec!["A-A".into(), "b-b".into()]);
        point(&mut cmd, 0.0, 0.0);
        point(&mut cmd, 1.0, 0.0);
        point(&mut cmd, 0.5, 1.0);
        assert_eq!(cmd.prompt(), "Nome del taglio <C-C>:");
        let result = cmd.on_text_input("a-a").unwrap();
        assert!(matches!(result, CmdResult::ReportError(m) if m.contains("Esiste già")));
        let result = cmd.on_text_input("   ").unwrap();
        assert!(matches!(result, CmdResult::ReportError(_)));
        let cut = committed(cmd.on_text_input("Corpo scala").unwrap());
        assert_eq!(cut.name, "Corpo scala");
    }

    /// Punti coincidenti o un lato di vista sul tracciato si richiedono.
    #[test]
    fn degenerate_points_ask_again() {
        let mut cmd = CutCommand::new(Vec::new());
        point(&mut cmd, 1.0, 1.0);
        point(&mut cmd, 1.0, 1.0);
        assert!(cmd.prompt().contains("Secondo"), "{}", cmd.prompt());
        point(&mut cmd, 5.0, 1.0);
        point(&mut cmd, 3.0, 1.0);
        assert!(cmd.prompt().contains("lato"), "{}", cmd.prompt());
        point(&mut cmd, 3.0, 2.0);
        assert!(cmd.prompt().starts_with("Nome"));
        assert!(matches!(point(&mut cmd, 0.0, 0.0), CmdResult::ReportError(_)), "un clic non è un nome");
    }

    #[test]
    fn it_keeps_the_layer_and_has_a_name() {
        let cmd = CutCommand::new(Vec::new());
        assert_eq!(cmd.name(), "AL_TAGLIO");
        assert!(cmd.preserve_commit_layer() && cmd.preserve_commit_style());
    }
}
