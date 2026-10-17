//! The Undo and Group keywords of a "Select objects" prompt: the command asks
//! for a whole selection set (the previous one, or a named group's members)
//! and the host puts it on the scene.

use super::*;

impl OpenCADStudio {
    /// Make `set` the selection exactly (Undo), then tell the command.
    pub(super) fn handle_select_set(&mut self, set: Vec<Handle>) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].scene.deselect_all();
        self.tabs[i].scene.select_entities(&set);
        self.refresh_properties();
        self.command_line
            .push_info(crate::tf!("{} object(s) selected.", set.len()).as_ref());
        self.feed_command(StepInput::SelectionComplete(set))
    }

    /// Add (or, in Remove mode, take away) the members of the group `name`.
    pub(super) fn handle_select_group(&mut self, name: String) -> Task<Message> {
        let i = self.active_tab;
        let members: Option<Vec<Handle>> = self.tabs[i]
            .scene
            .groups()
            .find(|group| group.name.eq_ignore_ascii_case(name.trim()))
            .map(|group| group.entities.clone());
        let Some(members) = members else {
            self.command_line
                .push_error(crate::tf!("Group \"{name}\" not found.").as_ref());
            return Task::none();
        };
        let members: Vec<Handle> = members
            .into_iter()
            .filter(|handle| self.tabs[i].scene.document.get_entity(*handle).is_some())
            .collect();
        for handle in &members {
            if self.select_remove_mode {
                self.tabs[i].scene.deselect_entity(*handle);
            } else {
                self.tabs[i].scene.select_entity(*handle, false);
            }
        }
        self.refresh_properties();
        let selected: Vec<Handle> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(handle, _)| handle)
            .collect();
        self.command_line
            .push_info(crate::tf!("{} object(s) in the group.", members.len()).as_ref());
        self.feed_command(StepInput::SelectionComplete(selected))
    }
}
