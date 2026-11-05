//! Il comando BIM `AL_MODELLO3D` della parte 5 (spec
//! `2026-11-05-bim-modello-3d-design.md`, §5.1 e §5.2), lato app.
//!
//! Costruisce i modelli di fatto e di progetto (`bim::model3d::build_models`),
//! li posa come `3DFACE` (`bim::model3d_place::place`) e salva le cinque viste
//! con nome che inquadrano il modello di progetto (`bim::model3d_views`).
//!
//! - La prima volta i modelli stanno a destra di tutto ciò che c'è nello
//!   spazio modello; poi restano dove sono (l'angolo in basso a sinistra dei
//!   modelli di prima, R127).
//! - Quelli di prima si **sostituiscono**; layer e APPID in un passo di
//!   annullamento a sé la prima volta, il resto (facce e viste) in un solo
//!   passo.
//! - Senza nessun prisma (nessun locale con altezza) dice che non c'è nulla da
//!   estrudere e non cambia niente: i modelli di prima restano.
//! - La camera non cambia: le viste si richiamano con VIEW.

use crate::app::{Message, OpenCADStudio};
use crate::bim::model3d::build_models;
use crate::bim::model3d_place::{
    self, existing_anchor, models_in, place, ModelPhase, LAYER_FATTO, LAYER_PROGETTO,
};
use crate::bim::model3d_views;
use crate::bim::section_build::SectionOptions;
use crate::bim::units::DrawingUnit;
use codec::{EntityType, Handle};
use iced::Task;

use super::bim::room_tolerance;

const MODELLO3D: &str = "AL_MODELLO3D";

/// Quanto staccare i modelli da ciò che c'è la prima volta, in metri.
const MARGIN_M: f64 = 5.0;

impl OpenCADStudio {
    /// `AL_MODELLO3D`: spazio modello, unità, costruzione, posa, sostituzione
    /// e viste.
    pub(super) fn bim_modello3d(&mut self, i: usize, cmd: &str) -> Task<Message> {
        if !self.bim_in_model_space(i, MODELLO3D) {
            return Task::none();
        }
        let Some(unit) = self.bim_unit_or_ask(i, cmd.trim()) else {
            return self.finish_dispatch(MODELLO3D);
        };
        self.bim_pending = None;
        let opts = SectionOptions {
            unit,
            tol: unit.tolerance(),
            room_tol: room_tolerance(unit),
        };
        let (fatto, progetto) = build_models(&self.tabs[i].scene.document, &opts);
        let mut notices = fatto.notices.clone();
        crate::bim::extract::push_unique(&mut notices, progetto.notices.clone());
        for notice in &notices {
            self.command_line.push_info(&notice.text());
        }
        if fatto.prisms == 0 && progetto.prisms == 0 {
            self.command_line.push_info(
                "AL_MODELLO3D: niente da estrudere (nessun locale con altezza: AL_ALTEZZA): i \
                 modelli già nel disegno restano com'erano.",
            );
            return Task::none();
        }
        let old: Vec<Handle> = models_in(&self.tabs[i].scene.document)
            .into_iter()
            .map(|(handle, _)| handle)
            .collect();
        if old.iter().any(|&handle| self.tabs[i].scene.is_layer_locked(handle)) {
            self.command_line.push_error(&format!(
                "AL_MODELLO3D: il layer {LAYER_FATTO} o {LAYER_PROGETTO} è bloccato: sblocca per \
                 rigenerare i modelli."
            ));
            return Task::none();
        }
        let per_metre = unit.per_metre();
        let anchor = existing_anchor(&self.tabs[i].scene.document)
            .unwrap_or_else(|| right_of_everything(&self.tabs[i].scene, unit));
        let placed = place(&fatto, &progetto, anchor, per_metre);
        let views = framing(&placed).map(|(lo, hi)| model3d_views::build(lo, hi));
        let faces = placed.len();

        if model3d_place::missing_layers(&self.tabs[i].scene.document) {
            self.push_undo_snapshot(i, "AL_LAYER");
            model3d_place::ensure_layers(&mut self.tabs[i].scene.document);
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.refresh_layer_panel();
        }
        self.push_undo_snapshot(i, MODELLO3D);
        let scene = &mut self.tabs[i].scene;
        if !old.is_empty() {
            scene.erase_entities(&old);
        }
        for entity in placed {
            scene.add_entity(entity);
        }
        let (created, replaced) = match views {
            Some(views) => model3d_views::install(&mut scene.document, views),
            None => (0, 0),
        };
        self.tabs[i].dirty = true;
        self.finish_pending_history(i);
        self.refresh_properties();
        self.command_line.push_info(&format!(
            "AL_MODELLO3D: fatto {} prismi, progetto {} prismi; {faces} facce 3D{}.",
            fatto.prisms,
            progetto.prisms,
            if old.is_empty() {
                String::new()
            } else {
                format!(", {} di prima sostituite", old.len())
            }
        ));
        if created + replaced > 0 {
            self.command_line.push_info(&format!(
                "AL_MODELLO3D: viste con nome {} (VIEW per richiamarle).",
                model3d_views::names().join(", ")
            ));
        }
        Task::none()
    }
}

/// Il parallelepipedo delle facce del modello di progetto; se il progetto non
/// ha facce, quello di tutte.
fn framing(placed: &[EntityType]) -> Option<([f64; 3], [f64; 3])> {
    let corners = |phase: Option<ModelPhase>| -> Vec<[f64; 3]> {
        placed
            .iter()
            .filter_map(|e| match e {
                EntityType::Face3D(face) => Some(face),
                _ => None,
            })
            .filter(|face| {
                phase.map_or(true, |p| {
                    face.common
                        .extended_data
                        .get_record(crate::bim::APP_MODELLO3D)
                        .and_then(model3d_place::decode)
                        == Some(p)
                })
            })
            .flat_map(|face| {
                [
                    face.first_corner,
                    face.second_corner,
                    face.third_corner,
                    face.fourth_corner,
                ]
            })
            .map(|c| [c.x, c.y, c.z])
            .collect()
    };
    let progetto = corners(Some(ModelPhase::Progetto));
    if let Some(found) = model3d_views::bounds(progetto.iter()) {
        return Some(found);
    }
    model3d_views::bounds(corners(None).iter())
}

/// L'angolo in basso a destra dell'estensione dello spazio modello, staccato
/// di [`MARGIN_M`]; senza estensione l'origine.
fn right_of_everything(scene: &crate::scene::Scene, unit: DrawingUnit) -> [f64; 2] {
    scene.model_space_extents().map_or([0.0, 0.0], |(min, max)| {
        [f64::from(max.x) + MARGIN_M * unit.per_metre(), f64::from(min.y)]
    })
}
