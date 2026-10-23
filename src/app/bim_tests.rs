//! Comandi BIM dall'app (spec §3 e §5.1): `AL_DEMOLISCI`, `AL_NUOVO` e la
//! scelta dell'unità `AL_UNITA`, guidati come da uno script o dalla riga di
//! comando. Coprono ciò che i test di `bim::commands` non vedono: l'unità letta
//! dal disegno, i layer creati in un passo di annullamento a sé, l'istantanea
//! della geometria e il rilievo che resta intatto.
//!
//! Negli script lo spessore si scrive col punto («0.12»): la virgola farebbe
//! leggere al runner le coordinate (0;12).

use super::{Message, OpenCADStudio};
use crate::bim::fixtures::{case_a, Form, LAYER_MURI};
use crate::bim::record;
use crate::bim::units::DrawingUnit;
use crate::bim::zone::{missing_layers, zones_in, ZoneState};
use crate::bim::{
    ACI_GIALLO, ACI_ROSSO, APP_LOCALE, APP_ZONA, LAYER_DEMOLIZIONI, LAYER_LOCALI,
    LAYER_LOCALI_PROGETTO, LAYER_NUOVE,
};
use codec::entities::Line;
use codec::{CadDocument, Color, EntityType, Handle, Vector3};

fn fresh_app() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app
}

/// Le entità nello spazio modello del tab attivo, con `INSUNITS` = `insunits`.
fn load(app: &mut OpenCADStudio, entities: Vec<EntityType>, insunits: i16) {
    let i = app.active_tab;
    app.tabs[i].scene.document.header.insertion_units = insunits;
    for entity in entities {
        app.tabs[i].scene.add_entity(entity);
    }
}

/// Una riga intera, come da script: il comando, i suoi argomenti, poi Invio.
fn run(app: &mut OpenCADStudio, cmd: &str) {
    app.automation_op(&format!(r#"{{"op":"run","cmd":"{cmd}"}}"#));
    let i = app.active_tab;
    app.finish_pending_history(i);
}

/// Una risposta al comando in corso, senza Invio finale.
fn answer(app: &mut OpenCADStudio, text: &str) {
    let task = app.feed_active_cmd(text);
    app.drive_headless_task(task).expect("nessun dialogo");
}

fn undo(app: &mut OpenCADStudio) {
    let _ = app.update(Message::Undo);
}

fn doc(app: &OpenCADStudio) -> &CadDocument {
    &app.tabs[app.active_tab].scene.document
}

fn history(app: &OpenCADStudio) -> String {
    app.command_line.history_plain_text()
}

fn active_command(app: &OpenCADStudio) -> Option<&'static str> {
    app.tabs[app.active_tab]
        .active_cmd
        .as_ref()
        .map(|c| c.name())
}

fn undo_steps(app: &OpenCADStudio) -> usize {
    app.tabs[app.active_tab].history.undo_stack.len()
}

/// Le entità del rilievo (layer `MURI`) per intero: `EntityType` confronta
/// handle, layer, colore, tipo e spessore di linea, geometria e il resto.
fn rilievo_snapshot(app: &OpenCADStudio) -> Vec<(Handle, EntityType)> {
    let mut entities: Vec<(Handle, EntityType)> = doc(app)
        .entities()
        .filter(|entity| entity.common().layer == LAYER_MURI)
        .map(|entity| (entity.common().handle, entity.clone()))
        .collect();
    entities.sort_by_key(|(handle, _)| handle.value());
    entities
}

fn layer_color(app: &OpenCADStudio, name: &str) -> Option<Color> {
    doc(app).layers.get(name).map(|layer| layer.color.clone())
}

#[test]
fn demolish_tramezzo_from_command_line() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 6);
    let before = rilievo_snapshot(&app);
    assert_eq!(before.len(), 12, "il caso A a linee continue ha 12 LINE");

    run(&mut app, "AL_DEMOLISCI 4.05,0 4.05,3 N");

    let zones = zones_in(doc(&app));
    assert_eq!(zones.len(), 1, "storia: {}", history(&app));
    assert_eq!(zones[0].zone.state, ZoneState::Demolito);
    assert!(!zones[0].zone.varco);
    assert!((zones[0].zone.thickness.unwrap() - 0.10).abs() < 1e-9);
    let hatch = doc(&app).get_entity(zones[0].handle).unwrap();
    assert_eq!(hatch.common().layer, LAYER_DEMOLIZIONI);
    assert_eq!(hatch.common().color, Color::from_index(ACI_GIALLO));
    assert_eq!(rilievo_snapshot(&app), before, "il rilievo non si tocca");
    assert_eq!(doc(&app).header.insertion_units, 6);
    assert_eq!(active_command(&app), None);

    // Un solo annullamento rimuove la zona; i layer sono un passo a sé.
    undo(&mut app);
    assert!(zones_in(doc(&app)).is_empty());
    assert!(
        !missing_layers(doc(&app)),
        "i layer restano dopo il primo annullamento"
    );
    undo(&mut app);
    assert!(
        missing_layers(doc(&app)),
        "il secondo annullamento toglie i layer"
    );
    assert_eq!(rilievo_snapshot(&app), before);
}

#[test]
fn unit_is_asked_and_insunits_untouched() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Centimetri), 0);

    run(&mut app, "AL_DEMOLISCI");
    assert_eq!(active_command(&app), Some("AL_UNITA"));
    assert!(
        history(&app).contains(
            "Unità del disegno non specificata (INSUNITS = 0). [Millimetri/Centimetri/Metri]:"
        ),
        "storia: {}",
        history(&app)
    );
    assert_eq!(record::read(doc(&app)).unit, None);
    assert!(
        missing_layers(doc(&app)),
        "nessun layer prima di sapere l'unità"
    );

    answer(&mut app, "CM");
    assert_eq!(record::read(doc(&app)).unit, Some(DrawingUnit::Centimetri));
    assert_eq!(
        doc(&app).header.insertion_units,
        0,
        "INSUNITS non si scrive mai"
    );
    assert_eq!(
        active_command(&app),
        Some("AL_DEMOLISCI"),
        "AL_DEMOLISCI riparte"
    );
    assert_eq!(app.bim_pending, None);

    // La zona si misura in centimetri: il tramezzo è spesso 10.
    for token in ["405,0", "405,300", "N"] {
        answer(&mut app, token);
    }
    let zones = zones_in(doc(&app));
    assert_eq!(zones.len(), 1, "storia: {}", history(&app));
    assert!((zones[0].zone.thickness.unwrap() - 10.0).abs() < 1e-6);
    assert_eq!(doc(&app).header.insertion_units, 0);

    // Zona, layer e unità: tre passi di annullamento.
    undo(&mut app);
    undo(&mut app);
    assert_eq!(record::read(doc(&app)).unit, Some(DrawingUnit::Centimetri));
    undo(&mut app);
    assert_eq!(
        record::read(doc(&app)).unit,
        None,
        "AL_UNITA è un passo di annullamento"
    );
    assert_eq!(doc(&app).header.insertion_units, 0);
}

/// Uno script risponde all'unità nella stessa riga, e il comando prosegue.
#[test]
fn script_answers_the_unit_inline() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Millimetri), 0);

    run(&mut app, "AL_DEMOLISCI MM 4050,0 4050,3000 N");

    let zones = zones_in(doc(&app));
    assert_eq!(zones.len(), 1, "storia: {}", history(&app));
    assert!((zones[0].zone.thickness.unwrap() - 100.0).abs() < 1e-6);
    assert_eq!(record::read(doc(&app)).unit, Some(DrawingUnit::Millimetri));
    assert_eq!(doc(&app).header.insertion_units, 0);
}

#[test]
fn known_unit_from_the_record_is_not_asked_again() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Centimetri), 0);
    run(&mut app, "AL_DEMOLISCI");
    answer(&mut app, "CM");
    let _ = app.update(Message::CommandEscape);

    run(&mut app, "AL_NUOVO");
    assert_eq!(active_command(&app), Some("AL_NUOVO"));
}

#[test]
fn inches_are_refused() {
    for command in ["AL_DEMOLISCI 4.05,0 4.05,3 N", "AL_NUOVO 2,0 2,3 0.12"] {
        let mut app = fresh_app();
        load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 1);
        let before = rilievo_snapshot(&app);

        run(&mut app, command);

        assert!(
            history(&app).contains(
                "Unità del disegno non supportata (INSUNITS = 1): usa millimetri, centimetri o metri."
            ),
            "{command}: {}",
            history(&app)
        );
        assert!(zones_in(doc(&app)).is_empty(), "{command}: nessuna zona");
        assert!(missing_layers(doc(&app)), "{command}: nessun layer");
        assert_eq!(record::read(doc(&app)).unit, None);
        assert_eq!(doc(&app).header.insertion_units, 1);
        assert_eq!(active_command(&app), None);
        assert_eq!(rilievo_snapshot(&app), before);
    }
}

#[test]
fn layers_created_once_with_colors() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 6);

    run(&mut app, "AL_NUOVO 2,0 2,3 0.12");
    assert_eq!(
        layer_color(&app, LAYER_DEMOLIZIONI),
        Some(Color::from_index(ACI_GIALLO))
    );
    assert_eq!(
        layer_color(&app, LAYER_NUOVE),
        Some(Color::from_index(ACI_ROSSO))
    );
    assert!(layer_color(&app, LAYER_LOCALI).is_some());
    assert!(layer_color(&app, LAYER_LOCALI_PROGETTO).is_some());
    assert!(doc(&app).app_ids.contains(APP_ZONA) && doc(&app).app_ids.contains(APP_LOCALE));
    let zones = zones_in(doc(&app));
    assert_eq!(zones.len(), 1, "storia: {}", history(&app));
    assert_eq!(zones[0].zone.state, ZoneState::Nuovo);
    let hatch = doc(&app).get_entity(zones[0].handle).unwrap();
    assert_eq!(hatch.common().layer, LAYER_NUOVE);
    assert_eq!(hatch.common().color, Color::from_index(ACI_ROSSO));

    // Il secondo comando trova i layer: un solo passo in più, la zona.
    let steps = undo_steps(&app);
    run(&mut app, "AL_NUOVO 6,0 6,3 0.12");
    assert_eq!(zones_in(doc(&app)).len(), 2, "storia: {}", history(&app));
    assert_eq!(undo_steps(&app), steps + 1);
}

#[test]
fn new_wall_proposes_the_last_thickness() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 6);
    run(&mut app, "AL_NUOVO 2,0 2,3 0.12");
    run(&mut app, "AL_NUOVO 6,0 6,3 0.2");

    // Asse e Invio: lo spessore è quello dell'ultima zona nuova.
    run(&mut app, "AL_NUOVO 1,0 1,3");
    assert!(
        history(&app).contains("Spessore del muro <0,2"),
        "storia: {}",
        history(&app)
    );
    let zones = zones_in(doc(&app));
    assert_eq!(zones.len(), 3, "storia: {}", history(&app));
    let last = zones.iter().max_by_key(|z| z.handle.value()).unwrap();
    assert!((last.zone.thickness.unwrap() - 0.2).abs() < 1e-9);
}

#[test]
fn off_plane_walls_are_reported() {
    let mut app = fresh_app();
    let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
    let mut raised = Line::from_points(Vector3::new(0.0, 5.0, 1.0), Vector3::new(4.0, 5.0, 1.0));
    raised.common.layer = LAYER_MURI.to_string();
    entities.push(EntityType::Line(raised));
    load(&mut app, entities, 6);

    run(&mut app, "AL_DEMOLISCI");
    assert!(
        history(&app).contains("1 entità fuori dal piano XY ignorate."),
        "storia: {}",
        history(&app)
    );
}

/// R21: in un layout senza finestra attiva (spazio carta) i comandi si
/// rifiutano e non creano nulla: né domanda dell'unità, né record, né layer,
/// né passi di annullamento.
#[test]
fn paper_space_is_refused() {
    for (command, insunits) in [
        ("AL_DEMOLISCI 4.05,0 4.05,3 N", 6),
        ("AL_NUOVO 2,0 2,3 0.12", 6),
        ("AL_DEMOLISCI", 0),
    ] {
        let mut app = fresh_app();
        load(
            &mut app,
            case_a(Form::Continua, DrawingUnit::Metri),
            insunits,
        );
        let i = app.active_tab;
        app.tabs[i].scene.set_current_layout("Layout1".to_string());
        assert!(app.tabs[i].scene.active_viewport.is_none(), "spazio carta");
        let before = rilievo_snapshot(&app);
        let steps = undo_steps(&app);
        let name = command.split(' ').next().unwrap();

        run(&mut app, command);

        assert!(
            history(&app).contains(&format!(
                "{name}: i comandi di ristrutturazione lavorano solo nello spazio modello."
            )),
            "{command}: {}",
            history(&app)
        );
        assert_eq!(active_command(&app), None, "{command}: nessuna domanda");
        assert!(
            !doc(&app)
                .entities()
                .any(|e| matches!(e, EntityType::Hatch(_))),
            "{command}: nessuna campitura, in nessuno spazio"
        );
        assert!(missing_layers(doc(&app)), "{command}: nessun layer");
        assert_eq!(
            record::read(doc(&app)).unit,
            None,
            "{command}: nessun record"
        );
        assert_eq!(undo_steps(&app), steps, "{command}: nessun passo");
        assert_eq!(app.bim_pending, None);
        assert_eq!(rilievo_snapshot(&app), before);
    }
}
