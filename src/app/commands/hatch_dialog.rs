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

    /// Hide the dialog and start the collector for "Add: Pick points" or
    /// "Add: Select objects".
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_add(
        &mut self,
        kind: crate::ui::window::hatch_dialog::AddKind,
    ) -> Task<Message> {
        use crate::ui::window::hatch_dialog::{AddKind, Flow};
        let i = self.active_tab;
        let Some(state) = self.hatch_dialog.as_ref() else {
            return Task::none();
        };
        if state.owner_tab_id != self.tabs[i].id {
            self.hatch_dialog_cancel();
            return Task::none();
        }
        let Some(resolved) = state.settings.resolve() else {
            return Task::none();
        };
        let settings = state.settings.clone();
        let origin = self.hatch_origin_for(i);
        // Rebuild the boundary data: the drawing may have changed since the
        // dialog opened or since the last round.
        let (plane, boundary_sources, outlines) = self.hatch_boundary_context(i);
        let saved = (kind == AddKind::Objects).then(|| self.selected_handles(i));
        if saved.is_some() {
            self.tabs[i].scene.deselect_all();
        }
        let command = HatchCommand::collecting(
            outlines.clone(),
            boundary_sources.clone(),
            plane,
            kind == AddKind::Objects,
        )
        .with_origin(origin)
        .with_settings(&resolved);
        if let Some(state) = self.hatch_dialog.as_mut() {
            state.plane = plane;
            state.outlines = outlines;
            state.boundary_sources = boundary_sources;
            state.saved_selection = saved;
            state.flow = match kind {
                AddKind::Points => Flow::Pick,
                AddKind::Objects => Flow::Select,
            };
        }
        self.hatch_last = settings;
        self.active_modal = None;
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    /// The hatch origin the next command should use: the picked one when the
    /// dialog is in "Specified origin" mode, the drawing's otherwise.
    fn hatch_origin_for(&self, i: usize) -> [f64; 2] {
        let document = self.tabs[i].scene.document.hatch_origin();
        match self.hatch_dialog.as_ref() {
            Some(state) if state.settings.origin_mode == OriginMode::Specified => {
                state.specified_origin.unwrap_or(document)
            }
            _ => document,
        }
    }

    /// "Click to set new origin": hide the dialog and take one point.
    pub(in crate::app) fn hatch_dialog_pick_origin(&mut self) -> Task<Message> {
        use crate::modules::draw::draw::hatch_flows::HatchOriginPickCommand;
        use crate::ui::window::hatch_dialog::Flow;
        let i = self.active_tab;
        let Some(state) = self.hatch_dialog.as_mut() else {
            return Task::none();
        };
        if state.owner_tab_id != self.tabs[i].id {
            self.hatch_dialog_cancel();
            return Task::none();
        }
        state.flow = Flow::Origin;
        self.active_modal = None;
        let command = HatchOriginPickCommand;
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Task::none()
    }

    /// Preview: hide the dialog and show what OK would create. Does not touch
    /// the remembered settings.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_preview(&mut self) -> Task<Message> {
        use crate::modules::draw::draw::hatch_flows::HatchPreviewCommand;
        use crate::ui::window::hatch_dialog::Flow;
        let i = self.active_tab;
        let Some(state) = self.hatch_dialog.as_ref() else {
            return Task::none();
        };
        if state.owner_tab_id != self.tabs[i].id {
            self.hatch_dialog_cancel();
            return Task::none();
        }
        if !state.can_ok() {
            return Task::none();
        }
        let document_origin = self.tabs[i].scene.document.hatch_origin();
        let Some(command) = hatch_command_from_state(state, document_origin) else {
            return Task::none();
        };
        let models = command.preview_models();
        if let Some(state) = self.hatch_dialog.as_mut() {
            state.flow = Flow::Preview;
        }
        self.active_modal = None;
        let preview = HatchPreviewCommand::new(models);
        self.command_line.push_info(&preview.prompt());
        self.tabs[i].active_cmd = Some(Box::new(preview));
        self.refresh_area_preview(i);
        Task::none()
    }

    /// The collector finished a round: take its regions into the dialog,
    /// restore the selection and show the dialog again.
    #[inline(never)]
    pub(in crate::app) fn handle_hatch_boundaries_picked(
        &mut self,
        regions: Vec<(
            crate::modules::draw::draw::hatch_settings::HatchRegion,
            RegionOrigin,
        )>,
        objects: Vec<Handle>,
    ) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        let owned = self
            .hatch_dialog
            .as_ref()
            .is_some_and(|state| state.owner_tab_id == self.tabs[i].id);
        if !owned {
            // The flow belongs to another tab or is gone: never apply it here.
            self.hatch_dialog_cancel();
            return Task::none();
        }
        if let Some(state) = self.hatch_dialog.as_mut() {
            for (region, origin) in regions {
                add_region(&mut state.regions, region, origin);
            }
            for handle in objects {
                if !state.taken_objects.contains(&handle) {
                    state.taken_objects.push(handle);
                }
            }
        }
        self.hatch_dialog_resume(i, None);
        Task::none()
    }

    /// Back to the visible dialog after a hidden step. `origin` is the point a
    /// "Click to set new origin" round picked, in plane coordinates.
    pub(in crate::app) fn hatch_dialog_resume(&mut self, i: usize, origin: Option<[f64; 2]>) {
        use crate::ui::window::hatch_dialog::Flow;
        let owned = self
            .hatch_dialog
            .as_ref()
            .is_some_and(|state| state.owner_tab_id == self.tabs[i].id);
        if !owned {
            self.hatch_dialog_cancel();
            return;
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].scene.clear_preview_wire();
        let saved = self
            .hatch_dialog
            .as_mut()
            .and_then(|state| state.saved_selection.take());
        if let Some(saved) = saved {
            self.hatch_restore_selection(i, saved);
        }
        if let Some(state) = self.hatch_dialog.as_mut() {
            state.flow = Flow::None;
            if let Some(point) = origin {
                state.specified_origin = Some(point);
                state.settings.origin_mode = OriginMode::Specified;
            }
        }
        self.active_modal = Some(ModalKind::Hatch);
    }

    /// The `Dispatch` strings the hidden-step commands end with. A successful
    /// Pick/Select round does not come this way: it is
    /// `CmdResult::HatchBoundariesPicked`.
    pub(in crate::app) fn dispatch_hatch_dialog(
        &mut self,
        cmd: &str,
        i: usize,
    ) -> Option<Task<Message>> {
        match cmd {
            "HATCH_PICK_CANCELLED" | "HATCH_PREVIEW_DONE" => {
                self.hatch_dialog_resume(i, None);
                Some(Task::none())
            }
            _ => {
                let rest = cmd.strip_prefix("HATCH_ORIGIN_PICKED ")?;
                let numbers: Vec<f64> = rest
                    .split_whitespace()
                    .filter_map(|part| part.parse::<f64>().ok())
                    .collect();
                if numbers.len() != 3 {
                    self.hatch_dialog_resume(i, None);
                    return Some(Task::none());
                }
                let local = self
                    .hatch_dialog
                    .as_ref()
                    .map(|state| {
                        state
                            .plane
                            .to_local(glam::DVec3::new(numbers[0], numbers[1], numbers[2]))
                    })
                    .map(|point| [point.x, point.y]);
                self.hatch_dialog_resume(i, local);
                Some(Task::none())
            }
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

    use crate::command::StepInput;
    use crate::ui::window::hatch_dialog::{AddKind, Flow};

    fn open_dialog(app: &mut OpenCADStudio) {
        let _ = app.dispatch_command("HATCH");
        assert!(app.hatch_dialog.is_some());
    }

    fn click_inside(app: &mut OpenCADStudio) {
        let i = app.active_tab;
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("a collector is running")
            .on_point(glam::DVec3::new(10.0, 5.0, 0.0));
        let _ = app.apply_cmd_result(result);
    }

    #[test]
    fn add_points_hides_the_dialog_and_starts_a_collector() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        let i = app.active_tab;
        assert!(app.active_modal.is_none());
        assert!(app.hatch_dialog.is_some(), "state survives while hidden");
        assert_eq!(app.hatch_dialog.as_ref().unwrap().flow, Flow::Pick);
        assert!(app.tabs[i].active_cmd.is_some());
    }

    #[test]
    fn pick_points_then_enter_returns_to_the_dialog_with_the_region() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        click_inside(&mut app);
        let _ = app.feed_command(StepInput::Enter);
        let i = app.active_tab;
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert!(app.tabs[i].active_cmd.is_none());
        let state = app.hatch_dialog.as_ref().unwrap();
        assert_eq!(state.flow, Flow::None);
        assert_eq!(state.regions.len(), 1);
        assert_eq!(state.regions[0].1, RegionOrigin::Points);
        assert_eq!(hatch_count(&app), 0, "nothing is created before OK");
    }

    #[test]
    fn picking_the_same_area_twice_keeps_one_region() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        for _ in 0..2 {
            let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
            click_inside(&mut app);
            let _ = app.feed_command(StepInput::Enter);
        }
        assert_eq!(app.hatch_dialog.as_ref().unwrap().regions.len(), 1);
    }

    #[test]
    fn escape_returns_to_the_dialog_without_adding() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        click_inside(&mut app);
        let _ = app.feed_command(StepInput::Escape);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        let state = app.hatch_dialog.as_ref().unwrap();
        assert_eq!(state.flow, Flow::None);
        assert!(state.regions.is_empty());
    }

    #[test]
    fn ok_after_picking_creates_the_hatch() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        click_inside(&mut app);
        let _ = app.feed_command(StepInput::Enter);
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 1);
        assert!(app.hatch_dialog.is_none());
    }

    #[test]
    fn add_updates_the_remembered_settings_and_cancel_keeps_them() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogField(Field::Scale("4".into())));
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        assert_eq!(app.hatch_last.scale, "4");
        let _ = app.feed_command(StepInput::Escape);
        let _ = app.update(Message::CloseModal);
        assert_eq!(app.hatch_last.scale, "4", "Cancel after an Add keeps the change");
        assert!(app.hatch_dialog.is_none(), "Cancel drops the dialog state");
        assert!(app.active_modal.is_none());
    }

    #[test]
    fn add_is_refused_while_a_field_is_invalid() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogField(Field::Angle("x".into())));
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
    }

    #[test]
    fn select_objects_clears_the_selection_and_escape_restores_it() {
        let mut app = new_app();
        let keep = add_line(&mut app, 100.0, 100.0, 110.0, 100.0);
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(keep, false);
        let _ = app.dispatch_command("HATCH");
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        assert!(app.tabs[i].scene.selected.is_empty(), "starts with an empty selection");
        let _ = app.feed_command(StepInput::Escape);
        assert!(app.tabs[i].scene.selected.contains(&keep), "selection restored");
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn select_objects_enter_restores_the_selection_too() {
        let mut app = app_with_rectangle();
        let i = app.active_tab;
        let keep = add_line(&mut app, 100.0, 100.0, 110.0, 100.0);
        app.tabs[i].scene.select_entity(keep, false);
        let _ = app.dispatch_command("HATCH");
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        let _ = app.feed_command(StepInput::Enter);
        assert!(app.tabs[i].scene.selected.contains(&keep));
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn two_areas_sharing_a_boundary_object_both_form() {
        let mut app = new_app();
        // Two squares side by side sharing the middle line.
        add_line(&mut app, 0.0, 0.0, 10.0, 0.0);
        add_line(&mut app, 10.0, 0.0, 20.0, 0.0);
        add_line(&mut app, 20.0, 0.0, 20.0, 10.0);
        add_line(&mut app, 20.0, 10.0, 10.0, 10.0);
        add_line(&mut app, 10.0, 10.0, 0.0, 10.0);
        add_line(&mut app, 0.0, 10.0, 0.0, 0.0);
        add_line(&mut app, 10.0, 0.0, 10.0, 10.0);
        let _ = app.dispatch_command("HATCH");
        for x in [5.0, 15.0] {
            let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
            let i = app.active_tab;
            let result = app.tabs[i]
                .active_cmd
                .as_mut()
                .unwrap()
                .on_point(glam::DVec3::new(x, 5.0, 0.0));
            let _ = app.apply_cmd_result(result);
            let _ = app.feed_command(StepInput::Enter);
        }
        assert_eq!(app.hatch_dialog.as_ref().unwrap().regions.len(), 2);
    }

    #[test]
    fn preview_shows_without_creating_and_returns() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogPreview);
        let i = app.active_tab;
        assert!(app.active_modal.is_none());
        assert_eq!(app.hatch_dialog.as_ref().unwrap().flow, Flow::Preview);
        assert!(app.tabs[i].active_cmd.is_some());
        assert_eq!(hatch_count(&app), 0);
        assert!(
            !app.tabs[i].scene.preview_hatches.is_empty(),
            "the preview is drawn while the dialog is hidden"
        );
        let _ = app.feed_command(StepInput::Enter);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(app.hatch_dialog.as_ref().unwrap().flow, Flow::None);
        assert_eq!(hatch_count(&app), 0);
        assert!(
            app.tabs[i].scene.preview_hatches.is_empty(),
            "the preview is gone once the dialog is back"
        );
    }

    #[test]
    fn preview_does_not_touch_the_remembered_settings() {
        let mut app = app_with_rectangle();
        let before = app.hatch_last.clone();
        open_dialog(&mut app);
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogField(Field::Scale("7".into())));
        let _ = app.update(Message::HatchDialogPreview);
        let _ = app.feed_command(StepInput::Escape);
        let _ = app.update(Message::CloseModal);
        assert_eq!(app.hatch_last, before, "Preview then Cancel changes nothing");
    }

    #[test]
    fn preview_needs_a_region() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogPreview);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn origin_pick_stores_the_point_in_plane_coordinates_and_returns() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogField(Field::OriginMode(
            crate::modules::draw::draw::hatch_settings::OriginMode::Specified,
        )));
        // A plane away from the world origin with local x along world +Y and
        // local y along world -X: the identity plane would hide a missing
        // conversion.
        app.hatch_dialog.as_mut().unwrap().plane = crate::command::WorkingPlane::new(
            glam::DVec3::new(100.0, -40.0, 0.0),
            glam::DVec3::Y,
            glam::DVec3::NEG_X,
        );
        let _ = app.update(Message::HatchDialogPickOrigin);
        assert_eq!(app.hatch_dialog.as_ref().unwrap().flow, Flow::Origin);
        let i = app.active_tab;
        // World (103, -35): from the plane origin that is (+3, +5), so local
        // x = 5 (along +Y) and local y = -3 (along -X). Worked out by hand.
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_point(glam::DVec3::new(103.0, -35.0, 0.0));
        let _ = app.apply_cmd_result(result);
        let state = app.hatch_dialog.as_ref().unwrap();
        assert_eq!(state.flow, Flow::None);
        let local = state.specified_origin.expect("an origin was picked");
        assert!((local[0] - 5.0).abs() < 1.0e-9, "local x was {}", local[0]);
        assert!((local[1] + 3.0).abs() < 1.0e-9, "local y was {}", local[1]);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn origin_escape_returns_without_a_point() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogField(Field::OriginMode(
            crate::modules::draw::draw::hatch_settings::OriginMode::Specified,
        )));
        let _ = app.update(Message::HatchDialogPickOrigin);
        let _ = app.feed_command(StepInput::Escape);
        assert!(app.hatch_dialog.as_ref().unwrap().specified_origin.is_none());
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    fn add_rect(app: &mut OpenCADStudio, x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Handle> {
        vec![
            add_line(app, x0, y0, x1, y0),
            add_line(app, x1, y0, x1, y1),
            add_line(app, x1, y1, x0, y1),
            add_line(app, x0, y1, x0, y0),
        ]
    }

    fn click_at(app: &mut OpenCADStudio, x: f64, y: f64) {
        let i = app.active_tab;
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("a collector is running")
            .on_point(glam::DVec3::new(x, y, 0.0));
        let _ = app.apply_cmd_result(result);
    }

    /// Hand the collector the objects the user would have picked in the
    /// viewport (the direct path: no mouse), then finish with Enter.
    fn choose_objects(app: &mut OpenCADStudio, handles: Vec<Handle>) {
        let _ = app.feed_command(StepInput::SelectionComplete(handles));
        let _ = app.feed_command(StepInput::Enter);
    }

    #[test]
    fn select_objects_collects_the_chosen_boundary_objects() {
        let mut app = new_app();
        let sides = add_rect(&mut app, 0.0, 0.0, 20.0, 10.0);
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        choose_objects(&mut app, sides.clone());
        let state = app.hatch_dialog.as_ref().unwrap();
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(state.regions.len(), 1);
        assert_eq!(state.regions[0].1, RegionOrigin::Objects);
        assert_eq!(state.taken_objects.len(), 4);
        for side in &sides {
            assert!(state.taken_objects.contains(side));
        }
    }

    /// Two squares side by side; returns (square A, square B), both including
    /// the shared middle line.
    fn two_squares(app: &mut OpenCADStudio) -> (Vec<Handle>, Vec<Handle>) {
        let bottom_left = add_line(app, 0.0, 0.0, 10.0, 0.0);
        let bottom_right = add_line(app, 10.0, 0.0, 20.0, 0.0);
        let right = add_line(app, 20.0, 0.0, 20.0, 10.0);
        let top_right = add_line(app, 20.0, 10.0, 10.0, 10.0);
        let top_left = add_line(app, 10.0, 10.0, 0.0, 10.0);
        let left = add_line(app, 0.0, 10.0, 0.0, 0.0);
        let middle = add_line(app, 10.0, 0.0, 10.0, 10.0);
        (
            vec![bottom_left, top_left, left, middle],
            vec![bottom_right, right, top_right, middle],
        )
    }

    #[test]
    fn a_second_select_objects_starts_empty_and_reuses_already_taken_objects() {
        let mut app = new_app();
        let (square_a, square_b) = two_squares(&mut app);
        let far = add_line(&mut app, 100.0, 100.0, 110.0, 100.0);
        let i = app.active_tab;
        open_dialog(&mut app);

        // First round: A. The unrelated global selection is hidden meanwhile.
        app.tabs[i].scene.select_entity(far, false);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        assert!(app.tabs[i].scene.selected.is_empty(), "round 1 starts empty");
        choose_objects(&mut app, square_a.clone());
        assert_eq!(app.hatch_dialog.as_ref().unwrap().regions.len(), 1);
        assert!(app.tabs[i].scene.selected.contains(&far), "selection restored");

        // Second round: B shares the middle line already used by A.
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        assert!(
            app.tabs[i].scene.selected.is_empty(),
            "round 2 does not inherit the global selection"
        );
        choose_objects(&mut app, square_b);
        assert_eq!(
            app.hatch_dialog.as_ref().unwrap().regions.len(),
            2,
            "no filtering by already-used handles: both areas form"
        );

        // Third round: A again adds nothing.
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        choose_objects(&mut app, square_a);
        assert_eq!(app.hatch_dialog.as_ref().unwrap().regions.len(), 2);
    }

    #[test]
    fn the_same_area_by_points_and_by_objects_is_one_region() {
        // Points first, then objects.
        let mut app = new_app();
        let sides = add_rect(&mut app, 0.0, 0.0, 20.0, 10.0);
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        click_inside(&mut app);
        let _ = app.feed_command(StepInput::Enter);
        assert_eq!(app.hatch_dialog.as_ref().unwrap().regions.len(), 1);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        choose_objects(&mut app, sides);
        assert_eq!(app.hatch_dialog.as_ref().unwrap().regions.len(), 1);
        // And the other way round.
        let mut app = new_app();
        let sides = add_rect(&mut app, 0.0, 0.0, 20.0, 10.0);
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        choose_objects(&mut app, sides);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        click_inside(&mut app);
        let _ = app.feed_command(StepInput::Enter);
        assert_eq!(app.hatch_dialog.as_ref().unwrap().regions.len(), 1);
    }

    #[test]
    fn leaving_select_objects_puts_the_exact_previous_selection_back() {
        for (finish_with_escape, had_selection) in
            [(true, true), (false, true), (true, false), (false, false)]
        {
            let mut app = new_app();
            let keep = add_line(&mut app, 100.0, 100.0, 110.0, 100.0);
            let other = add_line(&mut app, 200.0, 100.0, 210.0, 100.0);
            let i = app.active_tab;
            if had_selection {
                app.tabs[i].scene.select_entity(keep, false);
            }
            open_dialog(&mut app);
            let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
            // The user picks something else while the round runs.
            app.tabs[i].scene.select_entity(other, false);
            let input = if finish_with_escape {
                StepInput::Escape
            } else {
                StepInput::Enter
            };
            let _ = app.feed_command(input);
            let selected = &app.tabs[i].scene.selected;
            assert!(!selected.contains(&other), "round pick must not leak");
            assert_eq!(selected.contains(&keep), had_selection);
            assert_eq!(selected.len(), usize::from(had_selection));
            assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        }
    }

    #[test]
    fn the_boundary_data_is_rebuilt_for_every_add() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        let _ = app.feed_command(StepInput::Escape);
        let before = app.hatch_dialog.as_ref().unwrap().boundary_sources.len();
        // The drawing changes while the dialog is open.
        add_rect(&mut app, 100.0, 0.0, 120.0, 10.0);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        click_at(&mut app, 110.0, 5.0);
        let _ = app.feed_command(StepInput::Enter);
        let state = app.hatch_dialog.as_ref().unwrap();
        assert_eq!(state.boundary_sources.len(), before + 4);
        assert_eq!(state.regions.len(), 1, "the new rectangle was seen");
    }

    /// A second drawing tab next to the first (the first stays active);
    /// returns (first_index, second_index).
    fn open_second_tab(app: &mut OpenCADStudio) -> (usize, usize) {
        let first = app.active_tab;
        let _ = app.push_test_document();
        let second = app.tabs.len() - 1;
        assert_ne!(first, second);
        (first, second)
    }

    fn start_flow(app: &mut OpenCADStudio, flow: &str) {
        match flow {
            "pick" => {
                let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
            }
            "select" => {
                let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
            }
            "preview" => {
                app.hatch_dialog.as_mut().unwrap().regions.push(region());
                let _ = app.update(Message::HatchDialogPreview);
            }
            "origin" => {
                let _ = app.update(Message::HatchDialogField(Field::OriginMode(
                    crate::modules::draw::draw::hatch_settings::OriginMode::Specified,
                )));
                let _ = app.update(Message::HatchDialogPickOrigin);
            }
            other => panic!("unknown flow {other}"),
        }
    }

    const FLOWS: [&str; 4] = ["pick", "select", "preview", "origin"];

    #[test]
    fn switching_tab_in_any_hidden_flow_abandons_it() {
        for flow in FLOWS {
            let mut app = app_with_rectangle();
            let (first, second) = open_second_tab(&mut app);
            let _ = app.update(Message::TabSwitch(first));
            open_dialog(&mut app);
            start_flow(&mut app, flow);
            assert!(app.tabs[first].active_cmd.is_some(), "{flow}: flow started");
            let _ = app.update(Message::TabSwitch(second));
            assert!(app.hatch_dialog.is_none(), "{flow}: state dropped");
            assert!(app.active_modal.is_none(), "{flow}: no dialog");
            assert!(app.tabs[first].active_cmd.is_none(), "{flow}: owner's command stopped");
            assert_eq!(hatch_count(&app), 0, "{flow}");
        }
    }

    #[test]
    fn closing_the_owner_tab_in_any_hidden_flow_abandons_it() {
        for flow in FLOWS {
            let mut app = app_with_rectangle();
            let (first, _second) = open_second_tab(&mut app);
            let _ = app.update(Message::TabSwitch(first));
            open_dialog(&mut app);
            start_flow(&mut app, flow);
            let id = app.tabs[first].id;
            let _ = app.update(Message::TabClose(id));
            assert!(app.hatch_dialog.is_none(), "{flow}");
            assert!(app.active_modal.is_none(), "{flow}");
        }
    }

    #[test]
    fn switching_tab_with_the_dialog_visible_closes_it() {
        let mut app = app_with_rectangle();
        let (first, second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        open_dialog(&mut app);
        let _ = app.update(Message::TabSwitch(second));
        assert!(app.hatch_dialog.is_none());
        assert!(app.active_modal.is_none());
    }

    #[test]
    fn selection_is_restored_when_a_select_round_is_abandoned() {
        let mut app = new_app();
        let (first, second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        let keep = add_line(&mut app, 100.0, 100.0, 110.0, 100.0);
        app.tabs[first].scene.select_entity(keep, false);
        let _ = app.dispatch_command("HATCH");
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        let _ = app.update(Message::TabSwitch(second));
        assert!(app.tabs[first].scene.selected.contains(&keep));
    }

    #[test]
    fn a_stale_collector_result_is_never_applied_to_another_tab() {
        let mut app = app_with_rectangle();
        let (first, second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        // The user somehow lands on the other tab with the result still in flight.
        app.active_tab = second;
        let _ = app.apply_cmd_result(crate::command::CmdResult::HatchBoundariesPicked {
            regions: vec![region()],
            objects: Vec::new(),
        });
        assert!(app.hatch_dialog.is_none());
        assert_eq!(hatch_count(&app), 0);
    }

    #[test]
    fn ok_on_another_tab_is_refused() {
        let mut app = app_with_rectangle();
        let (first, second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        open_dialog(&mut app);
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        app.active_tab = second;
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 0);
        assert!(app.hatch_dialog.is_none());
    }
}
