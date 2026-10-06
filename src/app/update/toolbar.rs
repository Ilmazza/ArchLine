//! ArchLine: update handling for the dockable classic toolbars.

use crate::app::{Message, OpenCADStudio};
use crate::ui::toolbar_dock::{
    bar_length, bar_size, ToolbarDrag, ToolbarMsg, DRAG_THRESHOLD, GRIP_ANCHOR, HOLD_MS,
};
use crate::ui::toolbar_layout::{clamp_floating, resolve_drop, DropCtx, ToolbarId};

impl OpenCADStudio {
    pub(super) fn on_toolbar(&mut self, m: ToolbarMsg) -> iced::Task<Message> {
        match m {
            ToolbarMsg::Grab(id) => {
                self.toolbar_drag = Some(ToolbarDrag {
                    id,
                    origin: None,
                    cursor: None,
                    target: None,
                });
            }
            ToolbarMsg::DragMove(p) => {
                if let Some(drag) = &mut self.toolbar_drag {
                    // The first move is the layer reporting the press position
                    // itself: remember it, and only start once the pointer has
                    // really travelled (a click on a grip must not move a bar).
                    let origin = *drag.origin.get_or_insert(p);
                    let travelled = (p.x - origin.x).hypot(p.y - origin.y);
                    if travelled < DRAG_THRESHOLD && drag.target.is_none() {
                        return iced::Task::none();
                    }
                    let win = self.win_size;
                    let size = bar_size(drag.id, false);
                    let float_at =
                        clamp_floating((p.x - GRIP_ANCHOR, p.y - GRIP_ANCHOR), size, win);
                    let ctx = DropCtx {
                        win,
                        length: &bar_length,
                    };
                    drag.cursor = Some(p);
                    drag.target = Some(resolve_drop(
                        &self.toolbars,
                        drag.id,
                        (p.x, p.y),
                        float_at,
                        &ctx,
                    ));
                }
            }
            ToolbarMsg::DragRelease => {
                if let Some(drag) = self.toolbar_drag.take() {
                    if let Some(target) = drag.target {
                        self.toolbars.move_to(drag.id, target);
                        self.save_config();
                    }
                }
            }
            ToolbarMsg::Redock(id) => {
                // A double click is Grab then Redock in one event: end the drag
                // the Grab started, or its layer would re-float the bar.
                self.toolbar_drag = None;
                self.toolbars.redock(id);
                self.save_config();
            }
            ToolbarMsg::HoldStart(bar, tool) => {
                self.tool_hold = Some(crate::app::ToolHold {
                    bar,
                    tool,
                    pressed_at: iced::time::Instant::now(),
                    fired: false,
                });
            }
            ToolbarMsg::HoldCancel => {
                self.tool_hold = None;
            }
            ToolbarMsg::HoldTick(now) => {
                if let Some(h) = &mut self.tool_hold {
                    let held = now.saturating_duration_since(h.pressed_at);
                    if !h.fired && held >= std::time::Duration::from_millis(HOLD_MS) {
                        h.fired = true;
                        self.ribbon.open_dropdown =
                            Some(crate::ui::classic_toolbar::flyout_id(h.bar, h.tool));
                    }
                }
            }
            ToolbarMsg::HoldEnd { tool_id, event } => {
                let opened_flyout = self.tool_hold.take().is_some_and(|h| h.fired);
                if !opened_flyout {
                    return self.update(Message::RibbonToolClick { tool_id, event });
                }
            }
            ToolbarMsg::Toggle(id) => {
                let shown = self.toolbars.is_visible(id);
                // Keep one bar: the list opens from a bar, so with none left
                // there would be nothing to right-click.
                let visible = ToolbarId::all()
                    .iter()
                    .filter(|b| self.toolbars.is_visible(**b))
                    .count();
                if shown && visible <= 1 {
                    self.command_line.push_info(
                        crate::t!("At least one toolbar must stay visible (TOOLBARRESET restores the defaults).")
                            .as_ref(),
                    );
                    return iced::Task::none();
                }
                self.toolbars.set_visible(id, !shown);
                self.save_config();
            }
            ToolbarMsg::Reset => {
                self.toolbars = Default::default();
                self.toolbar_drag = None;
                self.save_config();
            }
        }
        iced::Task::none()
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};
    use crate::ui::toolbar_dock::ToolbarMsg;
    use crate::ui::toolbar_layout::{DockSlot, Edge, Placement, ToolbarId, ToolbarLayout};
    use iced::Point;

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        // Tests load the user's persisted config; reset to a known layout.
        app.toolbars = ToolbarLayout::default();
        app.win_size = (1600.0, 900.0);
        app
    }

    fn toolbar(app: &mut OpenCADStudio, m: ToolbarMsg) {
        let _ = app.update(Message::Toolbar(m));
    }

    /// What the real widget delivers: the freshly mounted drag layer reports
    /// a first move at the grip, then the pointer travels to `to`.
    fn drag(app: &mut OpenCADStudio, id: ToolbarId, to: Point) {
        toolbar(app, ToolbarMsg::Grab(id));
        toolbar(app, ToolbarMsg::DragMove(Point::new(20.0, 100.0)));
        toolbar(app, ToolbarMsg::DragMove(to));
        toolbar(app, ToolbarMsg::DragRelease);
    }

    #[test]
    fn dragging_a_bar_to_the_left_edge_docks_it_there() {
        let mut app = app();
        let insert = ToolbarId::from_key("menu:Insert").unwrap();
        drag(&mut app, insert, Point::new(8.0, 400.0));
        assert!(app.toolbar_drag.is_none());
        assert_eq!(
            app.toolbars.placement(insert),
            Placement::Docked(DockSlot { edge: Edge::Left, lane: 0, index: 1 })
        );
    }

    #[test]
    fn dragging_into_the_middle_floats_the_bar_inside_the_window() {
        let mut app = app();
        drag(&mut app, ToolbarId::Draw, Point::new(800.0, 450.0));
        match app.toolbars.placement(ToolbarId::Draw) {
            Placement::Floating { x, y, .. } => {
                assert!(x >= 0.0 && y >= 0.0);
                assert!(x < 800.0 && y < 450.0, "top-left is offset from the pointer");
            }
            other => panic!("expected floating, got {other:?}"),
        }
    }

    #[test]
    fn a_click_on_the_grip_without_dragging_changes_nothing() {
        // Review focus 5, as the real widget behaves: the drag layer reports
        // a move at the grip itself before the release. For Layers that grip
        // sits where `resolve_drop` would otherwise float the bar.
        let mut app = app();
        let before = app.toolbars.clone();
        toolbar(&mut app, ToolbarMsg::Grab(ToolbarId::Layers));
        toolbar(&mut app, ToolbarMsg::DragMove(Point::new(20.0, 131.0)));
        toolbar(&mut app, ToolbarMsg::DragRelease);
        assert_eq!(app.toolbars, before);
        assert!(app.toolbar_drag.is_none());
    }

    #[test]
    fn a_grab_released_with_no_move_at_all_changes_nothing() {
        let mut app = app();
        let before = app.toolbars.clone();
        toolbar(&mut app, ToolbarMsg::Grab(ToolbarId::Modify));
        toolbar(&mut app, ToolbarMsg::DragRelease);
        assert_eq!(app.toolbars, before);
    }

    #[test]
    fn escape_cancels_a_drag_and_keeps_the_layout() {
        let mut app = app();
        let before = app.toolbars.clone();
        toolbar(&mut app, ToolbarMsg::Grab(ToolbarId::Draw));
        toolbar(&mut app, ToolbarMsg::DragMove(Point::new(20.0, 100.0)));
        toolbar(&mut app, ToolbarMsg::DragMove(Point::new(800.0, 450.0)));
        let _ = app.update(Message::CommandEscape);
        assert!(app.toolbar_drag.is_none());
        toolbar(&mut app, ToolbarMsg::DragRelease);
        assert_eq!(app.toolbars, before);
    }

    #[test]
    fn redock_stops_the_drag_so_a_double_click_does_not_refloat_the_bar() {
        // A double click on a floating bar's title arrives as Grab then
        // Redock in the same event; the drag layer then reports pointer moves.
        let mut app = app();
        drag(&mut app, ToolbarId::Layers, Point::new(800.0, 450.0));
        assert!(matches!(
            app.toolbars.placement(ToolbarId::Layers),
            Placement::Floating { .. }
        ));
        toolbar(&mut app, ToolbarMsg::Grab(ToolbarId::Layers));
        toolbar(&mut app, ToolbarMsg::Redock(ToolbarId::Layers));
        assert!(app.toolbar_drag.is_none());
        toolbar(&mut app, ToolbarMsg::DragMove(Point::new(300.0, 300.0)));
        toolbar(&mut app, ToolbarMsg::DragMove(Point::new(800.0, 450.0)));
        toolbar(&mut app, ToolbarMsg::DragRelease);
        assert_eq!(
            app.toolbars.placement(ToolbarId::Layers),
            Placement::Docked(DockSlot { edge: Edge::Top, lane: 1, index: 0 })
        );
    }

    #[test]
    fn redock_and_reset_restore_positions() {
        let mut app = app();
        drag(&mut app, ToolbarId::Layers, Point::new(800.0, 450.0));
        toolbar(&mut app, ToolbarMsg::Redock(ToolbarId::Layers));
        assert_eq!(
            app.toolbars.placement(ToolbarId::Layers),
            Placement::Docked(DockSlot { edge: Edge::Top, lane: 1, index: 0 })
        );
        drag(&mut app, ToolbarId::Draw, Point::new(800.0, 450.0));
        toolbar(&mut app, ToolbarMsg::Reset);
        assert_eq!(app.toolbars, ToolbarLayout::default());
    }

    #[test]
    fn toggling_a_hidden_group_bar_opens_it_floating_and_again_hides_it() {
        let mut app = app();
        let id = *ToolbarId::all()
            .iter()
            .find(|id| !id.is_builtin())
            .expect("a group bar");
        assert!(!app.toolbars.is_visible(id));
        toolbar(&mut app, ToolbarMsg::Toggle(id));
        assert!(matches!(app.toolbars.placement(id), Placement::Floating { .. }));
        toolbar(&mut app, ToolbarMsg::Toggle(id));
        assert!(!app.toolbars.is_visible(id));
    }

    #[test]
    fn toggling_a_builtin_hides_it_and_compacts_its_lane() {
        let mut app = app();
        toolbar(&mut app, ToolbarMsg::Toggle(ToolbarId::Layers));
        assert!(!app.toolbars.is_visible(ToolbarId::Layers));
        assert_eq!(app.toolbars.lanes(Edge::Top).len(), 1);
        toolbar(&mut app, ToolbarMsg::Toggle(ToolbarId::Layers));
        assert!(matches!(
            app.toolbars.placement(ToolbarId::Layers),
            Placement::Floating { .. }
        ));
    }

    /// A draw-bar button that has variants (e.g. CIRCLE).
    fn variant_tool() -> &'static str {
        use crate::ui::classic_toolbar::{items_for, ClassicItem};
        items_for(ToolbarId::Draw)
            .iter()
            .find_map(|i| match i {
                ClassicItem::Button(b) if !b.variants.is_empty() => Some(b.main.id),
                _ => None,
            })
            .expect("the Draw bar has a dropdown")
    }

    fn tick(app: &mut OpenCADStudio, ms: u64) {
        let now = iced::time::Instant::now() + std::time::Duration::from_millis(ms);
        toolbar(app, ToolbarMsg::HoldTick(now));
    }

    #[test]
    fn a_long_press_opens_the_variants_flyout() {
        let mut app = app();
        let tool = variant_tool();
        toolbar(&mut app, ToolbarMsg::HoldStart(ToolbarId::Draw, tool));
        tick(&mut app, 100);
        assert!(app.ribbon.open_dropdown.is_none(), "too early");
        tick(&mut app, 600);
        assert_eq!(
            app.ribbon.open_dropdown.as_deref(),
            Some(crate::ui::classic_toolbar::flyout_id(ToolbarId::Draw, tool).as_str())
        );
        assert!(crate::ui::classic_toolbar::flyout_overlay(&app.ribbon, 1600.0, 900.0).is_some());
        // The ribbon's own overlay must leave the id alone, so `view_main` falls
        // through to the flyout.
        assert!(app
            .ribbon
            .dropdown_overlay(&[], &[], (1600.0, 900.0), false, &[])
            .is_none());
    }

    #[test]
    fn a_short_press_runs_the_command_and_opens_nothing() {
        // Review focus 5.
        let mut app = OpenCADStudio::new_for_test();
        app.toolbars = ToolbarLayout::default();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let tool = variant_tool();
        let event = crate::modules::ModuleEvent::Command(tool.to_string());
        toolbar(&mut app, ToolbarMsg::HoldStart(ToolbarId::Draw, tool));
        toolbar(
            &mut app,
            ToolbarMsg::HoldEnd { tool_id: tool.to_string(), event },
        );
        assert!(app.tool_hold.is_none());
        assert!(app.ribbon.open_dropdown.is_none());
        assert!(app.tabs[i].active_cmd.is_some(), "{tool} should have started");
    }

    #[test]
    fn a_held_press_does_not_run_the_command_on_release() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let tool = variant_tool();
        toolbar(&mut app, ToolbarMsg::HoldStart(ToolbarId::Draw, tool));
        tick(&mut app, 600);
        let event = crate::modules::ModuleEvent::Command(tool.to_string());
        toolbar(
            &mut app,
            ToolbarMsg::HoldEnd { tool_id: tool.to_string(), event },
        );
        assert!(app.tabs[i].active_cmd.is_none(), "the flyout opened instead");
        assert!(app.ribbon.open_dropdown.is_some(), "the flyout stays open");
    }

    #[test]
    fn leaving_the_button_cancels_the_hold() {
        let mut app = app();
        let tool = variant_tool();
        toolbar(&mut app, ToolbarMsg::HoldStart(ToolbarId::Draw, tool));
        toolbar(&mut app, ToolbarMsg::HoldCancel);
        tick(&mut app, 600);
        assert!(app.tool_hold.is_none());
        assert!(app.ribbon.open_dropdown.is_none());
    }

    #[test]
    fn the_last_visible_bar_cannot_be_hidden() {
        // Final review: with every bar hidden nothing is left to right-click.
        let mut app = app();
        for id in ToolbarId::all().to_vec() {
            if app.toolbars.is_visible(id) {
                toolbar(&mut app, ToolbarMsg::Toggle(id));
            }
        }
        let shown = ToolbarId::all()
            .iter()
            .filter(|id| app.toolbars.is_visible(**id))
            .count();
        assert_eq!(shown, 1, "the last bar stays so the list stays reachable");
    }

    #[test]
    fn the_flyout_belongs_to_the_bar_that_was_pressed() {
        // Final review: the same tool can sit in two bars (TEXT is a submenu
        // button on Draw and a plain button on the Text bar).
        use crate::ui::classic_toolbar::{flyout_id, variants_of};
        let mut app = app();
        let text = ToolbarId::all()
            .iter()
            .copied()
            .find(|id| id.title() == "Text")
            .expect("a Text bar");
        assert!(variants_of(ToolbarId::Draw, "TEXT").is_some());
        assert!(variants_of(text, "TEXT").is_none(), "the Text bar has no flyout there");
        assert_ne!(flyout_id(ToolbarId::Draw, "TEXT"), flyout_id(text, "TEXT"));
        toolbar(&mut app, ToolbarMsg::HoldStart(ToolbarId::Draw, "TEXT"));
        tick(&mut app, 600);
        assert_eq!(
            app.ribbon.open_dropdown.as_deref(),
            Some(flyout_id(ToolbarId::Draw, "TEXT").as_str())
        );
    }
}
