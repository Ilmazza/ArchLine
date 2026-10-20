// Missing-fonts prompt — shown after a drawing opens with `.shx` fonts this
// machine doesn't have. ArchLine: for each font the user picks what to draw it
// with (`crate::font_alt`, remembered in settings.json); with ARCHLINE_ONLINE=1
// it can also still fetch the fonts from a repository (see `fonts/README.md`
// in the repo root).

use crate::app::Message;
use crate::font_alt::{self, StandIn, Substitute};
use crate::ui::style::form::dialog_button_styled_opt;
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Element, Fill, Length, Shrink, Theme};
use std::collections::HashMap;

fn muted_style(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().background.weak.text),
    }
}

/// What the dialog says about the stand-in a font gets if nothing is chosen.
fn stand_in_note(name: &str) -> String {
    match font_alt::stand_in(name) {
        StandIn::Approximate(label) => {
            crate::tf!("Default stand-in: {label}. Character widths differ from the original.")
                .into_owned()
        }
        StandIn::Exact(label) => {
            crate::tf!("Default stand-in: {label} (same widths).").into_owned()
        }
        StandIn::None => {
            crate::t!("No stand-in known for this name: it is drawn with Standard.").into_owned()
        }
    }
}

/// The prompt: one row per missing font with the stand-in to draw it with, the
/// OK that remembers the choices, a skip and — only online — the download.
pub fn view_window<'a>(
    fonts: &'a [String],
    picks: &'a HashMap<String, Substitute>,
    font_source: &'a str,
    downloading: bool,
    online: bool,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let content_width = if matches!(sizing.width, Length::Fill) {
        Fill
    } else {
        Shrink
    };
    let content_height = if matches!(sizing.height, Length::Fill) {
        Fill
    } else {
        Shrink
    };
    let listed: Vec<Element<'a, Message>> = fonts
        .iter()
        .map(|name| {
            let options = font_alt::choices(name);
            let selected = picks
                .get(name)
                .and_then(|pick| options.iter().find(|c| &c.value == pick))
                .or_else(|| options.first())
                .cloned();
            let owner = name.clone();
            let menu = pick_list(selected, options, |choice| choice.label.clone())
                .on_select(move |choice| Message::MissingFontsPick(owner.clone(), choice.value))
                .text_size(11)
                .width(Length::Fixed(250.0));
            row![
                column![
                    text(name.as_str()).size(12),
                    text(stand_in_note(name)).size(10).style(muted_style),
                ]
                .spacing(2)
                .width(Fill),
                menu,
            ]
            .spacing(12)
            .align_y(iced::Center)
            .into()
        })
        .collect();

    let apply = dialog_button_styled_opt(
        crate::t!("OK"),
        (!downloading).then_some(Message::MissingFontsApply),
        button::primary,
    );
    let mut actions = row![
        Space::new().width(content_width),
        dialog_button_styled_opt(
            crate::t!("Skip"),
            (!downloading).then_some(Message::MissingFontsDismiss),
            button::secondary,
        ),
    ]
    .spacing(8);
    if online {
        actions = actions.push(dialog_button_styled_opt(
            if downloading {
                crate::t!("Downloading...")
            } else {
                crate::t!("Download available")
            },
            (!downloading).then_some(Message::MissingFontsDownload),
            button::secondary,
        ));
    }
    let actions = actions.push(apply);

    let description = if online {
        crate::t!(
            "This drawing uses fonts that are not installed on this machine. Choose what to draw each one with, or download any redistributable fonts that exist in the selected repository."
        )
    } else {
        crate::t!(
            "This drawing uses fonts that are not installed on this machine. Choose what to draw each one with; the choice is remembered on this computer."
        )
    };

    let mut body = column![
        text(crate::t!("Missing fonts")).size(20),
        text(description).size(11).style(muted_style),
        // The right padding keeps the names clear of the scrollbar, which
        // a list as wide as its longest name would otherwise draw over.
        container(
            scrollable(column(listed).spacing(8).padding(iced::Padding {
                right: 14.0,
                ..iced::Padding::ZERO
            }))
            .height(content_height),
        )
        .padding([10, 12])
        .width(content_width)
        .height(content_height)
        .style(container::bordered_box),
        text(crate::t!(
            "Windows fonts are filled outlines, not stroke fonts: their character widths differ from the SHX, so dimensions and labels may shift."
        ))
        .size(10)
        .style(muted_style),
    ]
    .spacing(10)
    .width(sizing.width)
    .height(sizing.height);
    if online {
        body = body.push(
            column![
                text(crate::t!("Font source — leave empty for the community repository, or point at any folder (e.g. your company's font server):"))
                    .size(10)
                    .style(muted_style),
                text_input(
                    crate::t!("https://server/fonts or a GitHub raw folder").as_ref(),
                    font_source,
                )
                .on_input(Message::MissingFontsSourceChanged)
                .size(11)
                .padding([4, 8])
                .width(content_width),
            ]
            .spacing(3),
        );
    }
    body = body.push(actions);

    container(body).padding([12, 16]).into()
}
