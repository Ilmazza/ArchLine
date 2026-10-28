//! STRETCH for the object types `handle_stretch_entities` has no vertex logic
//! for: points, splines, leaders, multilines, wipeouts and non-associative
//! hatches. Each moves the vertices that lie inside a crossing window by
//! the displacement, the way editing those vertices by their grips would.

use crate::scene::model::object::{GripApply, GripDef, GripShape};
use crate::scene::view::dispatch;
use codec::EntityType;
use glam::DVec3;

/// The grips of `entity` that stand for one of its vertices, leaving out the
/// ones that move or reshape the whole object: a spline's mode menu, a
/// leader's centre, a hatch's pattern origin.
fn vertex_grips(entity: &EntityType) -> Vec<GripDef> {
    let grips = dispatch::grips(entity);
    match entity {
        EntityType::Point(_) | EntityType::MLine(_) | EntityType::Wipeout(_) => grips,
        EntityType::Spline(_) => grips
            .into_iter()
            .filter(|grip| !matches!(grip.shape, GripShape::Dropdown | GripShape::DropdownAdjacent))
            .collect(),
        EntityType::Leader(leader) => {
            let vertices = leader.vertices.len();
            grips.into_iter().filter(|grip| grip.id < vertices).collect()
        }
        // An associative hatch follows its boundary objects; only a free one is
        // stretched, by the vertices of its own boundary (the square grips: the
        // pattern origin is the round one).
        EntityType::Hatch(hatch) if !hatch.is_associative => grips
            .into_iter()
            .filter(|grip| matches!(grip.shape, GripShape::Square))
            .collect(),
        _ => Vec::new(),
    }
}

/// Stretch `entity` by its vertex grips inside the window. Returns whether
/// anything moved.
pub(super) fn stretch_by_grips(
    entity: &mut EntityType,
    in_win: &dyn Fn(f64, f64) -> bool,
    delta: DVec3,
) -> bool {
    // Taken before anything moves, and applied as absolute positions: a grip
    // that drags a neighbour along (a leader's elbow) then still lands exactly
    // where its own displacement puts it.
    let moves: Vec<(usize, DVec3)> = vertex_grips(entity)
        .into_iter()
        .filter(|grip| in_win(grip.world.x, grip.world.y))
        .map(|grip| (grip.id, grip.world + delta))
        .collect();
    for (id, target) in &moves {
        dispatch::apply_grip(entity, *id, GripApply::Absolute(*target));
    }
    !moves.is_empty()
}

#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;
    use codec::{EntityType, Handle};
    use codec::entities::{
        BoundaryEdge, BoundaryPath, Hatch, Leader, MLine, Point, PolylineEdge, Spline, Wipeout,
    };
    use codec::types::{Vector2, Vector3};
    use glam::DVec3;

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// Stretch `handle` with a crossing window around the origin only.
    fn stretch_at_origin(app: &mut OpenCADStudio, handle: Handle) {
        let window = (DVec3::new(-1.0, -1.0, -1.0), DVec3::new(1.0, 1.0, 1.0));
        let _ = app.handle_stretch_entities(vec![handle], vec![window], Vec::new(), DVec3::new(5.0, 3.0, 0.0));
    }

    fn entity(app: &OpenCADStudio, handle: Handle) -> EntityType {
        app.tabs[0].scene.document.get_entity(handle).cloned().expect("entity")
    }

    fn near(p: &Vector3, x: f64, y: f64) -> bool {
        (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9
    }

    #[test]
    fn a_point_in_the_window_moves() {
        let mut app = app();
        let inside = app.tabs[0].scene.add_entity(EntityType::Point(Point::at(Vector3::ZERO)));
        let outside = app.tabs[0]
            .scene
            .add_entity(EntityType::Point(Point::at(Vector3::new(50.0, 50.0, 0.0))));
        let window = (DVec3::new(-1.0, -1.0, -1.0), DVec3::new(1.0, 1.0, 1.0));
        let _ = app.handle_stretch_entities(
            vec![inside, outside],
            vec![window],
            Vec::new(),
            DVec3::new(5.0, 3.0, 0.0),
        );
        let EntityType::Point(p) = entity(&app, inside) else { panic!() };
        assert!(near(&p.location, 5.0, 3.0), "{:?}", p.location);
        let EntityType::Point(p) = entity(&app, outside) else { panic!() };
        assert!(near(&p.location, 50.0, 50.0));
    }

    #[test]
    fn a_spline_moves_only_the_control_points_in_the_window() {
        let mut app = app();
        let spline = Spline::from_control_points(
            3,
            vec![
                Vector3::ZERO,
                Vector3::new(3.0, 5.0, 0.0),
                Vector3::new(7.0, 5.0, 0.0),
                Vector3::new(10.0, 0.0, 0.0),
            ],
        );
        let h = app.tabs[0].scene.add_entity(EntityType::Spline(spline));
        stretch_at_origin(&mut app, h);
        let EntityType::Spline(s) = entity(&app, h) else { panic!() };
        assert!(near(&s.control_points[0], 5.0, 3.0), "{:?}", s.control_points);
        assert!(near(&s.control_points[1], 3.0, 5.0));
        assert!(near(&s.control_points[3], 10.0, 0.0));
    }

    #[test]
    fn a_leader_moves_the_vertex_in_the_window() {
        let mut app = app();
        let leader = Leader::from_vertices(vec![
            Vector3::ZERO,
            Vector3::new(10.0, 10.0, 0.0),
            Vector3::new(20.0, 10.0, 0.0),
        ]);
        let h = app.tabs[0].scene.add_entity(EntityType::Leader(leader));
        stretch_at_origin(&mut app, h);
        let EntityType::Leader(l) = entity(&app, h) else { panic!() };
        assert!(near(&l.vertices[0], 5.0, 3.0), "{:?}", l.vertices);
        assert!(near(&l.vertices[1], 10.0, 10.0) && near(&l.vertices[2], 20.0, 10.0));
    }

    #[test]
    fn a_multiline_moves_the_vertex_in_the_window() {
        let mut app = app();
        let mline = MLine::from_points(&[
            Vector3::ZERO,
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
        ]);
        let h = app.tabs[0].scene.add_entity(EntityType::MLine(mline));
        stretch_at_origin(&mut app, h);
        let EntityType::MLine(m) = entity(&app, h) else { panic!() };
        assert!(near(&m.vertices[0].position, 5.0, 3.0), "{:?}", m.vertices[0].position);
        assert!(near(&m.vertices[1].position, 10.0, 0.0));
        assert!(near(&m.vertices[2].position, 10.0, 10.0));
    }

    #[test]
    fn a_wipeout_moves_the_corner_in_the_window() {
        let mut app = app();
        let wipeout = Wipeout::polygonal(
            &[
                Vector2::new(0.0, 0.0),
                Vector2::new(10.0, 0.0),
                Vector2::new(10.0, 10.0),
                Vector2::new(0.0, 10.0),
            ],
            0.0,
        );
        let h = app.tabs[0].scene.add_entity(EntityType::Wipeout(wipeout));
        stretch_at_origin(&mut app, h);
        let corners: Vec<_> = crate::scene::view::dispatch::grips(&entity(&app, h))
            .into_iter()
            .map(|g| g.world)
            .collect();
        assert!((corners[0] - DVec3::new(5.0, 3.0, 0.0)).length() < 1e-6, "{corners:?}");
        assert!((corners[1] - DVec3::new(10.0, 0.0, 0.0)).length() < 1e-6, "{corners:?}");
        assert!((corners[2] - DVec3::new(10.0, 10.0, 0.0)).length() < 1e-6, "{corners:?}");
    }

    fn square_hatch(associative: bool) -> Hatch {
        let mut hatch = Hatch::solid();
        hatch.is_associative = associative;
        let mut path = BoundaryPath::external();
        path.edges.push(BoundaryEdge::Polyline(PolylineEdge::new(
            vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(10.0, 0.0),
                Vector2::new(10.0, 10.0),
                Vector2::new(0.0, 10.0),
            ],
            true,
        )));
        hatch.paths.push(path);
        hatch
    }

    fn hatch_vertices(app: &OpenCADStudio, h: Handle) -> Vec<Vector3> {
        let EntityType::Hatch(hatch) = entity(app, h) else { panic!() };
        match &hatch.paths[0].edges[0] {
            BoundaryEdge::Polyline(p) => p.vertices.clone(),
            _ => panic!(),
        }
    }

    #[test]
    fn a_non_associative_hatch_moves_the_boundary_vertex_in_the_window() {
        let mut app = app();
        let h = app.tabs[0].scene.add_entity(EntityType::Hatch(Box::new(square_hatch(false))));
        stretch_at_origin(&mut app, h);
        let v = hatch_vertices(&app, h);
        assert!(near(&v[0], 5.0, 3.0), "{v:?}");
        assert!(near(&v[1], 10.0, 0.0) && near(&v[2], 10.0, 10.0));
    }

    #[test]
    fn an_associative_hatch_follows_its_boundary_and_is_not_stretched_itself() {
        let mut app = app();
        let h = app.tabs[0].scene.add_entity(EntityType::Hatch(Box::new(square_hatch(true))));
        stretch_at_origin(&mut app, h);
        let v = hatch_vertices(&app, h);
        assert!(near(&v[0], 0.0, 0.0), "{v:?}");
    }
}
