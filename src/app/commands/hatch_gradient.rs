//! The HATCH window's gradient side on the app: resolving "Use Current",
//! the colour picks, the keys with an overlay open. (Tests below also cover
//! the Properties panel's gradient colours.)

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
}
