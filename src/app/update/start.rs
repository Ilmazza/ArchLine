//! ArchLine: update handling for the Start page (classic workspace).

use crate::app::{Message, OpenCADStudio};
use crate::ui::classic_start::StartMsg;

impl OpenCADStudio {
    pub(super) fn on_start(&mut self, m: StartMsg) -> iced::Task<Message> {
        self.start_ui.update(m);
        iced::Task::none()
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};
    use crate::ui::classic_start::{RecentSort, RecentView, StartMsg, StartNav};

    #[test]
    fn start_messages_reach_the_start_page_state() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.update(Message::Start(StartMsg::Sort(RecentSort::Name)));
        let _ = app.update(Message::Start(StartMsg::View(RecentView::List)));
        let _ = app.update(Message::Start(StartMsg::Search("rilievo".into())));
        let _ = app.update(Message::Start(StartMsg::Nav(StartNav::Learning)));
        assert_eq!(app.start_ui.sort, RecentSort::Name);
        assert_eq!(app.start_ui.view, RecentView::List);
        assert_eq!(app.start_ui.search, "rilievo");
        assert_eq!(app.start_ui.nav, StartNav::Learning);
    }

    #[test]
    fn the_modification_dates_of_the_recent_files_are_read_off_the_ui_thread() {
        use std::time::SystemTime;
        let dir = std::env::temp_dir().join(format!("archline-recent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let there = dir.join("there.dwg");
        std::fs::write(&there, b"x").unwrap();
        let gone = dir.join("gone.dwg");
        let dates = crate::app::recent::read_dates(&[there.clone(), gone.clone()]);
        assert_eq!(dates.len(), 2);
        assert!(matches!(dates[0], (ref p, Some(t)) if *p == there && t <= SystemTime::now()));
        assert_eq!(dates[1], (gone, None), "a file that is not there has no date");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_recent_file_gets_its_date_requested_and_a_loaded_one_is_not_read_again() {
        let mut app = OpenCADStudio::new_for_test();
        app.recent_files = vec!["C:/x/a.dwg".into(), "C:/x/b.dwg".into()];
        app.start_ui.dates.insert("C:/x/a.dwg".into(), None);
        let missing = app.recent_files_without_date();
        assert_eq!(missing, vec![std::path::PathBuf::from("C:/x/b.dwg")]);
    }
}
