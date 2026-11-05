//! Il comando BIM `AL_SEZIONE` della parte 4 (spec
//! `2026-11-05-bim-sezioni-prospetti-design.md`, §5.1), lato app.
//!
//! Legge i tagli del disegno (`AL_TAGLIO`), costruisce per ognuno i tre disegni
//! (fatto, demolizioni e costruzioni, progetto: `bim::section_build`), li posa
//! nello spazio modello e **sostituisce** quelli del lancio precedente, che si
//! riconoscono dal record `ARCHLINE_SEZIONE`. La prima volta stanno a destra
//! dell'estensione del modello; poi restano dove sono (R117). Layer e APPID in
//! un passo di annullamento a sé, il resto in un solo passo `AL_SEZIONE`. Il
//! rilievo, le zone, i locali e le aperture non si toccano.

use crate::app::{Message, OpenCADStudio};
use crate::bim::cut::cuts_in;
use crate::bim::section_build::{build_sections, SectionOptions};
use crate::bim::section_place::{self, existing_anchor, place, sections_in};
use crate::bim::units::DrawingUnit;
use codec::Handle;
use iced::Task;

use super::bim::room_tolerance;

const CMD: &str = "AL_SEZIONE";

/// Quanto staccare le sezioni dal modello la prima volta, in metri.
const MARGIN_M: f64 = 5.0;
/// Altezza dei testi del disegno, in metri (come `section_build`).
const TEXT_HEIGHT_M: f64 = 0.25;

impl OpenCADStudio {
    /// `AL_SEZIONE`: spazio modello, unità, tagli, costruzione, posa e
    /// sostituzione. Senza tagli o senza nulla da disegnare lo dice e non
    /// cambia niente (le sezioni di prima restano).
    pub(super) fn bim_sezione(&mut self, i: usize, cmd: &str) -> Task<Message> {
        if !self.bim_in_model_space(i, CMD) {
            return Task::none();
        }
        let Some(unit) = self.bim_unit_or_ask(i, cmd.trim()) else {
            return self.finish_dispatch(CMD);
        };
        self.bim_pending = None;
        let doc = &self.tabs[i].scene.document;
        let cuts = cuts_in(doc);
        if cuts.is_empty() {
            self.command_line
                .push_error("AL_SEZIONE: nessun taglio nel disegno. Traccialo con AL_TAGLIO.");
            return Task::none();
        }
        let opts = SectionOptions {
            unit,
            tol: unit.tolerance(),
            room_tol: room_tolerance(unit),
        };
        let build = build_sections(doc, &cuts, &opts);
        for notice in &build.notices {
            self.command_line.push_info(&notice.text());
        }
        if build.drawings.is_empty() {
            self.command_line.push_info(
                "AL_SEZIONE: niente da disegnare (nessun locale o muro con altezza lungo i tagli): \
                 le sezioni già nel disegno restano com'erano.",
            );
            return Task::none();
        }
        let old: Vec<Handle> = sections_in(&self.tabs[i].scene.document)
            .into_iter()
            .map(|(handle, _, _)| handle)
            .collect();
        if old.iter().any(|&handle| self.tabs[i].scene.is_layer_locked(handle)) {
            self.command_line.push_error(
                "AL_SEZIONE: il layer AL-SEZIONI è bloccato: sblocca per rigenerare le sezioni.",
            );
            return Task::none();
        }
        let per_metre = unit.per_metre();
        let anchor = existing_anchor(&self.tabs[i].scene.document).unwrap_or_else(|| {
            right_of_model(&self.tabs[i].scene, unit)
        });
        let drawings = build.drawings.len();
        let placed = place(build.drawings, anchor, TEXT_HEIGHT_M * per_metre);

        if section_place::missing_layer(&self.tabs[i].scene.document) {
            self.push_undo_snapshot(i, "AL_LAYER");
            section_place::ensure_layer(&mut self.tabs[i].scene.document);
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.refresh_layer_panel();
        }
        self.push_undo_snapshot(i, CMD);
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
            "AL_SEZIONE: {drawings} disegni per {} tagli{}.",
            cuts.len(),
            if old.is_empty() {
                String::new()
            } else {
                format!(", {} entità di prima sostituite", old.len())
            }
        ));
        Task::none()
    }
}

/// L'angolo in alto a destra dell'estensione del modello, staccato di
/// [`MARGIN_M`]; un modello senza estensione ha l'angolo nell'origine.
fn right_of_model(scene: &crate::scene::Scene, unit: DrawingUnit) -> [f64; 2] {
    scene.model_space_extents().map_or([0.0, 0.0], |(_, max)| {
        [f64::from(max.x) + MARGIN_M * unit.per_metre(), f64::from(max.y)]
    })
}
