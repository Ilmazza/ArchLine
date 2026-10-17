//! Hatch system variables. `HPGAPTOL` is the largest gap in a boundary that
//! HATCH and GRADIENT still treat as closed (BOUNDARY has its own Tolerance
//! option, the same mechanism). 0 means the objects must close exactly. The
//! drawing's header has no slot for it, so it holds for the session. Typeable
//! on its own (`HPGAPTOL 0.5`) and through `SETVAR`; a bare name prompts for
//! the value.

use crate::app::{Message, OpenCADStudio};
use iced::Task;
use std::cell::Cell;

/// The variables this family answers for, for the command registry and the
/// `SETVAR ?` listing.
pub(super) const HATCH_SYSVARS: &[&str] = &["HPGAPTOL"];

/// What a boundary must close to when `HPGAPTOL` is 0.
const EXACT: f64 = 1.0e-6;
/// The reference's range for `HPGAPTOL`, in drawing units.
const MAX_GAP: f64 = 5000.0;

thread_local! {
    static HPGAPTOL: Cell<f64> = const { Cell::new(0.0) };
}

/// The `SETVAR ?` line for this family.
pub(super) fn setvar_listing() -> String {
    format!("SETVAR (hatch): {}", HATCH_SYSVARS.join(" "))
}

/// The gap tolerance HATCH and GRADIENT build their boundaries with.
pub(crate) fn gap_tolerance() -> f64 {
    let gap = HPGAPTOL.with(Cell::get);
    if gap > 0.0 { gap } else { EXACT }
}

impl OpenCADStudio {
    pub(super) fn dispatch_hatchvars(&mut self, cmd: &str, _i: usize) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("SETVAR ").map(str::trim).unwrap_or(cmd);
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").to_ascii_uppercase();
        if !HATCH_SYSVARS.contains(&name.as_str()) {
            return None;
        }
        match parts.next().map(str::trim).filter(|v| !v.is_empty()) {
            Some(v) => {
                match v.replace(',', ".").parse::<f64>().ok().filter(|g| (0.0..=MAX_GAP).contains(g)) {
                    Some(gap) => {
                        HPGAPTOL.with(|c| c.set(gap));
                        self.command_line.push_output(&format!("{name} = {gap}"));
                    }
                    None => self.command_line.push_error(
                        crate::t!("SETVAR: HPGAPTOL requires a number from 0 to 5000.").as_ref(),
                    ),
                }
            }
            None => {
                let current = HPGAPTOL.with(Cell::get);
                self.command_line.push_output(
                    crate::tf!("Enter new value for {name} <{current}>:").as_ref(),
                );
                self.pending_setvar = Some(name);
            }
        }
        Some(Task::none())
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};

    fn run(app: &mut OpenCADStudio, cmd: &str) -> String {
        let start = app.command_line.history.len();
        let _ = app.run_command_line(cmd);
        app.command_line.history[start..]
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn hatches(app: &OpenCADStudio) -> usize {
        app.tabs[0]
            .scene
            .document
            .entities()
            .filter(|e| matches!(e, codec::EntityType::Hatch(_)))
            .count()
    }

    /// A 10 x 10 square whose last side stops 0.5 short of the corner.
    fn square_with_a_gap() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        for line in ["0,0 10,0", "10,0 10,10", "10,10 0,10", "0,10 0,0.5"] {
            app.automation_op(&format!(r#"{{"op":"run","cmd":"LINE {line}"}}"#));
            let _ = app.update(Message::CommandEscape);
        }
        app
    }

    #[test]
    fn a_gap_in_the_boundary_stops_hatch_by_default() {
        let mut app = square_with_a_gap();
        run(&mut app, "-HATCH 5,5");
        assert_eq!(hatches(&app), 0);
    }

    #[test]
    fn hpgaptol_lets_hatch_close_a_gap_up_to_that_size() {
        let mut app = square_with_a_gap();
        run(&mut app, "HPGAPTOL 1");
        run(&mut app, "-HATCH 5,5");
        assert_eq!(hatches(&app), 1);
    }

    #[test]
    fn hpgaptol_smaller_than_the_gap_does_not_close_it() {
        let mut app = square_with_a_gap();
        run(&mut app, "HPGAPTOL 0.1");
        run(&mut app, "-HATCH 5,5");
        assert_eq!(hatches(&app), 0);
    }

    /// The HATCH window works from the same outlines, so it closes the gap too.
    #[test]
    fn the_hatch_window_honours_hpgaptol() {
        let mut app = square_with_a_gap();
        run(&mut app, "HATCH");
        assert_eq!(app.hatch_dialog.as_ref().expect("the window opens").outlines.len(), 0);
        app.hatch_dialog_cancel();
        run(&mut app, "HPGAPTOL 1");
        run(&mut app, "HATCH");
        assert_eq!(app.hatch_dialog.as_ref().expect("the window opens").outlines.len(), 1);
    }

    #[test]
    fn hpgaptol_is_read_and_set_like_a_system_variable() {
        let mut app = square_with_a_gap();
        assert!(run(&mut app, "SETVAR HPGAPTOL 0.25").contains("HPGAPTOL = 0.25"));
        assert!(run(&mut app, "HPGAPTOL 3").contains("HPGAPTOL = 3"));
        let out = run(&mut app, "HPGAPTOL -1");
        assert!(out.contains("0 to 5000"), "got: {out:?}");
        let out = run(&mut app, "HPGAPTOL");
        assert!(out.contains("<3>"), "a rejected value leaves it unchanged; got: {out:?}");
    }
}
