//! The HATCH dialog on the app side: opening it, collecting areas while it is
//! hidden, and turning its state into a hatch on OK.

use iced::Task;

use crate::app::{Message, ModalKind, OpenCADStudio};
use crate::command::{CadCommand, WorkingPlane};
use crate::modules::draw::draw::hatch::{object_regions, HatchCommand};
use crate::modules::draw::draw::hatch_settings::{add_region, FillTab, OriginMode, RegionOrigin};
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

/// The plane a hatch lies in: where HATCHEDIT measures its pattern origin.
fn hatch_plane(hatch: &codec::entities::Hatch) -> WorkingPlane {
    let storage = crate::entities::curve::ocs_plane(hatch.normal, hatch.elevation);
    WorkingPlane::new(
        glam::DVec3::from_array(storage.origin),
        glam::DVec3::from_array(storage.x_axis),
        glam::DVec3::from_array(storage.y_axis),
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

    /// `HATCH` / `GRADIENT`: open the dialog on the active drawing, on `tab`.
    /// The remembered settings fill both tabs; the command decides which one
    /// shows.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_open(&mut self, tab: FillTab) -> Task<Message> {
        // A dialog left over from an abandoned flow must not leak into this one.
        self.hatch_dialog_cancel();
        let i = self.active_tab;
        let (plane, boundary_sources, outlines) = self.hatch_boundary_context(i);
        let selected = self.selected_handles(i);
        let mut settings = self.hatch_last.clone();
        settings.tab = tab;
        let mut state = State::new(self.tabs[i].id, plane, outlines, boundary_sources, settings);
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
        // The dialog is the command: Enter / Space repeat it, on the same tab.
        let command = match tab {
            FillTab::Hatch => "HATCH",
            FillTab::Gradient => "GRADIENT",
        };
        self.tabs[i].last_cmd = Some(command.to_string());
        Task::none()
    }

    /// Double-click on a hatch and HATCHEDIT on a selected one: open the window
    /// on that hatch ("Hatch Edit"), whatever its fill (pattern, solid or
    /// gradient): the window opens on the tab of its kind.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_open_edit(&mut self, handle: Handle) -> Task<Message> {
        use crate::modules::draw::draw::hatch_edit_settings::EditTarget;
        // A window left over from an abandoned flow must not leak into this one.
        self.hatch_dialog_cancel();
        let i = self.active_tab;
        let (target, plane) = match self.tabs[i].scene.document.get_entity(handle) {
            Some(codec::EntityType::Hatch(hatch)) => {
                (EditTarget::from_hatch(handle, hatch), hatch_plane(hatch))
            }
            _ => return Task::none(),
        };
        if self.reject_locked_edit(i, handle) {
            return Task::none();
        }
        self.hatch_dialog = Some(State::for_edit(self.tabs[i].id, plane, target));
        self.active_modal = Some(ModalKind::Hatch);
        // The window is the command: Enter / Space repeat it.
        self.tabs[i].last_cmd = Some("HATCHEDIT".to_string());
        Task::none()
    }

    /// HATCHEDIT on the one selected hatch.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_open_edit_selected(&mut self, i: usize) -> Task<Message> {
        match self.hatchedit_window_target(i) {
            Some(handle) => self.hatch_dialog_open_edit(handle),
            None => Task::none(),
        }
    }

    /// The hatch HATCHEDIT opens the window on: the only selected object,
    /// when it is a hatch of any kind. Anything else keeps the command line.
    pub(in crate::app) fn hatchedit_window_target(&self, i: usize) -> Option<Handle> {
        let selected = self.selected_handles(i);
        let [handle] = selected.as_slice() else {
            return None;
        };
        match self.tabs[i].scene.document.get_entity(*handle) {
            Some(codec::EntityType::Hatch(_)) => Some(*handle),
            _ => None,
        }
    }

    /// Double-click on `handle` in model space: a hatch opens "Hatch Edit";
    /// `None` for any other object, which keeps its own double-click.
    #[inline(never)]
    pub(in crate::app) fn hatch_double_click(&mut self, handle: Handle) -> Option<Task<Message>> {
        let i = self.active_tab;
        matches!(
            self.tabs[i].scene.document.get_entity(handle),
            Some(codec::EntityType::Hatch(_))
        )
        .then(|| self.hatch_dialog_open_edit(handle))
    }

    /// A press on a grip of the selected hatch `handle` that is the
    /// second half of a double-click (the first click selected the hatch):
    /// open Hatch Edit instead of starting a grip edit. Without this, a
    /// double-click on the centre of a hatch lands on its pattern-origin grip
    /// (a solid or gradient one has it too, when associative).
    /// Same thresholds as the double-click in `on_viewport_left_release`;
    /// `cursor` is in the same tile coordinates as `last_vp_click_pos`.
    /// `None` leaves the grip to work as before.
    #[inline(never)]
    pub(in crate::app) fn hatch_double_click_on_grip(
        &mut self,
        i: usize,
        handle: Handle,
        cursor: iced::Point,
    ) -> Option<Task<Message>> {
        if self.tabs[i].active_cmd.is_some() || self.tabs[i].scene.current_layout != "Model" {
            return None;
        }
        let double = self.second_click_of_a_double_click(cursor);
        let is_hatch = matches!(
            self.tabs[i].scene.document.get_entity(handle),
            Some(codec::EntityType::Hatch(_))
        );
        if !double || !is_hatch {
            return None;
        }
        // The double-click is used up: a third quick press is a new gesture.
        self.last_vp_click_time = None;
        self.grip_hover = None;
        self.grip_popup = None;
        self.tabs[i]
            .scene
            .selection
            .borrow_mut()
            .clear_left_selection_gesture();
        Some(self.hatch_dialog_open_edit(handle))
    }

    /// Whether a press at `cursor` (tile coordinates) is the second click of
    /// a double-click: the thresholds of the double-click in
    /// `on_viewport_left_release`.
    fn second_click_of_a_double_click(&self, cursor: iced::Point) -> bool {
        self.last_vp_click_time
            .is_some_and(|time| time.elapsed().as_millis() < 400)
            && self
                .last_vp_click_pos
                .is_some_and(|last| (cursor.x - last.x).hypot(cursor.y - last.y) < 8.0)
    }

    /// A canvas point in the coordinates of the active model tile, as the
    /// release handler maps it before it records a click.
    fn tile_point(&self, i: usize, canvas: iced::Point) -> iced::Point {
        let size = self.tabs[i].scene.selection.borrow().vp_size;
        let offset = match self.tabs[i].scene.viewport_edit_frame(size) {
            Some((_, full)) => (full.x, full.y),
            None => {
                let tile = self.tabs[i].scene.active_model_tile_bounds(size.0, size.1);
                (tile.x, tile.y)
            }
        };
        iced::Point::new(canvas.x - offset.0, canvas.y - offset.1)
    }

    /// The hatch whose grip is hot in tab `i`, with no command running, in
    /// model space.
    fn hot_hatch_grip(&self, i: usize) -> Option<Handle> {
        if self.tabs[i].active_cmd.is_some() || self.tabs[i].scene.current_layout != "Model" {
            return None;
        }
        let handle = self.tabs[i].active_grip.as_ref()?.handle;
        match self.tabs[i].scene.document.get_entity(handle) {
            Some(codec::EntityType::Hatch(_)) => Some(handle),
            _ => None,
        }
    }

    /// The click that just made a grip of a hatch hot returns early
    /// from the release handler, before it records the click for double-click
    /// detection: record it here, so a quick second click can still open
    /// Hatch Edit. `canvas` is the release point in canvas coordinates.
    #[inline(never)]
    pub(in crate::app) fn hatch_note_hot_grip_click(&mut self, i: usize, canvas: iced::Point) {
        if self.hot_hatch_grip(i).is_none() {
            return;
        }
        self.last_vp_click_time = Some(iced::time::Instant::now());
        self.last_vp_click_pos = Some(self.tile_point(i, canvas));
    }

    /// A press while a grip of a hatch is hot: when it is the second
    /// click of a double-click, the grip is dropped as Escape drops it (the
    /// hatch back as it was, no base point left) and Hatch Edit opens.
    /// Otherwise `None`, and the press places the grip as before. `canvas` is
    /// the press point in canvas coordinates.
    #[inline(never)]
    pub(in crate::app) fn hatch_double_click_on_hot_grip(
        &mut self,
        i: usize,
        canvas: iced::Point,
    ) -> Option<Task<Message>> {
        let handle = self.hot_hatch_grip(i)?;
        let double = self.second_click_of_a_double_click(self.tile_point(i, canvas));
        // Paired or not, the recorded click is used up.
        self.last_vp_click_time = None;
        if !double {
            return None;
        }
        self.grip_pending = None;
        if self.cancel_active_grip_edit() {
            self.command_line.input.clear();
        }
        self.grip_hover = None;
        self.grip_popup = None;
        self.tabs[i]
            .scene
            .selection
            .borrow_mut()
            .clear_left_selection_gesture();
        Some(self.hatch_dialog_open_edit(handle))
    }

    /// The hatch whose fill is under `cursor`, for a double-click that hit
    /// no wire. A hatch carries no wire (`scene::convert::tessellate`), so
    /// the wire pick never finds one; the fill test is the one single clicks
    /// use to select it.
    #[inline(never)]
    pub(in crate::app) fn hatch_double_click_hit(
        &self,
        i: usize,
        cursor: iced::Point,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        candidates: Option<&rustc_hash::FxHashSet<Handle>>,
    ) -> Option<Handle> {
        crate::scene::pick::hit_test::click_hit_hatch(
            cursor,
            &self.tabs[i].scene.visible_hatches_for_click(candidates),
            view_rot,
            eye,
            bounds,
            candidates,
        )
    }

    /// The window's title: "Hatch Edit" on an existing hatch.
    pub(in crate::app) fn hatch_dialog_title(&self) -> String {
        let editing = self
            .hatch_dialog
            .as_ref()
            .is_some_and(|state| state.edit.is_some());
        if editing {
            crate::t!("Hatch Edit").into_owned()
        } else {
            crate::t!("Hatch and Gradient").into_owned()
        }
    }

    /// OK on "Hatch Edit": the fields the user changed, and only those, go to
    /// the hatch through the window's own HATCHEDIT operation (`Window`), as
    /// one undo step.
    #[inline(never)]
    fn hatch_dialog_ok_edit(&mut self) -> Task<Message> {
        use crate::modules::draw::draw::hatchedit::HatcheditCommand;
        let i = self.active_tab;
        let Some(state) = self.hatch_dialog.as_ref() else {
            return Task::none();
        };
        let Some(target) = state.edit.clone() else {
            return Task::none();
        };
        // The update acts on the active tab, so the window must belong to it.
        if state.owner_tab_id != self.tabs[i].id {
            self.hatch_dialog_cancel();
            return Task::none();
        }
        let Some(changes) = target
            .changes(&state.settings, state.specified_origin)
            .filter(|changes| !changes.is_empty())
        else {
            return Task::none();
        };
        self.hatch_dialog = None;
        self.hatch_close_color_window();
        if self.active_modal == Some(ModalKind::Hatch) {
            self.close_active_modal();
        }
        // The hatch may have gone, or its layer been locked, while the window
        // was open: then nothing is applied and nothing is left running.
        let handle = target.handle;
        // What the user changed is measured against the window as it opened;
        // what they left alone is the hatch as it is now (it may have changed
        // while the window was hidden for the origin pick).
        let current = match self.tabs[i].scene.document.get_entity(handle) {
            Some(codec::EntityType::Hatch(hatch)) => {
                crate::modules::draw::draw::hatch_edit_settings::EditTarget::from_hatch(
                    handle, hatch,
                )
            }
            _ => {
                self.command_line
                    .push_error(crate::t!("HATCHEDIT: hatch entity not found.").as_ref());
                return Task::none();
            }
        };
        if self.reject_locked_edit(i, handle) {
            return Task::none();
        }
        // Run it as HATCHEDIT does from the command line, so the end of the
        // command (nothing left selected, the hatch kept as "Previous") is
        // the same.
        self.tabs[i].active_cmd = Some(Box::new(HatcheditCommand::with_handle(
            handle,
            current.pattern.clone(),
            current.scale,
            current.angle_deg,
            false,
        )));
        self.apply_cmd_result(current.apply_result(&changes))
    }

    /// A field of the window changed. "Select Color..." also opens the
    /// colour window, which only the app can do.
    pub(in crate::app) fn hatch_dialog_field(&mut self, field: Field) -> Task<Message> {
        if let Field::SelectColor(slot) = field {
            return self.hatch_dialog_select_color(slot);
        }
        if let Some(state) = self.hatch_dialog.as_mut() {
            state.apply(field);
        }
        Task::none()
    }

    /// OK: create the hatch the dialog describes through the same commit paths
    /// the command line uses.
    #[inline(never)]
    pub(in crate::app) fn hatch_dialog_ok(&mut self) -> Task<Message> {
        // Enter / OK with the palette open means "use the chosen pattern".
        if self.hatch_palette_open() {
            return self.hatch_dialog_palette(PaletteAction::Apply);
        }
        if self.hatch_dialog.as_ref().is_some_and(|state| state.edit.is_some()) {
            return self.hatch_dialog_ok_edit();
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
        // The colour is resolved here, from the owner tab (the active one, as
        // checked above): a gradient's colours are in its own settings, so it
        // keeps the entity defaults.
        let style = (state.settings.tab == FillTab::Hatch)
            .then(|| self.hatch_creation_style(i, state.settings.color));
        let command = command.with_creation_style(style);
        let settings = state.settings.clone();
        self.hatch_last = settings;
        self.hatch_dialog = None;
        self.hatch_close_color_window();
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
            // A tab click also ends a search, or the click would show nothing.
            PaletteAction::Tab(category) => {
                palette.category = category;
                palette.search.clear();
            }
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
        // Even without a state: a colour window of the HATCH window never
        // outlives it.
        self.hatch_close_color_window();
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
        // Editing keeps the hatch's own boundaries.
        if state.edit.is_some() {
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
        // Editing has no Preview: the hatch on screen is the preview.
        if state.edit.is_some() || !state.can_ok() {
            return Task::none();
        }
        let document_origin = self.tabs[i].scene.document.hatch_origin();
        let Some(command) = hatch_command_from_state(state, document_origin) else {
            return Task::none();
        };
        // A gradient previews in its own colours.
        let preview_color = (state.settings.tab == FillTab::Hatch)
            .then(|| self.hatch_preview_rgba(i, state.settings.color));
        let models = command.with_preview_color(preview_color).preview_models();
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
    fn a_tab_click_ends_the_search_and_keeps_the_selection() {
        let mut app = app_with_palette();
        palette_do(&mut app, PaletteAction::Pick("BRICK".into()));
        palette_do(&mut app, PaletteAction::Search("ansi".into()));
        palette_do(&mut app, PaletteAction::Tab(PatternCategory::Iso));
        let palette = palette_of(&app).unwrap();
        assert!(palette.search.is_empty(), "the tab shows its own patterns");
        assert_eq!(palette.category, PatternCategory::Iso);
        assert_eq!(palette.selected.as_deref(), Some("BRICK"));
        assert_eq!(pattern_of(&app), "ANSI31");
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

    // ── Hatch Edit: the window on an existing hatch ────────────────────────

    use crate::modules::draw::draw::hatch_settings::{HatchSettings, OriginMode};
    use codec::entities::HatchStyleType;

    /// The rectangle hatched through the window with `edit` applied to its
    /// fields first; nothing stays selected and `hatch_last` is put back to
    /// the defaults. Returns the hatch.
    fn hatch_made_with(app: &mut OpenCADStudio, edit: &[Field]) -> Handle {
        open_dialog(app);
        app.hatch_dialog.as_mut().unwrap().regions.push(region());
        for field in edit {
            let _ = app.update(Message::HatchDialogField(field.clone()));
        }
        let _ = app.update(Message::HatchDialogOk);
        let i = app.active_tab;
        assert_eq!(app.tabs[i].scene.hatches.len(), 1, "one hatch made");
        app.hatch_last = HatchSettings::default();
        *app.tabs[i].scene.hatches.keys().next().unwrap()
    }

    /// The rectangle (four lines) with an associative ANSI31 hatch.
    fn app_with_hatch() -> (OpenCADStudio, Handle) {
        let mut app = app_with_rectangle();
        let hatch = hatch_made_with(&mut app, &[]);
        assert!(stored(&app, hatch).is_associative, "the lines bound it");
        (app, hatch)
    }

    fn stored(app: &OpenCADStudio, handle: Handle) -> codec::entities::Hatch {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Hatch(hatch)) => hatch.clone(),
            other => panic!("not a hatch: {other:?}"),
        }
    }

    fn set_stored(app: &mut OpenCADStudio, handle: Handle, edit: impl FnOnce(&mut codec::entities::Hatch)) {
        let i = app.active_tab;
        match app.tabs[i].scene.document.get_entity_mut(handle) {
            Some(codec::EntityType::Hatch(hatch)) => edit(hatch),
            other => panic!("not a hatch: {other:?}"),
        }
    }

    fn edit_handle(app: &OpenCADStudio) -> Option<Handle> {
        app.hatch_dialog
            .as_ref()
            .and_then(|state| state.edit.as_ref())
            .map(|edit| edit.handle)
    }

    fn undo_depth(app: &OpenCADStudio) -> usize {
        app.tabs[app.active_tab].history.undo_stack.len()
    }

    fn last_line(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .last()
            .map(|entry| entry.text.clone())
            .unwrap_or_default()
    }

    fn field(app: &mut OpenCADStudio, field: Field) {
        let _ = app.update(Message::HatchDialogField(field));
    }

    #[test]
    fn the_edit_window_is_filled_from_the_hatch_and_not_from_the_remembered_settings() {
        let mut app = app_with_rectangle();
        let hatch = hatch_made_with(
            &mut app,
            &[
                Field::Scale("2".into()),
                Field::Angle("30".into()),
                Field::IslandStyle(HatchStyleType::Outer),
            ],
        );
        app.hatch_last = HatchSettings {
            pattern: "BRICK".into(),
            angle: "45".into(),
            scale: "9".into(),
            associative: false,
            separate: true,
            retain: false,
            island_detection: false,
            island_style: HatchStyleType::Ignore,
            origin_mode: OriginMode::Specified,
            ..HatchSettings::default()
        };
        let remembered = app.hatch_last.clone();
        let _ = app.hatch_dialog_open_edit(hatch);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(edit_handle(&app), Some(hatch));
        let settings = &app.hatch_dialog.as_ref().unwrap().settings;
        assert_eq!(settings.pattern, "ANSI31");
        assert_eq!(settings.scale, "2");
        assert_eq!(settings.angle, "30");
        assert!(settings.island_detection);
        assert_eq!(settings.island_style, HatchStyleType::Outer);
        assert!(settings.associative);
        assert!(!settings.separate && !settings.retain);
        assert_eq!(settings.origin_mode, OriginMode::Current);
        assert_eq!(app.hatch_last, remembered, "opening reads nothing from it");
        assert_eq!(app.hatch_dialog_title(), "Hatch Edit");
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        assert_eq!(app.tabs[app.active_tab].last_cmd.as_deref(), Some("HATCHEDIT"));
    }

    #[test]
    fn leaving_the_tab_during_the_origin_pick_abandons_the_edit() {
        let (mut app, hatch) = app_with_hatch();
        let before = stored(&app, hatch);
        let (first, second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::OriginMode(OriginMode::Specified));
        let _ = app.update(Message::HatchDialogPickOrigin);
        assert!(app.tabs[first].active_cmd.is_some(), "the origin pick runs");
        let _ = app.update(Message::TabSwitch(second));
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
        assert!(app.tabs[first].active_cmd.is_none(), "the owner's pick is stopped");
        assert_eq!(app.tabs[first].scene.document.get_entity(hatch).cloned(),
            Some(codec::EntityType::Hatch(before)));
    }

    #[test]
    fn another_command_during_the_origin_pick_abandons_the_edit() {
        let (mut app, hatch) = app_with_hatch();
        let before = stored(&app, hatch);
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::OriginMode(OriginMode::Specified));
        let _ = app.update(Message::HatchDialogPickOrigin);
        let _ = app.dispatch_command("LINE");
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
        assert_eq!(command_name(&app, app.active_tab), Some("LINE"));
        assert_eq!(stored(&app, hatch), before);
    }

    #[test]
    fn the_creation_window_keeps_its_title() {
        let mut app = app_with_rectangle();
        open_dialog(&mut app);
        assert_eq!(app.hatch_dialog_title(), "Hatch and Gradient");
    }

    #[test]
    fn ok_changes_only_the_touched_fields_and_one_undo_puts_everything_back() {
        let (mut app, hatch) = app_with_hatch();
        set_stored(&mut app, hatch, |h| {
            h.common.color = codec::types::Color::Index(3);
            h.common.transparency = codec::types::Transparency::T_20;
        });
        let before = stored(&app, hatch);
        let depth = undo_depth(&app);
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Pattern("BRICK".into()));
        field(&mut app, Field::Scale("3".into()));
        field(&mut app, Field::IslandDetection(false));
        let _ = app.update(Message::HatchDialogOk);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none(), "closed");
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        let after = stored(&app, hatch);
        assert_eq!(after.pattern.name, "BRICK");
        assert_eq!(after.pattern_scale, 3.0);
        assert_eq!(after.style, HatchStyleType::Ignore);
        assert_eq!(after.pattern_angle, before.pattern_angle, "angle untouched");
        assert_eq!(after.common.color, before.common.color);
        assert_eq!(after.common.layer, before.common.layer);
        assert_eq!(after.common.transparency, before.common.transparency);
        assert_eq!(after.is_associative, before.is_associative);
        assert_eq!(after.paths, before.paths, "boundaries and their links untouched");
        assert_eq!(hatch_count(&app), 1, "the same hatch, no new one");
        assert_eq!(undo_depth(&app), depth + 1, "one undo step");
        let _ = app.update(Message::Undo);
        assert_eq!(stored(&app, hatch), before, "one undo restores it all");
    }

    /// OK on `fields` alone must change exactly what `expected` says and
    /// leave everything else of the hatch as it was; one undo restores it.
    fn edit_one(
        app: &mut OpenCADStudio,
        hatch: Handle,
        fields: &[Field],
        expected: impl FnOnce(&mut codec::entities::Hatch),
    ) {
        let before = stored(app, hatch);
        let depth = undo_depth(app);
        let _ = app.hatch_dialog_open_edit(hatch);
        for f in fields {
            field(app, f.clone());
        }
        let _ = app.update(Message::HatchDialogOk);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none(), "closed");
        let mut want = before.clone();
        expected(&mut want);
        assert_eq!(stored(app, hatch), want);
        assert_eq!(undo_depth(app), depth + 1, "one undo step");
        let _ = app.update(Message::Undo);
        assert_eq!(stored(app, hatch), before, "one undo restores it all");
    }

    #[test]
    fn each_gradient_field_changes_only_itself() {
        use codec::types::Color;
        let mut app = app_with_rectangle();
        let hatch = gradient_made(&mut app, &[]);
        edit_one(&mut app, hatch, &[Field::GradientShape(5)], |h| {
            h.gradient_color.name = "CURVED".into()
        });
        edit_one(&mut app, hatch, &[Field::GradientColor1(Color::Index(2))], |h| {
            h.gradient_color.colors[0].color = Color::Index(2)
        });
        edit_one(&mut app, hatch, &[Field::GradientColor2(Color::Index(4))], |h| {
            h.gradient_color.colors[1].color = Color::Index(4)
        });
        edit_one(&mut app, hatch, &[Field::GradientCentered(false)], |h| {
            h.gradient_color.shift = 1.0
        });
        edit_one(&mut app, hatch, &[Field::GradientAngle("30".into())], |h| {
            h.gradient_color.angle = 30f64.to_radians();
            h.pattern_angle = 30f64.to_radians();
        });
        edit_one(&mut app, hatch, &[Field::GradientOneColor(true)], |h| {
            h.gradient_color.is_single_color = true;
            h.gradient_color.color_tint = 1.0;
        });
    }

    #[test]
    fn a_hidden_gradient_field_does_not_enable_ok() {
        let mut app = app_with_rectangle();
        let hatch = gradient_made(&mut app, &[]); // two colours: the tint is hidden
        let before = stored(&app, hatch);
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::GradientTint(0.1));
        assert!(!app.hatch_dialog.as_ref().unwrap().can_ok());
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch), "nothing to apply: still open");
        assert_eq!(stored(&app, hatch), before);
    }

    #[test]
    fn a_solid_edits_like_a_hatch_and_keeps_its_colour() {
        let mut app = app_with_rectangle();
        let solid = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
        edit_one(&mut app, solid, &[Field::IslandStyle(HatchStyleType::Outer)], |h| {
            h.style = HatchStyleType::Outer
        });
        edit_one(
            &mut app,
            solid,
            &[Field::Color(crate::modules::draw::draw::hatch_settings::HatchColor::Color(
                codec::types::Color::Index(1),
            ))],
            |h| h.common.color = codec::types::Color::Index(1),
        );
    }

    /// One conversion: `fields`, OK, the new kind, boundaries untouched, one
    /// undo step; one undo restores the hatch and one redo brings it back.
    /// Returns the converted hatch (the undo is left undone).
    fn convert(
        app: &mut OpenCADStudio,
        hatch: Handle,
        fields: &[Field],
        to: crate::entities::hatch_fill::FillKind,
    ) -> codec::entities::Hatch {
        let before = stored(app, hatch);
        let depth = undo_depth(app);
        let _ = app.hatch_dialog_open_edit(hatch);
        for f in fields {
            field(app, f.clone());
        }
        let _ = app.update(Message::HatchDialogOk);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none(), "closed");
        let after = stored(app, hatch);
        assert_eq!(crate::entities::hatch_fill::FillKind::of(&after), to);
        // The scene's cached model follows the kind right after OK, not only
        // after some later rebuild (undo and redo rebuild it too).
        {
            use crate::entities::hatch_fill::FillKind;
            use crate::scene::model::hatch_model::HatchPattern;
            let model = &app.tabs[app.active_tab].scene.hatches[&hatch].pattern;
            let follows = match to {
                FillKind::Solid => matches!(model, HatchPattern::Solid),
                FillKind::Pattern => matches!(model, HatchPattern::Pattern(_)),
                FillKind::Gradient => matches!(model, HatchPattern::Gradient { .. }),
            };
            assert!(follows, "the model follows the kind {to:?}");
        }
        assert_eq!(after.paths, before.paths, "boundaries and links");
        assert_eq!(undo_depth(app), depth + 1, "one undo step");
        let _ = app.update(Message::Undo);
        // As text: a hatch from a file may hold NaN, which is not equal to itself.
        assert_eq!(format!("{:?}", stored(app, hatch)), format!("{before:?}"), "one undo restores it all");
        assert_eq!(undo_depth(app), depth);
        let _ = app.update(Message::Redo);
        assert_eq!(stored(app, hatch), after, "and one redo brings it back");
        let _ = app.update(Message::Undo);
        assert_eq!(format!("{:?}", stored(app, hatch)), format!("{before:?}"));
        after
    }

    /// Every way from one kind of fill to another is one OK and one undo, and
    /// leaves boundaries, links and island style alone. Most OKs also carry a
    /// colour and an island style: a second undo snapshot taken between the
    /// fill and the rest would leave the first undo half-way.
    #[test]
    fn all_six_conversions_are_one_undo_each() {
        use crate::entities::hatch_fill::{read_gradient, FillKind};
        use crate::modules::draw::draw::hatch_settings::{FillTab, HatchColor};
        use crate::scene::model::hatch_model::GradientKind;
        let red = || Field::Color(HatchColor::Color(codec::types::Color::Index(1)));
        let outer = || Field::IslandStyle(HatchStyleType::Outer);

        // pattern -> solid, pattern -> gradient
        let (mut app, hatch) = app_with_hatch();
        let before = stored(&app, hatch);
        let after = convert(
            &mut app,
            hatch,
            &[Field::Pattern("SOLID".into()), red(), outer()],
            FillKind::Solid,
        );
        assert_eq!(after.common.color, codec::types::Color::Index(1));
        assert_eq!(after.style, HatchStyleType::Outer);
        let after = convert(&mut app, hatch, &[Field::Tab(FillTab::Gradient), outer()], FillKind::Gradient);
        assert_eq!(after.is_associative, before.is_associative);
        assert_eq!(after.style, HatchStyleType::Outer);
        assert_eq!(after.common.color, before.common.color, "the Gradient tab has no entity colour");

        // solid -> pattern, solid -> gradient
        let mut app = app_with_rectangle();
        let solid = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
        let after = convert(
            &mut app,
            solid,
            &[Field::Pattern("ANSI31".into()), red(), outer()],
            FillKind::Pattern,
        );
        assert_eq!(after.pattern.name, "ANSI31");
        assert_eq!(after.common.color, codec::types::Color::Index(1));
        assert!(!after.pattern.lines.is_empty(), "real pattern lines");
        convert(&mut app, solid, &[Field::Tab(FillTab::Gradient)], FillKind::Gradient);

        // gradient -> pattern, gradient -> solid (same stored name "SOLID")
        let mut app = app_with_rectangle();
        let gradient = gradient_made(&mut app, &[Field::GradientShape(4)]);
        let before = stored(&app, gradient);
        let after = convert(
            &mut app,
            gradient,
            &[Field::Tab(FillTab::Hatch), red(), outer()],
            FillKind::Pattern,
        );
        assert!(!after.gradient_color.enabled);
        assert_eq!(after.common.color, codec::types::Color::Index(1));
        assert_eq!(after.style, HatchStyleType::Outer);
        assert_eq!(read_gradient(&stored(&app, gradient)).kind, GradientKind::CHOICES[4].0);
        assert_eq!(stored(&app, gradient), before);
        let after = convert(
            &mut app,
            gradient,
            &[Field::Tab(FillTab::Hatch), Field::Pattern("SOLID".into()), red()],
            FillKind::Solid,
        );
        assert!(!after.gradient_color.enabled && after.is_solid);
        assert_eq!(after.common.color, codec::types::Color::Index(1));
    }

    #[test]
    fn converting_an_associative_hatch_with_islands_leaves_its_structure_alone() {
        // Review focus: several paths, links to source objects, island style.
        let mut app = app_with_rectangle();
        let hatch = hatch_made_with(&mut app, &[Field::IslandStyle(HatchStyleType::Outer)]);
        let before = stored(&app, hatch);
        assert!(before.is_associative);
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Tab(crate::modules::draw::draw::hatch_settings::FillTab::Gradient));
        let _ = app.update(Message::HatchDialogOk);
        let after = stored(&app, hatch);
        assert_eq!(after.paths, before.paths);
        assert_eq!(after.is_associative, before.is_associative);
        assert_eq!(after.style, before.style);
        assert_eq!(after.common, before.common);
    }

    /// A hatch read from a file can carry a zero or non-finite scale or
    /// angle (a solid often does). The window shows the defaults 1 and 0
    /// instead, and an OK for anything else must not write them to the hatch.
    #[test]
    fn a_hatch_with_an_unusable_scale_or_angle_opens_with_defaults_and_keeps_them_stored() {
        use crate::modules::draw::draw::hatch_settings::HatchColor;
        let cases = [(0.0, 0.0), (f64::NAN, f64::NAN), (f64::INFINITY, f64::INFINITY), (-2.0, 0.0)];
        for (scale, angle) in cases {
            let mut app = app_with_rectangle();
            let solid = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
            set_stored(&mut app, solid, |h| {
                h.pattern_scale = scale;
                h.pattern_angle = angle;
            });
            // NaN is not equal to itself: compare the whole entity as text.
            let text = |h: &codec::entities::Hatch| format!("{h:?}");
            let before = stored(&app, solid);

            // Opening and OK: nothing to apply, nothing written.
            let _ = app.hatch_dialog_open_edit(solid);
            {
                let state = app.hatch_dialog.as_ref().unwrap();
                assert_eq!(state.settings.scale, "1", "scale {scale}");
                assert_eq!(state.settings.angle, "0", "angle {angle}");
                assert!(state.fields_valid() && !state.can_ok(), "nothing changed: {scale}");
            }
            let _ = app.update(Message::HatchDialogOk);
            assert_eq!(app.active_modal, Some(ModalKind::Hatch), "still open");
            assert_eq!(text(&stored(&app, solid)), text(&before));
            app.hatch_dialog_cancel();

            // The colour alone: the entity differs from before only there.
            let _ = app.hatch_dialog_open_edit(solid);
            field(&mut app, Field::Color(HatchColor::Color(codec::types::Color::Index(1))));
            assert!(app.hatch_dialog.as_ref().unwrap().can_ok(), "scale {scale}");
            let _ = app.update(Message::HatchDialogOk);
            assert!(app.hatch_dialog.is_none(), "closed");
            let mut want = before.clone();
            want.common.color = codec::types::Color::Index(1);
            assert_eq!(text(&stored(&app, solid)), text(&want), "scale {scale}: only the colour");
            let _ = app.update(Message::Undo);
            assert_eq!(text(&stored(&app, solid)), text(&before));

            // The style alone, then the angle alone (the scale stays unusable).
            let _ = app.hatch_dialog_open_edit(solid);
            field(&mut app, Field::IslandStyle(HatchStyleType::Outer));
            let _ = app.update(Message::HatchDialogOk);
            let mut want = before.clone();
            want.style = HatchStyleType::Outer;
            assert_eq!(text(&stored(&app, solid)), text(&want), "scale {scale}: only the style");
            let _ = app.update(Message::Undo);

            let _ = app.hatch_dialog_open_edit(solid);
            field(&mut app, Field::Angle("30".into()));
            let _ = app.update(Message::HatchDialogOk);
            let after = stored(&app, solid);
            assert!((after.pattern_angle - 30f64.to_radians()).abs() < 1e-6, "angle {angle}");
            assert_eq!(
                after.pattern_scale.to_bits(),
                scale.to_bits(),
                "scale {scale}: the angle edit leaves it alone"
            );
            let _ = app.update(Message::Undo);
            assert_eq!(text(&stored(&app, solid)), text(&before));
        }
    }

    /// Turning such a solid into a pattern gives the pattern the scale and
    /// angle the window showed (1 and 0), not 1e-6.
    #[test]
    fn a_solid_with_an_unusable_scale_becomes_a_pattern_at_the_scale_shown() {
        use crate::entities::hatch_fill::FillKind;
        let mut app = app_with_rectangle();
        let solid = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
        set_stored(&mut app, solid, |h| {
            h.pattern_scale = 0.0;
            h.pattern_angle = f64::NAN;
        });
        let after = convert(&mut app, solid, &[Field::Pattern("ANSI31".into())], FillKind::Pattern);
        assert_eq!(after.pattern_scale, 1.0);
        assert_eq!(after.pattern_angle, 0.0);
        assert!(after.pattern.lines.iter().all(|l| l.angle.is_finite() && l.offset.x.is_finite()));
    }

    /// A conversion from the window is one OK and one undo; the scene's fill
    /// model follows the new kind; boundaries, links, island style and the
    /// entity's own properties stay as they were.
    #[test]
    fn converting_a_pattern_hatch_in_the_window_is_one_undo_and_the_scene_follows() {
        use crate::entities::hatch_fill::FillKind;
        use crate::modules::draw::draw::hatch_settings::{FillTab, HatchColor};
        use crate::scene::model::hatch_model::HatchPattern;
        let model = |app: &OpenCADStudio, h: Handle| app.tabs[app.active_tab].scene.hatches[&h].clone();
        let (mut app, hatch) = app_with_hatch();
        let before = stored(&app, hatch);
        assert_eq!(FillKind::of(&before), FillKind::Pattern);

        // pattern -> gradient: only the tab changed
        let depth = undo_depth(&app);
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Tab(FillTab::Gradient));
        let _ = app.update(Message::HatchDialogOk);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none(), "closed");
        let after = stored(&app, hatch);
        assert_eq!(FillKind::of(&after), FillKind::Gradient);
        assert_eq!(after.paths, before.paths, "boundaries and their links");
        assert_eq!(after.is_associative, before.is_associative);
        assert_eq!(after.style, before.style);
        assert_eq!(after.common, before.common);
        assert!(
            matches!(model(&app, hatch).pattern, HatchPattern::Gradient { .. }),
            "the scene's model follows the kind"
        );
        assert_eq!(undo_depth(&app), depth + 1, "one undo step");
        let _ = app.update(Message::Undo);
        assert_eq!(stored(&app, hatch), before, "one undo restores it all");
        assert!(!matches!(model(&app, hatch).pattern, HatchPattern::Gradient { .. }));

        // pattern -> solid, with a colour of its own in the same OK
        let depth = undo_depth(&app);
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Pattern("SOLID".into()));
        field(&mut app, Field::Color(HatchColor::Color(codec::types::Color::Index(1))));
        let _ = app.update(Message::HatchDialogOk);
        let after = stored(&app, hatch);
        assert_eq!(FillKind::of(&after), FillKind::Solid);
        assert_eq!(after.common.color, codec::types::Color::Index(1));
        assert_eq!(after.common.layer, before.common.layer);
        assert_eq!(after.paths, before.paths);
        assert_eq!(after.style, before.style);
        assert!(matches!(model(&app, hatch).pattern, HatchPattern::Solid));
        assert_eq!(undo_depth(&app), depth + 1, "one undo step");
        let _ = app.update(Message::Undo);
        assert_eq!(stored(&app, hatch), before, "one undo restores it all");
    }

    #[test]
    fn a_style_only_change_leaves_the_pattern_geometry_exactly_as_it_was() {
        // Scale and angle that f32 cannot hold exactly, and an origin away
        // from zero: re-applying them would move the pattern lines a little.
        let (mut app, hatch) = app_with_hatch();
        set_stored(&mut app, hatch, |h| {
            h.pattern_scale = 0.1;
            h.pattern_angle = 0.1;
            assert!(h.set_pattern_origin(codec::types::Vector2::new(1.234_567_8, 5.678_912_3)));
        });
        let before = stored(&app, hatch);
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::IslandStyle(HatchStyleType::Outer));
        let _ = app.update(Message::HatchDialogOk);
        let after = stored(&app, hatch);
        assert_eq!(after.style, HatchStyleType::Outer);
        assert_eq!(after.pattern_scale, 0.1);
        assert_eq!(after.pattern_angle, 0.1);
        assert_eq!(after.pattern, before.pattern, "pattern lines untouched");
    }

    #[test]
    fn a_scale_change_keeps_the_pattern_and_scales_its_lines() {
        let (mut app, hatch) = app_with_hatch();
        let before = stored(&app, hatch);
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Scale("2".into()));
        let _ = app.update(Message::HatchDialogOk);
        let after = stored(&app, hatch);
        assert_eq!(after.pattern.name, before.pattern.name);
        assert_eq!(after.pattern_scale, 2.0);
        let (old, new) = (&before.pattern.lines[0], &after.pattern.lines[0]);
        assert!((new.offset.x - 2.0 * old.offset.x).abs() < 1.0e-9);
        assert!((new.offset.y - 2.0 * old.offset.y).abs() < 1.0e-9);
    }

    #[test]
    fn an_angle_only_change_rotates_the_pattern_lines_by_that_angle() {
        let (mut app, hatch) = app_with_hatch();
        let before = stored(&app, hatch);
        assert_eq!(before.pattern_angle, 0.0);
        let origin = before.pattern_origin();
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Angle("30".into()));
        let _ = app.update(Message::HatchDialogOk);
        let after = stored(&app, hatch);
        assert_eq!(after.pattern_scale, before.pattern_scale, "scale untouched");
        let theta = 30f64.to_radians();
        assert!((after.pattern_angle - theta).abs() < 1.0e-12);
        let (sin, cos) = theta.sin_cos();
        let rotate = |x: f64, y: f64| (x * cos - y * sin, x * sin + y * cos);
        assert_eq!(after.pattern.lines.len(), before.pattern.lines.len());
        for (old, new) in before.pattern.lines.iter().zip(&after.pattern.lines) {
            assert!((new.angle - (old.angle + theta)).abs() < 1.0e-9, "line angle");
            let (ox, oy) = rotate(old.offset.x, old.offset.y);
            assert!((new.offset.x - ox).abs() < 1.0e-9 && (new.offset.y - oy).abs() < 1.0e-9);
            let (bx, by) = rotate(old.base_point.x - origin.x, old.base_point.y - origin.y);
            assert!((new.base_point.x - (origin.x + bx)).abs() < 1.0e-9);
            assert!((new.base_point.y - (origin.y + by)).abs() < 1.0e-9);
            assert_eq!(new.dash_lengths, old.dash_lengths, "dashes not scaled");
        }
    }

    #[test]
    fn dash_hatchedit_style_change_keeps_scale_and_angle_exactly() {
        // The f32 guard of the HATCHEDIT update applies to the command line
        // too: a style change no longer rewrites scale and angle rounded to
        // f32, nor moves the pattern lines.
        let (mut app, hatch) = app_with_hatch();
        set_stored(&mut app, hatch, |h| {
            h.pattern_scale = 0.1;
            h.pattern_angle = 0.1;
            assert!(h.set_pattern_origin(codec::types::Vector2::new(1.234_567_8, 5.678_912_3)));
        });
        let before = stored(&app, hatch);
        select_only(&mut app, hatch);
        let _ = app.dispatch_command("-HATCHEDIT");
        let _ = app.feed_command(StepInput::Text("S".into()));
        let _ = app.feed_command(StepInput::Text("O".into()));
        let after = stored(&app, hatch);
        assert_eq!(after.style, HatchStyleType::Outer);
        assert_eq!(after.pattern_scale, 0.1);
        assert_eq!(after.pattern_angle, 0.1);
        assert_eq!(after.pattern, before.pattern, "pattern lines untouched");
    }

    #[test]
    fn cancel_and_escape_change_nothing() {
        for close in [Message::CloseModal, Message::CommandEscape] {
            let (mut app, hatch) = app_with_hatch();
            let before = stored(&app, hatch);
            let depth = undo_depth(&app);
            let _ = app.hatch_dialog_open_edit(hatch);
            field(&mut app, Field::Scale("5".into()));
            field(&mut app, Field::Associative(false));
            assert!(app.hatch_dialog.as_ref().unwrap().can_ok(), "there was something to apply");
            let _ = app.update(close);
            assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
            assert_eq!(stored(&app, hatch), before);
            assert_eq!(undo_depth(&app), depth);
        }
    }

    #[test]
    fn escape_with_the_palette_open_closes_only_the_palette_in_edit_too() {
        let (mut app, hatch) = app_with_hatch();
        let _ = app.hatch_dialog_open_edit(hatch);
        palette_do(&mut app, PaletteAction::Open);
        palette_do(&mut app, PaletteAction::Pick("BRICK".into()));
        let _ = app.update(Message::CloseModal);
        assert!(palette_of(&app).is_none());
        assert_eq!(edit_handle(&app), Some(hatch), "the edit window stays");
        palette_do(&mut app, PaletteAction::Open);
        palette_do(&mut app, PaletteAction::Pick("BRICK".into()));
        palette_do(&mut app, PaletteAction::Apply);
        assert_eq!(pattern_of(&app), "BRICK");
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(stored(&app, hatch).pattern.name, "BRICK");
    }

    #[test]
    fn switching_associative_off_disassociates_the_hatch() {
        let (mut app, hatch) = app_with_hatch();
        assert!(stored(&app, hatch).paths.iter().any(|p| !p.boundary_handles.is_empty()));
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Associative(false));
        let _ = app.update(Message::HatchDialogOk);
        let after = stored(&app, hatch);
        assert!(!after.is_associative);
        assert!(after.paths.iter().all(|p| p.boundary_handles.is_empty()));
    }

    #[test]
    fn a_hatch_that_is_not_associative_cannot_be_associated_here() {
        let (mut app, hatch) = app_with_hatch();
        set_stored(&mut app, hatch, |h| {
            h.is_associative = false;
            for path in &mut h.paths {
                path.boundary_handles.clear();
            }
        });
        let before = stored(&app, hatch);
        let _ = app.hatch_dialog_open_edit(hatch);
        assert!(!app.hatch_dialog.as_ref().unwrap().settings.associative);
        field(&mut app, Field::Associative(true));
        assert!(!app.hatch_dialog.as_ref().unwrap().settings.associative);
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch), "nothing to apply: still open");
        assert_eq!(stored(&app, hatch), before);
    }

    #[test]
    fn ok_without_a_change_does_nothing() {
        let (mut app, hatch) = app_with_hatch();
        let before = stored(&app, hatch);
        let depth = undo_depth(&app);
        let _ = app.hatch_dialog_open_edit(hatch);
        let _ = app.update(Message::HatchDialogOk);
        let _ = app.update(Message::CommandFinalize);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(edit_handle(&app), Some(hatch), "the same edit window, still open");
        assert_eq!(stored(&app, hatch), before);
        assert_eq!(undo_depth(&app), depth);
    }

    #[test]
    fn enter_in_the_edit_window_is_ok() {
        let (mut app, hatch) = app_with_hatch();
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Scale("2".into()));
        let _ = app.update(Message::CommandFinalize);
        assert_eq!(stored(&app, hatch).pattern_scale, 2.0);
        assert!(app.hatch_dialog.is_none());
    }

    #[test]
    fn editing_never_touches_the_remembered_settings() {
        let (mut app, hatch) = app_with_hatch();
        app.hatch_last.scale = "7".into();
        let remembered = app.hatch_last.clone();
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Scale("3".into()));
        field(&mut app, Field::Pattern("BRICK".into()));
        palette_do(&mut app, PaletteAction::Open);
        palette_do(&mut app, PaletteAction::Pick("ANSI37".into()));
        palette_do(&mut app, PaletteAction::Apply);
        let _ = app.update(Message::HatchDialogOk);
        assert_eq!(stored(&app, hatch).pattern.name, "ANSI37");
        assert_eq!(app.hatch_last, remembered);
    }

    #[test]
    fn add_and_preview_are_refused_in_edit() {
        let (mut app, hatch) = app_with_hatch();
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Scale("2".into()));
        for message in [
            Message::HatchDialogAdd(AddKind::Points),
            Message::HatchDialogAdd(AddKind::Objects),
            Message::HatchDialogPreview,
        ] {
            let _ = app.update(message);
            assert_eq!(app.active_modal, Some(ModalKind::Hatch));
            assert_eq!(app.hatch_dialog.as_ref().unwrap().flow, Flow::None);
            assert!(app.tabs[app.active_tab].active_cmd.is_none());
        }
        assert_eq!(hatch_count(&app), 1);
    }

    #[test]
    fn a_hatch_erased_while_the_window_is_open_is_left_alone() {
        let (mut app, hatch) = app_with_hatch();
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Scale("2".into()));
        let i = app.active_tab;
        app.tabs[i].scene.erase_entities(&[hatch]);
        let depth = undo_depth(&app);
        let _ = app.update(Message::HatchDialogOk);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none(), "closed");
        assert!(app.tabs[i].active_cmd.is_none(), "no HATCHEDIT left running");
        assert!(app.tabs[i].scene.document.get_entity(hatch).is_none());
        assert_eq!(undo_depth(&app), depth, "nothing applied");
        assert!(last_line(&app).contains("not found"), "{}", last_line(&app));
    }

    #[test]
    fn a_hatch_erased_during_the_origin_pick_is_left_alone_too() {
        let (mut app, hatch) = app_with_hatch();
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::OriginMode(OriginMode::Specified));
        let _ = app.update(Message::HatchDialogPickOrigin);
        let i = app.active_tab;
        app.tabs[i].scene.erase_entities(&[hatch]);
        click_at(&mut app, 3.0, 4.0);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch), "back to the window");
        let _ = app.update(Message::HatchDialogOk);
        assert!(app.hatch_dialog.is_none());
        assert!(app.tabs[i].active_cmd.is_none());
        assert!(app.tabs[i].scene.document.get_entity(hatch).is_none());
    }

    #[test]
    fn a_hatch_whose_layer_was_locked_meanwhile_is_left_alone() {
        let (mut app, hatch) = app_with_hatch();
        let before = stored(&app, hatch);
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Scale("2".into()));
        let i = app.active_tab;
        app.tabs[i].scene.document.layers.get_mut("0").unwrap().flags.locked = true;
        let _ = app.update(Message::HatchDialogOk);
        assert!(app.hatch_dialog.is_none());
        assert!(app.tabs[i].active_cmd.is_none(), "no HATCHEDIT left running");
        assert_eq!(stored(&app, hatch), before);
        assert!(last_line(&app).contains("locked"), "{}", last_line(&app));
    }

    #[test]
    fn a_new_origin_is_picked_in_the_hatch_plane_and_applied() {
        let (mut app, hatch) = app_with_hatch();
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::OriginMode(OriginMode::Specified));
        assert!(!app.hatch_dialog.as_ref().unwrap().can_ok(), "no point yet");
        let _ = app.update(Message::HatchDialogPickOrigin);
        assert_eq!(app.hatch_dialog.as_ref().unwrap().flow, Flow::Origin);
        assert!(app.active_modal.is_none(), "hidden while picking");
        click_at(&mut app, 3.0, 4.0);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        let _ = app.update(Message::HatchDialogOk);
        let origin = stored(&app, hatch).pattern_origin();
        assert!((origin.x - 3.0).abs() < 1.0e-9 && (origin.y - 4.0).abs() < 1.0e-9, "{origin:?}");
    }

    #[test]
    fn untouched_fields_follow_the_hatch_as_it_is_at_ok() {
        // The hatch changes while the window is hidden for the origin pick
        // (an undo, say): OK must not put back the values it opened with.
        let (mut app, hatch) = app_with_hatch();
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::OriginMode(OriginMode::Specified));
        let _ = app.update(Message::HatchDialogPickOrigin);
        set_stored(&mut app, hatch, |h| {
            h.pattern_scale = 5.0;
            h.pattern_angle = 0.25;
        });
        let meanwhile = stored(&app, hatch);
        click_at(&mut app, 3.0, 4.0);
        let _ = app.update(Message::HatchDialogOk);
        let after = stored(&app, hatch);
        assert_eq!(after.pattern_scale, 5.0, "scale untouched");
        assert_eq!(after.pattern_angle, 0.25, "angle untouched");
        assert_eq!(after.pattern.lines.len(), meanwhile.pattern.lines.len());
        assert_eq!(after.pattern.lines[0].offset, meanwhile.pattern.lines[0].offset);
        let origin = after.pattern_origin();
        assert!((origin.x - 3.0).abs() < 1.0e-9 && (origin.y - 4.0).abs() < 1.0e-9);
    }

    #[test]
    fn leaving_or_closing_the_tab_abandons_the_edit() {
        for close in [false, true] {
            let (mut app, hatch) = app_with_hatch();
            let before = stored(&app, hatch);
            let (first, second) = open_second_tab(&mut app);
            let _ = app.update(Message::TabSwitch(first));
            let _ = app.hatch_dialog_open_edit(hatch);
            field(&mut app, Field::Scale("2".into()));
            assert_eq!(edit_handle(&app), Some(hatch));
            if close {
                // A dirty tab would only ask to save first.
                app.tabs[first].dirty = false;
                let id = app.tabs[first].id;
                let _ = app.update(Message::TabClose(id));
                assert!(app.tabs.iter().all(|tab| tab.id != id), "owner closed");
            } else {
                let _ = app.update(Message::TabSwitch(second));
                assert_eq!(app.tabs[first].scene.document.get_entity(hatch).cloned(),
                    Some(codec::EntityType::Hatch(before.clone())), "untouched");
            }
            assert!(app.hatch_dialog.is_none(), "close={close}");
            assert!(app.active_modal.is_none(), "close={close}");
        }
    }

    #[test]
    fn ok_on_another_tab_is_refused_in_edit() {
        let (mut app, hatch) = app_with_hatch();
        let before = stored(&app, hatch);
        let (first, second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        let _ = app.hatch_dialog_open_edit(hatch);
        field(&mut app, Field::Scale("2".into()));
        assert_eq!(edit_handle(&app), Some(hatch));
        app.active_tab = second;
        let _ = app.update(Message::HatchDialogOk);
        assert!(app.hatch_dialog.is_none());
        app.active_tab = first;
        assert_eq!(stored(&app, hatch), before);
    }

    /// A gradient made through the window (tab Gradient) and stored.
    fn gradient_made(app: &mut OpenCADStudio, fields: &[Field]) -> Handle {
        let mut all = vec![Field::Tab(crate::modules::draw::draw::hatch_settings::FillTab::Gradient)];
        all.extend_from_slice(fields);
        hatch_made_with(app, &all)
    }

    #[test]
    fn a_solid_hatch_opens_the_window_on_the_hatch_tab() {
        let mut app = app_with_rectangle();
        let solid = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
        assert!(stored(&app, solid).is_solid);
        let _ = app.hatch_dialog_open_edit(solid);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(edit_handle(&app), Some(solid));
        let settings = &app.hatch_dialog.as_ref().unwrap().settings;
        assert_eq!(settings.tab, crate::modules::draw::draw::hatch_settings::FillTab::Hatch);
        assert_eq!(settings.pattern, "SOLID");
        assert_eq!(app.hatch_dialog_title(), "Hatch Edit");
    }

    #[test]
    fn a_gradient_hatch_opens_the_window_on_the_gradient_tab_with_its_values() {
        use crate::modules::draw::draw::hatch_settings::FillTab;
        let mut app = app_with_rectangle();
        let gradient = gradient_made(
            &mut app,
            &[
                Field::GradientShape(3),
                Field::GradientColor1(codec::types::Color::Index(1)),
                Field::GradientAngle("30".into()),
            ],
        );
        let _ = app.hatch_dialog_open_edit(gradient);
        assert_eq!(edit_handle(&app), Some(gradient));
        let settings = &app.hatch_dialog.as_ref().unwrap().settings;
        assert_eq!(settings.tab, FillTab::Gradient);
        assert_eq!(settings.gradient.shape, 3);
        // Creation stores the true colour of ACI 1; the window shows what is stored.
        assert_eq!(settings.gradient.color1, codec::types::Color::Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(settings.gradient.angle, "30");
        assert_eq!(app.tabs[app.active_tab].last_cmd.as_deref(), Some("HATCHEDIT"));
    }

    #[test]
    fn a_gradient_with_no_stops_from_another_program_opens_too() {
        let (mut app, hatch) = app_with_hatch();
        set_stored(&mut app, hatch, |h| h.gradient_color.enabled = true);
        let _ = app.hatch_dialog_open_edit(hatch);
        assert_eq!(edit_handle(&app), Some(hatch));
        assert_eq!(
            app.hatch_dialog.as_ref().unwrap().settings.gradient.color1,
            crate::entities::hatch_fill::DEFAULT_GRADIENT_COLOR1
        );
    }

    #[test]
    fn opening_the_window_on_anything_but_a_hatch_does_nothing() {
        let (mut app, _hatch) = app_with_hatch();
        let line = add_line(&mut app, 100.0, 0.0, 110.0, 0.0);
        let _ = app.hatch_dialog_open_edit(line);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
    }

    #[test]
    fn a_hatch_on_a_locked_layer_does_not_open_the_window() {
        let (mut app, hatch) = app_with_hatch();
        let i = app.active_tab;
        app.tabs[i].scene.document.layers.get_mut("0").unwrap().flags.locked = true;
        let _ = app.hatch_dialog_open_edit(hatch);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
        assert!(last_line(&app).contains("locked"), "{}", last_line(&app));
    }

    // ── HATCHEDIT and -HATCHEDIT ───────────────────────────────────────────

    fn select_only(app: &mut OpenCADStudio, handle: Handle) {
        let i = app.active_tab;
        app.tabs[i].scene.deselect_all();
        app.tabs[i].scene.select_entity(handle, false);
    }

    #[test]
    fn hatchedit_on_one_selected_pattern_hatch_opens_the_window() {
        let (mut app, hatch) = app_with_hatch();
        select_only(&mut app, hatch);
        let _ = app.dispatch_command("HATCHEDIT");
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(edit_handle(&app), Some(hatch));
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
    }

    #[test]
    fn hatchedit_from_the_ribbon_opens_the_window() {
        use crate::modules::ModuleEvent;
        let (mut app, hatch) = app_with_hatch();
        select_only(&mut app, hatch);
        let _ = app.update(Message::RibbonToolClick {
            tool_id: "HATCHEDIT".to_string(),
            event: ModuleEvent::Command("HATCHEDIT".to_string()),
        });
        assert_eq!(edit_handle(&app), Some(hatch));
    }

    #[test]
    fn ok_ends_like_hatchedit_from_the_command_line() {
        // The window...
        let (mut app, hatch) = app_with_hatch();
        select_only(&mut app, hatch);
        let _ = app.dispatch_command("HATCHEDIT");
        field(&mut app, Field::IslandStyle(HatchStyleType::Outer));
        let _ = app.update(Message::HatchDialogOk);
        let i = app.active_tab;
        let window = (
            stored(&app, hatch).style,
            app.tabs[i].scene.selected.is_empty(),
            app.tabs[i].prev_selection.clone(),
            app.tabs[i].active_cmd.is_none(),
            last_line(&app),
        );
        // ...and the command line, on the same drawing.
        let (mut app, hatch) = app_with_hatch();
        select_only(&mut app, hatch);
        let _ = app.dispatch_command("-HATCHEDIT");
        let _ = app.feed_command(StepInput::Text("S".into()));
        let _ = app.feed_command(StepInput::Text("O".into()));
        let i = app.active_tab;
        let command_line = (
            stored(&app, hatch).style,
            app.tabs[i].scene.selected.is_empty(),
            app.tabs[i].prev_selection.clone(),
            app.tabs[i].active_cmd.is_none(),
            last_line(&app),
        );
        assert_eq!(window, command_line);
        assert_eq!(window.0, HatchStyleType::Outer);
        assert_eq!(window.2, vec![hatch], "kept as Previous");
    }

    fn assert_command_line_hatchedit(app: &mut OpenCADStudio, hatch: Handle, channel: &str) {
        let i = app.active_tab;
        assert!(app.hatch_dialog.is_none(), "{channel}: no window");
        assert!(app.active_modal.is_none(), "{channel}: no window");
        assert_eq!(command_name(app, i), Some("HATCHEDIT"), "{channel}: the command runs");
        let _ = app.feed_command(StepInput::Text("S".into()));
        let _ = app.feed_command(StepInput::Text("O".into()));
        assert_eq!(stored(app, hatch).style, HatchStyleType::Outer, "{channel}: and works");
        assert!(app.tabs[i].active_cmd.is_none(), "{channel}");
    }

    #[test]
    fn dash_hatchedit_and_scripted_hatchedit_run_the_command_line() {
        let (mut app, hatch) = app_with_hatch();
        select_only(&mut app, hatch);
        let _ = app.dispatch_command("-HATCHEDIT");
        assert_command_line_hatchedit(&mut app, hatch, "-HATCHEDIT");

        let (mut app, hatch) = app_with_hatch();
        select_only(&mut app, hatch);
        app.scripted_dispatch = true;
        let _ = app.dispatch_command("HATCHEDIT");
        app.scripted_dispatch = false;
        assert_command_line_hatchedit(&mut app, hatch, "scripted_dispatch");

        let (mut app, hatch) = app_with_hatch();
        select_only(&mut app, hatch);
        let _ = app.update(Message::ScriptLine("HATCHEDIT".into()));
        assert!(!app.scripted_dispatch, "the flag is restored");
        assert_command_line_hatchedit(&mut app, hatch, "ScriptLine");
    }

    #[test]
    fn hatchedit_without_a_selection_still_asks_for_the_hatch() {
        let (mut app, _hatch) = app_with_hatch();
        let _ = app.dispatch_command("HATCHEDIT");
        let i = app.active_tab;
        assert!(app.hatch_dialog.is_none());
        assert_eq!(command_name(&app, i), Some("HATCHEDIT"));
        assert!(app.tabs[i].active_cmd.as_ref().unwrap().needs_entity_pick());
    }

    #[test]
    fn hatchedit_on_a_solid_or_gradient_opens_the_window_but_dash_hatchedit_does_not() {
        let mut app = app_with_rectangle();
        let solid = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
        select_only(&mut app, solid);
        let _ = app.dispatch_command("HATCHEDIT");
        assert_eq!(edit_handle(&app), Some(solid));
        let _ = app.update(Message::CloseModal);
        assert!(app.hatch_dialog.is_none());
        let _ = app.dispatch_command("-HATCHEDIT");
        assert!(app.hatch_dialog.is_none());
        assert_eq!(command_name(&app, app.active_tab), Some("HATCHEDIT"));

        let mut app = app_with_rectangle();
        let gradient = gradient_made(&mut app, &[]);
        select_only(&mut app, gradient);
        let _ = app.dispatch_command("HATCHEDIT");
        assert_eq!(edit_handle(&app), Some(gradient));
        assert_eq!(
            app.hatch_dialog.as_ref().unwrap().settings.tab,
            crate::modules::draw::draw::hatch_settings::FillTab::Gradient
        );
    }

    #[test]
    fn dash_hatchedit_and_scripted_hatchedit_run_the_command_line_on_a_solid_too() {
        let mut app = app_with_rectangle();
        let solid = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
        select_only(&mut app, solid);
        app.scripted_dispatch = true;
        let _ = app.dispatch_command("HATCHEDIT");
        app.scripted_dispatch = false;
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
        assert_eq!(command_name(&app, app.active_tab), Some("HATCHEDIT"));
    }

    #[test]
    fn hatchedit_on_two_selected_hatches_runs_the_command_line() {
        let (mut app, hatch) = app_with_hatch();
        let i = app.active_tab;
        let mut copy = stored(&app, hatch);
        copy.common.handle = Handle::NULL;
        let second = app.tabs[i].scene.add_entity(codec::EntityType::Hatch(copy));
        assert!(app.tabs[i].scene.hatches.contains_key(&second), "a second hatch");
        select_only(&mut app, hatch);
        app.tabs[i].scene.select_entity(second, false);
        assert_eq!(app.selected_handles(i).len(), 2);
        let _ = app.dispatch_command("HATCHEDIT");
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
        assert_eq!(command_name(&app, i), Some("HATCHEDIT"), "one hatch is asked for");
    }

    #[test]
    fn hatchedit_on_a_hatch_and_a_line_runs_the_command_line() {
        let (mut app, hatch) = app_with_hatch();
        let i = app.active_tab;
        let line = add_line(&mut app, 100.0, 0.0, 110.0, 0.0);
        select_only(&mut app, hatch);
        app.tabs[i].scene.select_entity(line, false);
        let _ = app.dispatch_command("HATCHEDIT");
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
    }

    /// `src/modules/draw/draw/hatchedit.rs` registers the pair.
    #[test]
    fn the_hatchedit_command_registers_its_dash_form() {
        let registered = inventory::iter::<crate::command::CommandRegistration>().any(|r| {
            r.names.contains(&"HATCHEDIT") && r.names.contains(&"-HATCHEDIT")
        });
        assert!(registered);
    }

    /// The one-shot list of `src/app/commands/mod.rs` names it next to
    /// `-INSERT` and `-HATCH`.
    #[test]
    fn the_known_command_list_names_dash_hatchedit() {
        let listed = inventory::iter::<crate::command::CommandRegistration>().any(|r| {
            r.names.contains(&"-INSERT")
                && r.names.contains(&"-HATCH")
                && r.names.contains(&"-HATCHEDIT")
        });
        assert!(listed);
    }

    // ── Double-click in the viewport ───────────────────────────────────────

    /// A viewport of 800 x 600 framing the drawing.
    fn frame(app: &mut OpenCADStudio) {
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().vp_size = (800.0, 600.0);
        app.tabs[i].scene.fit_all();
    }

    fn screen_point(app: &OpenCADStudio, x: f64, y: f64) -> iced::Point {
        let i = app.active_tab;
        let p = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(
                glam::DVec3::new(x, y, 0.0),
                iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
            )
            .expect("on screen");
        assert!(p.x > 0.0 && p.x < 800.0 && p.y > 0.0 && p.y < 600.0, "{p:?}");
        iced::Point::new(p.x, p.y)
    }

    fn double_click(app: &mut OpenCADStudio, x: f64, y: f64) {
        let p = screen_point(app, x, y);
        for _ in 0..2 {
            let _ = app.update(Message::ViewportMove(p));
            let _ = app.update(Message::ViewportLeftPress);
            let _ = app.update(Message::ViewportLeftRelease);
        }
    }

    #[test]
    fn double_clicking_inside_a_pattern_hatch_opens_the_edit_window() {
        let (mut app, hatch) = app_with_hatch();
        frame(&mut app);
        // Inside the fill, away from the boundary lines and the centre grip.
        double_click(&mut app, 4.0, 3.0);
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(edit_handle(&app), Some(hatch));
        assert_eq!(app.hatch_dialog.as_ref().unwrap().settings.pattern, "ANSI31");
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
    }

    #[test]
    fn double_clicking_near_the_edge_of_a_hatch_without_boundary_objects_opens_it_too() {
        let (mut app, hatch) = app_with_hatch();
        let i = app.active_tab;
        let lines: Vec<Handle> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter(|entity| matches!(entity, codec::EntityType::Line(_)))
            .map(|entity| entity.common().handle)
            .collect();
        assert_eq!(lines.len(), 4);
        app.tabs[i].scene.erase_entities(&lines);
        frame(&mut app);
        double_click(&mut app, 0.2, 9.8);
        assert_eq!(edit_handle(&app), Some(hatch));
    }

    #[test]
    fn double_clicking_a_solid_or_gradient_hatch_opens_the_window() {
        let mut app = app_with_rectangle();
        let solid = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
        frame(&mut app);
        double_click(&mut app, 4.0, 3.0);
        assert_eq!(edit_handle(&app), Some(solid));

        let mut app = app_with_rectangle();
        let gradient = gradient_made(&mut app, &[]);
        frame(&mut app);
        double_click(&mut app, 4.0, 3.0);
        assert_eq!(edit_handle(&app), Some(gradient));
        assert_eq!(
            app.hatch_dialog.as_ref().unwrap().settings.tab,
            crate::modules::draw::draw::hatch_settings::FillTab::Gradient
        );
    }

    #[test]
    fn double_clicking_a_hatch_on_a_locked_layer_opens_nothing() {
        let (mut app, _hatch) = app_with_hatch();
        let i = app.active_tab;
        app.tabs[i].scene.document.layers.get_mut("0").unwrap().flags.locked = true;
        frame(&mut app);
        double_click(&mut app, 4.0, 3.0);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
        assert!(last_line(&app).contains("locked"), "{}", last_line(&app));
    }

    fn click(app: &mut OpenCADStudio, x: f64, y: f64) {
        let p = screen_point(app, x, y);
        let _ = app.update(Message::ViewportMove(p));
        let _ = app.update(Message::ViewportLeftPress);
        let _ = app.update(Message::ViewportLeftRelease);
    }

    /// HATCHEDIT is past its pick step, and its options edit `hatch`.
    fn assert_hatchedit_picked(app: &mut OpenCADStudio, hatch: Handle) {
        let i = app.active_tab;
        let command = app.tabs[i].active_cmd.as_ref().expect("HATCHEDIT still runs");
        assert_eq!(command.name(), "HATCHEDIT");
        assert!(!command.needs_entity_pick(), "past the pick step: {}", last_line(app));
        let _ = app.feed_command(StepInput::Text("S".into()));
        let _ = app.feed_command(StepInput::Text("O".into()));
        assert_eq!(stored(app, hatch).style, HatchStyleType::Outer, "the picked hatch");
    }

    #[test]
    fn hatchedit_without_a_selection_picks_the_hatch_by_its_fill() {
        let (mut app, hatch) = app_with_hatch();
        frame(&mut app);
        let _ = app.dispatch_command("HATCHEDIT");
        assert!(app.tabs[app.active_tab].active_cmd.as_ref().unwrap().needs_entity_pick());
        click(&mut app, 4.0, 3.0);
        assert!(!last_line(&app).contains("Nothing found"), "{}", last_line(&app));
        assert_hatchedit_picked(&mut app, hatch);
    }

    #[test]
    fn hatchedit_picks_a_hatch_at_its_edge_when_no_object_bounds_it() {
        let (mut app, hatch) = app_with_hatch();
        let i = app.active_tab;
        let lines: Vec<Handle> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter(|entity| matches!(entity, codec::EntityType::Line(_)))
            .map(|entity| entity.common().handle)
            .collect();
        app.tabs[i].scene.erase_entities(&lines);
        frame(&mut app);
        let _ = app.dispatch_command("HATCHEDIT");
        click(&mut app, 0.2, 9.8);
        assert!(!last_line(&app).contains("not a hatch"), "{}", last_line(&app));
        assert_hatchedit_picked(&mut app, hatch);
    }

    // ── Double-click on the grip at the hatch centre ───────────────────────

    /// Snaps off, so a grip drag lands where the cursor goes.
    fn no_snaps(app: &mut OpenCADStudio) {
        app.snapper.snap_enabled = false;
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
    }

    #[test]
    fn double_clicking_the_centre_of_a_hatch_opens_the_window_not_a_grip_edit() {
        let (mut app, hatch) = app_with_hatch();
        frame(&mut app);
        // (10, 5) is the centroid: the pattern-origin grip of the selected hatch.
        double_click(&mut app, 10.0, 5.0);
        let i = app.active_tab;
        assert!(app.tabs[i].active_grip.is_none(), "no grip edit started");
        assert_eq!(edit_handle(&app), Some(hatch));
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn a_single_press_on_the_centre_grip_still_drags_it() {
        let (mut app, hatch) = app_with_hatch();
        no_snaps(&mut app);
        frame(&mut app);
        click(&mut app, 10.0, 5.0);
        let i = app.active_tab;
        assert!(app.tabs[i].scene.selected.contains(&hatch), "the first click selects it");
        // Time passes: the next press is not the second half of a double-click.
        app.last_vp_click_time = None;
        let before = stored(&app, hatch);
        let grip = screen_point(&app, 10.0, 5.0);
        let _ = app.update(Message::ViewportMove(grip));
        let _ = app.update(Message::ViewportLeftPress);
        assert!(app.tabs[i].active_grip.is_some(), "the grip is engaged");
        let _ = app.update(Message::ViewportMove(screen_point(&app, 12.0, 6.0)));
        let _ = app.update(Message::ViewportLeftRelease);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
        assert!(app.tabs[i].active_grip.is_none(), "the drag was committed");
        assert_ne!(stored(&app, hatch).pattern, before.pattern, "the pattern moved");
    }

    /// The hatch selected by a click away from its grips, as a gesture of
    /// some time ago.
    fn select_hatch_earlier(app: &mut OpenCADStudio, hatch: Handle) {
        click(app, 4.0, 3.0);
        assert!(app.tabs[app.active_tab].scene.selected.contains(&hatch));
        app.last_vp_click_time = None;
    }

    #[test]
    fn double_clicking_the_centre_of_an_already_selected_hatch_opens_the_window() {
        let (mut app, hatch) = app_with_hatch();
        no_snaps(&mut app);
        frame(&mut app);
        select_hatch_earlier(&mut app, hatch);
        let before = stored(&app, hatch);
        let dirty = app.tabs[app.active_tab].dirty;
        // The first click lands on the centre grip and makes it hot; the hand
        // drifts 5 px before the second (still a double-click), so the hot
        // grip has already dragged the pattern a little.
        let p = screen_point(&app, 10.0, 5.0);
        for at in [p, iced::Point::new(p.x + 4.0, p.y + 3.0)] {
            let _ = app.update(Message::ViewportMove(at));
            let _ = app.update(Message::ViewportLeftPress);
            let _ = app.update(Message::ViewportLeftRelease);
        }
        let i = app.active_tab;
        assert!(app.tabs[i].active_grip.is_none(), "the hot grip is dropped");
        assert!(!app.tabs[i].grip_base_pending);
        assert_eq!(edit_handle(&app), Some(hatch));
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(stored(&app, hatch), before, "the grip changed nothing");
        assert_eq!(app.tabs[i].dirty, dirty);
    }

    /// The rectangle with an associative SOLID hatch / gradient: they carry
    /// the pattern-origin grip like a pattern hatch does (`Grippable for
    /// Hatch`, grip 0 if associative).
    fn associative_solid() -> (OpenCADStudio, Handle) {
        let mut app = app_with_rectangle();
        let hatch = hatch_made_with(&mut app, &[Field::Pattern("SOLID".into())]);
        assert!(stored(&app, hatch).is_solid && stored(&app, hatch).is_associative);
        (app, hatch)
    }

    fn associative_gradient() -> (OpenCADStudio, Handle) {
        let mut app = app_with_rectangle();
        let hatch = gradient_made(&mut app, &[]);
        assert!(stored(&app, hatch).gradient_color.enabled && stored(&app, hatch).is_associative);
        (app, hatch)
    }

    fn double_clicking_the_centre_opens_the_window(app: &mut OpenCADStudio, hatch: Handle) {
        frame(app);
        // (10, 5) is the centroid: the grip of the selected hatch.
        double_click(app, 10.0, 5.0);
        let i = app.active_tab;
        assert!(app.tabs[i].active_grip.is_none(), "no grip edit started");
        assert_eq!(edit_handle(app), Some(hatch));
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
    }

    #[test]
    fn double_clicking_the_centre_of_a_solid_hatch_opens_the_window_not_a_grip_edit() {
        let (mut app, hatch) = associative_solid();
        double_clicking_the_centre_opens_the_window(&mut app, hatch);
        let settings = &app.hatch_dialog.as_ref().unwrap().settings;
        assert_eq!(settings.pattern, "SOLID");
    }

    #[test]
    fn double_clicking_the_centre_of_a_gradient_hatch_opens_the_window_not_a_grip_edit() {
        let (mut app, hatch) = associative_gradient();
        double_clicking_the_centre_opens_the_window(&mut app, hatch);
        assert_eq!(
            app.hatch_dialog.as_ref().unwrap().settings.tab,
            crate::modules::draw::draw::hatch_settings::FillTab::Gradient
        );
    }

    /// The hot-grip path: the hatch was selected earlier, the first click
    /// lands on the centre grip and makes it hot, the second is quick.
    fn double_clicking_the_hot_centre_grip_opens_the_window(app: &mut OpenCADStudio, hatch: Handle) {
        no_snaps(app);
        frame(app);
        select_hatch_earlier(app, hatch);
        let before = stored(app, hatch);
        let dirty = app.tabs[app.active_tab].dirty;
        let depth = undo_depth(app);
        let p = screen_point(app, 10.0, 5.0);
        for at in [p, iced::Point::new(p.x + 4.0, p.y + 3.0)] {
            let _ = app.update(Message::ViewportMove(at));
            let _ = app.update(Message::ViewportLeftPress);
            let _ = app.update(Message::ViewportLeftRelease);
        }
        let i = app.active_tab;
        assert!(app.tabs[i].active_grip.is_none(), "the hot grip is dropped");
        assert!(!app.tabs[i].grip_base_pending);
        assert_eq!(edit_handle(app), Some(hatch));
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(stored(app, hatch), before, "the grip changed nothing");
        assert_eq!(app.tabs[i].dirty, dirty);
        assert_eq!(undo_depth(app), depth, "and left no undo step");
    }

    #[test]
    fn double_clicking_the_centre_of_an_already_selected_solid_hatch_opens_the_window() {
        let (mut app, hatch) = associative_solid();
        double_clicking_the_hot_centre_grip_opens_the_window(&mut app, hatch);
    }

    #[test]
    fn double_clicking_the_centre_of_an_already_selected_gradient_hatch_opens_the_window() {
        let (mut app, hatch) = associative_gradient();
        double_clicking_the_hot_centre_grip_opens_the_window(&mut app, hatch);
    }

    #[test]
    fn a_hot_hatch_grip_is_still_placed_by_a_second_click_that_is_not_a_double_click() {
        // A slow second click, and a quick one too far away to be a double click.
        for pause in [true, false] {
            let (mut app, hatch) = app_with_hatch();
            no_snaps(&mut app);
            frame(&mut app);
            select_hatch_earlier(&mut app, hatch);
            let before = stored(&app, hatch);
            click(&mut app, 10.0, 5.0);
            let i = app.active_tab;
            assert!(app.tabs[i].active_grip.is_some(), "pause={pause}: the grip is hot");
            if pause {
                app.last_vp_click_time = None;
            }
            click(&mut app, 12.0, 6.0);
            assert!(app.tabs[i].active_grip.is_none(), "pause={pause}: placed");
            assert!(app.hatch_dialog.is_none() && app.active_modal.is_none(), "pause={pause}");
            assert_ne!(stored(&app, hatch).pattern, before.pattern, "pause={pause}: moved");
        }
    }

    #[test]
    fn double_clicking_the_hot_grip_of_another_object_places_it_as_before() {
        let (mut app, _hatch) = app_with_hatch();
        let line = add_line(&mut app, 30.0, 0.0, 40.0, 0.0);
        no_snaps(&mut app);
        frame(&mut app);
        click(&mut app, 33.0, 0.0);
        let i = app.active_tab;
        assert!(app.tabs[i].scene.selected.contains(&line));
        app.last_vp_click_time = None;
        let depth = undo_depth(&app);
        // The first click makes the midpoint grip hot; the quick second one
        // places it, as it always did (a grip edit, one undo step).
        double_click(&mut app, 35.0, 0.0);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
        assert!(app.tabs[i].active_grip.is_none(), "placed");
        assert_eq!(undo_depth(&app), depth + 1, "committed, not dropped");
    }

    #[test]
    fn double_clicking_a_grip_of_another_object_still_engages_it() {
        let (mut app, _hatch) = app_with_hatch();
        let line = add_line(&mut app, 30.0, 0.0, 40.0, 0.0);
        frame(&mut app);
        // (35, 0) is the line's midpoint grip once the first click selects it.
        double_click(&mut app, 35.0, 0.0);
        let i = app.active_tab;
        assert!(app.tabs[i].scene.selected.contains(&line));
        assert!(app.tabs[i].active_grip.is_some(), "the grip is engaged as before");
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
    }

    #[test]
    fn double_clicking_a_hatch_while_a_command_runs_opens_nothing() {
        let (mut app, _hatch) = app_with_hatch();
        frame(&mut app);
        let _ = app.dispatch_command("LINE");
        double_click(&mut app, 4.0, 3.0);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
        assert_eq!(command_name(&app, app.active_tab), Some("LINE"));
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
