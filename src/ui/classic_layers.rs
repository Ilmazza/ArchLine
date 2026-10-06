//! Layer and property controls of the AutoCAD-classic workspace.
//!
//! A second strip under the top toolbar: layer manager, layer combo, the
//! object-based layer commands, then the Color / Linetype / Lineweight combos.
//! The combos reuse the ribbon's dropdown overlays and messages
//! (`ToggleRibbonDropdown`), so only the closed buttons live here.

use std::sync::OnceLock;

use iced::widget::{button, container, row, text};
use iced::{Color, Element, Length, Theme};

use super::classic_toolbar::{item_el, plain, separator};
use crate::app::Message;
use crate::modules::{registry, RibbonItem, ToolDef};
use crate::ui::icons;
use crate::ui::toolbar_layout::ToolbarId;
use crate::ui::properties::{acad_color_display, linetype_display_name, LwItem};
use crate::ui::ribbon::{
    combo_btn_style, Ribbon, LAYER_COMBO_ID, PROP_COLOR_ID, PROP_LINETYPE_ID, PROP_LW_ID,
};
use crate::ui::wrap_bar::PosReport;

const LAYER_COMBO_W: f32 = 220.0;
const PROP_COMBO_W: f32 = 130.0;
/// Width taken by the three state icons, swatch, arrow, spacing and padding of
/// the layer combo; the rest is the name.
const LAYER_FIXED_W: f32 = 14.0 * 3.0 + 12.0 + 9.0 + 4.0 * 4.0 + 8.0 * 2.0;
/// About 6 px per glyph at size 11.
const GLYPH_W: f32 = 6.0;

/// Layer buttons taken from the ribbon's "Layers" group.
pub struct LayerTools {
    /// Layer manager (`LAYERS`).
    pub manager: Option<ToolDef>,
    /// Object-based layer commands, in ribbon order.
    pub commands: Vec<ToolDef>,
}

pub fn layer_tools() -> &'static LayerTools {
    static TOOLS: OnceLock<LayerTools> = OnceLock::new();
    TOOLS.get_or_init(|| {
        let mut manager = None;
        let mut commands = Vec::new();
        for module in registry::all_modules() {
            if module.id() != "draw" {
                continue;
            }
            for group in module.ribbon_groups() {
                if group.title != "Layers" {
                    continue;
                }
                for item in &group.tools {
                    match item {
                        RibbonItem::LargeTool(t) => manager = Some(t.clone()),
                        RibbonItem::LayerComboGroup { row2, row3 } => {
                            commands.extend(row2.iter().chain(row3.iter()).cloned());
                        }
                        _ => {}
                    }
                }
            }
        }
        LayerTools { manager, commands }
    })
}

fn swatch(color: Color) -> Element<'static, Message> {
    container(text(""))
        .style(move |theme: &Theme| container::Style {
            background: Some(iced::Background::Color(color)),
            border: iced::Border {
                color: theme.palette().background.strong.color,
                width: 1.0,
                radius: 1.0.into(),
            },
            ..Default::default()
        })
        .width(12)
        .height(12)
        .into()
}

/// Closed combo button: `lead` widgets, the elided `label`, a drop arrow. Opens
/// the ribbon overlay registered under `id`.
fn combo(
    id: &'static str,
    is_open: bool,
    width: f32,
    lead: Vec<Element<'static, Message>>,
    label_w: f32,
    label: &str,
) -> Element<'static, Message> {
    let label = crate::ui::text_util::elide(label, ((label_w / GLYPH_W) as usize).max(4));
    let content = row(lead)
        .push(container(text(label).size(11)).width(label_w).clip(true))
        .push(icons::themed_arrow_down(9.0))
        .spacing(4)
        .align_y(iced::Center);
    let btn = button(content)
        .on_press(Message::ToggleRibbonDropdown(id.to_string()))
        .style(move |theme: &Theme, status| combo_btn_style(theme, is_open, status, 3.0))
        .padding([3, 8])
        .width(Length::Fixed(width));
    PosReport::new(id, btn).into()
}

fn layer_combo(ribbon: &Ribbon) -> Element<'static, Message> {
    let info = ribbon.layer_infos.iter().find(|l| l.name == ribbon.active_layer);
    let color = info.map(|l| l.color).unwrap_or(Color::WHITE);
    let visible = info.map(|l| l.visible).unwrap_or(true);
    let frozen = info.map(|l| l.frozen).unwrap_or(false);
    let locked = info.map(|l| l.locked).unwrap_or(false);
    combo(
        LAYER_COMBO_ID,
        ribbon.open_dropdown.as_deref() == Some(LAYER_COMBO_ID),
        LAYER_COMBO_W,
        vec![
            icons::semantic(icons::layer_visible(visible), 14.0),
            icons::semantic(icons::layer_freeze(frozen), 14.0),
            icons::semantic(icons::layer_lock(locked), 14.0),
            swatch(color),
        ],
        LAYER_COMBO_W - LAYER_FIXED_W,
        &ribbon.active_layer,
    )
}

fn prop_combo(
    ribbon: &Ribbon,
    id: &'static str,
    swatch_color: Option<Color>,
    label: &str,
) -> Element<'static, Message> {
    let lead: Vec<Element<'static, Message>> = swatch_color.map(swatch).into_iter().collect();
    // Padding, spacing and arrow; the swatch adds its own width and gap.
    let fixed = 8.0 * 2.0 + 9.0 + 4.0 + if lead.is_empty() { 0.0 } else { 12.0 + 4.0 };
    combo(
        id,
        ribbon.open_dropdown.as_deref() == Some(id),
        PROP_COMBO_W,
        lead,
        PROP_COMBO_W - fixed,
        label,
    )
}

/// The layer / properties controls as one row, without its strip container
/// (the dock wraps it in a bar like any other).
///
/// Kept out of `view_main` on purpose (see `toolbar_dock::frame`).
#[inline(never)]
pub fn layer_row(ribbon: &Ribbon) -> iced::widget::Row<'static, Message> {
    let tools = layer_tools();
    let mut r = row![].spacing(3).align_y(iced::Center);
    if let Some(m) = &tools.manager {
        r = r.push(item_el(&plain(m), false, ToolbarId::Layers));
    }
    r = r.push(layer_combo(ribbon));
    r = r.push(separator(false));
    for c in &tools.commands {
        r = r.push(item_el(&plain(c), false, ToolbarId::Layers));
    }
    r = r.push(separator(false));
    let (color_swatch, _) = acad_color_display(ribbon.active_color);
    r = r.push(prop_combo(
        ribbon,
        PROP_COLOR_ID,
        Some(color_swatch),
        &crate::ui::color_select::color_display_name(ribbon.active_color),
    ));
    r = r.push(prop_combo(
        ribbon,
        PROP_LINETYPE_ID,
        None,
        &linetype_display_name(&ribbon.active_linetype),
    ));
    r.push(prop_combo(
        ribbon,
        PROP_LW_ID,
        None,
        &LwItem(ribbon.active_lineweight).to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_commands_follow_the_ribbon_layers_group() {
        let t = layer_tools();
        assert_eq!(t.manager.as_ref().map(|m| m.id), Some("LAYERS"));
        let ids: Vec<_> = t.commands.iter().map(|c| c.id).collect();
        assert_eq!(
            ids,
            [
                "LAYOFF", "LAYFRZ", "LAYLCK", "LAYMCUR", "LAYISO", "LAYON", "LAYTHW", "LAYULK",
                "LAYMATCH", "LAYUNISO"
            ]
        );
    }
}
