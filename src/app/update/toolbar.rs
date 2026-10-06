//! ArchLine: update handling for the dockable classic toolbars.

use crate::app::{Message, OpenCADStudio};
use crate::ui::toolbar_dock::{bar_length, bar_size, ToolbarDrag, ToolbarMsg, GRIP_ANCHOR};
use crate::ui::toolbar_layout::{clamp_floating, resolve_drop, DropCtx};

impl OpenCADStudio {
    pub(super) fn on_toolbar(&mut self, m: ToolbarMsg) -> iced::Task<Message> {
        match m {
            ToolbarMsg::Grab(id) => {
                self.toolbar_drag = Some(ToolbarDrag {
                    id,
                    cursor: None,
                    target: None,
                });
            }
            ToolbarMsg::DragMove(p) => {
                if let Some(drag) = &mut self.toolbar_drag {
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
                self.toolbars.redock(id);
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

    #[test]
    fn dragging_a_bar_to_the_left_edge_docks_it_there() {
        let mut app = app();
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Block)));
        assert!(app.toolbar_drag.is_some());
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragMove(Point::new(8.0, 400.0))));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragRelease));
        assert!(app.toolbar_drag.is_none());
        assert_eq!(
            app.toolbars.placement(ToolbarId::Block),
            Placement::Docked(DockSlot { edge: Edge::Left, lane: 0, index: 1 })
        );
    }

    #[test]
    fn dragging_into_the_middle_floats_the_bar_inside_the_window() {
        let mut app = app();
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Draw)));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragMove(Point::new(800.0, 450.0))));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragRelease));
        match app.toolbars.placement(ToolbarId::Draw) {
            Placement::Floating { x, y, .. } => {
                assert!(x >= 0.0 && y >= 0.0);
                assert!(x < 800.0 && y < 450.0, "top-left is offset from the pointer");
            }
            other => panic!("expected floating, got {other:?}"),
        }
    }

    #[test]
    fn a_click_on_the_grip_without_moving_changes_nothing() {
        // Review focus 5.
        let mut app = app();
        let before = app.toolbars.clone();
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Modify)));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragRelease));
        assert_eq!(app.toolbars, before);
        assert!(app.toolbar_drag.is_none());
    }

    #[test]
    fn escape_cancels_a_drag_and_keeps_the_layout() {
        let mut app = app();
        let before = app.toolbars.clone();
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Draw)));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragMove(Point::new(800.0, 450.0))));
        let _ = app.update(Message::CommandEscape);
        assert!(app.toolbar_drag.is_none());
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragRelease));
        assert_eq!(app.toolbars, before);
    }

    #[test]
    fn redock_and_reset_restore_positions() {
        let mut app = app();
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Layers)));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragMove(Point::new(800.0, 450.0))));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragRelease));
        assert!(matches!(
            app.toolbars.placement(ToolbarId::Layers),
            Placement::Floating { .. }
        ));
        let _ = app.update(Message::Toolbar(ToolbarMsg::Redock(ToolbarId::Layers)));
        assert_eq!(
            app.toolbars.placement(ToolbarId::Layers),
            Placement::Docked(DockSlot { edge: Edge::Top, lane: 1, index: 0 })
        );
        let _ = app.update(Message::Toolbar(ToolbarMsg::Grab(ToolbarId::Draw)));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragMove(Point::new(800.0, 450.0))));
        let _ = app.update(Message::Toolbar(ToolbarMsg::DragRelease));
        let _ = app.update(Message::Toolbar(ToolbarMsg::Reset));
        assert_eq!(app.toolbars, ToolbarLayout::default());
    }
}
