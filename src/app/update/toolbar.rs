//! ArchLine: update handling for the dockable classic toolbars.

use crate::app::{Message, OpenCADStudio};
use crate::ui::toolbar_dock::{
    bar_length, bar_size, ToolbarDrag, ToolbarMsg, DRAG_THRESHOLD, GRIP_ANCHOR,
};
use crate::ui::toolbar_layout::{clamp_floating, resolve_drop, DropCtx};

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
            ToolbarMsg::Toggle(id) => {
                let shown = self.toolbars.is_visible(id);
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
        drag(&mut app, ToolbarId::Block, Point::new(8.0, 400.0));
        assert!(app.toolbar_drag.is_none());
        assert_eq!(
            app.toolbars.placement(ToolbarId::Block),
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
}
