//! The HATCH window's gradient side on the app: resolving "Use Current",
//! the colour picks, the keys with an overlay open. (Tests below also cover
//! the Properties panel's gradient colours.)

use codec::types::{Color, Transparency};

use crate::app::OpenCADStudio;
use crate::modules::draw::draw::hatch_settings::HatchColor;

impl OpenCADStudio {
    /// The drawing's current colour (CECOLOR) in tab `i`: what "Use Current"
    /// means. Not the ribbon's colour, which follows the selection (and HATCH
    /// keeps its preselected boundaries selected until OK).
    pub(in crate::app) fn hatch_current_color(&self, i: usize) -> Color {
        self.tabs[i].scene.document.header.current_entity_color
    }

    /// The colour and transparency a hatch made by the window gets in tab
    /// `i` (the window's owner): "Use Current" is that drawing's current
    /// colour; a chosen colour is that colour. The transparency is the
    /// current one of that tab's drawing.
    pub(in crate::app) fn hatch_creation_style(
        &self,
        i: usize,
        color: HatchColor,
    ) -> (Color, Transparency) {
        let color = match color {
            HatchColor::UseCurrent => self.hatch_current_color(i),
            HatchColor::Color(color) => color,
        };
        (color, self.tabs[i].scene.document.current_entity_transparency())
    }

    /// The RGBA the preview of a pattern or solid is drawn with in tab `i`:
    /// the colour the hatch will be drawn with. ByLayer resolves through that
    /// tab's current layer (ByBlock falls back as the renderer draws it
    /// outside a block), colour 7 follows the tab's background as the
    /// renderer adapts it, and the preview is translucent like every other.
    pub(in crate::app) fn hatch_preview_rgba(&self, i: usize, color: HatchColor) -> [f32; 4] {
        let (color, _) = self.hatch_creation_style(i, color);
        let scene = &self.tabs[i].scene;
        let rgba = match color {
            Color::ByLayer => scene.layer_color(&self.tabs[i].active_layer),
            other => crate::scene::convert::tess_util::aci_to_rgba(&other),
        };
        let mut rgba = crate::scene::view::render::adapt_to_bg(rgba, scene.current_bg());
        rgba[3] = 0.75;
        rgba
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};
    use codec::types::Color;

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        app
    }

    /// A drawing holding one gradient hatch built straight in the document.
    fn app_with_gradient(stops: usize) -> (OpenCADStudio, codec::Handle) {
        use crate::entities::hatch_fill::{apply_gradient, GradientSpec};
        use crate::scene::model::hatch_model::GradientKind;
        let mut app = new_app();
        let i = app.active_tab;
        let mut hatch = codec::entities::Hatch::solid();
        apply_gradient(
            &mut hatch,
            &GradientSpec {
                kind: GradientKind::Linear,
                invert: false,
                one_color: false,
                color1: Color::Index(1),
                color2: Color::Index(5),
                tint: 1.0,
                angle_rad: 0.0,
                centered: true,
            },
        );
        hatch.gradient_color.colors.truncate(stops);
        let handle = app.tabs[i].scene.add_entity(codec::EntityType::Hatch(hatch));
        app.tabs[i].scene.select_entity(handle, true);
        app.refresh_properties();
        (app, handle)
    }

    fn stops(app: &OpenCADStudio, handle: codec::Handle) -> Vec<(f64, Color)> {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Hatch(h)) => {
                h.gradient_color.colors.iter().map(|e| (e.value, e.color)).collect()
            }
            other => panic!("not a hatch: {other:?}"),
        }
    }

    #[test]
    fn the_panel_colour_fields_replace_one_stop() {
        let (mut app, handle) = app_with_gradient(2);
        let _ = app.update(Message::PropColorFieldChanged {
            field: "gradient_color_2".into(),
            color: Color::Index(3),
        });
        assert_eq!(stops(&app, handle), vec![(0.0, Color::Index(1)), (1.0, Color::Index(3))]);
        let _ = app.update(Message::PropColorFieldChanged {
            field: "gradient_color_1".into(),
            color: Color::Index(2),
        });
        assert_eq!(stops(&app, handle), vec![(0.0, Color::Index(2)), (1.0, Color::Index(3))]);
    }

    #[test]
    fn a_gradient_with_no_stops_gets_them_created() {
        let (mut app, handle) = app_with_gradient(0);
        let _ = app.update(Message::PropColorFieldChanged {
            field: "gradient_color_2".into(),
            color: Color::Index(3),
        });
        assert_eq!(stops(&app, handle), vec![(0.0, Color::Index(7)), (1.0, Color::Index(3))]);
    }

    // ── Creation from the window (HATCH / GRADIENT) ────────────────────────

    use crate::app::ModalKind;
    use crate::entities::hatch_fill::{read_gradient, rgba_of, tinted_second_color, FillKind};
    use crate::modules::draw::draw::hatch_settings::{
        FillTab, HatchColor, HatchRegion, RegionOrigin,
    };
    use crate::scene::model::hatch_model::{GradientKind, HatchPattern};
    use crate::ui::window::hatch_dialog::{AddKind, Field};

    fn add_line(app: &mut OpenCADStudio, x1: f64, y1: f64, x2: f64, y2: f64) {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Line(codec::entities::Line::from_points(
                codec::types::Vector3::new(x1, y1, 0.0),
                codec::types::Vector3::new(x2, y2, 0.0),
            )));
    }

    fn app_with_rectangle() -> OpenCADStudio {
        let mut app = new_app();
        add_line(&mut app, 0.0, 0.0, 20.0, 0.0);
        add_line(&mut app, 20.0, 0.0, 20.0, 10.0);
        add_line(&mut app, 20.0, 10.0, 0.0, 10.0);
        add_line(&mut app, 0.0, 10.0, 0.0, 0.0);
        app
    }

    fn region() -> (HatchRegion, RegionOrigin) {
        (
            HatchRegion {
                rings: vec![vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]]],
            },
            RegionOrigin::Points,
        )
    }

    /// Open the window with `command`, give it the rectangle and apply `fields`.
    fn open_with(app: &mut OpenCADStudio, command: &str, fields: &[Field]) {
        let _ = app.dispatch_command(command);
        app.hatch_dialog.as_mut().expect("the window opened").regions.push(region());
        for field in fields {
            let _ = app.update(Message::HatchDialogField(field.clone()));
        }
    }

    fn all_hatches(app: &OpenCADStudio) -> Vec<codec::entities::Hatch> {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                codec::EntityType::Hatch(hatch) => Some(hatch.clone()),
                _ => None,
            })
            .collect()
    }

    fn the_hatch(app: &OpenCADStudio) -> codec::entities::Hatch {
        let mut hatches = all_hatches(app);
        assert_eq!(hatches.len(), 1, "exactly one hatch was made");
        hatches.remove(0)
    }

    /// The scene model of the only hatch in the active tab.
    fn the_model(app: &OpenCADStudio) -> crate::scene::model::hatch_model::HatchModel {
        let hatches = &app.tabs[app.active_tab].scene.hatches;
        assert_eq!(hatches.len(), 1, "exactly one hatch model");
        hatches.values().next().unwrap().clone()
    }

    fn ok(app: &mut OpenCADStudio) {
        let _ = app.update(Message::HatchDialogOk);
    }

    /// Every vertex of the hatch's boundary paths, in world coordinates.
    fn world_boundary(hatch: &codec::entities::Hatch) -> Vec<glam::DVec3> {
        use codec::entities::BoundaryEdge;
        let plane = crate::entities::curve::ocs_plane(hatch.normal, hatch.elevation);
        let (origin, x, y) = (
            glam::DVec3::from_array(plane.origin),
            glam::DVec3::from_array(plane.x_axis),
            glam::DVec3::from_array(plane.y_axis),
        );
        let to_world = |u: f64, v: f64| origin + x * u + y * v;
        let mut points = Vec::new();
        for path in &hatch.paths {
            for edge in &path.edges {
                match edge {
                    BoundaryEdge::Line(line) => {
                        points.push(to_world(line.start.x, line.start.y));
                        points.push(to_world(line.end.x, line.end.y));
                    }
                    BoundaryEdge::Polyline(polyline) => {
                        points.extend(polyline.vertices.iter().map(|v| to_world(v.x, v.y)));
                    }
                    other => panic!("a straight boundary edge, got {other:?}"),
                }
            }
        }
        assert!(!points.is_empty(), "the hatch has a boundary");
        points
    }

    #[test]
    fn hatch_opens_on_the_hatch_tab_and_gradient_on_the_gradient_tab() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("HATCH");
        assert_eq!(app.hatch_dialog.as_ref().unwrap().settings.tab, FillTab::Hatch);
        assert_eq!(app.tabs[app.active_tab].last_cmd.as_deref(), Some("HATCH"));
        let _ = app.update(Message::CloseModal);
        let _ = app.dispatch_command("GRADIENT");
        assert_eq!(app.active_modal, Some(ModalKind::Hatch));
        assert_eq!(app.hatch_dialog.as_ref().unwrap().settings.tab, FillTab::Gradient);
        assert_eq!(app.tabs[app.active_tab].last_cmd.as_deref(), Some("GRADIENT"));
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
    }

    #[test]
    fn the_remembered_tab_never_decides_how_the_window_opens() {
        let mut app = app_with_rectangle();
        app.hatch_last.tab = FillTab::Gradient;
        let _ = app.dispatch_command("HATCH");
        assert_eq!(app.hatch_dialog.as_ref().unwrap().settings.tab, FillTab::Hatch);
        let _ = app.update(Message::CloseModal);
        app.hatch_last.tab = FillTab::Hatch;
        let _ = app.dispatch_command("GRADIENT");
        assert_eq!(app.hatch_dialog.as_ref().unwrap().settings.tab, FillTab::Gradient);
    }

    #[test]
    fn the_gradient_tab_makes_a_gradient_whatever_the_pattern_name() {
        let mut app = app_with_rectangle();
        // The Hatch tab's pattern is SOLID, but the tab in force is Gradient.
        open_with(
            &mut app,
            "GRADIENT",
            &[Field::Pattern("SOLID".into()), Field::GradientShape(2)],
        );
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert_eq!(FillKind::of(&hatch), FillKind::Gradient);
        assert_eq!(hatch.gradient_color.name, "INVCYLINDER");
    }

    #[test]
    fn the_hatch_tab_with_solid_makes_a_solid_and_not_a_gradient() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[Field::Pattern("SOLID".into())]);
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert_eq!(FillKind::of(&hatch), FillKind::Solid);
        assert!(!hatch.gradient_color.enabled);
    }

    #[test]
    fn every_one_of_the_nine_shapes_is_made_and_read_back() {
        for (shape, (kind, invert)) in GradientKind::CHOICES.iter().copied().enumerate() {
            let mut app = app_with_rectangle();
            open_with(&mut app, "GRADIENT", &[Field::GradientShape(shape)]);
            ok(&mut app);
            let hatch = the_hatch(&app);
            assert_eq!(hatch.gradient_color.name, kind.dxf_name(invert), "shape {shape}");
            let read = read_gradient(&hatch);
            assert_eq!((read.kind, read.invert), (kind, invert), "shape {shape}");
            match the_model(&app).pattern {
                HatchPattern::Gradient { kind: k, invert: inv, .. } => {
                    assert_eq!((k, inv), (kind, invert), "shape {shape}: the scene model")
                }
                other => panic!("shape {shape}: a gradient model, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_one_colour_gradient_keeps_its_tint_and_ignores_the_hidden_colour() {
        let mut app = app_with_rectangle();
        let red = Color::Index(1);
        open_with(
            &mut app,
            "GRADIENT",
            &[
                Field::GradientOneColor(true),
                Field::GradientTint(0.25),
                Field::GradientColor1(red),
                Field::GradientColor2(Color::Index(3)),
                Field::GradientCentered(false),
                Field::GradientAngle("45".into()),
            ],
        );
        ok(&mut app);
        let hatch = the_hatch(&app);
        let g = &hatch.gradient_color;
        assert!(g.is_single_color);
        assert_eq!(g.color_tint, 0.25);
        // The scene model carries colours as RGBA and `add_hatch` writes the
        // stops from it: an index colour is stored as the true colour it shows.
        assert_eq!(g.colors[0].color, Color::Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(
            g.colors[1].color,
            tinted_second_color(rgba_of(red).unwrap(), 0.25),
            "the second stop is the tint, not Color 2"
        );
        assert_eq!(g.shift, 1.0);
        assert!((g.angle - 45f64.to_radians()).abs() < 1e-9);
        // The model the scene shows agrees with the stored one.
        match &the_model(&app).pattern {
            HatchPattern::Gradient { one_color, tint, shift, .. } => {
                assert!(*one_color);
                assert_eq!(*tint, 0.25);
                assert_eq!(*shift, 1.0);
            }
            other => panic!("a gradient model, got {other:?}"),
        }
    }

    #[test]
    fn a_two_colour_gradient_keeps_both_colours_and_its_centring() {
        let mut app = app_with_rectangle();
        let (first, second) = (Color::Index(1), Color::Rgb { r: 10, g: 200, b: 30 });
        open_with(
            &mut app,
            "GRADIENT",
            &[
                Field::GradientShape(3),
                Field::GradientColor1(first),
                Field::GradientColor2(second),
                Field::GradientTint(0.1),
                Field::GradientAngle("30".into()),
            ],
        );
        ok(&mut app);
        let hatch = the_hatch(&app);
        let g = &hatch.gradient_color;
        assert!(g.enabled && !g.is_single_color);
        assert_eq!(g.name, "SPHERICAL");
        // Index colour 1 is stored as the true colour it shows (see above);
        // a true colour is stored as it is.
        assert_eq!(
            g.colors.iter().map(|stop| (stop.value, stop.color)).collect::<Vec<_>>(),
            vec![(0.0, Color::Rgb { r: 255, g: 0, b: 0 }), (1.0, second)]
        );
        assert_eq!(g.shift, 0.0, "centred");
        assert!((g.angle - 30f64.to_radians()).abs() < 1e-9);
        assert!((hatch.pattern_angle - 30f64.to_radians()).abs() < 1e-9);
        let model = the_model(&app);
        assert_eq!(model.color, rgba_of(first).unwrap());
        match &model.pattern {
            HatchPattern::Gradient { color2, one_color, shift, kind, angle_deg, .. } => {
                assert_eq!(*color2, rgba_of(second).unwrap());
                assert!(!*one_color);
                assert_eq!(*shift, 0.0);
                assert_eq!(*kind, GradientKind::Spherical);
                assert!((*angle_deg - 30.0).abs() < 1e-4);
            }
            other => panic!("a gradient model, got {other:?}"),
        }
    }

    /// The drawing's current colour (CECOLOR) in the active tab, with the
    /// ribbon left on another colour: "Use Current" must read the drawing.
    fn set_current_colour(app: &mut OpenCADStudio, color: Color, ribbon: Color) {
        let i = app.active_tab;
        app.tabs[i].scene.document.header.current_entity_color = color;
        app.ribbon.active_color = ribbon;
    }

    #[test]
    fn use_current_follows_the_drawings_current_colour_and_a_chosen_colour_wins() {
        // Default current colour is ByLayer: the entity is ByLayer, as before.
        let mut app = app_with_rectangle();
        let i = app.active_tab;
        assert_eq!(app.tabs[i].scene.document.header.current_entity_color, Color::ByLayer);
        open_with(&mut app, "HATCH", &[]);
        ok(&mut app);
        assert_eq!(the_hatch(&app).common.color, Color::ByLayer);

        let mut app = app_with_rectangle();
        set_current_colour(&mut app, Color::Index(1), Color::Index(4));
        open_with(&mut app, "HATCH", &[]);
        ok(&mut app);
        assert_eq!(the_hatch(&app).common.color, Color::Index(1), "CECOLOR, not the ribbon");

        let mut app = app_with_rectangle();
        set_current_colour(&mut app, Color::ByLayer, Color::Index(4));
        open_with(&mut app, "HATCH", &[]);
        ok(&mut app);
        assert_eq!(the_hatch(&app).common.color, Color::ByLayer, "CECOLOR, not the ribbon");

        let mut app = app_with_rectangle();
        set_current_colour(&mut app, Color::Index(1), Color::Index(4));
        open_with(&mut app, "HATCH", &[Field::Color(HatchColor::Color(Color::Index(3)))]);
        ok(&mut app);
        assert_eq!(the_hatch(&app).common.color, Color::Index(3));
    }

    #[test]
    fn use_current_also_applies_to_solids_and_separate_hatches() {
        let mut app = app_with_rectangle();
        set_current_colour(&mut app, Color::Index(5), Color::Index(2));
        open_with(
            &mut app,
            "HATCH",
            &[Field::Pattern("SOLID".into()), Field::Separate(true)],
        );
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert_eq!(hatch.common.color, Color::Index(5));
        assert!(hatch.is_solid);
    }

    /// A 20 x 10 rectangle of four lines of colour `color`, selected, with
    /// the properties refreshed as the app does after a selection (the ribbon
    /// then shows the selection's colour).
    fn app_with_selected_rectangle(color: Color) -> OpenCADStudio {
        let mut app = new_app();
        let i = app.active_tab;
        for (a, b, c, d) in [
            (0.0, 0.0, 20.0, 0.0),
            (20.0, 0.0, 20.0, 10.0),
            (20.0, 10.0, 0.0, 10.0),
            (0.0, 10.0, 0.0, 0.0),
        ] {
            let mut line = codec::entities::Line::from_points(
                codec::types::Vector3::new(a, b, 0.0),
                codec::types::Vector3::new(c, d, 0.0),
            );
            line.common.color = color;
            let handle = app.tabs[i].scene.add_entity(codec::EntityType::Line(line));
            app.tabs[i].scene.select_entity(handle, false);
        }
        app.refresh_properties();
        app
    }

    #[test]
    fn use_current_is_the_drawings_colour_not_the_preselected_objects() {
        for (case, fields) in [
            ("pattern", vec![]),
            ("solid", vec![Field::Pattern("SOLID".into())]),
            ("separate", vec![Field::Separate(true)]),
        ] {
            for (boundary, current) in [
                (Color::Index(1), Color::ByLayer),
                (Color::ByLayer, Color::Index(3)),
            ] {
                let mut app = app_with_selected_rectangle(boundary);
                let i = app.active_tab;
                app.tabs[i].scene.document.header.current_entity_color = current;
                // What the app shows with the rectangle selected.
                assert_eq!(app.ribbon.active_color, boundary, "{case}: the ribbon follows");
                let _ = app.dispatch_command("HATCH");
                assert_eq!(
                    app.hatch_dialog.as_ref().unwrap().regions.len(),
                    1,
                    "{case}: the preselection seeds the area"
                );
                for field in &fields {
                    let _ = app.update(Message::HatchDialogField(field.clone()));
                }
                ok(&mut app);
                let hatch = the_hatch(&app);
                assert_eq!(hatch.common.color, current, "{case}: boundary {boundary:?}");
                assert_eq!(hatch.is_solid, case == "solid", "{case}");
            }
        }
    }

    #[test]
    fn the_hatch_takes_the_current_transparency_of_its_drawing() {
        let mut app = app_with_rectangle();
        let i = app.active_tab;
        let half = codec::types::Transparency::Explicit(128);
        assert!(app.tabs[i].scene.document.set_current_entity_transparency(half));
        open_with(&mut app, "HATCH", &[]);
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert_eq!(hatch.common.transparency, half);
        assert_eq!(hatch.common.color, Color::ByLayer);
    }

    #[test]
    fn the_gradient_tab_does_not_use_the_fill_colour() {
        let mut app = app_with_rectangle();
        set_current_colour(&mut app, Color::Index(1), Color::Index(1));
        open_with(
            &mut app,
            "GRADIENT",
            &[Field::Color(HatchColor::Color(Color::Index(3)))],
        );
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert_eq!(hatch.common.color, Color::ByLayer);
        assert_eq!(FillKind::of(&hatch), FillKind::Gradient);
    }

    #[test]
    fn ok_leaves_one_undo_step() {
        let mut app = app_with_rectangle();
        let entities_before = app.tabs[app.active_tab].scene.document.entities().count();
        open_with(&mut app, "GRADIENT", &[]);
        let before = app.tabs[app.active_tab].history.undo_stack.len();
        ok(&mut app);
        assert_eq!(app.tabs[app.active_tab].history.undo_stack.len(), before + 1);
        assert_eq!(FillKind::of(&the_hatch(&app)), FillKind::Gradient);
        let _ = app.update(Message::Undo);
        assert!(all_hatches(&app).is_empty(), "one undo removes the gradient");
        assert_eq!(
            app.tabs[app.active_tab].scene.document.entities().count(),
            entities_before,
            "and nothing else"
        );
    }

    #[test]
    fn dash_gradient_and_scripted_gradient_run_without_a_window() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("-GRADIENT");
        assert!(app.active_modal.is_none() && app.hatch_dialog.is_none());
        let i = app.active_tab;
        assert_eq!(app.tabs[i].active_cmd.as_ref().map(|c| c.name()), Some("GRADIENT"));
        let _ = app.update(Message::CommandEscape);

        app.scripted_dispatch = true;
        let _ = app.dispatch_command("GRADIENT");
        app.scripted_dispatch = false;
        assert!(app.active_modal.is_none() && app.hatch_dialog.is_none());
        assert_eq!(app.tabs[i].active_cmd.as_ref().map(|c| c.name()), Some("GRADIENT"));
        let _ = app.update(Message::CommandEscape);

        let _ = app.update(Message::ScriptLine("GRADIENT".into()));
        assert!(!app.scripted_dispatch, "the flag is restored afterwards");
        assert!(app.active_modal.is_none() && app.hatch_dialog.is_none());
        assert_eq!(app.tabs[i].active_cmd.as_ref().map(|c| c.name()), Some("GRADIENT"));
    }

    #[test]
    fn dash_gradient_makes_the_gradient_it_always_made() {
        let mut app = app_with_rectangle();
        set_current_colour(&mut app, Color::Index(1), Color::Index(1));
        let _ = app.dispatch_command("-GRADIENT");
        let i = app.active_tab;
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("-GRADIENT runs")
            .on_point(glam::DVec3::new(10.0, 5.0, 0.0));
        let _ = app.apply_cmd_result(result);
        let hatch = the_hatch(&app);
        let g = &hatch.gradient_color;
        assert!(g.enabled && !g.is_single_color);
        assert_eq!(g.name, "LINEAR");
        assert_eq!(
            g.colors.iter().map(|stop| stop.color).collect::<Vec<_>>(),
            vec![
                crate::entities::hatch_fill::DEFAULT_GRADIENT_COLOR1,
                crate::entities::hatch_fill::DEFAULT_GRADIENT_COLOR2,
            ]
        );
        assert_eq!(hatch.common.color, Color::ByLayer, "the command line keeps ByLayer");
        assert_eq!(hatch.normal, codec::types::Vector3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn dash_gradient_is_registered() {
        // The command's own registration carries both names...
        assert!(inventory::iter::<crate::command::CommandRegistration>().any(|registration| {
            registration.names.contains(&"GRADIENT") && registration.names.contains(&"-GRADIENT")
        }));
        // ...and the one-shot list of `app/commands/mod.rs` names it next to -HATCH.
        assert!(inventory::iter::<crate::command::CommandRegistration>().any(|registration| {
            registration.names.contains(&"-HATCH") && registration.names.contains(&"-GRADIENT")
        }));
    }

    #[test]
    fn the_remembered_settings_keep_both_tabs() {
        let mut app = app_with_rectangle();
        open_with(
            &mut app,
            "GRADIENT",
            &[
                Field::Scale("3".into()),
                Field::GradientShape(6),
                Field::Color(HatchColor::Color(Color::Index(2))),
            ],
        );
        ok(&mut app);
        assert_eq!(app.hatch_last.scale, "3");
        assert_eq!(app.hatch_last.gradient.shape, 6);
        assert_eq!(app.hatch_last.color, HatchColor::Color(Color::Index(2)));
        // Both tabs come back, on whichever tab the window opens.
        let _ = app.dispatch_command("HATCH");
        let settings = &app.hatch_dialog.as_ref().unwrap().settings;
        assert_eq!(settings.tab, FillTab::Hatch);
        assert_eq!((settings.scale.as_str(), settings.gradient.shape), ("3", 6));
    }

    #[test]
    fn preview_leaves_the_remembered_settings_and_add_takes_them() {
        let mut app = app_with_rectangle();
        let before = app.hatch_last.clone();
        open_with(&mut app, "GRADIENT", &[Field::GradientShape(4)]);
        let _ = app.update(Message::HatchDialogPreview);
        assert_eq!(app.hatch_last, before, "Preview remembers nothing");
        let _ = app.update(Message::CommandEscape);
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        assert_eq!(app.hatch_last.gradient.shape, 4, "Add remembers");
        assert_eq!(app.hatch_last.tab, FillTab::Gradient);
    }

    #[test]
    fn add_pick_points_works_from_the_gradient_tab() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("GRADIENT");
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        let i = app.active_tab;
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("a collector is running")
            .on_point(glam::DVec3::new(10.0, 5.0, 0.0));
        let _ = app.apply_cmd_result(result);
        let _ = app.feed_command(crate::command::StepInput::Enter);
        let state = app.hatch_dialog.as_ref().unwrap();
        assert_eq!(state.regions.len(), 1);
        assert_eq!(state.settings.tab, FillTab::Gradient, "back on the same tab");
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert_eq!(FillKind::of(&hatch), FillKind::Gradient);
        assert!(hatch.is_associative, "picked from lines: associative");
        assert_eq!(
            hatch.paths.iter().map(|path| path.boundary_handles.len()).sum::<usize>(),
            4,
            "the four lines are its boundary objects"
        );
    }

    #[test]
    fn the_collector_of_add_carries_no_preview_colour() {
        let mut app = app_with_rectangle();
        set_current_colour(&mut app, Color::Index(1), Color::Index(1));
        let _ = app.dispatch_command("HATCH");
        let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
        let i = app.active_tab;
        let collector = app.tabs[i].active_cmd.as_mut().expect("a collector is running");
        let _ = collector.on_point(glam::DVec3::new(10.0, 5.0, 0.0));
        let models = collector.hatch_preview_models().expect("it previews the areas");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].color, [0.15, 0.55, 1.0, 0.75], "the collector's own blue");
    }

    #[test]
    fn separate_gradients_are_one_each() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("GRADIENT");
        let state = app.hatch_dialog.as_mut().unwrap();
        state.regions.push(region());
        state.regions.push((
            HatchRegion {
                rings: vec![vec![[30.0, 0.0], [50.0, 0.0], [50.0, 10.0], [30.0, 10.0]]],
            },
            RegionOrigin::Points,
        ));
        let _ = app.update(Message::HatchDialogField(Field::Separate(true)));
        let _ = app.update(Message::HatchDialogField(Field::GradientShape(1)));
        ok(&mut app);
        let hatches = all_hatches(&app);
        assert_eq!(hatches.len(), 2);
        assert!(hatches.iter().all(|h| FillKind::of(h) == FillKind::Gradient));
        assert!(hatches.iter().all(|h| h.gradient_color.name == "CYLINDER"));
        // One each: every hatch holds one of the two rectangles.
        let mut lowest_x: Vec<f64> = hatches
            .iter()
            .map(|h| world_boundary(h).iter().map(|p| p.x).fold(f64::INFINITY, f64::min))
            .collect();
        lowest_x.sort_by(f64::total_cmp);
        assert!((lowest_x[0] - 0.0).abs() < 1e-9 && (lowest_x[1] - 30.0).abs() < 1e-9);
    }

    #[test]
    fn a_gradient_keeps_the_island_style_and_its_island() {
        let mut app = app_with_rectangle();
        let _ = app.dispatch_command("GRADIENT");
        app.hatch_dialog.as_mut().unwrap().regions.push((
            HatchRegion {
                rings: vec![
                    vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]],
                    vec![[5.0, 2.0], [15.0, 2.0], [15.0, 8.0], [5.0, 8.0]],
                ],
            },
            RegionOrigin::Points,
        ));
        let _ = app.update(Message::HatchDialogField(Field::IslandStyle(
            codec::entities::HatchStyleType::Outer,
        )));
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert_eq!(FillKind::of(&hatch), FillKind::Gradient);
        assert_eq!(hatch.style, codec::entities::HatchStyleType::Outer);
        assert_eq!(hatch.paths.len(), 2, "outer boundary and island");
    }

    // Review focus: a gradient on a layout and on a rotated plane.
    #[test]
    fn a_gradient_in_a_layout_lies_on_the_default_plane() {
        let mut app = new_app();
        let i = app.active_tab;
        app.tabs[i].scene.current_layout = "Layout1".to_string();
        assert!(!app.tabs[i].editing_model_space(), "really in paper space");
        for (a, b, c, d) in [
            (0.0, 0.0, 20.0, 0.0),
            (20.0, 0.0, 20.0, 10.0),
            (20.0, 10.0, 0.0, 10.0),
            (0.0, 10.0, 0.0, 0.0),
        ] {
            add_line(&mut app, a, b, c, d);
        }
        open_with(&mut app, "GRADIENT", &[]);
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert_eq!(hatch.normal, codec::types::Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(hatch.elevation, 0.0);
        assert_eq!(FillKind::of(&hatch), FillKind::Gradient);
        for point in world_boundary(&hatch) {
            assert!(point.z.abs() < 1e-9, "{point:?}");
            assert!((-1e-9..=20.0 + 1e-9).contains(&point.x), "{point:?}");
            assert!((-1e-9..=10.0 + 1e-9).contains(&point.y), "{point:?}");
        }
        let plane = the_model(&app).fill_plane.expect("the model knows its plane");
        assert_eq!(
            (plane.origin, plane.x_axis, plane.y_axis),
            ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
        );
    }

    #[test]
    fn a_gradient_on_a_rotated_plane_is_not_made_on_the_xy_plane() {
        let mut app = new_app();
        let i = app.active_tab;
        // The XZ plane at y = 5: local x along world X, local y along world Z.
        let mut ucs = codec::tables::Ucs::new("*ACTIVE*");
        ucs.origin = codec::types::Vector3::new(0.0, 5.0, 0.0);
        ucs.x_axis = codec::types::Vector3::new(1.0, 0.0, 0.0);
        ucs.y_axis = codec::types::Vector3::new(0.0, 0.0, 1.0);
        app.tabs[i].active_ucs = Some(ucs);
        open_with(&mut app, "GRADIENT", &[]);
        ok(&mut app);
        let hatch = the_hatch(&app);
        assert_eq!(FillKind::of(&hatch), FillKind::Gradient);
        assert!(hatch.normal.z.abs() < 1.0e-9, "{:?}", hatch.normal);
        assert!(hatch.normal.y.abs() > 0.99, "{:?}", hatch.normal);
        // The plane passes through y = 5, and the 20 x 10 area lies on it.
        assert!((hatch.elevation - 5.0 * hatch.normal.y).abs() < 1e-9, "{}", hatch.elevation);
        let points = world_boundary(&hatch);
        for point in &points {
            assert!((point.y - 5.0).abs() < 1e-9, "on the plane y = 5: {point:?}");
            assert!((-1e-9..=20.0 + 1e-9).contains(&point.x), "{point:?}");
            assert!((-1e-9..=10.0 + 1e-9).contains(&point.z), "{point:?}");
        }
        let top = points.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max);
        assert!((top - 10.0).abs() < 1e-9, "the area rises along world Z");
        // The scene model is drawn on the same plane.
        let plane = the_model(&app).fill_plane.expect("the model knows its plane");
        let normal = glam::DVec3::from_array(plane.x_axis)
            .cross(glam::DVec3::from_array(plane.y_axis))
            .normalize();
        assert!(normal.y.abs() > 0.99 && normal.z.abs() < 1e-9, "{normal:?}");
        assert!((plane.origin[1] - 5.0).abs() < 1e-9, "{:?}", plane.origin);
    }

    // ── Lifecycle of the Gradient window ──────────────────────────────────

    fn open_second_tab(app: &mut OpenCADStudio) -> (usize, usize) {
        let first = app.active_tab;
        let _ = app.push_test_document();
        let second = app.tabs.len() - 1;
        assert_ne!(first, second);
        (first, second)
    }

    #[test]
    fn the_style_and_the_preview_colour_are_read_from_the_given_tab() {
        let mut app = new_app();
        let (first, second) = open_second_tab(&mut app);
        let _ = app.update(Message::TabSwitch(first));
        assert_eq!(app.active_tab, first);
        // The second drawing: current colour ByLayer, a red layer 0 and its
        // own current transparency. The first (active) one: current colour
        // yellow. The ribbon shows yet another colour.
        app.tabs[second].scene.document.header.current_entity_color = Color::ByLayer;
        app.tabs[second].scene.document.layers.get_mut("0").unwrap().color = Color::Index(1);
        let quarter = codec::types::Transparency::Explicit(64);
        assert!(app.tabs[second].scene.document.set_current_entity_transparency(quarter));
        app.tabs[first].scene.document.header.current_entity_color = Color::Index(2);
        app.ribbon.active_color = Color::Index(6);

        assert_eq!(app.hatch_current_color(second), Color::ByLayer);
        assert_eq!(app.hatch_current_color(first), Color::Index(2));
        assert_eq!(app.hatch_preview_rgba(second, HatchColor::UseCurrent), [1.0, 0.0, 0.0, 0.75]);
        assert_eq!(app.hatch_preview_rgba(first, HatchColor::UseCurrent), [1.0, 1.0, 0.0, 0.75]);
        assert_eq!(
            app.hatch_creation_style(second, HatchColor::UseCurrent),
            (Color::ByLayer, quarter)
        );
        assert_eq!(
            app.hatch_creation_style(first, HatchColor::UseCurrent),
            (Color::Index(2), codec::types::Transparency::ByLayer)
        );
        assert_eq!(
            app.hatch_creation_style(first, HatchColor::Color(Color::Index(4))),
            (Color::Index(4), codec::types::Transparency::ByLayer)
        );
        // "Use Current" follows that drawing's current colour; a chosen one
        // wins over it.
        app.tabs[second].scene.document.header.current_entity_color = Color::Index(5);
        assert_eq!(app.hatch_creation_style(second, HatchColor::UseCurrent).0, Color::Index(5));
        assert_eq!(app.hatch_preview_rgba(second, HatchColor::UseCurrent), [0.0, 0.0, 1.0, 0.75]);
        assert_eq!(
            app.hatch_preview_rgba(second, HatchColor::Color(Color::Index(3))),
            [0.0, 1.0, 0.0, 0.75]
        );
    }

    #[test]
    fn leaving_or_closing_the_tab_during_a_gradient_preview_abandons_it() {
        for close in [false, true] {
            let mut app = app_with_rectangle();
            let (first, second) = open_second_tab(&mut app);
            let _ = app.update(Message::TabSwitch(first));
            open_with(&mut app, "GRADIENT", &[]);
            let _ = app.update(Message::HatchDialogPreview);
            assert!(app.tabs[first].active_cmd.is_some(), "close={close}: preview runs");
            let first_id = app.tabs[first].id;
            if close {
                let _ = app.update(Message::TabClose(first_id));
                assert!(app.tabs.iter().all(|tab| tab.id != first_id), "owner closed");
            } else {
                let _ = app.update(Message::TabSwitch(second));
                assert!(app.tabs[first].active_cmd.is_none(), "owner's preview stopped");
                // Back on the owner, a late OK finds nothing to apply.
                let _ = app.update(Message::TabSwitch(first));
                let _ = app.update(Message::HatchDialogOk);
            }
            assert!(app.hatch_dialog.is_none(), "close={close}: state dropped");
            assert!(app.active_modal.is_none(), "close={close}: no window");
            assert!(
                app.tabs.iter().all(|tab| tab.scene.hatches.is_empty()),
                "close={close}: nothing was created anywhere"
            );
        }
    }

    #[test]
    fn a_window_command_typed_while_the_window_is_hidden_starts_afresh() {
        for (first, then, tab) in [
            ("HATCH", "GRADIENT", FillTab::Gradient),
            ("GRADIENT", "HATCH", FillTab::Hatch),
        ] {
            let mut app = app_with_rectangle();
            open_with(&mut app, first, &[]);
            let _ = app.update(Message::HatchDialogAdd(AddKind::Points));
            let i = app.active_tab;
            assert!(app.tabs[i].active_cmd.is_some(), "{first}: the collector runs");
            let _ = app.dispatch_command(then);
            let state = app.hatch_dialog.as_ref().expect("a window again");
            assert_eq!(state.settings.tab, tab, "{first} then {then}");
            assert!(state.regions.is_empty(), "{first} then {then}: a fresh window");
            assert!(app.tabs[i].active_cmd.is_none(), "{first} then {then}: collector gone");
            assert_eq!(app.active_modal, Some(ModalKind::Hatch));
            assert!(all_hatches(&app).is_empty());
        }
    }

    #[test]
    fn enter_is_ok_and_escape_creates_nothing_on_the_gradient_tab() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "GRADIENT", &[]);
        let _ = app.update(Message::CommandEscape);
        assert!(app.hatch_dialog.is_none() && app.active_modal.is_none());
        assert!(all_hatches(&app).is_empty());

        open_with(&mut app, "GRADIENT", &[Field::GradientShape(5)]);
        let _ = app.update(Message::CommandFinalize);
        assert!(app.hatch_dialog.is_none());
        let hatch = the_hatch(&app);
        assert_eq!(FillKind::of(&hatch), FillKind::Gradient);
        assert_eq!(hatch.gradient_color.name, "CURVED");
    }

    // ── Preview ────────────────────────────────────────────────────────────

    fn preview_models(app: &mut OpenCADStudio) -> Vec<crate::scene::model::hatch_model::HatchModel> {
        let _ = app.update(Message::HatchDialogPreview);
        let i = app.active_tab;
        app.tabs[i]
            .active_cmd
            .as_ref()
            .expect("the preview runs")
            .hatch_preview_models()
            .expect("it has models")
    }

    #[test]
    fn the_preview_of_a_pattern_uses_the_current_colour_or_the_layers() {
        let mut app = app_with_rectangle();
        set_current_colour(&mut app, Color::Index(1), Color::Index(4));
        open_with(&mut app, "HATCH", &[]);
        let models = preview_models(&mut app);
        assert_eq!(models[0].color, [1.0, 0.0, 0.0, 0.75], "CECOLOR, not the ribbon");

        let mut app = app_with_rectangle();
        let i = app.active_tab;
        // A layer colour that is not the preview's old blue nor white.
        app.tabs[i].scene.document.layers.get_mut("0").unwrap().color = Color::Index(6);
        open_with(&mut app, "HATCH", &[]); // current colour ByLayer
        let mut layer = app.tabs[i].scene.layer_color(&app.tabs[i].active_layer);
        layer[3] = 0.75;
        assert_eq!(layer, [1.0, 0.0, 1.0, 0.75]);
        assert_eq!(preview_models(&mut app)[0].color, layer);
    }

    #[test]
    fn the_preview_uses_the_drawings_colour_not_the_preselected_objects() {
        for (boundary, current, expected) in [
            // Layer 0 is magenta below: ByLayer previews magenta.
            (Color::Index(1), Color::ByLayer, [1.0, 0.0, 1.0, 0.75]),
            (Color::ByLayer, Color::Index(3), [0.0, 1.0, 0.0, 0.75]),
        ] {
            let mut app = app_with_selected_rectangle(boundary);
            let i = app.active_tab;
            app.tabs[i].scene.document.layers.get_mut("0").unwrap().color = Color::Index(6);
            app.tabs[i].scene.document.header.current_entity_color = current;
            assert_eq!(app.ribbon.active_color, boundary, "the ribbon follows the selection");
            let _ = app.dispatch_command("HATCH");
            assert_eq!(app.hatch_dialog.as_ref().unwrap().regions.len(), 1);
            assert_eq!(preview_models(&mut app)[0].color, expected, "boundary {boundary:?}");
        }
    }

    #[test]
    fn the_preview_of_colour_7_is_adapted_to_the_background_like_the_hatch() {
        let white = [1.0, 1.0, 1.0, 0.75];
        let black = [0.0, 0.0, 0.0, 0.75];
        let dark = [0.1, 0.1, 0.1, 1.0];
        let light = [0.95, 0.95, 0.95, 1.0];
        for (case, layout, background, color, expected) in [
            ("model, dark", false, dark, HatchColor::UseCurrent, white),
            ("model, light", false, light, HatchColor::UseCurrent, black),
            ("model, light, chosen 7", false, light, HatchColor::Color(Color::Index(7)), black),
            ("model, light, red", false, light, HatchColor::Color(Color::Index(1)), [1.0, 0.0, 0.0, 0.75]),
            ("layout, white sheet", true, [1.0; 4], HatchColor::UseCurrent, black),
            ("layout, dark sheet", true, dark, HatchColor::UseCurrent, white),
        ] {
            let mut app = app_with_rectangle();
            let i = app.active_tab;
            // Current colour ByLayer on layer 0, colour 7: the default drawing.
            app.tabs[i].scene.document.layers.get_mut("0").unwrap().color = Color::Index(7);
            if layout {
                app.tabs[i].scene.current_layout = "Layout1".to_string();
                app.tabs[i].scene.paper_bg_color = background;
                app.tabs[i].scene.bg_color = dark; // must not be the one used
            } else {
                app.tabs[i].scene.bg_color = background;
                app.tabs[i].scene.paper_bg_color = [1.0; 4];
            }
            open_with(&mut app, "HATCH", &[Field::Color(color)]);
            assert_eq!(app.hatch_preview_rgba(i, color), expected, "{case}");
            assert_eq!(preview_models(&mut app)[0].color, expected, "{case}");
        }
    }

    #[test]
    fn the_gradient_preview_keeps_its_true_colours_on_a_light_background() {
        let mut app = app_with_rectangle();
        let i = app.active_tab;
        app.tabs[i].scene.bg_color = [0.95, 0.95, 0.95, 1.0];
        open_with(&mut app, "GRADIENT", &[Field::GradientColor1(Color::Index(7))]);
        assert_eq!(preview_models(&mut app)[0].color, [1.0, 1.0, 1.0, 0.75]);
    }

    #[test]
    fn the_preview_of_a_chosen_colour_is_that_colour() {
        let mut app = app_with_rectangle();
        open_with(&mut app, "HATCH", &[Field::Color(HatchColor::Color(Color::Index(3)))]);
        assert_eq!(preview_models(&mut app)[0].color, [0.0, 1.0, 0.0, 0.75]);
    }

    #[test]
    fn the_gradient_preview_shows_both_chosen_colours() {
        let mut app = app_with_rectangle();
        set_current_colour(&mut app, Color::Index(5), Color::Index(5));
        open_with(
            &mut app,
            "GRADIENT",
            &[Field::GradientColor1(Color::Index(1)), Field::GradientColor2(Color::Index(3))],
        );
        let models = preview_models(&mut app);
        assert_eq!(models[0].color, [1.0, 0.0, 0.0, 0.75], "colour 1, not the fill colour");
        match &models[0].pattern {
            HatchPattern::Gradient { color2, .. } => assert_eq!(*color2, [0.0, 1.0, 0.0, 0.75]),
            other => panic!("a gradient preview, got {other:?}"),
        }
    }

    #[test]
    fn the_gradient_preview_shows_the_tint_of_a_one_colour_gradient() {
        let mut app = app_with_rectangle();
        let red = Color::Index(1);
        open_with(
            &mut app,
            "GRADIENT",
            &[
                Field::GradientOneColor(true),
                Field::GradientColor1(red),
                Field::GradientTint(0.5),
                Field::GradientColor2(Color::Index(3)),
            ],
        );
        let models = preview_models(&mut app);
        let mut expected = rgba_of(tinted_second_color(rgba_of(red).unwrap(), 0.5)).unwrap();
        expected[3] = 0.75;
        let previewed = match &models[0].pattern {
            HatchPattern::Gradient { color2, .. } => *color2,
            other => panic!("a gradient preview, got {other:?}"),
        };
        assert_eq!(previewed, expected);
        // A different hidden Color 2 previews the same.
        let _ = app.update(Message::CommandEscape);
        let _ = app.update(Message::HatchDialogField(Field::GradientColor2(Color::Index(5))));
        match &preview_models(&mut app)[0].pattern {
            HatchPattern::Gradient { color2, .. } => assert_eq!(*color2, previewed),
            other => panic!("a gradient preview, got {other:?}"),
        }
        // And the hatch OK makes shows that same second colour.
        let _ = app.update(Message::CommandEscape);
        ok(&mut app);
        match &the_model(&app).pattern {
            HatchPattern::Gradient { color2, .. } => {
                assert_eq!(color2[..3], previewed[..3], "preview and hatch agree")
            }
            other => panic!("a gradient model, got {other:?}"),
        }
    }
}
