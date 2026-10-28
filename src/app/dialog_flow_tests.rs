//! Whole-dialog flows driven by the messages the interface sends, without a
//! window: the DATAEXTRACTION wizard, the Field dialog on a polyline, Print all
//! with one PDF per sheet. They cover what the unit tests of each piece cannot:
//! that the pieces are connected.

use super::{Message, OpenCADStudio};
use crate::ui::window::field_dialog::FieldDialogMsg;
use codec::entities::{Insert, LwPolyline};
use codec::types::{Vector2, Vector3};
use codec::EntityType;

fn fresh_app() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("archline-flow-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn send(app: &mut OpenCADStudio, message: Message) {
    let _ = app.update(message);
}

#[test]
fn the_field_dialog_follows_a_polyline_it_measures() {
    let mut app = fresh_app();
    let i = app.active_tab;
    let mut room = LwPolyline::from_points(vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(400.0, 0.0),
        Vector2::new(400.0, 300.0),
        Vector2::new(0.0, 300.0),
    ]);
    room.is_closed = true;
    let handle = app.tabs[i].scene.add_entity(EntityType::LwPolyline(room));

    let _ = app.run_command_line("FIELD");
    send(&mut app, Message::FieldDialog(FieldDialogMsg::Name("Object")));
    let _ = app.run_command_line(&format!("_FIELD_OBJECT {:X}", handle.value()));
    let state = app.field_dialog.as_ref().expect("the dialog is open");
    assert_eq!(state.object_prop, Some("Area"));
    // Insert, then click the text into place.
    send(&mut app, Message::FieldDialog(FieldDialogMsg::Ok));
    assert!(app.tabs[i].active_cmd.is_some(), "the placement command should be waiting for a point");
    send(&mut app, Message::CommandInput("10,10".to_string()));
    send(&mut app, Message::CommandSubmit);
    let text = app.tabs[i]
        .scene
        .document
        .entities()
        .find_map(|entity| match entity {
            EntityType::MText(text) => Some(text.common.handle),
            _ => None,
        })
        .expect("the field text was placed");
    let shown = |app: &OpenCADStudio| match app.tabs[i].scene.document.get_entity(text) {
        Some(EntityType::MText(text)) => text.value.clone(),
        other => panic!("not a text: {other:?}"),
    };
    assert_eq!(shown(&app), "120000.000000");

    // Widen the room by a metre; UPDATEFIELD brings the text along.
    if let Some(EntityType::LwPolyline(room)) = app.tabs[i].scene.document.get_entity_mut(handle) {
        room.vertices[1].location.x = 500.0;
        room.vertices[2].location.x = 500.0;
    }
    assert_eq!(shown(&app), "120000.000000", "the text keeps its value until a field update");
    crate::entities::field::update_fields(&mut app.tabs[i].scene.document, 32, None);
    assert_eq!(shown(&app), "150000.000000");
}

#[test]
fn print_all_writes_one_pdf_per_sheet_when_asked() {
    let dir = temp_dir("printall");
    let mut app = fresh_app();
    let i = app.active_tab;
    app.tabs[i].scene.document.add_layout("Piano terra").unwrap();
    app.tabs[i].scene.document.add_layout("Prospetto nord").unwrap();
    app.plot_dialog.background_publish = false;

    send(&mut app, Message::PrintAllOpen);
    let offered: Vec<String> = app.print_all_layouts.iter().map(|(name, _)| name.clone()).collect();
    assert!(offered.contains(&"Piano terra".to_string()), "{offered:?}");
    send(&mut app, Message::PrintAllPerSheet(true));
    assert!(app.print_all_per_sheet);
    send(&mut app, Message::PrintAllPdfPath(Some(dir.join("tavole.pdf"))));

    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert!(files.contains(&"tavole-Piano_terra.pdf".to_string()), "{files:?}");
    assert!(files.contains(&"tavole-Prospetto_nord.pdf".to_string()), "{files:?}");
    assert!(!files.contains(&"tavole.pdf".to_string()), "{files:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn print_all_without_the_option_writes_a_single_pdf() {
    let dir = temp_dir("printall-one");
    let mut app = fresh_app();
    let i = app.active_tab;
    app.tabs[i].scene.document.add_layout("Piano terra").unwrap();
    app.plot_dialog.background_publish = false;

    send(&mut app, Message::PrintAllOpen);
    send(&mut app, Message::PrintAllPdfPath(Some(dir.join("tavole.pdf"))));

    let files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files, ["tavole.pdf"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn attsync_gives_a_new_attribute_the_definitions_place_and_height() {
    use codec::entities::{AttributeDefinition, Line};
    use codec::types::Transform;
    let mut app = fresh_app();
    let i = app.active_tab;
    let scene = &mut app.tabs[i].scene;
    let mut definition = AttributeDefinition::new("NUM".into(), "Numero".into(), "1".into());
    definition.insertion_point = Vector3::new(1.0, 2.0, 0.0);
    definition.height = 3.0;
    let line = scene.add_entity(EntityType::Line(Line::from_points(Vector3::ZERO, Vector3::new(5.0, 0.0, 0.0))));
    let attdef = scene.add_entity(EntityType::AttributeDefinition(definition));
    let identity = Transform::identity();
    scene.create_block_from_entities(&[line, attdef], "PORTA", &identity, &identity).expect("the block");
    // A reference far from the origin, made before the attribute existed.
    scene.add_entity(EntityType::Insert(Insert::new("PORTA", Vector3::new(100.0, 50.0, 0.0))));

    let _ = app.run_command_line("_ATTSYNCBLOCK PORTA");

    let far = app.tabs[i]
        .scene
        .document
        .entities()
        .find_map(|entity| match entity {
            EntityType::Insert(insert) if insert.insert_point.x > 50.0 => Some(insert.clone()),
            _ => None,
        })
        .expect("the reference");
    let attribute = far.attributes.iter().find(|a| a.tag == "NUM").expect("the attribute was created");
    assert!(
        (attribute.insertion_point.x - 101.0).abs() < 1e-9 && (attribute.insertion_point.y - 52.0).abs() < 1e-9,
        "it sits at the definition's point on the reference: {:?}",
        attribute.insertion_point
    );
    assert!((attribute.height - 3.0).abs() < 1e-9, "height {}", attribute.height);
}

#[test]
fn the_missing_fonts_dialog_remembers_the_stand_in_chosen() {
    use crate::font_alt::{self, Substitute};
    let mut app = fresh_app();
    app.missing_fonts = Some(vec!["zz_flow_font.shx".into(), "zz_flow_other.shx".into()]);
    app.active_modal = Some(super::ModalKind::MissingFonts);

    send(
        &mut app,
        Message::MissingFontsPick("zz_flow_font.shx".into(), Substitute::Stroke("romand".into())),
    );
    send(&mut app, Message::MissingFontsApply);

    assert!(app.missing_fonts.is_none());
    assert!(app.active_modal.is_none());
    // The pick is kept, the font left on "Default" is marked as answered, and
    // both are in what gets written to settings.json.
    let saved = app.current_settings().font_substitutions;
    assert_eq!(saved["zz_flow_font"], "stroke:romand");
    assert_eq!(saved["zz_flow_other"], "default");
    assert_eq!(font_alt::snapshot()["zz_flow_font"], "stroke:romand");
    // Asked once: the same drawing opened again does not raise the dialog.
    let again = font_alt::review(vec!["zz_flow_font.shx".into(), "zz_flow_other.shx".into()]);
    assert!(again.ask.is_empty());
}
