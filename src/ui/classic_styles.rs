//! ArchLine: the AutoCAD "Styles" bar of the classic workspace: text, dimension,
//! table and multileader style, each a manager button and a combo.
//!
//! Like the layer combos, the closed buttons live here and the lists reuse the
//! ribbon's style overlay (`Ribbon::style_combo_overlay`) and its
//! `RibbonStyleChanged` message; the names and the current style are kept up
//! to date by the app whatever tab is showing.

use iced::widget::row;
use iced::Element;

use super::classic_layers::combo;
use super::classic_toolbar::{item_el, items_for};
use super::toolbar_layout::ToolbarId;
use crate::app::Message;
use crate::modules::StyleKey;
use crate::ui::ribbon::Ribbon;

/// Key of the bar in `toolbar_registry` and `settings.json`.
pub const BAR_KEY: &str = "ribbon:Styles";

/// Width of a closed combo.
const COMBO_W: f32 = 130.0;
/// Padding, spacing and arrow of a closed combo; the rest is the name.
const COMBO_FIXED_W: f32 = 8.0 * 2.0 + 9.0 + 4.0;

/// Style kind, combo id (the ribbon's own, so the overlay finds it), manager
/// command: in bar order.
pub const STYLES: [(StyleKey, &str, &str); 4] = [
    (StyleKey::TextStyle, "TEXT_STYLE_COMBO", "STYLE"),
    (StyleKey::DimStyle, "DIM_STYLE_COMBO", "DIMSTYLE"),
    (StyleKey::TableStyle, "TABLE_STYLE_COMBO", "TABLESTYLE"),
    (StyleKey::MLeaderStyle, "MLEADER_STYLE_COMBO", "MLEADERSTYLE"),
];

/// Estimated length of the bar: four buttons, four combos, spacing, grip.
pub const LENGTH: f32 = 24.0 * 4.0 + COMBO_W * 4.0 + 3.0 * 7.0 + 8.0 + 12.0;

/// The style kind and manager command of the combo `id`, for the ribbon overlay
/// when the open combo is one of this bar's.
pub fn combo_for(id: &str) -> Option<(StyleKey, Option<&'static str>)> {
    STYLES
        .iter()
        .find(|(_, combo_id, _)| *combo_id == id)
        .map(|(key, _, manager)| (*key, Some(*manager)))
}

fn active(ribbon: &Ribbon, key: StyleKey) -> &str {
    match key {
        StyleKey::TextStyle => &ribbon.active_text_style,
        StyleKey::DimStyle => &ribbon.active_dim_style,
        StyleKey::TableStyle => &ribbon.active_table_style,
        StyleKey::MLeaderStyle => &ribbon.active_mleader_style,
    }
}

/// The four style controls as one row, without its strip container (the dock
/// wraps it in a bar like any other).
///
/// Kept out of `view_main` on purpose (see `toolbar_dock::frame`).
#[inline(never)]
pub fn styles_row(ribbon: &Ribbon) -> iced::widget::Row<'static, Message> {
    let bar = ToolbarId::from_key(BAR_KEY);
    let buttons = bar.map(items_for).unwrap_or(&[]);
    let mut r = row![].spacing(3).align_y(iced::Center);
    for (i, (key, id, _)) in STYLES.iter().enumerate() {
        if let (Some(bar), Some(button)) = (bar, buttons.get(i)) {
            r = r.push(item_el(button, false, bar));
        }
        let el: Element<'static, Message> = combo(
            id,
            ribbon.open_dropdown.as_deref() == Some(*id),
            COMBO_W,
            Vec::new(),
            COMBO_W - COMBO_FIXED_W,
            active(ribbon, *key),
        );
        r = r.push(el);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::classic_toolbar::ClassicItem;

    #[test]
    fn the_bar_exists_with_one_manager_button_per_style() {
        let bar = ToolbarId::from_key(BAR_KEY).expect("the Styles bar is registered");
        let cmds: Vec<_> = items_for(bar)
            .iter()
            .filter_map(|i| match i {
                ClassicItem::Button(b) => Some(b.main.id),
                _ => None,
            })
            .collect();
        assert_eq!(cmds, STYLES.map(|(_, _, manager)| manager));
    }

    #[test]
    fn the_combo_ids_are_the_ribbons_own() {
        for (key, id, manager) in STYLES {
            assert_eq!(combo_for(id), Some((key, Some(manager))));
        }
        assert_eq!(combo_for("LAYER_COMBO"), None);
    }

    #[test]
    fn the_row_builds_with_empty_and_filled_state() {
        let mut ribbon = Ribbon::default();
        let _ = styles_row(&ribbon);
        ribbon.set_styles(
            vec!["Standard".into()],
            "Standard",
            vec!["ISO-25".into()],
            "ISO-25",
            vec![],
            "",
            vec![],
            "",
        );
        let _ = styles_row(&ribbon);
    }

    #[test]
    fn each_combo_opens_the_ribbons_style_list_even_when_no_annotate_tab_shows() {
        // The classic workspace has no Annotate tab: the overlay must find the
        // combo through `combo_for`, not through the ribbon groups.
        for (_, id, _) in STYLES {
            let mut ribbon = Ribbon::default();
            ribbon.open_dropdown = Some(id.to_string());
            assert!(
                ribbon.dropdown_overlay(&[], &[], (1600.0, 900.0), false, &[]).is_some(),
                "{id} opens nothing"
            );
        }
    }
}
