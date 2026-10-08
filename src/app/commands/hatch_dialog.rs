//! The HATCH dialog on the app side: opening it, collecting areas while it is
//! hidden, and turning its state into a hatch on OK.

use iced::Task;

use crate::app::{Message, ModalKind, OpenCADStudio};
use crate::command::{CadCommand, WorkingPlane};
use crate::modules::draw::draw::hatch::{object_regions, HatchCommand};
use crate::modules::draw::draw::hatch_settings::{add_region, OriginMode, RegionOrigin};
use crate::ui::window::hatch_dialog::{Field, State};
use codec::Handle;

/// A `HatchCommand` that would create what the dialog describes, or `None`
/// while a field is unusable or nothing is collected.
pub(in crate::app) fn hatch_command_from_state(
    state: &State,
    document_origin: [f64; 2],
) -> Option<HatchCommand> {
    let resolved = state.settings.resolve()?;
    let origin = match state.settings.origin_mode {
        OriginMode::Current => document_origin,
        OriginMode::Specified => state.specified_origin.unwrap_or(document_origin),
    };
    Some(
        HatchCommand::new(
            state.outlines.clone(),
            state.boundary_sources.clone(),
            Vec::new(),
            None,
            state.plane,
        )
        .with_origin(origin)
        .with_settings(&resolved)
        .with_regions(
            state
                .regions
                .iter()
                .map(|(region, _)| region.clone())
                .collect(),
        ),
    )
}

impl OpenCADStudio {
    /// The working plane and the boundary data HATCH works from, exactly as the
    /// command computes them: the UCS plane in model space, the default plane
    /// elsewhere.
    pub(in crate::app) fn hatch_boundary_context(
        &self,
        i: usize,
    ) -> (
        WorkingPlane,
        rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
        Vec<Vec<[f64; 2]>>,
    ) {
        let working_plane = if self.tabs[i].editing_model_space() {
            self.tabs[i].ucs_xform().working_plane()
        } else {
            WorkingPlane::default()
        };
        let normal = working_plane.z.normalize_or(glam::DVec3::Z);
        let elevation = working_plane.origin.dot(normal);
        let storage = crate::entities::curve::ocs_plane(
            codec::types::Vector3::new(normal.x, normal.y, normal.z),
            elevation,
        );
        let plane = WorkingPlane::new(
            glam::DVec3::from_array(storage.origin),
            glam::DVec3::from_array(storage.x_axis),
            glam::DVec3::from_array(storage.y_axis),
        );
        let boundary_sources = self.tabs[i].scene.boundary_sources_on_plane(plane, 1.0e-6);
        let outlines = crate::scene::boundary_faces(&boundary_sources, 1.0e-6);
        (plane, boundary_sources, outlines)
    }

    pub(in crate::app) fn selected_handles(&self, i: usize) -> Vec<Handle> {
        self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(handle, _)| handle)
            .collect()
    }

    /// `HATCH`: open the dialog on the active drawing.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_open(&mut self) -> Task<Message> {
        // A dialog left over from an abandoned flow must not leak into this one.
        self.hatch_dialog_cancel();
        let i = self.active_tab;
        let (plane, boundary_sources, outlines) = self.hatch_boundary_context(i);
        let selected = self.selected_handles(i);
        let mut state = State::new(
            self.tabs[i].id,
            plane,
            outlines,
            boundary_sources,
            self.hatch_last.clone(),
        );
        // Closed (or together closing) objects already selected seed the first
        // collection, as they used to skip the pick step.
        if !selected.is_empty() {
            let seeded = object_regions(&state.boundary_sources, &selected);
            if seeded.is_empty() {
                self.command_line.push_info(
                    crate::t!("HATCH: the selected objects form no closed boundary.").as_ref(),
                );
            }
            for region in seeded {
                add_region(&mut state.regions, region, RegionOrigin::Objects);
            }
            state.taken_objects = selected
                .into_iter()
                .filter(|handle| state.boundary_sources.contains_key(handle))
                .collect();
        }
        self.hatch_dialog = Some(state);
        self.active_modal = Some(ModalKind::Hatch);
        Task::none()
    }

    pub(in crate::app) fn hatch_dialog_field(&mut self, field: Field) {
        if let Some(state) = self.hatch_dialog.as_mut() {
            state.apply(field);
        }
    }

    /// OK: create the hatch the dialog describes through the same commit paths
    /// the command line uses.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_ok(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let Some(state) = self.hatch_dialog.as_ref() else {
            return Task::none();
        };
        // The commit handlers act on the active tab, so the dialog must belong
        // to it; a flow whose tab was left is abandoned, never applied.
        if state.owner_tab_id != self.tabs[i].id {
            self.hatch_dialog_cancel();
            return Task::none();
        }
        if !state.can_ok() {
            return Task::none();
        }
        let document_origin = self.tabs[i].scene.document.hatch_origin();
        let Some(mut command) = hatch_command_from_state(state, document_origin) else {
            return Task::none();
        };
        let settings = state.settings.clone();
        let result = command.on_enter();
        self.hatch_last = settings;
        self.hatch_dialog = None;
        if self.active_modal == Some(ModalKind::Hatch) {
            self.close_active_modal();
        }
        self.apply_cmd_result(result)
    }

    /// Cancel / X / Esc, and every abandonment (tab left or closed): drop the
    /// state, stop a hidden-step command in the owner tab, put the selection
    /// back and close the window.
    pub(in crate::app) fn hatch_dialog_cancel(&mut self) {
        if let Some(state) = self.hatch_dialog.take() {
            if let Some(index) = self.tabs.iter().position(|tab| tab.id == state.owner_tab_id) {
                if state.flow != crate::ui::window::hatch_dialog::Flow::None {
                    self.tabs[index].active_cmd = None;
                    self.tabs[index].scene.clear_preview_wire();
                }
                if let Some(saved) = state.saved_selection {
                    self.hatch_restore_selection(index, saved);
                }
            }
        }
        if self.active_modal == Some(ModalKind::Hatch) {
            self.close_active_modal();
        }
    }

    pub(in crate::app) fn hatch_restore_selection(&mut self, index: usize, saved: Vec<Handle>) {
        self.tabs[index].scene.deselect_all();
        for handle in saved {
            self.tabs[index].scene.select_entity(handle, false);
        }
        if index == self.active_tab {
            self.refresh_properties();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Message, ModalKind, OpenCADStudio};
    use crate::modules::draw::draw::hatch_settings::{HatchRegion, RegionOrigin};
    use crate::ui::window::hatch_dialog::Field;
    use codec::Handle;

    fn add_line(app: &mut OpenCADStudio, x1: f64, y1: f64, x2: f64, y2: f64) -> Handle {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Line(codec::entities::Line::from_points(
                codec::types::Vector3::new(x1, y1, 0.0),
                codec::types::Vector3::new(x2, y2, 0.0),
            )))
    }

    /// A test app whose first tab is a drawing: `new_for_test` leaves tab 0 as
    /// the Start page, where HATCH is not allowed.
    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        app
    }

    /// A drawing with one closed 20 x 10 rectangle made of four lines.
    fn app_with_rectangle() -> OpenCADStudio {
        let mut app = new_app();
        add_line(&mut app, 0.0, 0.0, 20.0, 0.0);
        add_line(&mut app, 20.0, 0.0, 20.0, 10.0);
        add_line(&mut app, 20.0, 10.0, 0.0, 10.0);
        add_line(&mut app, 0.0, 10.0, 0.0, 0.0);
        app
    }

    fn region() -> (HatchRegion, RegionOrigin) {
        (
            HatchRegion {
                rings: vec![vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]]],
            },
            RegionOrigin::Points,
        )
    }

    fn hatch_count(app: &OpenCADStudio) -> usize {
        app.tabs[app.active_tab].scene.hatches.len()
    }

    #[test]
    fn hatch_opens_the_dialog_instead_of_starting_a_command() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        let i = app.active_tab;
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert!(app.hatch_dialog.is_some());
        assert!(app.tabs[i].active_cmd.is_none());
        assert_eq!(
            app.hatch_dialog.as_ref().unwrap().owner_tab_id,
            app.tabs[i].id
        );
    }

    #[test]
    fn the_dialog_opens_in_a_layout_too() {
        // Paper space has no UCS plane: the default plane is used.
        let mut app = new_app();
        let i = app.active_tab;
        let before = app.tabs[i].editing_model_space();
        let _ = app.dispatch_command("HATCH");
        assert_eq!(app.active_modal, Some(ModalKind::Hatch), "model space was {before}");
    }

    #[test]
    fn ok_without_any_region_does_nothing() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch), "still open");
        assert_eq!(hatch_count(&app), 0);
    }

    #[test]
    fn ok_creates_the_hatch_and_closes_the_dialog() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogOk);
        assert!(app.active_modal.is_none());
        assert!(app.hatch_dialog.is_none());
        assert_eq!(hatch_count(&app), 1);
    }

    #[test]
    fn ok_with_an_invalid_field_does_nothing() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogField(Field::Scale("0".into())));
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 0);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn a_comma_decimal_scale_is_accepted() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogField(Field::Scale("0,5".into())));
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 1);
    }

    #[test]
    fn cancel_discards_the_state_and_creates_nothing() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::CloseModal);
        assert!(app.active_modal.is_none());
        assert!(app.hatch_dialog.is_none());
        assert_eq!(hatch_count(&app), 0);
    }

    #[test]
    fn cancel_without_add_or_ok_keeps_the_remembered_settings() {
        let mut app = app_with_rectangle();
        let before = app.hatch_last.clone();
        let _ = app.dispatch_command("HATCH");
        let _ = app.update(Message::HatchDialogField(Field::Scale("9".into())));
        let _ = app.update(Message::CloseModal);
        assert_eq!(app.hatch_last, before);
    }

    #[test]
    fn ok_remembers_the_settings_for_the_next_time() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogField(Field::Scale("3".into())));
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(app.hatch_last.scale, "3");
        let _ = app.dispatch_command("HATCH");
        assert_eq!(app.hatch_dialog.as_ref().unwrap().settings.scale, "3");
    }

    #[test]
    fn a_remembered_pattern_that_vanished_cannot_be_applied() {
        let mut app = app_with_rectangle();
        app.hatch_last.pattern = "NO_SUCH_PATTERN".into();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 0);
        assert!(app.hatch_dialog.is_some());
    }

    #[test]
    fn enter_in_the_dialog_acts_as_ok() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::CommandFinalize);
        assert_eq!(hatch_count(&app), 1);
    }

    #[test]
    fn preselected_boundary_objects_seed_the_first_regions() {
        let mut app = new_app();
        let handles = [
            add_line(&mut app, 0.0, 0.0, 20.0, 0.0),
            add_line(&mut app, 20.0, 0.0, 20.0, 10.0),
            add_line(&mut app, 20.0, 10.0, 0.0, 10.0),
            add_line(&mut app, 0.0, 10.0, 0.0, 0.0),
        ];
        let i = app.active_tab;
        for handle in handles {
            app.tabs[i].scene.select_entity(handle, false);
        }
        let _ = app.dispatch_command("HATCH");
        let state = app.hatch_dialog.as_ref().unwrap();
        assert_eq!(state.regions.len(), 1, "four open sides close one area");
        assert_eq!(state.regions[0].1, RegionOrigin::Objects);
    }

    #[test]
    fn preselection_that_encloses_nothing_opens_an_empty_dialog() {
        let mut app = new_app();
        let handle = add_line(&mut app, 0.0, 0.0, 20.0, 0.0);
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(handle, false);
        let _ = app.dispatch_command("HATCH");
        assert!(app.hatch_dialog.as_ref().unwrap().regions.is_empty());
    }
}
