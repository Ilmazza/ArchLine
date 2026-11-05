//! Il comando BIM `AL_STRATIGRAFIA` della parte 6 (spec
//! `2026-11-05-bim-particolari-design.md`, §5.1), lato app.
//!
//! Prende la riga intera (i materiali hanno spazi; per gli script c'è il gancio
//! in `command_driver/utilities.rs`, come per `AL_ABACO_CSV`):
//!
//! - `Nuova <nome> <Parete|Solaio> <materiale> <spessore>; …` crea;
//! - `Elenco` elenca con strati, totale e assegnazioni;
//! - `Rimuovi <nome>` toglie (le assegnazioni restano e lo diranno);
//! - `Assegna <nome> [x,y]` assegna alla selezione o all'elemento sotto il punto;
//! - `Togli [x,y]` toglie l'assegnazione.
//!
//! Gli spessori sono in unità del disegno (eco in cm). Le stratigrafie stanno
//! nell'XRecord `ARCHLINE_STRATIGRAFIE`, scritto in un passo di annullamento a
//! sé; le assegnazioni sono XDATA `ARCHLINE_STRATIGRAFIA` sulle zone nuove e sui
//! semi di progetto, in un passo solo. La logica pura sta in `crate::bim::strata`
//! e `crate::bim::strata_assign`.

use crate::app::{Message, OpenCADStudio};
use crate::bim::quantities_rooms::{fatto_room_rows, project_room_rows};
use crate::bim::record;
use crate::bim::strata::{self, cm, Kind, Stratigraphy};
use crate::bim::strata_assign::{
    self, assignment_of, assignments_in, element_of, project_room_outlines, target_at, Element,
};
use crate::bim::units::DrawingUnit;
use crate::bim::detail;
use crate::bim::{APP_STRATIGRAFIA, LAYER_PARTICOLARI};
use crate::scene::view::dispatch::set_entity_xdata;
use codec::Handle;
use iced::Task;

use super::bim::room_tolerance;

const CMD: &str = "AL_STRATIGRAFIA";

const USAGE: &str = "AL_STRATIGRAFIA  Nuova <nome> <Parete|Solaio> <materiale> <spessore>; … | Elenco | \
                     Rimuovi <nome> | Assegna <nome> [x,y] | Togli [x,y]  (spessori in unità del disegno)";

/// Il primo token di `text` e il resto, senza spazi in testa.
fn split_token(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    match text.find(char::is_whitespace) {
        Some(at) => (&text[..at], text[at..].trim_start()),
        None => (text, ""),
    }
}

/// Il punto `x,y` col punto decimale, se `text` non è vuoto.
fn point_of(text: &str) -> Result<Option<[f64; 2]>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let numbers: Option<Vec<f64>> = text
        .split(',')
        .map(|part| part.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect();
    match numbers.as_deref() {
        Some(&[x, y]) | Some(&[x, y, _]) => Ok(Some([x, y])),
        _ => Err(format!(
            "{CMD}: punto non valido \"{text}\": serve x,y col punto decimale."
        )),
    }
}

/// Gli strati in cm: «Intonaco 1,5 cm; Laterizio 9,0 cm».
fn layers_text(s: &Stratigraphy, unit: DrawingUnit) -> String {
    s.layers
        .iter()
        .map(|l| format!("{} {} cm", l.material, cm(l.thickness, unit)))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Le entità su cui agire e quante se ne saltano, per `Assegna` e `Togli`.
#[derive(Default)]
struct Targets {
    found: Vec<(Handle, Element)>,
    not_elements: usize,
    locked: usize,
}

impl OpenCADStudio {
    /// `AL_STRATIGRAFIA …` (spec parte 6, §5.1).
    pub(super) fn bim_stratigrafia(&mut self, i: usize, cmd: &str) -> Task<Message> {
        if !self.bim_in_model_space(i, CMD) {
            return Task::none();
        }
        let (_name, rest) = split_token(cmd);
        let (keyword, rest) = split_token(rest);
        if keyword.is_empty() {
            self.command_line.push_info(USAGE);
            return Task::none();
        }
        let Some(unit) = self.bim_unit_or_ask(i, cmd.trim()) else {
            return self.finish_dispatch(CMD);
        };
        self.bim_pending = None;
        match keyword.to_ascii_uppercase().as_str() {
            "NUOVA" | "N" => self.bim_strati_nuova(i, rest, unit),
            "ELENCO" | "E" => self.bim_strati_elenco(i, unit),
            "RIMUOVI" | "R" => self.bim_strati_rimuovi(i, rest),
            "ASSEGNA" | "A" => self.bim_strati_assegna(i, rest, unit),
            "TOGLI" | "T" => self.bim_strati_togli(i, rest, unit),
            _ => {
                self.command_line
                    .push_error(&format!("{CMD}: opzione \"{keyword}\" sconosciuta."));
                self.command_line.push_info(USAGE);
            }
        }
        Task::none()
    }

    /// Riscrive le stratigrafie in un passo di annullamento a sé.
    fn bim_strati_save(&mut self, i: usize, all: &[Stratigraphy]) -> bool {
        self.push_undo_snapshot(i, CMD);
        strata::write_all(&mut self.tabs[i].scene.document, all);
        if strata::read_all(&self.tabs[i].scene.document) != all {
            self.drop_unchanged_pending_history(i);
            self.command_line
                .push_error(&format!("{CMD}: impossibile salvare le stratigrafie nel disegno."));
            return false;
        }
        self.tabs[i].dirty = true;
        self.finish_pending_history(i);
        true
    }

    fn bim_strati_nuova(&mut self, i: usize, rest: &str, unit: DrawingUnit) {
        let (name, rest) = split_token(rest);
        let (kind_word, layers) = split_token(rest);
        let Some(kind) = Kind::parse(kind_word) else {
            self.command_line.push_error(&format!(
                "{CMD}: serve il tipo dopo il nome, Parete o Solaio (trovato \"{kind_word}\")."
            ));
            return;
        };
        let layers = match strata::parse_layers(layers) {
            Ok(layers) => layers,
            Err(message) => {
                self.command_line.push_error(&format!("{CMD}: {message}"));
                return;
            }
        };
        let new = Stratigraphy {
            name: name.to_string(),
            kind,
            layers,
        };
        let mut all = strata::read_all(&self.tabs[i].scene.document);
        if let Err(message) = strata::add(&mut all, new.clone()) {
            self.command_line.push_error(&format!("{CMD}: {message}"));
            return;
        }
        if self.bim_strati_save(i, &all) {
            self.command_line.push_info(&format!(
                "{CMD}: «{}» ({}) con {} strati: {}; totale {} cm.",
                new.name,
                new.kind.word(),
                new.layers.len(),
                layers_text(&new, unit),
                cm(new.total(), unit)
            ));
        }
    }

    fn bim_strati_elenco(&mut self, i: usize, unit: DrawingUnit) {
        let doc = &self.tabs[i].scene.document;
        let all = strata::read_all(doc);
        if all.is_empty() {
            self.command_line
                .push_info(&format!("{CMD}: nessuna stratigrafia nel disegno."));
            return;
        }
        let assigned = assignments_in(doc);
        let mut lines = Vec::new();
        for s in &all {
            let count = assigned
                .iter()
                .filter(|(_, _, name)| name.eq_ignore_ascii_case(&s.name))
                .count();
            lines.push(format!(
                "«{}» ({}), {} cm: {}; assegnata a {count} elementi.",
                s.name,
                s.kind.word(),
                cm(s.total(), unit),
                layers_text(s, unit)
            ));
        }
        let orphans = assigned
            .iter()
            .filter(|(_, _, name)| strata::find(&all, name).is_none())
            .count();
        for line in lines {
            self.command_line.push_info(&line);
        }
        if orphans > 0 {
            self.command_line.push_info(&format!(
                "{orphans} elementi hanno una stratigrafia che non c'è più."
            ));
        }
    }

    fn bim_strati_rimuovi(&mut self, i: usize, rest: &str) {
        let (name, _) = split_token(rest);
        let mut all = strata::read_all(&self.tabs[i].scene.document);
        if !strata::remove(&mut all, name) {
            self.command_line
                .push_error(&format!("{CMD}: nessuna stratigrafia «{name}»."));
            return;
        }
        if self.bim_strati_save(i, &all) {
            let left = assignments_in(&self.tabs[i].scene.document)
                .iter()
                .filter(|(_, _, n)| n.eq_ignore_ascii_case(name))
                .count();
            let tail = if left > 0 {
                format!(" {left} elementi la hanno ancora: riassegnali (Assegna) o toglila (Togli).")
            } else {
                String::new()
            };
            self.command_line
                .push_info(&format!("{CMD}: «{name}» rimossa.{tail}"));
        }
    }

    /// Gli elementi di `Assegna`/`Togli`: con un punto quello sotto il punto,
    /// altrimenti la selezione.
    fn bim_strati_targets(
        &mut self,
        i: usize,
        point: Option<[f64; 2]>,
        unit: DrawingUnit,
    ) -> Option<Targets> {
        let scene = &self.tabs[i].scene;
        let doc = &scene.document;
        let mut targets = Targets::default();
        let candidates: Vec<(Handle, Option<Element>)> = match point {
            Some(p) => {
                let default_height = record::read(doc).default_height;
                let room_tol = room_tolerance(unit);
                let (fatto, _) = fatto_room_rows(doc, unit, room_tol, default_height);
                let (project, _) = project_room_rows(
                    doc,
                    unit,
                    unit.tolerance(),
                    room_tol,
                    default_height,
                    &fatto,
                );
                let rooms = project_room_outlines(doc, &project);
                match target_at(doc, p, &rooms) {
                    Some((handle, element)) => vec![(handle, Some(element))],
                    None => {
                        self.command_line.push_error(&format!(
                            "{CMD}: nel punto non c'è una muratura nuova né un locale di progetto."
                        ));
                        return None;
                    }
                }
            }
            None => {
                let selected: Vec<Handle> = scene.selected_entities().into_iter().map(|(h, _)| h).collect();
                if selected.is_empty() {
                    self.command_line.push_error(&format!(
                        "{CMD}: nessun oggetto selezionato (o dai un punto x,y)."
                    ));
                    return None;
                }
                selected
                    .into_iter()
                    .map(|h| (h, doc.get_entity(h).and_then(|e| element_of(h, e))))
                    .collect()
            }
        };
        for (handle, element) in candidates {
            match element {
                None => targets.not_elements += 1,
                Some(_) if scene.is_layer_locked(handle) => targets.locked += 1,
                Some(element) => targets.found.push((handle, element)),
            }
        }
        Some(targets)
    }

    /// Scrive (o toglie) il record su `handles` in un passo solo.
    fn bim_strati_write(&mut self, i: usize, handles: &[Handle], name: Option<&str>) {
        if handles.is_empty() {
            return;
        }
        let values = name.map(strata_assign::encode);
        self.apply_property_op(i, CMD, handles, |app, handle| {
            set_entity_xdata(
                &mut app.tabs[i].scene.document,
                handle,
                APP_STRATIGRAFIA,
                values.clone(),
            );
        });
        self.finish_pending_history(i);
        self.refresh_properties();
    }

    fn bim_strati_skipped(&mut self, targets: &Targets, wrong_kind: usize) {
        if targets.not_elements > 0 {
            self.command_line.push_info(&format!(
                "{} oggetti non sono murature nuove né semi di locali di progetto: ignorati.",
                targets.not_elements
            ));
        }
        if wrong_kind > 0 {
            self.command_line.push_info(&format!(
                "{wrong_kind} elementi del tipo sbagliato ignorati (parete sulle murature nuove, \
                 solaio sui locali di progetto)."
            ));
        }
        if targets.locked > 0 {
            self.command_line.push_info(&format!(
                "Elementi su layer bloccati ignorati: {}.",
                targets.locked
            ));
        }
    }

    fn bim_strati_assegna(&mut self, i: usize, rest: &str, unit: DrawingUnit) {
        let (name, rest) = split_token(rest);
        let all = strata::read_all(&self.tabs[i].scene.document);
        let Some(chosen) = strata::find(&all, name).cloned() else {
            self.command_line
                .push_error(&format!("{CMD}: nessuna stratigrafia «{name}»."));
            return;
        };
        let point = match point_of(rest) {
            Ok(point) => point,
            Err(message) => {
                self.command_line.push_error(&message);
                return;
            }
        };
        let Some(targets) = self.bim_strati_targets(i, point, unit) else {
            return;
        };
        let doc = &self.tabs[i].scene.document;
        let (fit, wrong): (Vec<_>, Vec<_>) = targets
            .found
            .iter()
            .partition(|(_, element)| element.kind() == chosen.kind);
        let changed: Vec<Handle> = fit
            .iter()
            .map(|(handle, _)| *handle)
            .filter(|&handle| {
                doc.get_entity(handle).and_then(assignment_of).as_deref() != Some(chosen.name.as_str())
            })
            .collect();
        self.bim_strati_write(i, &changed, Some(&chosen.name));
        self.command_line.push_info(&format!(
            "{CMD}: «{}» ({}) assegnata a {} elementi.",
            chosen.name,
            chosen.kind.word(),
            changed.len()
        ));
        self.bim_strati_skipped(&targets, wrong.len());
    }

    fn bim_strati_togli(&mut self, i: usize, rest: &str, unit: DrawingUnit) {
        let point = match point_of(rest) {
            Ok(point) => point,
            Err(message) => {
                self.command_line.push_error(&message);
                return;
            }
        };
        let Some(targets) = self.bim_strati_targets(i, point, unit) else {
            return;
        };
        let doc = &self.tabs[i].scene.document;
        let changed: Vec<Handle> = targets
            .found
            .iter()
            .map(|(handle, _)| *handle)
            .filter(|&handle| doc.get_entity(handle).and_then(assignment_of).is_some())
            .collect();
        self.bim_strati_write(i, &changed, None);
        self.command_line.push_info(&format!(
            "{CMD}: stratigrafia tolta da {} elementi.",
            changed.len()
        ));
        self.bim_strati_skipped(&targets, 0);
    }
}

const PARTICOLARE: &str = "AL_PARTICOLARE";

/// Quanto staccare i particolari da ciò che c'è la prima volta, in metri.
const MARGIN_M: f64 = 5.0;

impl OpenCADStudio {
    /// `AL_PARTICOLARE` (spec parte 6, §5.2): i particolari di tutte le
    /// stratigrafie, posati la prima volta a destra dello spazio modello e poi
    /// dove sono; quelli di prima si sostituiscono in un passo; layer e APPID in
    /// un passo a sé. Senza stratigrafie lo dice e non cambia nulla.
    pub(super) fn bim_particolare(&mut self, i: usize, cmd: &str) -> Task<Message> {
        if !self.bim_in_model_space(i, PARTICOLARE) {
            return Task::none();
        }
        let Some(unit) = self.bim_unit_or_ask(i, cmd.trim()) else {
            return self.finish_dispatch(PARTICOLARE);
        };
        self.bim_pending = None;
        let all = strata::read_all(&self.tabs[i].scene.document);
        if all.is_empty() {
            self.command_line.push_info(&format!(
                "{PARTICOLARE}: nessuna stratigrafia nel disegno (AL_STRATIGRAFIA Nuova): i                  particolari già nel disegno restano com'erano."
            ));
            return Task::none();
        }
        let old: Vec<Handle> = detail::details_in(&self.tabs[i].scene.document)
            .into_iter()
            .map(|(handle, _)| handle)
            .collect();
        if old.iter().any(|&handle| self.tabs[i].scene.is_layer_locked(handle)) {
            self.command_line.push_error(&format!(
                "{PARTICOLARE}: il layer {LAYER_PARTICOLARI} è bloccato: sblocca per rigenerare i                  particolari."
            ));
            return Task::none();
        }
        let anchor = detail::existing_anchor(&self.tabs[i].scene.document).unwrap_or_else(|| {
            self.tabs[i]
                .scene
                .model_space_extents()
                .map_or([0.0, 0.0], |(_, max)| {
                    [
                        f64::from(max.x) + MARGIN_M * unit.per_metre(),
                        f64::from(max.y),
                    ]
                })
        });
        let placed = detail::place(&all, unit, anchor);

        if detail::missing_layer(&self.tabs[i].scene.document) {
            self.push_undo_snapshot(i, "AL_LAYER");
            detail::ensure_layer(&mut self.tabs[i].scene.document);
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.refresh_layer_panel();
        }
        self.push_undo_snapshot(i, PARTICOLARE);
        let scene = &mut self.tabs[i].scene;
        if !old.is_empty() {
            scene.erase_entities(&old);
        }
        for entity in placed {
            scene.add_entity(entity);
        }
        self.tabs[i].dirty = true;
        self.finish_pending_history(i);
        self.refresh_properties();
        let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        self.command_line.push_info(&format!(
            "{PARTICOLARE}: {} particolari ({}){}.",
            all.len(),
            names.join(", "),
            if old.is_empty() {
                String::new()
            } else {
                format!(", {} entità di prima sostituite", old.len())
            }
        ));
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_points() {
        assert_eq!(split_token("  Nuova T12 Parete a 1"), ("Nuova", "T12 Parete a 1"));
        assert_eq!(split_token("Elenco"), ("Elenco", ""));
        assert_eq!(point_of("").unwrap(), None);
        assert_eq!(point_of("2,1.5").unwrap(), Some([2.0, 1.5]));
        assert!(point_of("2;1").is_err());
    }
}
