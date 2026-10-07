//! ArchLine: the Start page in the style of AutoCAD 2025 (classic workspace).
//!
//! A sidebar on the left (title, Open / New, the sections, Options and
//! Plugins), the Recent documents in the middle as cards with a sort and a
//! search, and an optional column on the right that only has content when
//! `ARCHLINE_ONLINE` / `ARCHLINE_PROMO` are on. This file holds the state, the
//! pure rules (filter, sort, date format) and the views.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// How the recent documents are ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecentSort {
    /// The order of the recent list: the file opened last comes first.
    #[default]
    LastOpened,
    Name,
    /// The file's modification date, newest first.
    Modified,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecentView {
    #[default]
    Grid,
    List,
}

/// The section chosen in the sidebar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StartNav {
    #[default]
    Recent,
    Learning,
}

#[derive(Clone, Debug)]
pub enum StartMsg {
    Nav(StartNav),
    Sort(RecentSort),
    View(RecentView),
    Search(String),
    /// Modification dates read off the UI thread; `None` = no such file.
    DatesLoaded(Vec<(PathBuf, Option<SystemTime>)>),
}

/// State of the Start page that is not part of the app's settings.
#[derive(Debug, Default)]
pub struct StartUi {
    pub nav: StartNav,
    pub sort: RecentSort,
    pub view: RecentView,
    pub search: String,
    pub dates: HashMap<PathBuf, Option<SystemTime>>,
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}

impl StartUi {
    pub fn update(&mut self, msg: StartMsg) {
        match msg {
            StartMsg::Nav(n) => self.nav = n,
            StartMsg::Sort(s) => self.sort = s,
            StartMsg::View(v) => self.view = v,
            StartMsg::Search(s) => self.search = s,
            StartMsg::DatesLoaded(dates) => self.dates.extend(dates),
        }
    }

    /// The recent files to show: those whose name contains the search text
    /// (ignoring case), in the chosen order.
    pub fn visible<'a>(&self, recents: &'a [PathBuf]) -> Vec<&'a PathBuf> {
        let needle = self.search.trim().to_lowercase();
        let mut out: Vec<&PathBuf> = recents
            .iter()
            .filter(|p| needle.is_empty() || file_name(p).to_lowercase().contains(&needle))
            .collect();
        match self.sort {
            RecentSort::LastOpened => {}
            RecentSort::Name => out.sort_by_key(|p| file_name(p).to_lowercase()),
            RecentSort::Modified => {
                let date = |p: &PathBuf| self.dates.get(p).copied().flatten();
                // Newest first; no date (never read, or no such file) after the dated ones.
                out.sort_by(|a, b| match (date(a), date(b)) {
                    (Some(x), Some(y)) => y.cmp(&x),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                });
            }
        }
        out
    }
}

/// `dd/mm/yyyy` of a Unix time shifted by the zone's offset (both in seconds).
pub fn format_date(unix_secs: i64, offset_secs: i64) -> String {
    let days = (unix_secs + offset_secs).div_euclid(86_400);
    // Howard Hinnant's civil_from_days: days since 1970-01-01 to a Gregorian date.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{day:02}/{month:02}/{year:04}")
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

use iced::widget::{
    button, column, container, hover, pick_list, row, scrollable, stack, text, text_input, Space,
};
use iced::{Background, Border, Element, Length, Theme};

use crate::app::Message;

/// Narrower than this the page falls back to the tabbed layout.
pub const MIN_WIDTH: f32 = 760.0;
/// Width of the sidebar.
pub const SIDEBAR_W: f32 = 280.0;
/// A recent-document card: width, height of its preview, gap between cards.
pub const CARD_W: f32 = 196.0;
pub const PREVIEW_H: f32 = 140.0;
pub const CARD_GAP: f32 = 12.0;

/// Everything the page reads from the app.
pub struct StartData<'a> {
    pub recents: &'a [PathBuf],
    pub thumbs: &'a HashMap<PathBuf, Option<iced::widget::image::Handle>>,
    pub recent_limit: usize,
    pub recent_limit_input: &'a str,
    pub recent_max: usize,
    pub videos: &'a [crate::videos::VideoEntry],
    pub videos_loading: bool,
    pub video_thumbs: &'a HashMap<String, iced::widget::image::Handle>,
    pub discussions: &'a [crate::discussions::DiscussionEntry],
    pub discussions_loading: bool,
    pub patrons: &'a [(String, i64)],
    pub ui: &'a StartUi,
}

impl std::fmt::Display for RecentSort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            RecentSort::LastOpened => crate::t!("Last opened"),
            RecentSort::Name => crate::t!("Name"),
            RecentSort::Modified => crate::t!("Modified"),
        };
        f.write_str(label.as_ref())
    }
}

const SORTS: [RecentSort; 3] = [RecentSort::LastOpened, RecentSort::Name, RecentSort::Modified];

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.68)),
    }
}

/// The whole page: sidebar, the chosen section, the optional right column.
#[inline(never)]
pub fn page<'a>(d: StartData<'a>, _width: f32) -> Element<'a, Message> {
    let right = right_column(&d);
    let middle: Element<'a, Message> = match d.ui.nav {
        StartNav::Learning if crate::privacy::online() => learning(&d),
        _ => recent(&d),
    };
    let mut body = row![sidebar(&d), middle].spacing(0).height(Length::Fill);
    if let Some(right) = right {
        body = body.push(right);
    }
    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.base.color)),
            ..Default::default()
        })
        .into()
}

fn outline_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let pair = match status {
        button::Status::Hovered | button::Status::Pressed => palette.background.strong,
        _ => palette.background.weak,
    };
    button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}

fn wide_button<'a>(label: String, msg: Message) -> Element<'a, Message> {
    button(text(label).size(14))
        .on_press(msg)
        .padding([10, 16])
        .width(Length::Fill)
        .style(outline_style)
        .into()
}

fn nav_item<'a>(label: String, nav: StartNav, active: bool) -> Element<'a, Message> {
    button(text(label).size(14))
        .on_press(Message::Start(StartMsg::Nav(nav)))
        .padding([12, 16])
        .width(Length::Fill)
        .style(move |theme: &Theme, status| {
            let palette = theme.palette();
            let lit = active || matches!(status, button::Status::Hovered);
            button::Style {
                background: lit.then_some(Background::Color(if active {
                    palette.background.strong.color
                } else {
                    palette.background.weak.color
                })),
                text_color: palette.background.base.text,
                border: Border::default(),
                ..Default::default()
            }
        })
        .into()
}

fn link_button<'a>(label: String, msg: Message) -> Element<'a, Message> {
    button(text(label).size(13))
        .on_press(msg)
        .padding([4, 0])
        .style(|theme: &Theme, status| {
            let palette = theme.palette();
            button::Style {
                background: None,
                text_color: match status {
                    button::Status::Hovered => palette.primary.strong.color,
                    _ => palette.primary.base.color,
                },
                border: Border::default(),
                ..Default::default()
            }
        })
        .into()
}

#[inline(never)]
fn sidebar<'a>(d: &StartData<'a>) -> Element<'a, Message> {
    let online = crate::privacy::online();
    let mut nav = column![nav_item(
        crate::t!("Recent").into_owned(),
        StartNav::Recent,
        d.ui.nav == StartNav::Recent
    )];
    if online {
        nav = nav.push(nav_item(
            crate::t!("Learning").into_owned(),
            StartNav::Learning,
            d.ui.nav == StartNav::Learning,
        ));
    }
    let mut links = column![
        link_button(crate::tr!("action", "options"), Message::OptionsOpen),
        link_button(crate::tr!("action", "plugins"), Message::PluginManagerOpen),
    ]
    .spacing(6);
    if online {
        links = links
            .push(link_button(
                crate::t!("Online help").into_owned(),
                Message::RibbonToolClick {
                    tool_id: "HELP".to_string(),
                    event: crate::modules::ModuleEvent::Command("HELP".to_string()),
                },
            ))
            .push(link_button(
                crate::t!("Community forum").into_owned(),
                Message::OpenUrl(crate::discussions::DISCUSSIONS_URL.to_string()),
            ));
    }
    container(
        column![
            text(crate::privacy::APP_NAME)
                .size(32)
                .style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.palette().background.base.text),
                }),
            Space::new().height(24),
            wide_button(crate::tr!("start", "open-file"), Message::OpenFile),
            Space::new().height(10),
            wide_button(crate::tr!("start", "new-drawing"), Message::TabNew),
            Space::new().height(24),
            nav,
            Space::new().height(Length::Fill),
            container(links).padding([0, 16]),
        ]
        .height(Length::Fill),
    )
    .width(Length::Fixed(SIDEBAR_W))
    .height(Length::Fill)
    .padding([32, 24])
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.weak.color)),
        ..Default::default()
    })
    .into()
}

#[inline(never)]
fn recent<'a>(d: &StartData<'a>) -> Element<'a, Message> {
    let ui = d.ui;
    let view_btn = |view: RecentView, icon: &'static [u8]| {
        let active = ui.view == view;
        button(crate::ui::icons::themed(icon, 14.0))
            .on_press(Message::Start(StartMsg::View(view)))
            .padding([6, 8])
            .style(move |theme: &Theme, status| {
                let palette = theme.palette();
                let lit = active || matches!(status, button::Status::Hovered);
                button::Style {
                    background: lit.then_some(Background::Color(palette.background.strong.color)),
                    text_color: palette.background.base.text,
                    border: Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
    };
    let toolbar = row![
        view_btn(RecentView::List, crate::ui::icons::MENU),
        view_btn(RecentView::Grid, crate::ui::icons::GRID),
        Space::new().width(12),
        text(crate::t!("Sort by").into_owned()).size(13),
        pick_list(Some(ui.sort), SORTS, |s: &RecentSort| s.to_string())
            .on_select(|s| Message::Start(StartMsg::Sort(s)))
            .text_size(13),
        Space::new().width(Length::Fill),
        text_input(crate::t!("Search").as_ref(), &ui.search)
            .on_input(|s| Message::Start(StartMsg::Search(s)))
            .size(13)
            .width(Length::Fixed(220.0)),
    ]
    .spacing(8)
    .align_y(iced::Center);

    let shown = ui.visible(d.recents);
    let gutter = iced::Padding {
        right: 16.0,
        ..iced::Padding::ZERO
    };
    let body: Element<'a, Message> = if d.recents.is_empty() {
        text(crate::tr!("start", "no-recent-files"))
            .size(13)
            .style(muted)
            .into()
    } else if shown.is_empty() {
        text(crate::t!("No document matches the search").into_owned())
            .size(13)
            .style(muted)
            .into()
    } else if ui.view == RecentView::Grid {
        let cards: Vec<Element<'a, Message>> = shown.iter().map(|p| card(p, d)).collect();
        scrollable(
            container(
                iced::widget::Row::with_children(cards)
                    .spacing(CARD_GAP)
                    .wrap()
                    .vertical_spacing(CARD_GAP),
            )
            .padding(gutter),
        )
        .height(Length::Fill)
        .into()
    } else {
        let rows = shown.iter().map(|p| list_row(p, d));
        scrollable(column(rows).spacing(2).padding(gutter))
            .height(Length::Fill)
            .into()
    };
    container(
        column![
            text(crate::t!("Recent").into_owned()).size(26),
            Space::new().height(16),
            toolbar,
            Space::new().height(16),
            body,
            Space::new().height(12),
            keep_row(d),
        ]
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding([32, 32])
    .into()
}

fn date_text(p: &Path, d: &StartData<'_>) -> Option<String> {
    let modified = d.ui.dates.get(p).copied().flatten()?;
    let secs = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    let offset = (crate::entities::field::utc_offset_days() * 86_400.0).round() as i64;
    Some(format_date(secs, offset))
}

fn preview<'a>(p: &Path, d: &StartData<'a>, w: Length, h: Length) -> Element<'a, Message> {
    let inner: Element<'a, Message> = match d.thumbs.get(p).and_then(|o| o.as_ref()) {
        Some(handle) => iced::widget::image(handle.clone())
            .content_fit(iced::ContentFit::Contain)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        None => crate::ui::icons::themed_secondary(crate::ui::icons::DOC, 48.0),
    };
    container(inner)
        .width(w)
        .height(h)
        .center_x(w)
        .center_y(h)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.base.color)),
            ..Default::default()
        })
        .into()
}

fn remove_button<'a>(p: &Path) -> Element<'a, Message> {
    button(crate::ui::icons::themed(crate::ui::icons::CLOSE, 11.0))
        .on_press(Message::RecentRemove(p.to_path_buf()))
        .padding([4, 6])
        .style(|theme: &Theme, status| {
            let palette = theme.palette();
            button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered => palette.danger.weak.color,
                    _ => palette.background.strong.color,
                })),
                text_color: palette.background.base.text,
                border: Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .into()
}

/// One recent document as a card; the cross that drops it from the list shows
/// on hover.
fn card<'a>(p: &Path, d: &StartData<'a>) -> Element<'a, Message> {
    let make = |with_remove: bool| -> Element<'a, Message> {
        let name = crate::ui::text_util::elide(&file_name(p), 44);
        let info = column![
            text(name).size(13).height(Length::Fixed(34.0)),
            text(date_text(p, d).unwrap_or_default()).size(11).style(muted),
        ]
        .spacing(2);
        let body = button(column![
            preview(p, d, Length::Fixed(CARD_W), Length::Fixed(PREVIEW_H)),
            container(info).padding([8, 10]).width(Length::Fixed(CARD_W)),
        ])
        .on_press(Message::OpenRecent(p.to_path_buf()))
        .padding(0)
        .style(|theme: &Theme, status| {
            let palette = theme.palette();
            button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered => palette.background.strong.color,
                    _ => palette.background.weak.color,
                })),
                text_color: palette.background.base.text,
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        });
        if with_remove {
            stack![
                body,
                container(remove_button(p))
                    .width(Length::Fixed(CARD_W))
                    .padding(6)
                    .align_x(iced::alignment::Horizontal::Right),
            ]
            .into()
        } else {
            body.into()
        }
    };
    hover(make(false), make(true)).into()
}

fn list_row<'a>(p: &Path, d: &StartData<'a>) -> Element<'a, Message> {
    let dir = p
        .parent()
        .map(|x| x.to_string_lossy().into_owned())
        .unwrap_or_default();
    let open = button(
        row![
            preview(p, d, Length::Fixed(60.0), Length::Fixed(44.0)),
            column![
                text(crate::ui::text_util::elide(&file_name(p), 60)).size(13),
                text(crate::ui::text_util::elide(&dir, 80))
                    .size(11)
                    .style(muted),
            ]
            .spacing(2)
            .width(Length::Fill),
            text(date_text(p, d).unwrap_or_default()).size(12).style(muted),
        ]
        .spacing(12)
        .align_y(iced::Center),
    )
    .on_press(Message::OpenRecent(p.to_path_buf()))
    .padding([4, 8])
    .width(Length::Fill)
    .style(|theme: &Theme, status| {
        let palette = theme.palette();
        button::Style {
            background: matches!(status, button::Status::Hovered)
                .then_some(Background::Color(palette.background.weak.color)),
            text_color: palette.background.base.text,
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    });
    row![open, remove_button(p)]
        .spacing(4)
        .align_y(iced::Center)
        .into()
}

/// Keep N recent files: the same control the old page had.
fn keep_row<'a>(d: &StartData<'a>) -> Element<'a, Message> {
    const STEP: usize = 5;
    let shown = d.recent_limit_input.parse::<usize>().unwrap_or(d.recent_limit);
    let step_btn = |icon: &'static [u8], to: usize| {
        button(crate::ui::icons::themed(icon, 11.0))
            .on_press(Message::SetRecentLimit(to))
            .padding([3, 6])
            .style(outline_style)
    };
    row![
        text(crate::tr!("start", "keep-recent-files"))
            .size(11)
            .style(muted),
        step_btn(crate::ui::icons::MINUS, shown.saturating_sub(STEP)),
        text_input("", d.recent_limit_input)
            .on_input(Message::RecentLimitInput)
            .on_submit(Message::SetRecentLimit(shown))
            .size(12)
            .padding([2, 6])
            .width(Length::Fixed(46.0)),
        step_btn(crate::ui::icons::PLUS, shown.saturating_add(STEP)),
        text(format!("/ {}", d.recent_max)).size(11).style(muted),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

/// Learning: the tutorial videos (online only).
#[inline(never)]
fn learning<'a>(d: &StartData<'a>) -> Element<'a, Message> {
    let mut list = column![].spacing(10);
    for v in d.videos {
        let mut card = column![].spacing(6).width(Length::Fixed(CARD_W));
        if let Some(handle) = d.video_thumbs.get(&v.id) {
            card = card.push(
                iced::widget::image(handle.clone())
                    .width(Length::Fixed(CARD_W))
                    .height(Length::Fixed(CARD_W * 9.0 / 16.0))
                    .content_fit(iced::ContentFit::Contain),
            );
        }
        card = card.push(text(v.title.clone()).size(12));
        list = list.push(
            iced::widget::mouse_area(card)
                .interaction(iced::mouse::Interaction::Pointer)
                .on_press(Message::OpenUrl(crate::videos::watch_url(&v.id))),
        );
    }
    if d.videos.is_empty() {
        let note = if d.videos_loading {
            crate::tr!("start", "loading-videos")
        } else {
            crate::tr!("start", "videos-online")
        };
        list = list.push(text(note).size(13).style(muted));
    }
    container(
        column![
            text(crate::t!("Learning").into_owned()).size(26),
            Space::new().height(16),
            link_button(
                crate::tr!("start", "open-playlist"),
                Message::OpenUrl(crate::videos::PLAYLIST_URL.to_string())
            ),
            Space::new().height(12),
            scrollable(list).height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding([32, 32])
    .into()
}

/// Discussions (`ARCHLINE_ONLINE=1`) and supporters (`ARCHLINE_PROMO=1`);
/// `None` (no column at all) when both are off.
#[inline(never)]
fn right_column<'a>(d: &StartData<'a>) -> Option<Element<'a, Message>> {
    let online = crate::privacy::online();
    let promo = crate::privacy::show_promo();
    if !online && !promo {
        return None;
    }
    let mut col = column![].spacing(10).width(Length::Fill);
    if online {
        col = col.push(text(crate::tr!("start", "discussions")).size(18));
        for disc in d.discussions {
            col = col.push(
                iced::widget::mouse_area(
                    column![
                        text(disc.title.clone()).size(12),
                        text(format!("#{}", disc.number)).size(10).style(muted),
                    ]
                    .spacing(2),
                )
                .interaction(iced::mouse::Interaction::Pointer)
                .on_press(Message::OpenUrl(disc.url.clone())),
            );
        }
        if d.discussions.is_empty() {
            let note = if d.discussions_loading {
                crate::tr!("start", "loading-discussions")
            } else {
                crate::tr!("start", "discussions-online")
            };
            col = col.push(text(note).size(12).style(muted));
        }
    }
    if promo {
        col = col.push(text(crate::tr!("start", "supporters")).size(18));
        for (name, cents) in d.patrons {
            col = col.push(
                row![
                    text(name.clone()).size(12).width(Length::Fill),
                    text(format!("${:.2}", *cents as f64 / 100.0))
                        .size(12)
                        .style(muted),
                ]
                .spacing(6),
            );
        }
    }
    Some(
        container(scrollable(col).height(Length::Fill))
            .width(Length::Fixed(SIDEBAR_W))
            .height(Length::Fill)
            .padding(24)
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|n| PathBuf::from(format!("C:/lavori/{n}"))).collect()
    }

    fn names(v: &[&PathBuf]) -> Vec<String> {
        v.iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn last_opened_keeps_the_order_of_the_recent_list() {
        let recents = paths(&["b.dwg", "a.dwg", "c.dwg"]);
        let ui = StartUi::default();
        assert_eq!(ui.sort, RecentSort::LastOpened);
        assert_eq!(names(&ui.visible(&recents)), ["b.dwg", "a.dwg", "c.dwg"]);
    }

    #[test]
    fn search_matches_the_file_name_ignoring_case_and_the_folder() {
        let recents = paths(&["Planimetria.dwg", "Rilievo.dwg", "Tavole.dwg"]);
        let mut ui = StartUi::default();
        ui.update(StartMsg::Search("  RILI ".into()));
        assert_eq!(names(&ui.visible(&recents)), ["Rilievo.dwg"]);
        // The folder name is not searched: every path here sits in `lavori`.
        ui.update(StartMsg::Search("lavori".into()));
        assert!(ui.visible(&recents).is_empty());
        ui.update(StartMsg::Search(String::new()));
        assert_eq!(ui.visible(&recents).len(), 3);
    }

    #[test]
    fn sorting_by_name_ignores_case() {
        let recents = paths(&["b.dwg", "A.dwg", "c.dwg"]);
        let mut ui = StartUi::default();
        ui.update(StartMsg::Sort(RecentSort::Name));
        assert_eq!(names(&ui.visible(&recents)), ["A.dwg", "b.dwg", "c.dwg"]);
    }

    #[test]
    fn sorting_by_modified_puts_the_newest_first_and_unknown_last() {
        let recents = paths(&["old.dwg", "none.dwg", "new.dwg", "gone.dwg"]);
        let at = |s| Some(UNIX_EPOCH + Duration::from_secs(s));
        let mut ui = StartUi::default();
        ui.update(StartMsg::DatesLoaded(vec![
            (recents[0].clone(), at(100)),
            (recents[2].clone(), at(900)),
            (recents[3].clone(), None),
        ]));
        ui.update(StartMsg::Sort(RecentSort::Modified));
        // `none.dwg` was never loaded and `gone.dwg` has no date: both after the dated ones,
        // in their list order.
        assert_eq!(
            names(&ui.visible(&recents)),
            ["new.dwg", "old.dwg", "none.dwg", "gone.dwg"]
        );
    }

    #[test]
    fn messages_set_the_view_the_section_and_store_the_dates() {
        let mut ui = StartUi::default();
        assert_eq!(ui.view, RecentView::Grid);
        assert_eq!(ui.nav, StartNav::Recent);
        ui.update(StartMsg::View(RecentView::List));
        ui.update(StartMsg::Nav(StartNav::Learning));
        assert_eq!((ui.view, ui.nav), (RecentView::List, StartNav::Learning));
        let p = PathBuf::from("x.dwg");
        ui.update(StartMsg::DatesLoaded(vec![(p.clone(), Some(UNIX_EPOCH))]));
        assert_eq!(ui.dates.get(&p), Some(&Some(UNIX_EPOCH)));
    }

    #[test]
    fn dates_read_day_month_year_in_the_local_zone() {
        assert_eq!(format_date(0, 0), "01/01/1970");
        assert_eq!(format_date(1_791_331_200, 0), "07/10/2026");
        // 23:30 UTC on a leap day is already the 1st of March two hours east.
        assert_eq!(format_date(1_709_249_400, 0), "29/02/2024");
        assert_eq!(format_date(1_709_249_400, 2 * 3600), "01/03/2024");
        // Before the epoch the zone must not wrap the year.
        assert_eq!(format_date(-1, 0), "31/12/1969");
    }

    /// Owns what the page borrows, so a test can build the real widget.
    struct Fixture {
        recents: Vec<PathBuf>,
        thumbs: std::collections::HashMap<PathBuf, Option<iced::widget::image::Handle>>,
        video_thumbs: std::collections::HashMap<String, iced::widget::image::Handle>,
        ui: StartUi,
    }

    impl Fixture {
        fn new(names: &[&str]) -> Self {
            Self {
                recents: paths(names),
                thumbs: Default::default(),
                video_thumbs: Default::default(),
                ui: StartUi::default(),
            }
        }

        fn page(&self) -> iced::Element<'_, crate::app::Message> {
            page(
                StartData {
                    recents: &self.recents,
                    thumbs: &self.thumbs,
                    recent_limit: 20,
                    recent_limit_input: "20",
                    recent_max: 100,
                    videos: &[],
                    videos_loading: false,
                    video_thumbs: &self.video_thumbs,
                    discussions: &[],
                    discussions_loading: false,
                    patrons: &[],
                    ui: &self.ui,
                },
                1400.0,
            )
        }
    }

    fn bounds_of(ui: &mut iced_test::Simulator<'_, crate::app::Message>, text: &str) -> iced::Rectangle {
        let target = ui.find(text).unwrap_or_else(|e| panic!("{text}: {e:?}"));
        iced_test::selector::Bounded::bounds(&target)
    }

    #[test]
    fn the_sidebar_has_the_title_and_open_and_new_publish_the_usual_messages() {
        use crate::app::Message;
        let f = Fixture::new(&["a.dwg"]);
        let mut ui = iced_test::simulator(f.page());
        assert!(ui.find(crate::privacy::APP_NAME).is_ok());
        ui.click(crate::tr!("start", "open-file").as_str()).expect("an Open button");
        ui.click(crate::tr!("start", "new-drawing").as_str()).expect("a New button");
        let msgs: Vec<Message> = ui.into_messages().collect();
        assert!(msgs.iter().any(|m| matches!(m, Message::OpenFile)), "Open");
        assert!(msgs.iter().any(|m| matches!(m, Message::TabNew)), "New");
    }

    #[test]
    fn every_recent_file_has_a_card_and_a_click_opens_it() {
        use crate::app::Message;
        let f = Fixture::new(&["Rilievo.dwg", "Tavole.dwg"]);
        let mut ui = iced_test::simulator(f.page());
        assert!(ui.find("Tavole.dwg").is_ok());
        ui.click("Rilievo.dwg").expect("a card for Rilievo");
        let msgs: Vec<Message> = ui.into_messages().collect();
        assert!(
            msgs.iter().any(|m| matches!(m, Message::OpenRecent(p) if p.ends_with("Rilievo.dwg"))),
            "clicking the card opens the file"
        );
    }

    #[test]
    fn the_search_hides_the_cards_that_do_not_match() {
        let mut f = Fixture::new(&["Rilievo.dwg", "Tavole.dwg"]);
        f.ui.update(StartMsg::Search("tav".into()));
        let mut ui = iced_test::simulator(f.page());
        assert!(ui.find("Tavole.dwg").is_ok());
        assert!(ui.find("Rilievo.dwg").is_err(), "Rilievo does not match the search");
    }

    #[test]
    fn an_empty_list_and_an_empty_search_each_say_so() {
        let f = Fixture::new(&[]);
        let mut ui = iced_test::simulator(f.page());
        assert!(ui.find(crate::tr!("start", "no-recent-files").as_str()).is_ok());
        let mut f = Fixture::new(&["a.dwg"]);
        f.ui.update(StartMsg::Search("zzz".into()));
        let mut ui = iced_test::simulator(f.page());
        assert!(ui.find(crate::t!("No document matches the search").as_ref()).is_ok());
    }

    #[test]
    fn offline_the_page_has_no_learning_section_and_no_right_column() {
        let f = Fixture::new(&["a.dwg"]);
        let mut ui = iced_test::simulator(f.page());
        assert!(ui.find(crate::t!("Learning").as_ref()).is_err());
        assert!(ui.find(crate::tr!("start", "discussions").as_str()).is_err());
        assert!(ui.find(crate::tr!("start", "supporters").as_str()).is_err());
        // What is left of the old buttons still has a home in the sidebar.
        assert!(ui.find(crate::tr!("action", "options").as_str()).is_ok());
        assert!(ui.find(crate::tr!("action", "plugins").as_str()).is_ok());
    }

    #[test]
    fn the_cards_are_laid_out_on_a_grid_to_the_right_of_the_sidebar() {
        let f = Fixture::new(&["a.dwg", "b.dwg", "c.dwg"]);
        let mut ui = iced_test::simulator(f.page());
        let a = bounds_of(&mut ui, "a.dwg");
        let b = bounds_of(&mut ui, "b.dwg");
        let c = bounds_of(&mut ui, "c.dwg");
        assert_eq!(b.x - a.x, CARD_W + CARD_GAP, "cards sit side by side");
        assert_eq!(c.x - b.x, CARD_W + CARD_GAP);
        assert_eq!(a.y, b.y, "the first row");
        assert!(a.x >= SIDEBAR_W, "the cards start after the sidebar: {}", a.x);
    }

    #[test]
    fn the_cards_wrap_when_the_row_is_full() {
        let names: Vec<String> = (0..12).map(|i| format!("f{i:02}.dwg")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let f = Fixture::new(&refs);
        let mut ui = iced_test::simulator(f.page());
        // The last card is below the fold of the scrollable: use its layout bounds.
        let layout_of = |ui: &mut iced_test::Simulator<'_, crate::app::Message>, t: &str| {
            let target = ui.find(t).unwrap_or_else(|e| panic!("{t}: {e:?}"));
            iced_test::selector::Bounded::bounds(&target)
        };
        let first = layout_of(&mut ui, "f00.dwg");
        let last = layout_of(&mut ui, "f11.dwg");
        assert!(last.y > first.y, "twelve cards do not fit one row of a 1400 px page");
    }

    #[test]
    fn the_list_view_shows_the_same_files_as_rows() {
        let mut f = Fixture::new(&["a.dwg", "b.dwg"]);
        f.ui.update(StartMsg::View(RecentView::List));
        let mut ui = iced_test::simulator(f.page());
        let a = bounds_of(&mut ui, "a.dwg");
        let b = bounds_of(&mut ui, "b.dwg");
        assert_eq!(a.x, b.x, "rows are stacked");
        assert!(b.y > a.y);
    }

    #[test]
    fn an_overflowing_keep_count_does_not_panic_the_page() {
        // The plus step is built eagerly from the typed text.
        let f = Fixture::new(&["a.dwg"]);
        let element = page(
            StartData {
                recents: &f.recents,
                thumbs: &f.thumbs,
                recent_limit: 50,
                recent_limit_input: "18446744073709551615",
                recent_max: 100,
                videos: &[],
                videos_loading: false,
                video_thumbs: &f.video_thumbs,
                discussions: &[],
                discussions_loading: false,
                patrons: &[],
                ui: &f.ui,
            },
            1400.0,
        );
        let _ = iced_test::simulator(element);
    }
}
