//! Comandi BIM della parte 2 (spec `2026-10-28-bim-abachi-quantita-design.md`),
//! lato app: per ora `AL_ALTEZZA` (§5.1).
//!
//! `AL_ALTEZZA` scrive l'altezza a tre livelli (§4.1) dove la chiede Mauro: la
//! predefinita del livello nel record di progetto (`L`), o l'altezza propria
//! di zone e locali selezionati (`S`, e `R` per toglierla). Le altezze si
//! digitano e si salvano in unità del disegno, come lo spessore (§3); la riga
//! di comando le ripete in metri («altezza 270 cm = 2,70 m») e avvisa, senza
//! bloccare, fuori da 0,50–10 m. Prima di leggere un numero serve l'unità del
//! disegno, chiesta come negli altri comandi `AL_*` (`AL_UNITA`). La logica
//! pura sta in `crate::bim` (`height::range_warning`,
//! `rooms::room_height_edits`).

use crate::app::{Message, OpenCADStudio};
use crate::bim::height::range_warning;
use crate::bim::record;
use crate::bim::rooms::{self, room_height_edits, rooms_in, same_room, Room};
use crate::bim::units::{format_decimal, DrawingUnit};
use crate::bim::zone::{self, Zone};
use crate::bim::{APP_LOCALE, APP_ZONA};
use crate::command::{CadCommand, KeywordCommand};
use crate::scene::view::dispatch::set_entity_xdata;
use crate::scene::Scene;
use codec::xdata::XDataValue;
use codec::Handle;
use iced::Task;
use std::collections::HashMap;

use super::bim::parse_positive;

const CMD: &str = "AL_ALTEZZA";

const ALTEZZA_PROMPT: &str = "AL_ALTEZZA  [Livello/Selezione/Rimuovi]:";

impl OpenCADStudio {
    /// `AL_ALTEZZA [L|S <valore> | R]` (spec parte 2, §5.1). Da solo chiede la
    /// parola chiave e il valore, che tornano qui come riga intera. Prima di
    /// tutto lo spazio modello; poi la riga, la selezione per `S` e `R`, e
    /// l'unità del disegno: un valore sbagliato o una selezione vuota non fanno
    /// chiedere l'unità.
    pub(super) fn bim_altezza(&mut self, i: usize, cmd: &str) -> Task<Message> {
        if !self.bim_in_model_space(i, CMD) {
            return Task::none();
        }
        let mut words = cmd.split_whitespace();
        let _name = words.next();
        let Some(keyword) = words.next() else {
            return self.bim_start_altezza(i);
        };
        let args: Vec<&str> = words.collect();
        let change = match AltezzaChange::parse(keyword, &args) {
            Ok(change) => change,
            Err(message) => {
                self.command_line.push_error(&message);
                return Task::none();
            }
        };
        if matches!(change, AltezzaChange::Selezione(_))
            && self.tabs[i].scene.selected_entities().is_empty()
        {
            self.command_line
                .push_error("AL_ALTEZZA: nessun oggetto selezionato.");
            return Task::none();
        }
        let Some(unit) = self.bim_unit_or_ask(i, cmd.trim()) else {
            return self.finish_dispatch(CMD);
        };
        // Un rilancio lasciato a metà (AL_UNITA annullato) non vale più.
        self.bim_pending = None;
        match change {
            AltezzaChange::Livello(value) => self.bim_altezza_livello(i, value, unit),
            AltezzaChange::Selezione(height) => self.bim_altezza_selezione(i, height, unit),
        }
        Task::none()
    }

    /// `AL_ALTEZZA` da solo: l'unità (il valore che segue è in unità del
    /// disegno, e il prompt la dice), poi `[Livello/Selezione/Rimuovi]`.
    fn bim_start_altezza(&mut self, i: usize) -> Task<Message> {
        let Some(unit) = self.bim_unit_or_ask(i, CMD) else {
            return self.finish_dispatch(CMD);
        };
        self.bim_pending = None;
        let (livello, selezione) = value_prompts(unit);
        let ask = KeywordCommand::new(
            CMD,
            ALTEZZA_PROMPT,
            vec![
                ("Livello", "L", Some(livello)),
                ("Selezione", "S", Some(selezione)),
                ("Rimuovi", "R", None),
            ],
        );
        self.command_line.push_info(&ask.prompt());
        self.tabs[i].active_cmd = Some(Box::new(ask));
        self.finish_dispatch(CMD)
    }

    /// `AL_ALTEZZA L <valore>`: la predefinita del livello nel record di
    /// progetto, in un passo di annullamento a sé (come `AL_UNITA`), solo se
    /// cambia. Senza dizionario radice il record non si scrive: lo si dice e
    /// non resta un passo vuoto.
    fn bim_altezza_livello(&mut self, i: usize, value: f64, unit: DrawingUnit) {
        let echo = height_echo(value, unit);
        let mut rec = record::read(&self.tabs[i].scene.document);
        if rec.default_height == Some(value) {
            self.command_line.push_info(&format!(
                "AL_ALTEZZA: {echo} è già la predefinita del livello."
            ));
        } else {
            self.push_undo_snapshot(i, CMD);
            rec.default_height = Some(value);
            let doc = &mut self.tabs[i].scene.document;
            record::write(doc, &rec);
            if record::read(doc).default_height != Some(value) {
                self.drop_unchanged_pending_history(i);
                self.command_line
                    .push_error("AL_ALTEZZA: impossibile salvare l'altezza nel disegno.");
                return;
            }
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.command_line.push_info(&format!(
                "AL_ALTEZZA: {echo} come predefinita del livello."
            ));
        }
        self.bim_altezza_range_warning(value, unit);
    }

    /// `AL_ALTEZZA S <valore>` / `R` sulla selezione: zone e locali che
    /// cambiano davvero, tutti in un solo passo di annullamento (nessuno se
    /// nulla cambia). Si riscrive solo l'XDATA (`set_entity_xdata`, che scarta
    /// anche il blocco grezzo di un DWG): layer, colore, geometria e testo
    /// restano, il rilievo e ciò che non è zona né locale non si toccano.
    fn bim_altezza_selezione(&mut self, i: usize, height: Option<f64>, unit: DrawingUnit) {
        let plan = altezza_plan(&self.tabs[i].scene, height, unit.tolerance());
        if !plan.records.is_empty() {
            let mut handles: Vec<Handle> = plan.records.keys().copied().collect();
            handles.sort_by_key(|handle| handle.value());
            let records = &plan.records;
            self.apply_property_op(i, CMD, &handles, |app, handle| {
                if let Some((app_name, values)) = records.get(&handle) {
                    set_entity_xdata(
                        &mut app.tabs[i].scene.document,
                        handle,
                        app_name,
                        Some(values.clone()),
                    );
                }
            });
            self.finish_pending_history(i);
        }
        let what = match height {
            Some(value) => height_echo(value, unit),
            None => "altezza propria tolta".to_string(),
        };
        self.command_line.push_info(&format!(
            "AL_ALTEZZA: {what}; {} zone e {} locali modificati.",
            plan.zones, plan.rooms
        ));
        if let Some(value) = height {
            self.bim_altezza_range_warning(value, unit);
        }
        if plan.not_ours > 0 {
            self.command_line.push_info(&format!(
                "{} oggetti non sono zone né locali e sono stati ignorati.",
                plan.not_ours
            ));
        }
        if plan.locked_zones > 0 {
            self.command_line.push_info(&format!(
                "Zone su layer bloccati ignorate: {}.",
                plan.locked_zones
            ));
        }
        if plan.locked_rooms > 0 {
            self.command_line.push_info(&format!(
                "Locali su layer bloccati ignorati: {}.",
                plan.locked_rooms
            ));
        }
    }

    /// L'avviso fuori da 0,50–10 m (spec parte 2, §3): un messaggio, non un
    /// blocco. `value` è già un numero positivo (`parse_positive`).
    fn bim_altezza_range_warning(&mut self, value: f64, unit: DrawingUnit) {
        if let Some(notice) = range_warning(value, unit) {
            self.command_line.push_info(&notice.text());
        }
    }
}

/// Che cosa chiede `AL_ALTEZZA <parola> [valore]`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum AltezzaChange {
    /// La predefinita del livello, nel record di progetto.
    Livello(f64),
    /// L'altezza propria di zone e locali selezionati; `None` la toglie.
    Selezione(Option<f64>),
}

impl AltezzaChange {
    /// La parola chiave (breve o intera, senza badare alle maiuscole) e il suo
    /// valore. Ciò che non si capisce è un errore, mai un valore inventato.
    fn parse(keyword: &str, args: &[&str]) -> Result<Self, String> {
        match keyword.to_ascii_uppercase().as_str() {
            "L" | "LIVELLO" => height_value(args).map(Self::Livello),
            "S" | "SELEZIONE" => height_value(args).map(|value| Self::Selezione(Some(value))),
            "R" | "RIMUOVI" => match args {
                [] => Ok(Self::Selezione(None)),
                [extra, ..] => Err(format!(
                    "AL_ALTEZZA: argomento in più \"{extra}\" dopo \"{keyword}\"."
                )),
            },
            _ => Err(format!(
                "AL_ALTEZZA: opzione \"{keyword}\" sconosciuta. Usa Livello, Selezione o Rimuovi."
            )),
        }
    }
}

/// Il valore di `L` / `S`: un solo argomento, un numero maggiore di zero
/// (`parse_positive`: niente zero, negativi, `NaN` o infiniti).
fn height_value(args: &[&str]) -> Result<f64, String> {
    match args {
        [] => Err("AL_ALTEZZA: manca il valore dell'altezza.".to_string()),
        [text] => parse_positive(text).ok_or_else(|| {
            format!(
                "AL_ALTEZZA: valore non valido \"{text}\": serve un numero maggiore di zero, \
                 in unità del disegno."
            )
        }),
        [_, extra, ..] => Err(format!(
            "AL_ALTEZZA: argomento in più \"{extra}\" dopo il valore."
        )),
    }
}

/// I prompt del valore di `Livello` e di `Selezione`, con l'unità del disegno
/// in cui lo si digita.
fn value_prompts(unit: DrawingUnit) -> (&'static str, &'static str) {
    match unit {
        DrawingUnit::Millimetri => ("Altezza predefinita del livello (mm):", "Altezza (mm):"),
        DrawingUnit::Centimetri => ("Altezza predefinita del livello (cm):", "Altezza (cm):"),
        DrawingUnit::Metri => ("Altezza predefinita del livello (m):", "Altezza (m):"),
    }
}

/// L'altezza digitata con la sua unità e, se non è il metro, in metri con due
/// decimali: «altezza 270 cm = 2,70 m», «altezza 2,7 cm = 0,03 m», «altezza
/// 2,70 m». Il valore digitato si ripete senza zeri in coda e con la virgola,
/// così un errore di scala si vede subito (spec parte 2, §3).
fn height_echo(value: f64, unit: DrawingUnit) -> String {
    let metres = format_decimal(value / unit.per_metre());
    let typed = value.to_string().replace('.', ",");
    match unit {
        DrawingUnit::Metri => format!("altezza {metres} m"),
        DrawingUnit::Centimetri => format!("altezza {typed} cm = {metres} m"),
        DrawingUnit::Millimetri => format!("altezza {typed} mm = {metres} m"),
    }
}

/// Ciò che `AL_ALTEZZA S|R` farebbe sulla selezione, senza ancora toccare
/// nulla.
#[derive(Default)]
struct AltezzaPlan {
    /// I record XDATA nuovi (APPID e valori) di zone e parti di locale che
    /// cambiano davvero.
    records: HashMap<Handle, (&'static str, Vec<XDataValue>)>,
    /// Zone che cambiano.
    zones: usize,
    /// Locali che cambiano (un locale di fatto conta una volta, contorno e
    /// testo insieme).
    rooms: usize,
    /// Oggetti selezionati che non sono né zone né parti di locale.
    not_ours: usize,
    /// Zone su layer bloccati: non si toccano.
    locked_zones: usize,
    /// Locali con almeno una parte su un layer bloccato: non si toccano
    /// affatto, così contorno e testo non dicono altezze diverse.
    locked_rooms: usize,
}

/// Il piano di `AL_ALTEZZA S|R` con altezza `height` (`None` = togliere). Di
/// un locale basta una parte selezionata: cambiano tutte le sue parti
/// ([`room_height_edits`]).
fn altezza_plan(scene: &Scene, height: Option<f64>, tol: f64) -> AltezzaPlan {
    let doc = &scene.document;
    let selected: Vec<Handle> = scene
        .selected_entities()
        .into_iter()
        .map(|(handle, _)| handle)
        .collect();
    let edits = room_height_edits(doc, &selected, height, tol);
    let mut plan = AltezzaPlan::default();
    for &handle in &selected {
        let found = doc
            .get_entity(handle)
            .and_then(|entity| zone::read_zone(handle, entity));
        let Some(found) = found else {
            if !edits.iter().any(|(part, _, _)| *part == handle) {
                plan.not_ours += 1;
            }
            continue;
        };
        if scene.is_layer_locked(handle) {
            plan.locked_zones += 1;
            continue;
        }
        if found.zone.height != height {
            let new = Zone {
                height,
                ..found.zone
            };
            plan.records.insert(handle, (APP_ZONA, zone::encode(&new)));
            plan.zones += 1;
        }
    }
    let locked: Vec<&Room> = edits
        .iter()
        .filter(|(handle, _, _)| scene.is_layer_locked(*handle))
        .map(|(_, room, _)| room)
        .collect();
    plan.locked_rooms = count_rooms(&locked, tol);
    let current: HashMap<Handle, Option<f64>> = rooms_in(doc)
        .into_iter()
        .map(|(handle, room, _)| (handle, room.height))
        .collect();
    let mut changed: Vec<&Room> = Vec::new();
    for (handle, room, part) in &edits {
        if locked.iter().any(|seen| same_room(seen, room, tol))
            || current.get(handle) == Some(&height)
        {
            continue;
        }
        plan.records
            .insert(*handle, (APP_LOCALE, rooms::encode(room, *part)));
        changed.push(room);
    }
    plan.rooms = count_rooms(&changed, tol);
    plan
}

/// Quanti locali diversi ([`same_room`]) ci sono in `rooms`.
fn count_rooms(rooms: &[&Room], tol: f64) -> usize {
    let mut kept: Vec<&Room> = Vec::new();
    for room in rooms {
        if !kept.iter().any(|seen| same_room(seen, room, tol)) {
            kept.push(room);
        }
    }
    kept.len()
}

#[cfg(test)]
mod tests {
    use super::{height_echo, AltezzaChange};
    use crate::bim::units::DrawingUnit;

    #[test]
    fn echo_repeats_the_unit_and_the_metres() {
        assert_eq!(
            height_echo(270.0, DrawingUnit::Centimetri),
            "altezza 270 cm = 2,70 m"
        );
        assert_eq!(
            height_echo(2.7, DrawingUnit::Centimetri),
            "altezza 2,7 cm = 0,03 m"
        );
        assert_eq!(
            height_echo(2700.0, DrawingUnit::Millimetri),
            "altezza 2700 mm = 2,70 m"
        );
        assert_eq!(height_echo(2.7, DrawingUnit::Metri), "altezza 2,70 m");
        assert_eq!(height_echo(3.0, DrawingUnit::Metri), "altezza 3,00 m");
    }

    #[test]
    fn altezza_keywords_short_and_long_with_their_values() {
        for (keyword, args, want) in [
            ("L", &["2,70"][..], AltezzaChange::Livello(2.7)),
            ("livello", &["2.70"][..], AltezzaChange::Livello(2.7)),
            ("S", &["3"][..], AltezzaChange::Selezione(Some(3.0))),
            ("Selezione", &["3,00"][..], AltezzaChange::Selezione(Some(3.0))),
            ("r", &[][..], AltezzaChange::Selezione(None)),
            ("RIMUOVI", &[][..], AltezzaChange::Selezione(None)),
        ] {
            assert_eq!(AltezzaChange::parse(keyword, args), Ok(want), "{keyword}");
        }
        for (keyword, args) in [
            ("L", &[][..]),
            ("L", &["0"][..]),
            ("L", &["-1"][..]),
            ("S", &["abc"][..]),
            ("S", &["NaN"][..]),
            ("L", &["1", "2"][..]),
            ("R", &["2"][..]),
            ("X", &[][..]),
        ] {
            let error = AltezzaChange::parse(keyword, args).unwrap_err();
            assert!(
                error.starts_with("AL_ALTEZZA:"),
                "{keyword} {args:?}: {error}"
            );
        }
    }
}
