//! Il comando BIM `AL_ABACO` della parte 2 (spec
//! `2026-10-28-bim-abachi-quantita-design.md`, §5.2), lato app.
//!
//! `AL_ABACO [x,y]` calcola l'abaco (`abaco::compute_abaco`, con le
//! tolleranze di `AL_AGGIORNA` e `AL_ESTRAI`) e lo scrive come tabelle `TABLE`
//! sul layer `AL-ABACO` (`abaco_table`). Regole:
//! - spazio modello, poi l'eventuale punto (`x,y` col punto decimale, in
//!   coordinate del mondo), poi zone e locali: senza né le une né gli altri
//!   (e senza tabelle d'abaco) un messaggio e basta, senza chiedere l'unità;
//!   poi l'unità del disegno (`AL_UNITA`, che rilancia la riga intera);
//! - le tabelle già presenti si riscrivono dove si trovano, copie comprese,
//!   solo se cambiano; quelle su layer bloccati non si toccano e si contano;
//!   quelle di un tipo che non ha più righe si tolgono (un numero vecchio non
//!   resta nel disegno). Quelle mancanti vanno sotto l'ultima tabella che
//!   resta; se non ce n'è nessuna, nel punto dato, oppure al clic
//!   ([`AbacoPointCommand`], che rilancia `AL_ABACO <x>,<y>`), oppure, da uno
//!   script senza punto, a destra dell'estensione dello spazio modello con un
//!   margine pari alla larghezza della tabella più larga;
//! - il layer `AL-ABACO` e l'APPID, se mancano, in un passo di annullamento a
//!   sé (`AL_LAYER`, come `bim_ensure_layers`); sostituzioni, creazioni e
//!   rimozioni in un solo passo `AL_ABACO`, e nessun passo se nulla cambia;
//! - in riga di comando: tabelle create e sostituite, righe e righe
//!   incomplete, poi gli avvisi dell'abaco (prima un `RigaIncompleta` per riga
//!   incompleta, con il motivo). Rilievo, zone, locali e record non cambiano.
//!
//! La riga `AL_ABACO <x,y>` va al comando intera (elenco del runner in
//! `command_driver/utilities.rs`): da solo, in uno script, il comando scrive
//! subito, e il runner non deve provarlo senza il punto.

use crate::app::{Message, OpenCADStudio};
use crate::bim::abaco::{compute_abaco, render_tables, AbacoOptions, TableText};
use crate::bim::abaco_table::{self, kind_name, AbacoPlan};
use crate::bim::report::Notice;
use crate::bim::rooms::rooms_in;
use crate::bim::units::DrawingUnit;
use crate::bim::zone::zones_in;
use crate::bim::APP_ABACO;
use crate::command::{CadCommand, CmdResult};
use crate::scene::view::dispatch::set_entity_xdata;
use codec::{EntityType, Handle};
use glam::DVec3;
use iced::Task;

use super::bim::room_tolerance;

const CMD: &str = "AL_ABACO";

const POINT_PROMPT: &str = "AL_ABACO  Punto di inserimento delle tabelle:";

/// Attende il punto delle tabelle e rilancia `AL_ABACO <x>,<y>`, coordinate
/// del mondo col punto decimale; Invio annulla.
struct AbacoPointCommand;

impl CadCommand for AbacoPointCommand {
    fn name(&self) -> &'static str {
        CMD
    }
    fn prompt(&self) -> String {
        POINT_PROMPT.to_string()
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        CmdResult::Relaunch(format!("{CMD} {},{}", pt.x, pt.y), vec![])
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

impl OpenCADStudio {
    /// `AL_ABACO [x,y]` (spec parte 2, §5.2): vedi le regole del modulo.
    pub(super) fn bim_abaco(&mut self, i: usize, cmd: &str) -> Task<Message> {
        if !self.bim_in_model_space(i, CMD) {
            return Task::none();
        }
        let point = match point_argument(cmd) {
            Ok(point) => point,
            Err(message) => {
                self.command_line.push_error(&message);
                return Task::none();
            }
        };
        let doc = &self.tabs[i].scene.document;
        if zones_in(doc).is_empty()
            && rooms_in(doc).is_empty()
            && abaco_table::tables_in(doc).is_empty()
        {
            // Un rilancio lasciato a metà (AL_UNITA annullato) non vale più.
            self.bim_pending = None;
            self.command_line
                .push_info("AL_ABACO: nessuna zona né locale nel disegno: nessuna tabella.");
            return Task::none();
        }
        let Some(unit) = self.bim_unit_or_ask(i, cmd.trim()) else {
            return self.finish_dispatch(CMD);
        };
        self.bim_pending = None;
        let opts = AbacoOptions {
            unit,
            tol: unit.tolerance(),
            // R61: i locali alla tolleranza di `AL_LOCALE` e `AL_AGGIORNA`.
            room_tol: room_tolerance(unit),
        };
        let scene = &self.tabs[i].scene;
        let abaco = compute_abaco(&scene.document, &opts);
        let texts = render_tables(&abaco);
        let plan = abaco_table::plan(&scene.document, &texts, unit, |handle| {
            scene.is_layer_locked(handle)
        });
        let missing: Vec<&TableText> = plan
            .missing
            .iter()
            .filter_map(|kind| texts.iter().find(|text| text.kind == *kind))
            .collect();
        let at = match (missing.is_empty(), plan.below, point) {
            (true, _, _) => None,
            (false, Some(below), _) => Some(below),
            (false, None, Some(point)) => Some(point),
            (false, None, None) if self.scripted_dispatch => {
                let corner = scene
                    .model_space_extents()
                    .map(|(_, max)| [f64::from(max.x), f64::from(max.y)]);
                Some(right_of_extents(corner, &missing, unit))
            }
            (false, None, None) => {
                // Da una persona: il punto si clicca e torna qui come riga.
                let ask = AbacoPointCommand;
                self.command_line.push_info(&ask.prompt());
                self.tabs[i].active_cmd = Some(Box::new(ask));
                return self.finish_dispatch(CMD);
            }
        };
        let created = at.map_or_else(Vec::new, |at| abaco_table::stack(&missing, at, unit));
        self.bim_write_abaco(i, &plan, created);

        if point.is_some() && (missing.is_empty() || plan.below.is_some()) {
            self.command_line.push_info(if missing.is_empty() {
                "AL_ABACO: il punto indicato non serve: nessuna tabella da creare."
            } else {
                "AL_ABACO: il punto indicato non serve: le tabelle mancanti vanno sotto l'ultima esistente."
            });
        }
        let changes = missing.len() + plan.replace.len() + plan.remove.len();
        self.command_line
            .push_info(&if changes == 0 && plan.locked == 0 {
                "AL_ABACO: le tabelle sono già aggiornate.".to_string()
            } else {
                format!(
                    "AL_ABACO: tabelle create: {}; sostituite: {}.",
                    missing.len(),
                    plan.replace.len()
                )
            });
        for (_, kind) in &plan.remove {
            self.command_line.push_info(&format!(
                "AL_ABACO: tolta la tabella {}: non ha più righe.",
                kind_name(*kind)
            ));
        }
        if plan.locked > 0 {
            self.command_line.push_info(&format!(
                "Tabelle d'abaco su layer bloccati non aggiornate: {}.",
                plan.locked
            ));
        }
        let rows =
            abaco.demolizioni.len() + abaco.varchi.len() + abaco.nuove.len() + abaco.locali.len();
        let incomplete = abaco
            .notices
            .iter()
            .filter(|notice| matches!(notice, Notice::RigaIncompleta { .. }))
            .count();
        self.command_line.push_info(&format!(
            "AL_ABACO: righe: {rows}; incomplete: {incomplete}."
        ));
        self.bim_push_notices(&abaco.notices);
        Task::none()
    }

    /// Scrive il piano: il layer `AL-ABACO` e l'APPID in un passo a sé se si
    /// crea una tabella e mancano, poi sostituzioni, creazioni e rimozioni in
    /// un solo passo `AL_ABACO`, nessuno se nulla cambia.
    fn bim_write_abaco(&mut self, i: usize, plan: &AbacoPlan, created: Vec<EntityType>) {
        if !created.is_empty() && abaco_table::missing_layer(&self.tabs[i].scene.document) {
            self.push_undo_snapshot(i, "AL_LAYER");
            abaco_table::ensure_layer(&mut self.tabs[i].scene.document);
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.refresh_layer_panel();
        }
        if created.is_empty() && plan.replace.is_empty() && plan.remove.is_empty() {
            return;
        }
        self.push_undo_snapshot(i, CMD);
        let scene = &mut self.tabs[i].scene;
        for (handle, kind, table) in &plan.replace {
            scene.update_entity(EntityType::Table(Box::new(table.clone())));
            // Una tabella letta da un DWG tiene l'XDATA anche nel blocco
            // grezzo, che al salvataggio prevale sul record: lo scarta.
            set_entity_xdata(
                &mut scene.document,
                *handle,
                APP_ABACO,
                Some(abaco_table::encode(*kind)),
            );
        }
        for entity in created {
            scene.add_entity(entity);
        }
        let removed: Vec<Handle> = plan.remove.iter().map(|(handle, _)| *handle).collect();
        if !removed.is_empty() {
            scene.erase_entities(&removed);
        }
        self.tabs[i].dirty = true;
        self.finish_pending_history(i);
        self.refresh_properties();
    }
}

/// Il punto delle tabelle di uno script senza punto: a destra dell'angolo in
/// alto a destra dell'estensione dello spazio modello (`corner`), staccate
/// quanto la più larga delle tabelle da creare. Un modello senza estensione
/// le mette nell'origine.
fn right_of_extents(
    corner: Option<[f64; 2]>,
    missing: &[&TableText],
    unit: DrawingUnit,
) -> [f64; 2] {
    let Some([x, y]) = corner else {
        return [0.0, 0.0];
    };
    let width = abaco_table::stack(missing, [0.0, 0.0], unit)
        .iter()
        .filter_map(|entity| match entity {
            EntityType::Table(table) => Some(table.total_width()),
            _ => None,
        })
        .fold(0.0, f64::max);
    [x + width, y]
}

/// Il punto di `AL_ABACO <x,y>` (anche `x,y,z`: la quota non conta), col
/// punto decimale; `None` senza argomento. Ciò che non si capisce è un
/// errore, mai un punto inventato.
fn point_argument(cmd: &str) -> Result<Option<[f64; 2]>, String> {
    let mut words = cmd.split_whitespace();
    let _name = words.next();
    let Some(text) = words.next() else {
        return Ok(None);
    };
    if let Some(extra) = words.next() {
        return Err(format!(
            "AL_ABACO: argomento in più \"{extra}\" dopo il punto."
        ));
    }
    let numbers: Option<Vec<f64>> = text
        .split(',')
        .map(|part| part.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect();
    match numbers.as_deref() {
        Some(&[x, y]) | Some(&[x, y, _]) => Ok(Some([x, y])),
        _ => Err(format!(
            "AL_ABACO: punto non valido \"{text}\": serve x,y col punto decimale, per esempio 20.5,0."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::point_argument;

    #[test]
    fn point_argument_takes_x_y_with_the_decimal_point() {
        assert_eq!(point_argument("AL_ABACO"), Ok(None));
        assert_eq!(point_argument("AL_ABACO  "), Ok(None));
        assert_eq!(point_argument("AL_ABACO 20,0"), Ok(Some([20.0, 0.0])));
        assert_eq!(
            point_argument("al_abaco 20.5,-3.25"),
            Ok(Some([20.5, -3.25]))
        );
        assert_eq!(point_argument("AL_ABACO 1,2,3"), Ok(Some([1.0, 2.0])));
        for bad in [
            "AL_ABACO abc",
            "AL_ABACO 20",
            "AL_ABACO 20,",
            "AL_ABACO 1,2,3,4",
            "AL_ABACO NaN,0",
            "AL_ABACO inf,0",
            "AL_ABACO 20,0 30,0",
        ] {
            let error = point_argument(bad).unwrap_err();
            assert!(error.starts_with("AL_ABACO:"), "{bad}: {error}");
        }
    }
}
