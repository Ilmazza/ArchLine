//! ArchLine: app-state glue for the classic menu bar.
//!
//! Kept out of `view_main` (see `ui::classic_toolbar::top_bar`): that function
//! is so large that extra nesting there can overflow rustc's stack.

use crate::app::{Message, OpenCADStudio};
use crate::ui::classic_menu::{menu_bar, MenuCtx, TabEntry};
use iced::Element;

impl OpenCADStudio {
    /// One entry per open document tab, the active one flagged.
    pub(super) fn classic_menu_tabs(&self) -> Vec<TabEntry> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| TabEntry {
                index,
                name: tab.tab_display_name(),
                active: index == self.active_tab,
            })
            .collect()
    }

    /// The classic menu bar for the current state.
    #[inline(never)]
    pub(super) fn classic_menu_bar(&self) -> Element<'static, Message> {
        menu_bar(&MenuCtx {
            bindings: &self.shortcut_bindings,
            tabs: self.classic_menu_tabs(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};

    #[test]
    fn the_window_menu_lists_every_tab_and_flags_the_active_one() {
        let mut app = OpenCADStudio::new_for_test();
        // `TabNew` appends a drawing and makes it active.
        let _ = app.update(Message::TabNew);
        let _ = app.update(Message::TabNew);
        assert_eq!(app.tabs.len(), 3);
        assert_ne!(app.active_tab, 0);
        let tabs = app.classic_menu_tabs();
        assert_eq!(tabs.len(), app.tabs.len());
        assert_eq!(tabs.iter().filter(|t| t.active).count(), 1);
        assert!(tabs[app.active_tab].active);
        for (i, t) in tabs.iter().enumerate() {
            assert_eq!(t.index, i);
        }
        assert!(tabs.iter().all(|t| !t.name.is_empty()));
    }

    #[test]
    fn the_bar_builds_from_app_state() {
        let app = OpenCADStudio::new_for_test();
        let bar = app.classic_menu_bar();
        assert_eq!(bar.as_widget().size().width, iced::Length::Fill);
    }

    #[test]
    fn the_toolbar_frame_builds_for_default_and_all_floating_layouts() {
        use crate::ui::toolbar_layout::{Target, ToolbarId};
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = Default::default();
        let center: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let el = crate::ui::toolbar_dock::frame(true, &app.toolbars, &app.ribbon, None, app.win_size.1, center);
        assert_eq!(el.as_widget().size().height, iced::Length::Fill);

        // Review focus: every bar floating leaves all four edges empty.
        for (i, id) in ToolbarId::all().iter().copied().enumerate() {
            app.toolbars.move_to(id, Target::Float { x: 10.0 * i as f32, y: 10.0 });
        }
        let center: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let _ = crate::ui::toolbar_dock::frame(true, &app.toolbars, &app.ribbon, None, app.win_size.1, center);

        // Not classic: the centre comes back untouched.
        let center: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let el = crate::ui::toolbar_dock::frame(false, &app.toolbars, &app.ribbon, None, app.win_size.1, center);
        assert_eq!(el.as_widget().size().width, iced::Length::Shrink);
    }

    #[test]
    fn decorate_draws_floating_bars_and_the_drag_layer() {
        use crate::ui::toolbar_dock::{decorate, ToolbarDrag};
        use crate::ui::toolbar_layout::{Target, ToolbarId};
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = Default::default();
        app.toolbars
            .move_to(ToolbarId::Layers, Target::Float { x: 5000.0, y: 4000.0 });
        let win = (800.0, 600.0);
        let base: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let el = decorate(base, &app.toolbars, &app.ribbon, None, win, true);
        assert_eq!(el.as_widget().size().width, iced::Length::Fill);

        // While dragging, the layer still builds with a live target.
        let drag = ToolbarDrag {
            id: ToolbarId::Draw,
            origin: Some(iced::Point::new(10.0, 10.0)),
            cursor: Some(iced::Point::new(300.0, 300.0)),
            target: Some(Target::Float { x: 290.0, y: 290.0 }),
        };
        let base: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let _ = decorate(base, &app.toolbars, &app.ribbon, Some(&drag), win, true);

        // Not classic: untouched.
        let base: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let el = decorate(base, &app.toolbars, &app.ribbon, None, win, false);
        assert_eq!(el.as_widget().size().width, iced::Length::Shrink);
    }

    /// Feed events to the REAL drag layer (headless iced) and apply what it
    /// publishes to the app, like the runtime would.
    fn run_drag_layer(
        app: &mut OpenCADStudio,
        pointer: iced::Point,
        events: Vec<iced_core::Event>,
    ) {
        let layer = crate::ui::toolbar_dock::decorate(
            iced::widget::Space::new().into(),
            &app.toolbars,
            &app.ribbon,
            app.toolbar_drag.as_ref(),
            app.win_size,
            true,
        );
        let mut ui = iced_test::simulator(layer);
        ui.point_at(pointer);
        ui.simulate(events);
        for message in ui.into_messages() {
            let _ = app.update(message);
        }
    }

    #[test]
    fn clicking_a_grip_without_dragging_keeps_the_layout_with_the_real_widget() {
        use crate::ui::toolbar_dock::ToolbarMsg;
        use crate::ui::toolbar_layout::ToolbarId;
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = Default::default();
        app.win_size = (1600.0, 900.0);
        let before = app.toolbars.clone();
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Layers)));
        // Press happened on the grip of Layers; the layer is mounted fresh and
        // its first update reports the pointer as a move.
        let grip = iced::Point::new(20.0, 131.0);
        run_drag_layer(
            &mut app,
            grip,
            vec![
                iced_core::Event::Mouse(iced::mouse::Event::CursorMoved { position: grip }),
                iced_core::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )),
            ],
        );
        assert!(app.toolbar_drag.is_none(), "the release ended the drag");
        assert_eq!(app.toolbars, before, "a click must not move the bar");
    }

    #[test]
    fn leaving_the_window_during_a_drag_ends_it() {
        use crate::ui::toolbar_dock::ToolbarMsg;
        use crate::ui::toolbar_layout::ToolbarId;
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = Default::default();
        app.win_size = (1600.0, 900.0);
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Draw)));
        let inside = iced::Point::new(100.0, 100.0);
        let layer = crate::ui::toolbar_dock::decorate(
            iced::widget::Space::new().into(),
            &app.toolbars,
            &app.ribbon,
            app.toolbar_drag.as_ref(),
            app.win_size,
            true,
        );
        let mut ui = iced_test::simulator(layer);
        ui.point_at(inside);
        ui.simulate([iced_core::Event::Mouse(iced::mouse::Event::CursorMoved {
            position: inside,
        })]);
        // The button is released while the pointer is outside the window: the
        // layer never sees a ButtonReleased, only the pointer leaving.
        let outside = iced::Point::new(-50.0, -50.0);
        ui.point_at(outside);
        ui.simulate([iced_core::Event::Mouse(iced::mouse::Event::CursorMoved {
            position: outside,
        })]);
        for message in ui.into_messages() {
            let _ = app.update(message);
        }
        assert!(app.toolbar_drag.is_none(), "drag must not outlive the pointer");
    }

    #[test]
    fn a_floating_bar_is_as_wide_as_its_buttons_not_the_window() {
        use crate::ui::toolbar_dock::{decorate, floating_id};
        use crate::ui::toolbar_layout::{Target, ToolbarId};
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = Default::default();
        app.toolbars
            .move_to(ToolbarId::Draw, Target::Float { x: 20.0, y: 200.0 });
        let win = (1600.0, 900.0);
        let layer = decorate(
            iced::widget::Space::new().into(),
            &app.toolbars,
            &app.ribbon,
            None,
            win,
            true,
        );
        let mut ui = iced_test::simulator(layer);
        let frame = ui
            .find(floating_id(ToolbarId::Draw))
            .expect("floating frame present");
        let b = frame.visible_bounds().expect("frame visible");
        // Draw has at most 12 buttons of 36 px: far below 600, far from the
        // simulator window width the old `Fill` title strip stretched to.
        assert!(b.width < 600.0, "floating Draw bar is {} px wide", b.width);
        assert!(b.width > 100.0, "floating Draw bar is {} px wide", b.width);
    }

    #[test]
    fn classic_active_is_off_on_start_and_clean_screen() {
        assert!(!crate::workspace::classic_active(true, false));
        assert!(!crate::workspace::classic_active(false, true));
    }

    #[test]
    fn right_click_on_a_bar_opens_the_list_and_a_row_toggles_it() {
        use crate::ui::toolbar_layout::ToolbarId;
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = Default::default();
        app.win_size = (1600.0, 900.0);
        let center: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let el = crate::ui::toolbar_dock::frame(
            true,
            &app.toolbars,
            &app.ribbon,
            None,
            app.win_size.1,
            center,
        );
        let mut ui = iced_test::simulator(el);
        // The Draw bar hugs the left edge: (20, 20) is on it.
        let at = iced::Point::new(20.0, 20.0);
        ui.point_at(at);
        ui.simulate([iced_core::Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Right,
        ))]);
        ui.click("Modify").expect("the bar list shows Modify");
        for message in ui.into_messages() {
            let _ = app.update(message);
        }
        assert!(
            !app.toolbars.is_visible(ToolbarId::Modify),
            "clicking the ticked row hides the bar"
        );
    }

    #[test]
    fn the_bar_list_also_offers_hidden_bars_and_a_row_opens_them() {
        // Review focus 6.
        use crate::ui::toolbar_layout::{Placement, ToolbarId};
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = Default::default();
        app.win_size = (1600.0, 900.0);
        let dims = ToolbarId::all()
            .iter()
            .copied()
            .find(|id| id.title() == "Dimension")
            .expect("a Dimension bar");
        assert!(!app.toolbars.is_visible(dims));
        let center: iced::Element<'_, Message> = iced::widget::Space::new().into();
        let el = crate::ui::toolbar_dock::frame(
            true,
            &app.toolbars,
            &app.ribbon,
            None,
            app.win_size.1,
            center,
        );
        let mut ui = iced_test::simulator(el);
        ui.point_at(iced::Point::new(20.0, 20.0));
        ui.simulate([iced_core::Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Right,
        ))]);
        ui.click("Dimension").expect("the list shows the hidden Dimension bar");
        for message in ui.into_messages() {
            let _ = app.update(message);
        }
        assert!(matches!(app.toolbars.placement(dims), Placement::Floating { .. }));
    }

    #[test]
    fn pressing_a_variant_button_publishes_hold_start_then_hold_end_with_the_real_widget() {
        use crate::ui::classic_toolbar::{items_for, ClassicItem};
        use crate::ui::toolbar_dock::ToolbarMsg;
        use crate::ui::toolbar_layout::ToolbarId;
        let tool = items_for(ToolbarId::Draw)
            .iter()
            .find_map(|i| match i {
                ClassicItem::Button(b) if !b.variants.is_empty() => Some(b),
                _ => None,
            })
            .expect("a Draw dropdown");
        let el = crate::ui::classic_toolbar::item_el(
            &ClassicItem::Button(crate::ui::classic_toolbar::ClassicButton {
                main: tool.main.clone(),
                variants: tool.variants.clone(),
                tinted: tool.tinted,
            }),
            true,
            ToolbarId::Draw,
        );
        let mut ui = iced_test::simulator(el);
        let at = iced::Point::new(18.0, 18.0);
        ui.point_at(at);
        ui.simulate([
            iced_core::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            iced_core::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)),
        ]);
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Toolbar(ToolbarMsg::HoldStart(bar, id)) if *bar == ToolbarId::Draw && *id == tool.main.id)),
            "press must publish HoldStart: {messages:?}"
        );
        assert!(
            messages.iter().any(|m| matches!(m, Message::Toolbar(ToolbarMsg::HoldEnd { .. }))),
            "release must publish HoldEnd"
        );
        assert!(
            !messages.iter().any(|m| matches!(m, Message::RibbonToolClick { .. })),
            "a variant button must not run on press"
        );
    }

    #[test]
    fn docked_side_bars_leave_the_drawing_area_its_width() {
        // The Draw and Modify bars have separators between their buttons; a
        // separator that is `Fill` wide made each side lane swallow a third of
        // the window and squeezed the drawing area to a strip.
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = Default::default();
        let probe: iced::Element<'_, Message> = iced::widget::container(iced::widget::Space::new())
            .id("center-probe")
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .into();
        let el = crate::ui::toolbar_dock::frame(true, &app.toolbars, &app.ribbon, None, 768.0, probe);
        let mut ui = iced_test::simulator(el);
        let b = ui
            .find(iced::widget::Id::new("center-probe"))
            .expect("probe present")
            .visible_bounds()
            .expect("probe visible");

        assert!(b.width > 800.0, "the drawing area is only {} px wide", b.width);
    }
}
