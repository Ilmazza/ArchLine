//! The HATCH dialog on the app side: opening it, collecting areas while it is
//! hidden, and turning its state into a hatch on OK.

use iced::Task;

use crate::app::{Message, ModalKind, OpenCADStudio};
use crate::command::{CadCommand, WorkingPlane};
use crate::modules::draw::draw::hatch::{object_regions, HatchCommand};
use crate::modules::draw::draw::hatch_settings::{add_region, OriginMode, RegionOrigin};
use crate::ui::window::hatch_dialog::{Field, State};
use crate::ui::window::hatch_palette::{Palette, PaletteAction, PALETTE_SEARCH_ID};
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
        // The dialog is the command: Enter / Space repeat it.
        self.tabs[i].last_cmd = Some("HATCH".to_string());
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
        // Enter / OK with the palette open means "use the chosen pattern".
        if self.hatch_palette_open() {
            return self.hatch_dialog_palette(PaletteAction::Apply);
        }
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
        let Some(command) = hatch_command_from_state(state, document_origin) else {
            return Task::none();
        };
        let settings = state.settings.clone();
        self.hatch_last = settings;
        self.hatch_dialog = None;
        if self.active_modal == Some(ModalKind::Hatch) {
            self.close_active_modal();
        }
        // Run it as the active command, like `-HATCH` does: the end-of-command
        // bookkeeping (nothing left selected, the hatch kept as "Previous")
        // only happens for a command that was running.
        let result = self.tabs[i]
            .active_cmd
            .insert(Box::new(command))
            .on_enter();
        self.apply_cmd_result(result)
    }

    /// The pattern palette behind "...": every action of it lands here.
    /// Browsing (tab, search, pick) never changes the pattern; only `Apply`
    /// does, through the same `Field::Pattern` the drop-down uses.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_palette(
        &mut self,
        action: PaletteAction,
    ) -> Task<Message> {
        let Some(state) = self.hatch_dialog.as_mut() else {
            return Task::none();
        };
        if matches!(action, PaletteAction::Open) {
            if state.palette.is_some() {
                return Task::none();
            }
            state.palette = Some(Palette::open(&state.settings.pattern));
            return iced::widget::operation::focus(iced::widget::Id::new(PALETTE_SEARCH_ID));
        }
        let Some(palette) = state.palette.as_mut() else {
            return Task::none();
        };
        match action {
            PaletteAction::Open => {}
            PaletteAction::Close => state.palette = None,
            PaletteAction::Tab(category) => palette.category = category,
            PaletteAction::Search(text) => palette.search = text,
            PaletteAction::Pick(name) => {
                if let Some(entry) = crate::scene::model::hatch_patterns::find(&name) {
                    palette.selected = Some(entry.name.clone());
                }
            }
            PaletteAction::Apply => {
                if let Some(name) = palette.selected.clone() {
                    state.apply(Field::Pattern(name));
                    state.palette = None;
                }
            }
        }
        Task::none()
    }

    /// Esc / Cancel: close the palette if it is open. `true` when it was, so
    /// the caller leaves the window alone.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_close_palette(&mut self) -> bool {
        self.hatch_dialog
            .as_mut()
            .is_some_and(|state| state.palette.take().is_some())
    }

    /// The palette hides the window's own controls: while it is open, what
    /// would act on the window (OK, Add, Preview, origin) must not run.
    pub(in crate::app) fn hatch_palette_open(&self) -> bool {
        self.hatch_dialog
            .as_ref()
            .is_some_and(|state| state.palette.is_some())
    }

    /// Cancel / X / Esc, and every abandonment (tab left or closed, another
    /// command started): drop the state, stop a hidden-step command in the
    /// owner tab, put the selection back and close the window.
    pub(in crate::app) fn hatch_dialog_cancel(&mut self) {
        if let Some(state) = self.hatch_dialog.take() {
            if let Some(index) = self.tabs.iter().position(|tab| tab.id == state.owner_tab_id) {
                // Only a command of the flow itself is stopped: a stale flow
                // marker must never take another command down with it.
                let hatch_runs = self.tabs[index]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.name() == "HATCH");
                if state.flow != crate::ui::window::hatch_dialog::Flow::None && hatch_runs {
                    self.tabs[index].active_cmd = None;
                    self.tabs[index].snap_result = None;
                    self.tabs[index].scene.clear_preview_wire();
                    self.restore_pre_cmd_tangent();
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

    /// A new command is about to replace the running one in tab `i`: when the
    /// running one is a hidden step of the HATCH dialog, the flow ends here, so
    /// no state outlives its collector.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_abandon_for_new_command(&mut self, i: usize) {
        let in_flow = self.hatch_dialog.as_ref().is_some_and(|state| {
            state.owner_tab_id == self.tabs[i].id
                && state.flow != crate::ui::window::hatch_dialog::Flow::None
        });
        if in_flow {
            self.hatch_dialog_cancel();
        }
    }

    /// The tab `tab_id` is going away without `on_tab_close` (a queued close
    /// answered with Discard): a flow it owned can never finish.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_abandon_for_tab(&mut self, tab_id: u64) {
        if self
            .hatch_dialog
            .as_ref()
            .is_some_and(|state| state.owner_tab_id == tab_id)
        {
            self.hatch_dialog_cancel();
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
        if self.hatch_palette_open() {
            return Task::none();
        }
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
        if self.hatch_palette_open() {
            return Task::none();
        }
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
        if self.hatch_palette_open() {
            return Task::none();
        }
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
    fn the_dialog_opens_and_creates_a_hatch_in_a_layout_too() {
        // Paper space has no UCS plane: the default plane is used.
        let mut app = new_app();
        let i = app.active_tab;
        app.tabs[i].scene.current_layout = "Layout1".to_string();
        assert!(!app.tabs[i].editing_model_space(), "really in paper space");
        for (x0, y0, x1, y1) in [
            (0.0, 0.0, 20.0, 0.0),
            (20.0, 0.0, 20.0, 10.0),
            (20.0, 10.0, 0.0, 10.0),
            (0.0, 10.0, 0.0, 0.0),
        ] {
            add_line(&mut app, x0, y0, x1, y1);
        }
        let _ = app.dispatch_command("HATCH");
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        let state = app.hatch_dialog.as_ref().unwrap();
        assert_eq!(state.plane.z, glam::DVec3::Z, "the default plane");
        let _ = app.update(Message::HatchDialogAdd(crate::ui::window::hatch_dialog::AddKind::Points));
        click_inside(&mut app);
        let _ = app.feed_command(crate::command::StepInput::Enter);
        assert_eq!(app.hatch_dialog.as_ref().unwrap().regions.len(), 1);
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 1);
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

    fn assert_runs_dialog_free(app: &mut OpenCADStudio, channel: &str) {
        let i = app.active_tab;
        assert!(app.active_modal.is_none(), "{channel}: no dialog");
        assert!(app.hatch_dialog.is_none(), "{channel}: no dialog state");
        let name = app.tabs[i].active_cmd.as_ref().map(|command| command.name());
        assert_eq!(name, Some("HATCH"), "{channel}: the command-line HATCH runs");
    }

    #[test]
    fn dash_hatch_starts_the_command_without_a_dialog() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("-HATCH");
        assert_runs_dialog_free(&mut app, "-HATCH");
    }

    #[test]
    fn scripted_dispatch_makes_hatch_dialog_free() {
        let mut app = app_with_rectangle();
        app.scripted_dispatch = true;
        let _ = app.dispatch_command("HATCH");
        app.scripted_dispatch = false;
        assert_runs_dialog_free(&mut app, "scripted_dispatch");
    }

    #[test]
    fn script_lines_run_hatch_without_a_dialog() {
        let mut app = app_with_rectangle();
        let _ = app.update(Message::ScriptLine("HATCH".into()));
        assert_runs_dialog_free(&mut app, "ScriptLine");
        assert!(!app.scripted_dispatch, "the flag is restored afterwards");
    }

    #[test]
    fn interactive_hatch_still_opens_the_dialog_after_a_script() {
        let mut app = app_with_rectangle();
        let _ = app.update(Message::ScriptLine("HATCH".into()));
        let _ = app.update(Message::CommandEscape);
        let _ = app.dispatch_command("HATCH");
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn the_flag_is_restored_when_a_channel_nests() {
        let mut app = app_with_rectangle();
        app.scripted_dispatch = true;
        let _ = app.update(Message::ScriptLine("HATCH".into()));
        assert!(app.scripted_dispatch, "an outer scripted run stays scripted");
        app.scripted_dispatch = false;
    }

    #[test]
    fn command_registration_lists_dash_hatch() {
        let registered = inventory::iter::<crate::command::CommandRegistration>()
            .any(|registration| registration.names.contains(&"-HATCH"));
        assert!(registered);
    }

    /// The headless `--serve` op `run` without `"protocol"` (stdin and TCP) is a
    /// scripted channel too: nobody is there to answer a dialog.
    #[test]
    fn headless_serve_run_hatch_has_no_dialog() {
        let mut app = app_with_rectangle();
        let reply = app.automation_op(r#"{"op":"run","cmd":"HATCH"}"#);
        assert_ne!(reply["ok"], serde_json::json!(false), "{reply}");
        assert_runs_dialog_free(&mut app, "automation legacy run");
        assert!(!app.scripted_dispatch, "the flag is restored afterwards");
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

    /// A short line in tab `tab` (not the active one necessarily), selected.
    fn add_selected_line(app: &mut OpenCADStudio, tab: usize, y: f64) -> Handle {
        let handle = app.tabs[tab]
            .scene
            .add_entity(codec::EntityType::Line(codec::entities::Line::from_points(
                codec::types::Vector3::new(100.0, y, 0.0),
                codec::types::Vector3::new(110.0, y, 0.0),
            )));
        app.tabs[tab].scene.select_entity(handle, false);
        handle
    }

    fn hatches_in(app: &OpenCADStudio, tab: usize) -> usize {
        app.tabs[tab].scene.hatches.len()
    }

    #[test]
    fn switching_tab_in_any_hidden_flow_abandons_it() {
        for flow in FLOWS {
            let mut app = app_with_rectangle();
            let (first, second) = open_second_tab(&mut app);
            let _ = app.update(Message::TabSwitch(first));
            // A selection that encloses nothing: no region is seeded from it.
            let keep = add_selected_line(&mut app, first, 100.0);
            let other = add_selected_line(&mut app, second, 200.0);
            open_dialog(&mut app);
            start_flow(&mut app, flow);
            assert!(app.tabs[first].active_cmd.is_some(), "{flow}: flow started");
            if flow == "select" {
                assert!(
                    app.selected_handles(first).is_empty(),
                    "{flow}: a Select round starts from an empty selection"
                );
            } else {
                assert_eq!(app.selected_handles(first), vec![keep], "{flow}: untouched mid-flow");
            }
            let _ = app.update(Message::TabSwitch(second));
            assert!(app.hatch_dialog.is_none(), "{flow}: state dropped");
            assert!(app.active_modal.is_none(), "{flow}: no dialog");
            assert!(app.tabs[first].active_cmd.is_none(), "{flow}: owner's command stopped");
            assert_eq!(hatches_in(&app, first), 0, "{flow}: owner");
            assert_eq!(hatches_in(&app, second), 0, "{flow}: other tab");
            assert_eq!(hatch_count(&app), 0, "{flow}: active tab");
            assert_eq!(app.selected_handles(first), vec![keep], "{flow}: owner selection");
            assert_eq!(app.selected_handles(second), vec![other], "{flow}: other selection");
        }
    }

    #[test]
    fn closing_the_owner_tab_in_any_hidden_flow_abandons_it() {
        for flow in FLOWS {
            let mut app = app_with_rectangle();
            let (first, second) = open_second_tab(&mut app);
            let survivor_id = app.tabs[second].id;
            let survivor_line = add_selected_line(&mut app, second, 200.0);
            let _ = app.update(Message::TabSwitch(first));
            add_selected_line(&mut app, first, 100.0);
            open_dialog(&mut app);
            start_flow(&mut app, flow);
            assert!(app.tabs[first].active_cmd.is_some(), "{flow}: flow started");
            let id = app.tabs[first].id;
            assert!(!app.tabs[first].dirty, "{flow}: a dirty tab would only ask to save");
            let tabs_before = app.tabs.len();
            let _ = app.update(Message::TabClose(id));
            assert_eq!(app.tabs.len(), tabs_before - 1, "{flow}: tab closed");
            assert!(app.tabs.iter().all(|tab| tab.id != id), "{flow}: owner gone");
            assert!(app.hatch_dialog.is_none(), "{flow}");
            assert!(app.active_modal.is_none(), "{flow}");
            let survivor = app
                .tabs
                .iter()
                .position(|tab| tab.id == survivor_id)
                .expect("the other tab survives");
            assert!(app.tabs[survivor].active_cmd.is_none(), "{flow}: survivor has no command");
            assert_eq!(hatches_in(&app, survivor), 0, "{flow}: survivor hatches");
            assert_eq!(
                app.selected_handles(survivor),
                vec![survivor_line],
                "{flow}: survivor selection"
            );
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
        assert_eq!(hatches_in(&app, first), 0);
        assert!(app.tabs[first].active_cmd.is_none(), "the owner's collector is stopped");
        assert!(app.active_modal.is_none());
    }

    #[test]
    fn a_stale_dispatch_return_is_discarded_without_touching_the_active_tab() {
        let cases = [
            "HATCH_PICK_CANCELLED",
            "HATCH_PREVIEW_DONE",
            "HATCH_ORIGIN_PICKED 5 5 0",
        ];
        for text in cases {
            let mut app = app_with_rectangle();
            let (first, second) = open_second_tab(&mut app);
            let _ = app.update(Message::TabSwitch(first));
            open_dialog(&mut app);
            start_flow(&mut app, "pick");
            assert!(app.tabs[first].active_cmd.is_some(), "{text}: flow started");
            let hatch_origin = app.tabs[second].scene.document.hatch_origin();
            app.active_tab = second;
            let _ = app.dispatch_command(text);
            assert!(app.hatch_dialog.is_none(), "{text}: state discarded");
            assert!(app.active_modal.is_none(), "{text}: dialog not reopened");
            assert!(app.tabs[first].active_cmd.is_none(), "{text}: owner's command stopped");
            assert!(app.tabs[second].active_cmd.is_none(), "{text}");
            assert_eq!(hatches_in(&app, first), 0, "{text}");
            assert_eq!(hatches_in(&app, second), 0, "{text}");
            assert_eq!(
                app.tabs[second].scene.document.hatch_origin(),
                hatch_origin,
                "{text}: active document untouched"
            );
        }
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

    fn command_name(app: &OpenCADStudio, tab: usize) -> Option<&'static str> {
        app.tabs[tab].active_cmd.as_ref().map(|command| command.name())
    }

    #[test]
    fn starting_another_command_in_a_hidden_flow_abandons_it() {
        for flow in FLOWS {
            let mut app = new_app();
            // Two lines that enclose nothing: the user's own selection.
            let a = add_line(&mut app, 100.0, 100.0, 110.0, 100.0);
            let b = add_line(&mut app, 100.0, 120.0, 110.0, 120.0);
            let i = app.active_tab;
            app.tabs[i].scene.select_entity(a, false);
            app.tabs[i].scene.select_entity(b, false);
            open_dialog(&mut app);
            start_flow(&mut app, flow);
            assert!(app.tabs[i].active_cmd.is_some(), "{flow}: flow started");
            let _ = app.dispatch_command("LINE");
            assert!(app.hatch_dialog.is_none(), "{flow}: state discarded");
            assert!(app.active_modal.is_none(), "{flow}: no dialog");
            assert_eq!(command_name(&app, i), Some("LINE"), "{flow}: LINE runs");
            let mut selected = app.selected_handles(i);
            selected.sort();
            let mut expected = vec![a, b];
            expected.sort();
            assert_eq!(selected, expected, "{flow}: selection put back");
        }
    }

    #[test]
    fn a_hatch_after_an_interrupted_select_round_keeps_its_own_preselection() {
        let mut app = new_app();
        let a = add_line(&mut app, 100.0, 100.0, 110.0, 100.0);
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(a, false);
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        let _ = app.dispatch_command("LINE");
        let _ = app.update(Message::CommandEscape);
        // The user now selects a closed rectangle and starts HATCH again.
        app.tabs[i].scene.deselect_all();
        for handle in add_rect(&mut app, 0.0, 0.0, 20.0, 10.0) {
            app.tabs[i].scene.select_entity(handle, false);
        }
        let _ = app.dispatch_command("HATCH");
        let state = app.hatch_dialog.as_ref().expect("the dialog opened");
        assert_eq!(state.regions.len(), 1, "the new preselection seeds the dialog");
        assert_eq!(app.selected_handles(i).len(), 4, "and is still selected");
        assert!(!app.selected_handles(i).contains(&a), "the old selection did not come back");
    }

    #[test]
    fn a_tab_switch_after_an_interrupted_flow_leaves_the_new_command_alone() {
        let mut app = app_with_rectangle();
        let (first, second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        let _ = app.dispatch_command("LINE");
        let _ = app.update(Message::TabSwitch(second));
        assert_eq!(command_name(&app, first), Some("LINE"), "the LINE of the owner tab survives");
    }

    #[test]
    fn cancelling_a_flow_only_stops_a_hatch_command() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let i = app.active_tab;
        // A stale flow marker with an unrelated command in the owner tab.
        app.hatch_dialog.as_mut().unwrap().flow = Flow::Pick;
        app.set_active_command(
            i,
            Box::new(crate::modules::draw::draw::line::LineCommand::new()),
        );
        app.hatch_dialog_cancel();
        assert_eq!(command_name(&app, i), Some("LINE"));
    }

    #[test]
    fn closing_all_tabs_discarding_a_dirty_owner_abandons_its_flow() {
        let mut app = app_with_rectangle();
        let (first, _second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Objects));
        app.tabs[first].dirty = true;
        let owner_id = app.tabs[first].id;
        let _ = app.update(Message::DocTabCloseAll);
        assert!(app.tabs.iter().any(|tab| tab.id == owner_id), "waiting for the answer");
        let _ = app.update(Message::UnsavedDialogDiscard);
        assert!(app.tabs.iter().all(|tab| tab.id != owner_id), "owner tab discarded");
        assert!(app.hatch_dialog.is_none(), "the flow went with its tab");
        assert!(app.active_modal.is_none());
    }

    #[test]
    fn the_last_tab_discarded_while_in_a_flow_abandons_it_too() {
        let mut app = app_with_rectangle();
        assert_eq!(app.tabs.len(), 1);
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        app.tabs[0].dirty = true;
        let _ = app.update(Message::DocTabCloseAll);
        let _ = app.update(Message::UnsavedDialogDiscard);
        assert!(app.hatch_dialog.is_none());
    }

    #[test]
    fn ok_leaves_nothing_selected_and_remembers_the_hatch_as_previous() {
        let mut app = app_with_rectangle();
        let i = app.active_tab;
        open_dialog(&mut app);
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 1);
        let hatch = *app.tabs[i].scene.hatches.keys().next().unwrap();
        assert!(app.tabs[i].scene.selected.is_empty(), "nothing stays selected");
        assert_eq!(app.tabs[i].prev_selection, vec![hatch], "available as Previous");
        assert!(app.tabs[i].active_cmd.is_none());
    }

    #[test]
    fn a_second_hatch_after_ok_does_not_seed_from_the_first_hatch() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogOk);
        open_dialog(&mut app);
        let state = app.hatch_dialog.as_ref().unwrap();
        assert!(state.regions.is_empty());
        assert!(state.taken_objects.is_empty());
    }

    #[test]
    fn separate_hatches_all_end_unselected() {
        let mut app = new_app();
        add_rect(&mut app, 0.0, 0.0, 10.0, 10.0);
        add_rect(&mut app, 20.0, 0.0, 30.0, 10.0);
        let i = app.active_tab;
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogField(Field::Separate(true)));
        for x in [5.0, 25.0] {
            let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
            click_at(&mut app, x, 5.0);
            let _ = app.feed_command(StepInput::Enter);
        }
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 2);
        assert!(app.tabs[i].scene.selected.is_empty(), "no grips left on the last hatch");
        // The commit handler selects each new hatch exclusively (as with
        // -HATCH), so "Previous" holds the last one.
        let previous = app.tabs[i].prev_selection.clone();
        assert_eq!(previous.len(), 1);
        assert!(app.tabs[i].scene.hatches.contains_key(&previous[0]));
    }

    #[test]
    fn opening_the_dialog_makes_hatch_the_command_to_repeat() {
        let mut app = app_with_rectangle();
        let i = app.active_tab;
        let _ = app.dispatch_command("LINE");
        let _ = app.update(Message::CommandEscape);
        assert_eq!(app.tabs[i].last_cmd.as_deref(), Some("LINE"));
        let _ = app.dispatch_command("HATCH");
        assert_eq!(app.tabs[i].last_cmd.as_deref(), Some("HATCH"));
    }

    #[test]
    fn cancelling_a_flow_clears_the_snap_result() {
        let mut app = app_with_rectangle();
        let i = app.active_tab;
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        app.tabs[i].snap_result = Some(crate::snap::SnapResult {
            world: glam::DVec3::ZERO,
            model_point: None,
            screen: iced::Point::ORIGIN,
            snap_type: crate::snap::SnapType::Endpoint,
            tangent_obj: None,
            extension_base: None,
            extension_base2: None,
            extension_origin: None,
            extension_dir: None,
            viewport: None,
            source: None,
            secondary_source: None,
        });
        app.hatch_dialog_cancel();
        assert!(app.tabs[i].snap_result.is_none());
    }

    // ── Spec §10: undo, origin, entry points ───────────────────────────────

    fn entity_count(app: &OpenCADStudio) -> usize {
        app.tabs[app.active_tab].scene.document.entities().count()
    }

    #[test]
    fn one_undo_removes_everything_ok_created() {
        for (case, hatches, extra_boundaries) in [
            ("associative", 1, false),
            ("separate", 2, false),
            ("retain", 1, true),
        ] {
            let mut app = new_app();
            add_rect(&mut app, 0.0, 0.0, 10.0, 10.0);
            add_rect(&mut app, 20.0, 0.0, 30.0, 10.0);
            let before = entity_count(&app);
            open_dialog(&mut app);
            match case {
                "separate" => {
                    let _ = app.update(Message::HatchDialogField(Field::Separate(true)));
                }
                "retain" => {
                    let _ = app.update(Message::HatchDialogField(Field::Retain(true)));
                }
                _ => {}
            }
            for x in [5.0, 25.0] {
                let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
                click_at(&mut app, x, 5.0);
                let _ = app.feed_command(StepInput::Enter);
            }
            let _ = app.update(Message::HatchDialogOk);
            assert_eq!(hatch_count(&app), hatches, "{case}: hatches created");
            if extra_boundaries {
                assert!(entity_count(&app) > before + 1, "{case}: the outlines were kept");
            }
            let _ = app.update(Message::Undo);
            assert_eq!(hatch_count(&app), 0, "{case}: one undo removes the hatches");
            assert_eq!(entity_count(&app), before, "{case}: and everything else OK made");
        }
    }

    fn tilted_ucs(y: f64) -> codec::tables::Ucs {
        // The XZ plane at height y: local x along world X, local y along world Z.
        let mut ucs = codec::tables::Ucs::new("*ACTIVE*");
        ucs.origin = codec::types::Vector3::new(0.0, y, 0.0);
        ucs.x_axis = codec::types::Vector3::new(1.0, 0.0, 0.0);
        ucs.y_axis = codec::types::Vector3::new(0.0, 0.0, 1.0);
        ucs
    }

    fn add_line_3d(app: &mut OpenCADStudio, from: [f64; 3], to: [f64; 3]) {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Line(codec::entities::Line::from_points(
                codec::types::Vector3::new(from[0], from[1], from[2]),
                codec::types::Vector3::new(to[0], to[1], to[2]),
            )));
    }

    /// A drawing with a 20 x 10 rectangle in the XZ plane at y = 5 and a
    /// working plane (UCS) sitting on it.
    fn app_on_a_tilted_plane() -> OpenCADStudio {
        let mut app = new_app();
        let i = app.active_tab;
        app.tabs[i].active_ucs = Some(tilted_ucs(5.0));
        add_line_3d(&mut app, [0.0, 5.0, 0.0], [20.0, 5.0, 0.0]);
        add_line_3d(&mut app, [20.0, 5.0, 0.0], [20.0, 5.0, 10.0]);
        add_line_3d(&mut app, [20.0, 5.0, 10.0], [0.0, 5.0, 10.0]);
        add_line_3d(&mut app, [0.0, 5.0, 10.0], [0.0, 5.0, 0.0]);
        app
    }

    /// Add the rectangle by a click inside it (a world point on the tilted plane).
    fn pick_the_tilted_rectangle(app: &mut OpenCADStudio) {
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        let i = app.active_tab;
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("a collector is running")
            .on_point(glam::DVec3::new(10.0, 5.0, 5.0));
        let _ = app.apply_cmd_result(result);
        let _ = app.feed_command(StepInput::Enter);
        assert_eq!(
            app.hatch_dialog.as_ref().unwrap().regions.len(),
            1,
            "the rectangle was found"
        );
    }

    /// Where the pattern is anchored in the hatch that was saved in the
    /// drawing, in working-plane coordinates: what the file keeps of the origin.
    fn saved_pattern_base(app: &OpenCADStudio) -> [f64; 2] {
        let hatches: Vec<_> = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                codec::EntityType::Hatch(hatch) => Some(hatch),
                _ => None,
            })
            .collect();
        assert_eq!(hatches.len(), 1, "one hatch in the drawing");
        let base = hatches[0].pattern.lines[0].base_point;
        [base.x, base.y]
    }

    /// What the command-line HATCH saves on the same drawing when the drawing
    /// origin is `origin`.
    fn command_line_pattern_base(origin: [f64; 2]) -> [f64; 2] {
        let mut app = app_on_a_tilted_plane();
        let i = app.active_tab;
        assert!(app.tabs[i].scene.document.set_hatch_origin(origin));
        let _ = app.dispatch_command("-HATCH");
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("-HATCH runs")
            .on_point(glam::DVec3::new(10.0, 5.0, 5.0));
        let _ = app.apply_cmd_result(result);
        let _ = app.feed_command(StepInput::Enter);
        saved_pattern_base(&app)
    }

    fn assert_close(got: [f64; 2], expected: [f64; 2], label: &str) {
        assert!(
            (got[0] - expected[0]).abs() < 1.0e-4 && (got[1] - expected[1]).abs() < 1.0e-4,
            "{label}: {got:?} is not {expected:?}"
        );
    }

    #[test]
    fn a_specified_origin_reaches_the_hatch_on_a_rotated_plane() {
        let mut app = app_on_a_tilted_plane();
        open_dialog(&mut app);
        let plane = app.hatch_dialog.as_ref().unwrap().plane;
        assert!(plane.z.dot(glam::DVec3::Z).abs() < 1.0e-9, "the plane is not the identity");
        let _ = app.update(Message::HatchDialogField(Field::OriginMode(
            crate::modules::draw::draw::hatch_settings::OriginMode::Specified,
        )));
        app.hatch_dialog.as_mut().unwrap().specified_origin = Some([3.0, 4.0]);
        pick_the_tilted_rectangle(&mut app);
        let _ = app.update(Message::HatchDialogOk);
        let got = saved_pattern_base(&app);
        assert_close(got, [3.0, 4.0], "the specified origin");
        assert_close(got, command_line_pattern_base([3.0, 4.0]), "as the command line");
        assert_ne!(
            got,
            command_line_pattern_base([0.0, 0.0]),
            "the default origin gives another phase, or the test proves nothing"
        );
    }

    #[test]
    fn use_current_origin_is_read_again_from_the_drawing_at_ok() {
        let mut app = app_on_a_tilted_plane();
        let i = app.active_tab;
        open_dialog(&mut app);
        pick_the_tilted_rectangle(&mut app);
        // The drawing origin changes after the dialog opened and after the Add.
        assert!(app.tabs[i].scene.document.set_hatch_origin([7.0, 8.0]));
        let _ = app.update(Message::HatchDialogOk);
        let got = saved_pattern_base(&app);
        assert_close(got, [7.0, 8.0], "the current origin");
        assert_close(got, command_line_pattern_base([7.0, 8.0]), "as the command line");
    }

    #[test]
    fn specified_mode_without_a_picked_point_falls_back_to_the_current_origin() {
        let mut app = app_on_a_tilted_plane();
        let i = app.active_tab;
        open_dialog(&mut app);
        let _ = app.update(Message::HatchDialogField(Field::OriginMode(
            crate::modules::draw::draw::hatch_settings::OriginMode::Specified,
        )));
        pick_the_tilted_rectangle(&mut app);
        assert!(app.tabs[i].scene.document.set_hatch_origin([7.0, 8.0]));
        let _ = app.update(Message::HatchDialogOk);
        assert_close(saved_pattern_base(&app), [7.0, 8.0], "falls back to the current origin");
    }

    #[test]
    fn enter_with_an_invalid_field_does_nothing() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        let _ = app.update(Message::HatchDialogField(Field::Angle("x".into())));
        let _ = app.update(Message::CommandFinalize);
        assert_eq!(hatch_count(&app), 0);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch), "still open");
        assert!(app.hatch_dialog.is_some());
    }

    #[test]
    fn enter_with_no_area_does_nothing() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        let _ = app.update(Message::CommandFinalize);
        assert_eq!(hatch_count(&app), 0);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn the_ribbon_hatch_button_opens_the_dialog() {
        use crate::modules::ModuleEvent;
        let mut app = app_with_rectangle();
        let _ = app.update(Message::RibbonToolClick {
            tool_id: "HATCH".to_string(),
            event: ModuleEvent::Command("HATCH".to_string()),
        });
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert!(app.hatch_dialog.is_some());
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
    }

    // ── Pattern palette ("...") ────────────────────────────────────────────

    use crate::ui::window::hatch_palette::{category_of, Palette, PaletteAction, PatternCategory};

    fn palette_of(app: &OpenCADStudio) -> Option<&Palette> {
        app.hatch_dialog.as_ref().and_then(|state| state.palette.as_ref())
    }

    fn palette_do(app: &mut OpenCADStudio, action: PaletteAction) {
        let _ = app.update(Message::HatchDialogPalette(action));
    }

    fn pattern_of(app: &OpenCADStudio) -> String {
        app.hatch_dialog.as_ref().unwrap().settings.pattern.clone()
    }

    /// The dialog open with one area collected and the palette open on it.
    fn app_with_palette() -> OpenCADStudio {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        palette_do(&mut app, PaletteAction::Open);
        app
    }

    #[test]
    fn the_palette_is_closed_when_the_dialog_opens() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        assert!(palette_of(&app).is_none());
    }

    #[test]
    fn open_shows_the_tab_and_card_of_the_current_pattern() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        let _ = app.update(Message::HatchDialogField(Field::Pattern("brick".into())));
        palette_do(&mut app, PaletteAction::Open);
        let palette = palette_of(&app).expect("the palette is open");
        assert_eq!(palette.category, PatternCategory::Other);
        assert_eq!(palette.selected.as_deref(), Some("BRICK"));
        assert!(palette.search.is_empty());
        assert_eq!(app.active_modal, Some(ModalKind::Hatch), "same window");
    }

    #[test]
    fn open_on_a_pattern_missing_from_the_catalog_starts_on_ansi_with_no_card() {
        let mut app = app_with_rectangle();
        app.hatch_last.pattern = "NO_SUCH_PATTERN".into();
        let _ = app.dispatch_command("HATCH");
        palette_do(&mut app, PaletteAction::Open);
        let palette = palette_of(&app).expect("the button works with a bad pattern too");
        assert_eq!(palette.category, PatternCategory::Ansi);
        assert!(palette.selected.is_none());
    }

    #[test]
    fn open_again_keeps_what_was_already_chosen() {
        let mut app = app_with_palette();
        palette_do(&mut app, PaletteAction::Pick("BRICK".into()));
        palette_do(&mut app, PaletteAction::Open);
        assert_eq!(palette_of(&app).unwrap().selected.as_deref(), Some("BRICK"));
    }

    #[test]
    fn tab_and_search_change_the_view_but_not_the_selection() {
        let mut app = app_with_palette();
        let before = palette_of(&app).unwrap().selected.clone();
        assert_eq!(before.as_deref(), Some("ANSI31"));
        palette_do(&mut app, PaletteAction::Tab(PatternCategory::Iso));
        assert_eq!(palette_of(&app).unwrap().category, PatternCategory::Iso);
        assert_eq!(palette_of(&app).unwrap().selected, before);
        palette_do(&mut app, PaletteAction::Search("brick".into()));
        assert_eq!(palette_of(&app).unwrap().search, "brick");
        assert_eq!(palette_of(&app).unwrap().selected, before);
        assert_eq!(pattern_of(&app), "ANSI31", "nothing is applied by browsing");
    }

    #[test]
    fn pick_selects_without_applying() {
        let mut app = app_with_palette();
        palette_do(&mut app, PaletteAction::Pick("ISO02W100".into()));
        assert_eq!(
            palette_of(&app).unwrap().selected.as_deref(),
            Some("ISO02W100")
        );
        assert_eq!(pattern_of(&app), "ANSI31");
    }

    #[test]
    fn pick_of_a_name_that_is_not_in_the_catalog_is_ignored() {
        let mut app = app_with_palette();
        palette_do(&mut app, PaletteAction::Pick("NO_SUCH_PATTERN".into()));
        assert_eq!(palette_of(&app).unwrap().selected.as_deref(), Some("ANSI31"));
    }

    #[test]
    fn apply_sets_the_pattern_and_closes_the_palette() {
        let mut app = app_with_palette();
        palette_do(&mut app, PaletteAction::Pick("BRICK".into()));
        palette_do(&mut app, PaletteAction::Apply);
        assert_eq!(pattern_of(&app), "BRICK");
        assert!(palette_of(&app).is_none());
        assert_eq!(app.active_modal, Some(ModalKind::Hatch), "the window stays");
        assert_eq!(hatch_count(&app), 0, "nothing is created");
    }

    #[test]
    fn apply_with_nothing_selected_does_nothing() {
        let mut app = app_with_rectangle();
        app.hatch_last.pattern = "NO_SUCH_PATTERN".into();
        let _ = app.dispatch_command("HATCH");
        palette_do(&mut app, PaletteAction::Open);
        palette_do(&mut app, PaletteAction::Apply);
        assert!(palette_of(&app).is_some(), "still open");
        assert_eq!(pattern_of(&app), "NO_SUCH_PATTERN");
    }

    #[test]
    fn close_leaves_the_pattern_alone() {
        let mut app = app_with_palette();
        palette_do(&mut app, PaletteAction::Pick("BRICK".into()));
        palette_do(&mut app, PaletteAction::Close);
        assert!(palette_of(&app).is_none());
        assert_eq!(pattern_of(&app), "ANSI31");
        assert!(app.hatch_dialog.is_some());
    }

    #[test]
    fn actions_without_an_open_palette_do_nothing() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        for action in [
            PaletteAction::Close,
            PaletteAction::Tab(PatternCategory::Iso),
            PaletteAction::Search("x".into()),
            PaletteAction::Pick("BRICK".into()),
            PaletteAction::Apply,
        ] {
            palette_do(&mut app, action);
            assert!(palette_of(&app).is_none());
            assert_eq!(pattern_of(&app), "ANSI31");
        }
        // and with no dialog at all
        let mut app = new_app();
        palette_do(&mut app, PaletteAction::Open);
        assert!(app.hatch_dialog.is_none());
    }

    #[test]
    fn escape_with_the_palette_open_closes_only_the_palette() {
        let mut app = app_with_palette();
        palette_do(&mut app, PaletteAction::Pick("BRICK".into()));
        let _ = app.update(Message::CloseModal);
        assert!(palette_of(&app).is_none());
        assert!(app.hatch_dialog.is_some(), "the Hatch window is still there");
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(pattern_of(&app), "ANSI31", "Esc applies nothing");
        // the second Esc closes the window as before
        let _ = app.update(Message::CloseModal);
        assert!(app.hatch_dialog.is_none());
        assert!(app.active_modal.is_none());
    }

    #[test]
    fn the_keyboard_escape_takes_the_same_path() {
        let mut app = app_with_palette();
        let _ = app.update(Message::CommandEscape);
        assert!(palette_of(&app).is_none());
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        let _ = app.update(Message::CommandEscape);
        assert!(app.hatch_dialog.is_none());
    }

    #[test]
    fn escape_without_the_palette_still_closes_the_window() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        let _ = app.update(Message::CloseModal);
        assert!(app.hatch_dialog.is_none());
        assert!(app.active_modal.is_none());
    }

    #[test]
    fn enter_with_the_palette_open_applies_it_and_creates_nothing() {
        let mut app = app_with_palette();
        palette_do(&mut app, PaletteAction::Pick("BRICK".into()));
        let _ = app.update(Message::CommandFinalize);
        assert_eq!(pattern_of(&app), "BRICK");
        assert!(palette_of(&app).is_none());
        assert!(app.hatch_dialog.is_some(), "the window stays open");
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(hatch_count(&app), 0, "Enter was not the window's OK");
        // the next Enter is the window's OK
        let _ = app.update(Message::CommandFinalize);
        assert_eq!(hatch_count(&app), 1);
    }

    #[test]
    fn enter_with_the_palette_open_and_nothing_selected_does_nothing() {
        let mut app = app_with_rectangle();
        app.hatch_last.pattern = "NO_SUCH_PATTERN".into();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        palette_do(&mut app, PaletteAction::Open);
        let _ = app.update(Message::CommandFinalize);
        assert!(palette_of(&app).is_some());
        assert_eq!(hatch_count(&app), 0);
        assert!(app.hatch_dialog.is_some());
    }

    #[test]
    fn applying_a_pattern_over_a_missing_one_makes_the_window_usable_again() {
        let mut app = app_with_rectangle();
        app.hatch_last.pattern = "NO_SUCH_PATTERN".into();
        let _ = app.dispatch_command("HATCH");
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        {
            let state = app.hatch_dialog.as_ref().unwrap();
            assert!(state.pattern_message().is_some());
            assert!(!state.can_ok());
            assert!(!state.fields_valid());
        }
        palette_do(&mut app, PaletteAction::Open);
        palette_do(&mut app, PaletteAction::Pick("ANSI31".into()));
        palette_do(&mut app, PaletteAction::Apply);
        let state = app.hatch_dialog.as_ref().unwrap();
        assert!(state.pattern_message().is_none());
        assert!(state.fields_valid(), "Add and Preview are back");
        assert!(state.can_ok(), "OK is back");
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 1);
    }

    #[test]
    fn the_palette_does_not_touch_the_remembered_settings() {
        let mut app = app_with_palette();
        let before = app.hatch_last.clone();
        palette_do(&mut app, PaletteAction::Pick("BRICK".into()));
        palette_do(&mut app, PaletteAction::Apply);
        assert_eq!(app.hatch_last, before, "only Add / OK remember");
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(app.hatch_last.pattern, "BRICK");
    }

    #[test]
    fn cancelling_the_window_discards_the_palette_with_it() {
        let mut app = app_with_palette();
        palette_do(&mut app, PaletteAction::Pick("BRICK".into()));
        app.hatch_dialog_cancel();
        assert!(app.hatch_dialog.is_none());
        assert!(app.active_modal.is_none());
        let _ = app.dispatch_command("HATCH");
        assert!(palette_of(&app).is_none(), "a new window starts closed");
        assert_eq!(pattern_of(&app), "ANSI31");
    }

    #[test]
    fn switching_tab_with_the_palette_open_discards_everything() {
        let mut app = app_with_rectangle();
        let (first, second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        open_dialog(&mut app);
        palette_do(&mut app, PaletteAction::Open);
        assert!(palette_of(&app).is_some());
        let _ = app.update(Message::TabSwitch(second));
        assert!(app.hatch_dialog.is_none());
        assert!(app.active_modal.is_none());
    }

    #[test]
    fn the_windows_other_buttons_cannot_fire_behind_the_palette() {
        use crate::ui::window::hatch_dialog::{AddKind, Flow};
        for message in [
            Message::HatchDialogAdd(AddKind::Points),
            Message::HatchDialogAdd(AddKind::Objects),
            Message::HatchDialogPreview,
            Message::HatchDialogPickOrigin,
        ] {
            let mut app = app_with_palette();
            let _ = app.update(message);
            let i = app.active_tab;
            assert!(app.tabs[i].active_cmd.is_none(), "no hidden step started");
            assert_eq!(app.active_modal, Some(ModalKind::Hatch));
            assert_eq!(app.hatch_dialog.as_ref().unwrap().flow, Flow::None);
            assert!(palette_of(&app).is_some(), "the palette is untouched");
            assert_eq!(hatch_count(&app), 0);
        }
        // OK: the palette answers it, no hatch is made
        let mut app = app_with_palette();
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(hatch_count(&app), 0);
        assert!(app.hatch_dialog.is_some());
        assert!(palette_of(&app).is_none());
    }

    #[test]
    fn every_catalog_pattern_can_be_picked_and_applied() {
        // Whatever card is chosen, the window ends up exactly as if the
        // drop-down had chosen it.
        for entry in crate::scene::model::hatch_patterns::catalog() {
            let mut app = app_with_palette();
            palette_do(&mut app, PaletteAction::Tab(category_of(&entry.name)));
            palette_do(&mut app, PaletteAction::Pick(entry.name.clone()));
            palette_do(&mut app, PaletteAction::Apply);
            assert_eq!(pattern_of(&app), entry.name);
            assert!(app.hatch_dialog.as_ref().unwrap().can_ok(), "{}", entry.name);
        }
    }
}
