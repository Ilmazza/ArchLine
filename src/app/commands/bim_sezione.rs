//! I comandi BIM `AL_SEZIONE` e `AL_PROSPETTO` della parte 4 (spec
//! `2026-11-05-bim-sezioni-prospetti-design.md`, §5.1 e §5.2), lato app.
//!
//! - `AL_SEZIONE` legge i tagli del disegno (`AL_TAGLIO`), costruisce per ognuno
//!   i tre disegni (fatto, demolizioni e costruzioni, progetto:
//!   `bim::section_build`) e li posa;
//! - `AL_PROSPETTO <N|S|E|O|angolo> [nome]` costruisce i tre prospetti della
//!   direzione data (la normale uscente della facciata, in gradi dall'est
//!   in senso antiorario) e li posa.
//!
//! I disegni stanno nello spazio modello sul layer `AL-SEZIONI`, riconoscibili
//! dal record `ARCHLINE_SEZIONE` (tipo, nome e fase): a ogni lancio quelli dello
//! stesso tipo e nome si **sostituiscono**. La prima volta stanno a destra
//! dell'estensione del modello; poi restano dove sono (R117). Layer e APPID in
//! un passo di annullamento a sé, il resto in un solo passo. Il rilievo, le
//! zone, i locali e le aperture non si toccano.

use crate::app::{Message, OpenCADStudio};
use crate::bim::cut::cuts_in;
use crate::bim::section_build::{
    build_elevations, build_sections, DrawingKind, SectionBuild, SectionOptions,
};
use crate::bim::section_place::{self, drawings_in, existing_anchor_of, place};
use crate::bim::units::DrawingUnit;
use crate::command::{CadCommand, KeywordCommand};
use codec::Handle;
use iced::Task;

use super::bim::room_tolerance;

const SEZIONE: &str = "AL_SEZIONE";
const PROSPETTO: &str = "AL_PROSPETTO";

/// Quanto staccare i disegni dal modello la prima volta, in metri.
const MARGIN_M: f64 = 5.0;
/// Altezza dei testi del disegno, in metri (come `section_build`).
const TEXT_HEIGHT_M: f64 = 0.25;

impl OpenCADStudio {
    /// `AL_SEZIONE`: spazio modello, unità, tagli, costruzione, posa e
    /// sostituzione. Senza tagli o senza nulla da disegnare lo dice e non
    /// cambia niente (le sezioni di prima restano).
    pub(super) fn bim_sezione(&mut self, i: usize, cmd: &str) -> Task<Message> {
        if !self.bim_in_model_space(i, SEZIONE) {
            return Task::none();
        }
        let Some(unit) = self.bim_unit_or_ask(i, cmd.trim()) else {
            return self.finish_dispatch(SEZIONE);
        };
        self.bim_pending = None;
        let cuts = cuts_in(&self.tabs[i].scene.document);
        if cuts.is_empty() {
            self.command_line
                .push_error("AL_SEZIONE: nessun taglio nel disegno. Traccialo con AL_TAGLIO.");
            return Task::none();
        }
        let build = build_sections(&self.tabs[i].scene.document, &cuts, &section_options(unit));
        let what = format!("per {} tagli", cuts.len());
        self.bim_write_drawings(i, SEZIONE, DrawingKind::Sezione, None, build, unit, &what);
        Task::none()
    }

    /// `AL_PROSPETTO <N|S|E|O|angolo> [nome]`: la direzione è la normale
    /// uscente della facciata da vedere. Da solo chiede la direzione.
    pub(super) fn bim_prospetto(&mut self, i: usize, cmd: &str) -> Task<Message> {
        if !self.bim_in_model_space(i, PROSPETTO) {
            return Task::none();
        }
        let mut words = cmd.split_whitespace();
        let _name = words.next();
        let Some(direction) = words.next() else {
            let ask = KeywordCommand::new(
                PROSPETTO,
                "AL_PROSPETTO  Facciata da vedere [Nord/Sud/Est/Ovest]:",
                vec![
                    ("Nord", "N", None),
                    ("Sud", "S", None),
                    ("Est", "E", None),
                    ("Ovest", "O", None),
                ],
            );
            self.command_line.push_info(&ask.prompt());
            self.tabs[i].active_cmd = Some(Box::new(ask));
            return self.finish_dispatch(PROSPETTO);
        };
        let custom: Vec<&str> = words.collect();
        let (default_name, normal) = match parse_direction(direction) {
            Ok(found) => found,
            Err(message) => {
                self.command_line.push_error(&message);
                return Task::none();
            }
        };
        let name = if custom.is_empty() {
            default_name
        } else {
            custom.join(" ")
        };
        let Some(unit) = self.bim_unit_or_ask(i, cmd.trim()) else {
            return self.finish_dispatch(PROSPETTO);
        };
        self.bim_pending = None;
        let build = build_elevations(
            &self.tabs[i].scene.document,
            &name,
            normal,
            &section_options(unit),
        );
        let what = format!("«{name}»");
        self.bim_write_drawings(i, PROSPETTO, DrawingKind::Prospetto, Some(&name), build, unit, &what);
        Task::none()
    }

    /// Posa i disegni di `build` e sostituisce quelli di prima dello stesso
    /// tipo (e, per un prospetto, dello stesso nome). Senza disegni dice cosa
    /// manca e non cambia niente.
    #[allow(clippy::too_many_arguments)]
    fn bim_write_drawings(
        &mut self,
        i: usize,
        cmd: &str,
        kind: DrawingKind,
        name: Option<&str>,
        build: SectionBuild,
        unit: DrawingUnit,
        what: &str,
    ) {
        for notice in &build.notices {
            self.command_line.push_info(&notice.text());
        }
        if build.drawings.is_empty() {
            self.command_line.push_info(&format!(
                "{cmd}: niente da disegnare (nessun locale o muro con altezza): i disegni già \
                 nel disegno restano com'erano."
            ));
            return;
        }
        let old: Vec<Handle> = drawings_in(&self.tabs[i].scene.document, kind)
            .into_iter()
            .filter(|(_, found, _)| name.map_or(true, |n| n == found))
            .map(|(handle, _, _)| handle)
            .collect();
        if old.iter().any(|&handle| self.tabs[i].scene.is_layer_locked(handle)) {
            self.command_line.push_error(&format!(
                "{cmd}: il layer AL-SEZIONI è bloccato: sblocca per rigenerare i disegni."
            ));
            return;
        }
        let per_metre = unit.per_metre();
        let anchor = existing_anchor_of(&self.tabs[i].scene.document, kind, name)
            .unwrap_or_else(|| right_of_model(&self.tabs[i].scene, unit));
        let drawings = build.drawings.len();
        let placed = place(build.drawings, anchor, TEXT_HEIGHT_M * per_metre);

        if section_place::missing_layer(&self.tabs[i].scene.document) {
            self.push_undo_snapshot(i, "AL_LAYER");
            section_place::ensure_layer(&mut self.tabs[i].scene.document);
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.refresh_layer_panel();
        }
        self.push_undo_snapshot(i, cmd);
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
        self.command_line.push_info(&format!(
            "{cmd}: {drawings} disegni {what}{}.",
            if old.is_empty() {
                String::new()
            } else {
                format!(", {} entità di prima sostituite", old.len())
            }
        ));
    }
}

fn section_options(unit: DrawingUnit) -> SectionOptions {
    SectionOptions {
        unit,
        tol: unit.tolerance(),
        room_tol: room_tolerance(unit),
    }
}

/// Nome e normale uscente di una direzione: `N`, `S`, `E`, `O` (o `W`), oppure
/// un angolo in gradi dall'est in senso antiorario. Ciò che non si capisce è
/// un errore, mai una direzione inventata.
fn parse_direction(text: &str) -> Result<(String, [f64; 2]), String> {
    match text.to_uppercase().as_str() {
        "N" | "NORD" => Ok(("Nord".into(), [0.0, 1.0])),
        "S" | "SUD" => Ok(("Sud".into(), [0.0, -1.0])),
        "E" | "EST" => Ok(("Est".into(), [1.0, 0.0])),
        "O" | "W" | "OVEST" => Ok(("Ovest".into(), [-1.0, 0.0])),
        _ => {
            let degrees: f64 = text.replace(',', ".").parse().map_err(|_| {
                format!(
                    "AL_PROSPETTO: direzione \"{text}\" sconosciuta. Usa N, S, E, O o un angolo \
                     in gradi."
                )
            })?;
            if !degrees.is_finite() {
                return Err("AL_PROSPETTO: angolo non valido.".into());
            }
            let radians = degrees.to_radians();
            Ok((
                format!("{}°", text.trim().replace('.', ",")),
                [radians.cos(), radians.sin()],
            ))
        }
    }
}

/// L'angolo in alto a destra dell'estensione del modello, staccato di
/// [`MARGIN_M`]; un modello senza estensione ha l'angolo nell'origine.
fn right_of_model(scene: &crate::scene::Scene, unit: DrawingUnit) -> [f64; 2] {
    scene.model_space_extents().map_or([0.0, 0.0], |(_, max)| {
        [f64::from(max.x) + MARGIN_M * unit.per_metre(), f64::from(max.y)]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_are_the_outward_normals() {
        assert_eq!(parse_direction("n").unwrap(), ("Nord".to_string(), [0.0, 1.0]));
        assert_eq!(parse_direction("Sud").unwrap().1, [0.0, -1.0]);
        assert_eq!(parse_direction("E").unwrap().1, [1.0, 0.0]);
        assert_eq!(parse_direction("w").unwrap().1, [-1.0, 0.0]);
        let (name, normal) = parse_direction("90").unwrap();
        assert_eq!(name, "90°");
        assert!(normal[0].abs() < 1e-12 && (normal[1] - 1.0).abs() < 1e-12);
        assert!(parse_direction("nord-est").is_err());
        assert!(parse_direction("NaN").is_err());
    }
}
