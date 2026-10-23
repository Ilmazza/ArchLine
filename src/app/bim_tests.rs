//! Comandi BIM dall'app (spec §3 e §5.1): `AL_DEMOLISCI`, `AL_NUOVO`, la
//! scelta dell'unità `AL_UNITA` e `AL_STATO`, guidati come da uno script o dalla
//! riga di comando. Coprono ciò che i test di `bim::commands` non vedono: l'unità letta
//! dal disegno, i layer creati in un passo di annullamento a sé, l'istantanea
//! della geometria e il rilievo che resta intatto.
//!
//! Negli script lo spessore si scrive col punto («0.12»): la virgola farebbe
//! leggere al runner le coordinate (0;12).

use super::{Message, OpenCADStudio};
use crate::bim::fixtures::{case_a, Form, LAYER_MURI};
use crate::bim::record;
use crate::bim::units::DrawingUnit;
use crate::bim::zone::{missing_layers, read_zone, zones_in, Zone, ZoneState};
use crate::bim::{
    ACI_GIALLO, ACI_ROSSO, APP_LOCALE, APP_ZONA, LAYER_DEMOLIZIONI, LAYER_LOCALI,
    LAYER_LOCALI_PROGETTO, LAYER_NUOVE,
};
use crate::command::StepInput;
use codec::entities::Line;
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{CadDocument, Color, EntityType, Handle, Vector3};
use std::io::Cursor;

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

// ---------------------------------------------------------------- AL_STATO

/// Seleziona solo `handles`, come farebbe una selezione fatta a mano.
fn select(app: &mut OpenCADStudio, handles: &[Handle]) {
    let i = app.active_tab;
    app.tabs[i].scene.deselect_all();
    for &handle in handles {
        app.tabs[i].scene.select_entity(handle, false);
    }
}

/// La riga di comando riparte vuota: i messaggi letti dopo sono solo quelli del
/// comando appena dato (la storia tiene 64 righe e butta le prime).
fn forget_history(app: &mut OpenCADStudio) {
    app.command_line.history.clear();
}

fn zone_of(app: &OpenCADStudio, handle: Handle) -> Zone {
    let entity = doc(app).get_entity(handle).expect("la zona c'è");
    read_zone(handle, entity).expect("è una zona").zone
}

fn entity_of(app: &OpenCADStudio, handle: Handle) -> EntityType {
    doc(app).get_entity(handle).expect("l'entità c'è").clone()
}

/// Una riga data in un colpo solo a `dispatch_command`, senza passare dal
/// runner degli script: qui `3,20` arriva intero a `dispatch_bim`.
fn dispatch_line(app: &mut OpenCADStudio, line: &str) {
    let task = app.dispatch_command(line);
    app.drive_headless_task(task).expect("nessun dialogo");
    let i = app.active_tab;
    app.finish_pending_history(i);
}

/// Un passo del comando in corso (selezione, Invio) con l'esito pilotato.
fn feed(app: &mut OpenCADStudio, input: StepInput) {
    let task = app.feed_command(input);
    app.drive_headless_task(task).expect("nessun dialogo");
    let i = app.active_tab;
    app.finish_pending_history(i);
}

/// Il caso A in metri con un tramezzo demolito (spessore 0.10) e un muro nuovo
/// (0.12): `(app, demolita, nuova)`. Niente è selezionato.
fn app_with_two_zones() -> (OpenCADStudio, Handle, Handle) {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 6);
    run(&mut app, "AL_DEMOLISCI 4.05,0 4.05,3 N");
    run(&mut app, "AL_NUOVO 6,0 6,3 0.12");
    let (demolita, nuova) = {
        let zones = zones_in(doc(&app));
        assert_eq!(zones.len(), 2, "storia: {}", history(&app));
        let find = |state| zones.iter().find(|z| z.zone.state == state).unwrap().handle;
        (find(ZoneState::Demolito), find(ZoneState::Nuovo))
    };
    select(&mut app, &[]);
    (app, demolita, nuova)
}

#[test]
fn stato_asks_for_objects_then_the_keyword() {
    let (mut app, demolita, _) = app_with_two_zones();

    // Con una selezione chiede subito la parola chiave.
    select(&mut app, &[demolita]);
    forget_history(&mut app);
    run(&mut app, "AL_STATO");
    assert_eq!(active_command(&app), Some("AL_STATO"));
    assert!(
        history(&app).contains("AL_STATO  [Demolito/Nuovo/Varco/Altezza/Spessore]:"),
        "storia: {}",
        history(&app)
    );
    let _ = app.update(Message::CommandEscape);
    assert_eq!(active_command(&app), None);

    // Senza selezione raccoglie gli oggetti; poi la parola chiave vale per quelli.
    select(&mut app, &[]);
    forget_history(&mut app);
    run(&mut app, "AL_STATO");
    assert_eq!(active_command(&app), Some("AL_STATO"));
    assert!(
        history(&app).contains("AL_STATO  select objects, then press Enter:"),
        "storia: {}",
        history(&app)
    );
    select(&mut app, &[demolita]);
    feed(&mut app, StepInput::SelectionComplete(vec![demolita]));
    feed(&mut app, StepInput::Enter);
    answer(&mut app, "N");
    let i = app.active_tab;
    app.finish_pending_history(i);
    assert_eq!(active_command(&app), None);
    assert_eq!(zone_of(&app, demolita).state, ZoneState::Nuovo);
}

#[test]
fn stato_switches_to_nuovo() {
    let (mut app, demolita, _) = app_with_two_zones();
    let rilievo = rilievo_snapshot(&app);
    let before_entity = entity_of(&app, demolita);
    let before = zone_of(&app, demolita);
    let rings = zones_in(doc(&app))
        .into_iter()
        .find(|z| z.handle == demolita)
        .unwrap()
        .rings;
    assert_eq!(before.state, ZoneState::Demolito);
    select(&mut app, &[demolita]);
    let steps = undo_steps(&app);
    forget_history(&mut app);

    run(&mut app, "AL_STATO N");

    assert_eq!(active_command(&app), None);
    let hatch = doc(&app).get_entity(demolita).unwrap();
    assert_eq!(hatch.common().layer, LAYER_NUOVE);
    assert_eq!(hatch.common().color, Color::from_index(ACI_ROSSO));
    let after = zone_of(&app, demolita);
    assert_eq!(after.state, ZoneState::Nuovo);
    assert!(!after.varco);
    // Il resto del record (origine, asse, spessore) resta, e la geometria non si muove.
    assert_eq!(
        after,
        Zone {
            state: ZoneState::Nuovo,
            ..before.clone()
        }
    );
    let rings_after = zones_in(doc(&app))
        .into_iter()
        .find(|z| z.handle == demolita)
        .unwrap()
        .rings;
    assert_eq!(rings_after, rings);
    assert_eq!(undo_steps(&app), steps + 1, "un solo passo");
    assert!(
        history(&app).contains("AL_STATO: 1 zone modificate."),
        "storia: {}",
        history(&app)
    );
    assert_eq!(rilievo_snapshot(&app), rilievo, "il rilievo non si tocca");

    undo(&mut app);
    assert_eq!(
        entity_of(&app, demolita),
        before_entity,
        "annullato per intero"
    );

    // E di nuovo a demolito.
    select(&mut app, &[demolita]);
    run(&mut app, "AL_STATO N");
    run(&mut app, "AL_STATO D");
    let hatch = doc(&app).get_entity(demolita).unwrap();
    assert_eq!(hatch.common().layer, LAYER_DEMOLIZIONI);
    assert_eq!(hatch.common().color, Color::from_index(ACI_GIALLO));
    assert_eq!(zone_of(&app, demolita), before);
}

#[test]
fn stato_varco_only_on_demolished() {
    let (mut app, demolita, nuova) = app_with_two_zones();

    select(&mut app, &[demolita]);
    run(&mut app, "AL_STATO V");
    assert!(zone_of(&app, demolita).varco, "storia: {}", history(&app));
    run(&mut app, "AL_STATO V");
    assert!(!zone_of(&app, demolita).varco, "V inverte");

    // Su una zona nuova: messaggio e nessun cambio, nemmeno un passo di annullamento.
    select(&mut app, &[nuova]);
    let before = entity_of(&app, nuova);
    let steps = undo_steps(&app);
    forget_history(&mut app);
    run(&mut app, "AL_STATO V");
    assert!(
        history(&app).contains("Il varco vale solo per le zone demolite."),
        "storia: {}",
        history(&app)
    );
    assert!(history(&app).contains("AL_STATO: 0 zone modificate."));
    assert!(!zone_of(&app, nuova).varco);
    assert_eq!(entity_of(&app, nuova), before);
    assert_eq!(undo_steps(&app), steps);

    // Mista: la demolita cambia, la nuova no, il messaggio c'è.
    select(&mut app, &[demolita, nuova]);
    forget_history(&mut app);
    run(&mut app, "AL_STATO V");
    assert!(zone_of(&app, demolita).varco);
    assert!(!zone_of(&app, nuova).varco);
    assert!(history(&app).contains("Il varco vale solo per le zone demolite."));
    assert!(history(&app).contains("AL_STATO: 1 zone modificate."));
}

#[test]
fn stato_nuovo_is_never_a_varco() {
    let (mut app, demolita, _) = app_with_two_zones();
    select(&mut app, &[demolita]);
    run(&mut app, "AL_STATO V");
    assert!(zone_of(&app, demolita).varco);

    run(&mut app, "AL_STATO N");
    let nuova = zone_of(&app, demolita);
    assert_eq!(nuova.state, ZoneState::Nuovo);
    assert!(!nuova.varco, "una zona nuova non è un varco");

    // Tornando demolita non riprende il varco di prima.
    run(&mut app, "AL_STATO D");
    let demolita_again = zone_of(&app, demolita);
    assert_eq!(demolita_again.state, ZoneState::Demolito);
    assert!(!demolita_again.varco);
}

#[test]
fn stato_altezza_value() {
    let (mut app, demolita, _) = app_with_two_zones();
    select(&mut app, &[demolita]);
    assert_eq!(zone_of(&app, demolita).height, None);

    // Da script, col punto: il runner passa «3.20» al prompt dell'altezza.
    run(&mut app, "AL_STATO A 3.20");
    assert_eq!(
        zone_of(&app, demolita).height,
        Some(3.20),
        "storia: {}",
        history(&app)
    );

    // Riga intera data a `dispatch_command`: la virgola decimale arriva
    // intera a `dispatch_bim`, che la legge da sé.
    dispatch_line(&mut app, "AL_STATO A 2,70");
    assert_eq!(zone_of(&app, demolita).height, Some(2.70));

    // Dal prompt, come lo scrive Mauro nella riga di comando: «3,20» va al
    // comando prima che il runner lo prenda per un punto.
    run(&mut app, "AL_STATO");
    answer(&mut app, "A");
    assert!(
        history(&app).contains("Altezza:"),
        "storia: {}",
        history(&app)
    );
    let steps = undo_steps(&app);
    let _ = app.update(Message::CommandInput("3,20".to_string()));
    let task = app.update(Message::CommandSubmit);
    app.drive_headless_task(task).expect("nessun dialogo");
    let i = app.active_tab;
    app.finish_pending_history(i);
    assert_eq!(
        zone_of(&app, demolita).height,
        Some(3.20),
        "storia: {}",
        history(&app)
    );
    assert_eq!(undo_steps(&app), steps + 1);
    assert_eq!(active_command(&app), None);
}

#[test]
fn stato_spessore_value() {
    let (mut app, _, nuova) = app_with_two_zones();
    select(&mut app, &[nuova]);
    assert_eq!(zone_of(&app, nuova).thickness, Some(0.12));

    run(&mut app, "AL_STATO S 0.30");
    assert_eq!(zone_of(&app, nuova).thickness, Some(0.30));
    dispatch_line(&mut app, "AL_STATO S 0,25");
    assert_eq!(zone_of(&app, nuova).thickness, Some(0.25));
    // Il resto non si tocca.
    assert_eq!(zone_of(&app, nuova).state, ZoneState::Nuovo);
    assert_eq!(zone_of(&app, nuova).height, None);
}

#[test]
fn stato_bad_values_change_nothing() {
    let (mut app, demolita, _) = app_with_two_zones();
    select(&mut app, &[demolita]);
    let before = entity_of(&app, demolita);
    let steps = undo_steps(&app);

    for line in [
        "AL_STATO A 0",
        "AL_STATO A -2",
        "AL_STATO S 0,0",
        "AL_STATO S abc",
        "AL_STATO A 1.234,50",
        "AL_STATO A",
        "AL_STATO S",
        "AL_STATO X",
    ] {
        forget_history(&mut app);
        let revision = app.command_line.error_revision;
        dispatch_line(&mut app, line);
        assert_ne!(
            app.command_line.error_revision,
            revision,
            "{line}: un messaggio d'errore, storia: {}",
            history(&app)
        );
        assert!(
            app.command_line
                .last_error
                .as_deref()
                .is_some_and(|e| e.starts_with("AL_STATO:")),
            "{line}: {:?}",
            app.command_line.last_error
        );
        assert_eq!(entity_of(&app, demolita), before, "{line}: nessun cambio");
        assert_eq!(undo_steps(&app), steps, "{line}: nessun passo");
    }

    // Anche dal prompt: il valore sbagliato non cambia nulla.
    forget_history(&mut app);
    run(&mut app, "AL_STATO A abc");
    assert_eq!(entity_of(&app, demolita), before);
    assert_eq!(undo_steps(&app), steps);
    assert!(
        history(&app).contains("AL_STATO: valore non valido"),
        "storia: {}",
        history(&app)
    );
}

#[test]
fn stato_ignores_non_zones() {
    let (mut app, demolita, _) = app_with_two_zones();
    let rilievo = rilievo_snapshot(&app);
    let (line, _) = rilievo[0].clone();
    select(&mut app, &[line, demolita]);
    forget_history(&mut app);

    run(&mut app, "AL_STATO N");

    assert!(
        history(&app).contains("1 oggetti non sono zone e sono stati ignorati."),
        "storia: {}",
        history(&app)
    );
    assert!(history(&app).contains("AL_STATO: 1 zone modificate."));
    assert_eq!(zone_of(&app, demolita).state, ZoneState::Nuovo);
    assert_eq!(rilievo_snapshot(&app), rilievo, "la LINE resta com'era");

    // Solo rilievo selezionato: messaggi, nessun passo di annullamento.
    select(&mut app, &[line]);
    let steps = undo_steps(&app);
    forget_history(&mut app);
    run(&mut app, "AL_STATO D");
    assert!(history(&app).contains("1 oggetti non sono zone e sono stati ignorati."));
    assert!(history(&app).contains("AL_STATO: 0 zone modificate."));
    assert_eq!(undo_steps(&app), steps);
    assert_eq!(rilievo_snapshot(&app), rilievo);
    assert_eq!(
        doc(&app)
            .entities()
            .filter(|e| matches!(e, EntityType::Line(_)))
            .count(),
        12
    );
}

#[test]
fn stato_without_selection_says_so() {
    let (mut app, demolita, _) = app_with_two_zones();
    let before = entity_of(&app, demolita);
    let steps = undo_steps(&app);
    forget_history(&mut app);

    dispatch_line(&mut app, "AL_STATO N");

    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("AL_STATO: nessun oggetto selezionato.")
    );
    assert_eq!(entity_of(&app, demolita), before);
    assert_eq!(undo_steps(&app), steps);
}

#[test]
fn stato_many_zones_is_one_undo_step() {
    let (mut app, demolita, nuova) = app_with_two_zones();
    select(&mut app, &[demolita, nuova]);
    let (before_d, before_n) = (entity_of(&app, demolita), entity_of(&app, nuova));
    let steps = undo_steps(&app);
    forget_history(&mut app);

    run(&mut app, "AL_STATO A 2.70");

    assert_eq!(zone_of(&app, demolita).height, Some(2.70));
    assert_eq!(zone_of(&app, nuova).height, Some(2.70));
    assert!(history(&app).contains("AL_STATO: 2 zone modificate."));
    assert_eq!(undo_steps(&app), steps + 1);
    undo(&mut app);
    assert_eq!(entity_of(&app, demolita), before_d);
    assert_eq!(entity_of(&app, nuova), before_n);
}

#[test]
fn stato_already_in_that_state_is_not_a_change() {
    let (mut app, demolita, nuova) = app_with_two_zones();
    select(&mut app, &[demolita]);
    let before = entity_of(&app, demolita);
    let steps = undo_steps(&app);
    forget_history(&mut app);

    run(&mut app, "AL_STATO D");
    assert!(history(&app).contains("AL_STATO: 0 zone modificate."));
    assert_eq!(entity_of(&app, demolita), before);
    assert_eq!(undo_steps(&app), steps, "nessun passo se nulla cambia");

    // Con una demolita e una nuova selezionate, solo la nuova cambia.
    select(&mut app, &[demolita, nuova]);
    forget_history(&mut app);
    run(&mut app, "AL_STATO D");
    assert!(history(&app).contains("AL_STATO: 1 zone modificate."));
    assert_eq!(zone_of(&app, nuova).state, ZoneState::Demolito);
    assert_eq!(undo_steps(&app), steps + 1);
}

#[test]
fn stato_keeps_other_applications_xdata() {
    let (mut app, demolita, _) = app_with_two_zones();
    let i = app.active_tab;
    let mut other = ExtendedDataRecord::new("ALTRA_APP");
    other.add_value(XDataValue::Integer16(7));
    app.tabs[i]
        .scene
        .document
        .get_entity_mut(demolita)
        .unwrap()
        .common_mut()
        .extended_data
        .add_record(other.clone());
    select(&mut app, &[demolita]);

    run(&mut app, "AL_STATO N");

    let entity = entity_of(&app, demolita);
    let data = &entity.common().extended_data;
    assert_eq!(data.get_record("ALTRA_APP"), Some(&other));
    assert_eq!(
        data.records()
            .iter()
            .filter(|r| r.application_name == APP_ZONA)
            .count(),
        1,
        "un solo record di zona"
    );
    assert_eq!(zone_of(&app, demolita).state, ZoneState::Nuovo);
}

#[test]
fn stato_skips_zones_on_locked_layers() {
    let (mut app, demolita, nuova) = app_with_two_zones();
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .document
        .layers
        .get_mut(LAYER_DEMOLIZIONI)
        .unwrap()
        .flags
        .locked = true;
    select(&mut app, &[demolita, nuova]);
    let before = entity_of(&app, demolita);
    forget_history(&mut app);

    run(&mut app, "AL_STATO A 2.70");

    assert_eq!(entity_of(&app, demolita), before, "layer bloccato");
    assert_eq!(zone_of(&app, nuova).height, Some(2.70));
    assert!(
        history(&app).contains("Zone su layer bloccati ignorate: 1."),
        "storia: {}",
        history(&app)
    );
    assert!(history(&app).contains("AL_STATO: 1 zone modificate."));
}

/// R21 anche per `AL_STATO`: fuori dallo spazio modello si rifiuta e non si cambia nulla.
#[test]
fn stato_is_refused_outside_model_space() {
    let (mut app, demolita, _) = app_with_two_zones();
    let i = app.active_tab;
    let before = entity_of(&app, demolita);
    let steps = undo_steps(&app);
    app.tabs[i].scene.set_current_layout("Layout1".to_string());
    assert!(app.tabs[i].scene.active_viewport.is_none(), "spazio carta");

    for command in ["AL_STATO N", "AL_STATO"] {
        select(&mut app, &[demolita]);
        forget_history(&mut app);
        run(&mut app, command);
        assert!(
            history(&app).contains(
                "AL_STATO: i comandi di ristrutturazione lavorano solo nello spazio modello."
            ),
            "{command}: {}",
            history(&app)
        );
        assert_eq!(active_command(&app), None, "{command}: nessuna domanda");
        assert_eq!(undo_steps(&app), steps, "{command}: nessun passo");
    }
    app.tabs[i].scene.set_current_layout("Model".to_string());
    assert_eq!(entity_of(&app, demolita), before);
}

/// Una zona letta da un DWG tiene il suo XDATA anche nel blocco grezzo
/// (`raw_dwg_eed`): cambiarla deve scartarlo, o un salvataggio e una riapertura
/// riporterebbero lo stato di prima.
#[test]
fn stato_survives_a_dwg_save_and_reload() {
    let (app, _, _) = app_with_two_zones();
    let bytes = codec::DwgWriter::write_to_vec(doc(&app)).unwrap();
    let loaded = codec::DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap();
    let zone_handle = zones_in(&loaded)
        .into_iter()
        .find(|z| z.zone.state == ZoneState::Demolito)
        .expect("la zona demolita nel DWG")
        .handle;
    assert!(
        !loaded
            .get_entity(zone_handle)
            .unwrap()
            .common()
            .extended_data
            .raw_dwg_eed
            .is_empty(),
        "la zona letta da DWG porta il blocco grezzo (altrimenti la prova non dice nulla)"
    );

    // Un'altra sessione apre quel DWG e cambia la zona.
    let mut app = fresh_app();
    let i = app.active_tab;
    app.tabs[i].scene.document = loaded;
    app.tabs[i].scene.rebuild_derived_caches();
    select(&mut app, &[zone_handle]);
    run(&mut app, "AL_STATO N");
    run(&mut app, "AL_STATO A 2.70");
    run(&mut app, "AL_STATO S 0.20");
    let now = zone_of(&app, zone_handle);
    assert_eq!(now.state, ZoneState::Nuovo, "storia: {}", history(&app));

    // Salva in DWG e riapre: la zona è nuova, con altezza e spessore nuovi.
    let bytes = codec::DwgWriter::write_to_vec(doc(&app)).unwrap();
    let reread = codec::DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap();
    let found = zones_in(&reread)
        .into_iter()
        .find(|z| z.handle == zone_handle)
        .expect("la zona dopo il salvataggio");
    assert_eq!(found.zone.state, ZoneState::Nuovo, "non torna demolita");
    assert_eq!(found.zone.height, Some(2.70));
    assert_eq!(found.zone.thickness, Some(0.20));
    assert!(!found.zone.varco);
    let hatch = reread.get_entity(zone_handle).unwrap();
    assert_eq!(hatch.common().layer, LAYER_NUOVE);
    assert_eq!(hatch.common().color, Color::from_index(ACI_ROSSO));
    assert!(!missing_layers(&reread));
}
