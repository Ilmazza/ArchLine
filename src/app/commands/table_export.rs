//! TABLEEXPORT: writes a table to a CSV file.
//!
//! The text is the one `DATALINKUPDATE WRITE` writes back to a linked source
//! (`display::table_to_csv`), so a table exported and re-attached with DATALINK
//! comes back as it was.

use crate::app::{Message, OpenCADStudio};
use crate::command::{CadCommand, CmdResult};
use codec::{CadDocument, EntityType, Handle};
use glam::DVec3;
use iced::Task;
use std::path::{Path, PathBuf};

/// Writes the table `handle` to `path`.
pub(crate) fn export_table(doc: &CadDocument, handle: Handle, path: &Path) -> Result<(), String> {
    let Some(EntityType::Table(table)) = doc.get_entity(handle) else {
        return Err(crate::t!("TABLEEXPORT: the object is not a table.").into_owned());
    };
    std::fs::write(path, super::display::table_to_csv(table)).map_err(|error| error.to_string())
}

/// Waits for the user to pick the table to export.
struct TableExportPickCommand;

impl CadCommand for TableExportPickCommand {
    fn name(&self) -> &'static str {
        "TABLEEXPORT"
    }
    fn prompt(&self) -> String {
        crate::t!("TABLEEXPORT  Select a table:").into_owned()
    }
    fn needs_entity_pick(&self) -> bool {
        true
    }
    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        CmdResult::Relaunch(format!("TABLEEXPORT_BEGIN:{}", handle.value()), vec![handle])
    }
    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

impl OpenCADStudio {
    pub(super) fn dispatch_table_export(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        if cmd == "TABLEEXPORT" {
            let selected: Vec<Handle> = self.tabs[i]
                .scene
                .selected_entities()
                .into_iter()
                .filter_map(|(handle, entity)| matches!(entity, EntityType::Table(_)).then_some(handle))
                .collect();
            if let [handle] = selected[..] {
                return self.dispatch_table_export(&format!("TABLEEXPORT_BEGIN:{}", handle.value()), i);
            }
            let pick = TableExportPickCommand;
            self.command_line.push_info(&pick.prompt());
            self.tabs[i].active_cmd = Some(Box::new(pick));
            return Some(Task::none());
        }
        let handle = Handle::new(cmd.strip_prefix("TABLEEXPORT_BEGIN:")?.parse().ok()?);
        if !matches!(self.tabs[i].scene.document.get_entity(handle), Some(EntityType::Table(_))) {
            self.command_line
                .push_error(crate::t!("TABLEEXPORT: the object is not a table.").as_ref());
            return Some(Task::none());
        }
        Some(Task::perform(
            async {
                crate::sys::file_dialog()
                    .set_title(crate::t!("Export table").as_ref())
                    .set_file_name("table.csv")
                    .add_filter(crate::t!("CSV").as_ref(), &["csv"])
                    .save_file()
                    .await
                    .map(|file| crate::sys::handle_path(&file))
            },
            move |path| Message::TableExportPath(handle, path),
        ))
    }

    /// The save dialog answered: write the table, or say nothing was written.
    pub(crate) fn on_table_export_result(&mut self, handle: Handle, path: Option<PathBuf>) -> Task<Message> {
        let Some(path) = path else {
            self.command_line.push_info(crate::t!("TABLEEXPORT: cancelled.").as_ref());
            return Task::none();
        };
        match export_table(&self.tabs[self.active_tab].scene.document, handle, &path) {
            Ok(()) => self
                .command_line
                .push_output(crate::tf!("TABLEEXPORT: wrote {}.", path.display()).as_ref()),
            Err(error) => self.command_line.push_error(&error),
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::{Line, TableBuilder};
    use codec::types::Vector3;

    fn fresh_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn add_table(app: &mut OpenCADStudio) -> Handle {
        let mut table = TableBuilder::new(2, 2).at(Vector3::ZERO).build();
        table.set_cell_text(0, 0, "Porta");
        table.set_cell_text(0, 1, "90");
        table.set_cell_text(1, 0, "a,b");
        table.set_cell_text(1, 1, "dice \"x\"");
        app.tabs[app.active_tab].scene.add_entity(EntityType::Table(Box::new(table)))
    }

    fn temp_csv(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("archline-tableexport-{tag}-{}.csv", std::process::id()))
    }

    fn output(app: &OpenCADStudio, from: usize) -> String {
        app.command_line.history[from..].iter().map(|e| e.text.as_str()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_table_is_written_as_csv_with_quoted_fields() {
        let mut app = fresh_app();
        let handle = add_table(&mut app);
        let path = temp_csv("write");
        export_table(&app.tabs[app.active_tab].scene.document, handle, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(text, "Porta,90\r\n\"a,b\",\"dice \"\"x\"\"\"");
    }

    #[test]
    fn exporting_something_that_is_not_a_table_is_refused() {
        let mut app = fresh_app();
        let line = app.tabs[app.active_tab]
            .scene
            .add_entity(EntityType::Line(Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0))));
        let path = temp_csv("refuse");
        let error = export_table(&app.tabs[app.active_tab].scene.document, line, &path).unwrap_err();
        assert!(!path.exists());
        assert!(error.to_lowercase().contains("table"), "got {error:?}");
    }

    #[test]
    fn the_dialog_answer_writes_the_file_and_reports_it() {
        let mut app = fresh_app();
        let handle = add_table(&mut app);
        let path = temp_csv("result");
        let from = app.command_line.history.len();
        let _ = app.on_table_export_result(handle, Some(path.clone()));
        let written = path.exists();
        let _ = std::fs::remove_file(&path);
        assert!(written);
        assert!(output(&app, from).contains("TABLEEXPORT"), "got {:?}", output(&app, from));
    }

    #[test]
    fn a_cancelled_dialog_writes_nothing() {
        let mut app = fresh_app();
        let handle = add_table(&mut app);
        let from = app.command_line.history.len();
        let _ = app.on_table_export_result(handle, None);
        assert!(output(&app, from).to_lowercase().contains("cancel"), "got {:?}", output(&app, from));
    }

    #[test]
    fn without_a_selected_table_the_command_asks_for_one() {
        let mut app = fresh_app();
        add_table(&mut app);
        let from = app.command_line.history.len();
        let _ = app.run_command_line("TABLEEXPORT");
        assert!(app.tabs[app.active_tab].active_cmd.is_some(), "a pick command should be active");
        assert!(output(&app, from).to_lowercase().contains("select a table"), "got {:?}", output(&app, from));
    }

    #[test]
    fn a_selected_table_goes_straight_to_the_dialog() {
        let mut app = fresh_app();
        let handle = add_table(&mut app);
        app.tabs[app.active_tab].scene.select_entity(handle, true);
        let i = app.active_tab;
        assert!(app.dispatch_table_export("TABLEEXPORT", i).is_some(), "the command should claim TABLEEXPORT");
        assert!(app.tabs[i].active_cmd.is_none(), "no pick should be needed");
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["TABLEEXPORT"] });
