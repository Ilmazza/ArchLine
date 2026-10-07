//! Recent-files list backing the Start page's Recent Documents panel. The list
//! itself lives in the consolidated app config (native `settings.json` or web
//! `localStorage`, in the "recent" section); this module mutates the in-memory
//! list, persists it via `save_config`, and evicts matching web OPFS copies.

use super::OpenCADStudio;
use std::path::{Path, PathBuf};

/// Bounds and default for how many recent files are kept.
pub(super) const RECENT_MIN: usize = 5;
pub(super) const RECENT_MAX: usize = 100;
pub(super) const RECENT_DEFAULT: usize = 20;

impl OpenCADStudio {
    /// Record a freshly opened file at the top of the recents list. Returns
    /// the background task that decodes its thumbnail.
    pub(super) fn push_recent(&mut self, path: PathBuf) -> iced::Task<crate::app::Message> {
        self.recent_files.retain(|r| r != &path);
        self.recent_thumbs.remove(&path);
        self.recent_files.insert(0, path);
        let evicted = self
            .recent_files
            .split_off(self.recent_limit.min(self.recent_files.len()));
        for path in &evicted {
            self.recent_thumbs.remove(path);
        }
        remove_cached_copies(evicted);
        self.save_config();
        self.refresh_recent_thumbs()
    }

    /// Load what the Start page shows for the recents and has not got yet: the
    /// preview thumbnails and (ArchLine) the files' modification dates.
    pub(super) fn refresh_recent_thumbs(&mut self) -> iced::Task<crate::app::Message> {
        iced::Task::batch([self.load_missing_thumbs(), self.refresh_recent_dates()])
    }

    /// Recent files whose modification date has not been looked up yet.
    pub(crate) fn recent_files_without_date(&self) -> Vec<PathBuf> {
        self.recent_files
            .iter()
            .filter(|p| !self.start_ui.dates.contains_key(*p))
            .cloned()
            .collect()
    }

    /// Read the missing modification dates on a background thread, delivering
    /// them via [`Message::Start`]. The browser cannot stat a picked file, so
    /// the web build shows no dates.
    fn refresh_recent_dates(&mut self) -> iced::Task<crate::app::Message> {
        #[cfg(target_arch = "wasm32")]
        {
            iced::Task::none()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let missing = self.recent_files_without_date();
            if missing.is_empty() {
                return iced::Task::none();
            }
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(read_dates(&missing));
            });
            iced::Task::perform(
                async move { rx.await.unwrap_or_default() },
                |dates| {
                    crate::app::Message::Start(crate::ui::classic_start::StartMsg::DatesLoaded(
                        dates,
                    ))
                },
            )
        }
    }

    /// Decode any not-yet-cached DWG preview thumbnails for the current
    /// recents on a background thread, delivering them via
    /// [`Message::RecentThumbsLoaded`]. The synchronous version of this ran on
    /// the boot path and stalled the first frame for as long as it took to
    /// parse every recent DWG's preview — the Start page appeared seconds
    /// late. Cached per path (a `None` result is cached too); safe to call
    /// repeatedly.
    fn load_missing_thumbs(&mut self) -> iced::Task<crate::app::Message> {
        let missing: Vec<std::path::PathBuf> = self
            .recent_files
            .iter()
            .filter(|p| !self.recent_thumbs.contains_key(*p))
            .cloned()
            .collect();
        if missing.is_empty() {
            return iced::Task::none();
        }
        #[cfg(target_arch = "wasm32")]
        {
            return iced::Task::perform(
                async move {
                    let mut thumbnails = Vec::with_capacity(missing.len());
                    for path in missing {
                        let handle = if let Some(name) = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                        {
                            crate::io::web_recent::read_thumbnail(&name)
                                .await
                                .ok()
                                .flatten()
                                .map(|thumbnail| {
                                    iced::widget::image::Handle::from_rgba(
                                        thumbnail.width,
                                        thumbnail.height,
                                        thumbnail.rgba,
                                    )
                                })
                        } else {
                            None
                        };
                        thumbnails.push((path, handle));
                    }
                    thumbnails
                },
                crate::app::Message::RecentThumbsLoaded,
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let thumbs: Vec<_> = missing
                    .into_iter()
                    .map(|p| {
                        let h = crate::io::thumbnail::read_handle(&p);
                        (p, h)
                    })
                    .collect();
                let _ = tx.send(thumbs);
            });
            iced::Task::perform(
                async move { rx.await.unwrap_or_default() },
                crate::app::Message::RecentThumbsLoaded,
            )
        }
    }

    /// Drop a path from the recents list (manual removal from the Start page).
    pub(super) fn remove_recent(&mut self, path: &Path) {
        self.recent_files.retain(|r| r.as_path() != path);
        self.recent_thumbs.remove(path);
        self.start_ui.dates.remove(path);
        remove_cached_copies([path.to_path_buf()]);
        self.save_config();
    }

    /// Set how many recent files are kept, trim the current list to fit, and
    /// persist both.
    pub(super) fn set_recent_limit(&mut self, limit: usize) {
        self.recent_limit = limit.clamp(RECENT_MIN, RECENT_MAX);
        let evicted = self
            .recent_files
            .split_off(self.recent_limit.min(self.recent_files.len()));
        for path in &evicted {
            self.recent_thumbs.remove(path);
        }
        remove_cached_copies(evicted);
        self.save_config();
    }
}

/// The modification date of each file, `None` when it cannot be read (moved,
/// deleted, a drive that is not connected).
pub(crate) fn read_dates(paths: &[PathBuf]) -> Vec<(PathBuf, Option<std::time::SystemTime>)> {
    paths
        .iter()
        .map(|p| (p.clone(), std::fs::metadata(p).and_then(|m| m.modified()).ok()))
        .collect()
}

fn remove_cached_copies(paths: impl IntoIterator<Item = PathBuf>) {
    #[cfg(not(target_arch = "wasm32"))]
    let _ = paths;

    #[cfg(target_arch = "wasm32")]
    for path in paths {
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::io::web_recent::remove(&name).await;
        });
    }
}
