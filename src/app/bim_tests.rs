//! Comandi BIM dall'app (spec §3, §4.2, §5.1 e §5.2): `AL_DEMOLISCI`,
//! `AL_NUOVO`, la scelta dell'unità `AL_UNITA`, `AL_STATO`, `AL_LOCALE` e
//! `AL_AGGIORNA`, guidati come da uno script o dalla riga di comando. Coprono
//! ciò che i test di `bim::commands` non vedono: l'unità letta dal disegno, i
//! layer creati in un passo di annullamento a sé, l'istantanea della geometria
//! e il rilievo che resta intatto.
//!
//! Negli script lo spessore si scrive col punto («0.12»): la virgola farebbe
//! leggere al runner le coordinate (0;12). Il numero di un locale vuoto è
//! l'Invio con cui il runner chiude la riga: `AL_LOCALE 2,1.5 A` dà il punto,
//! il nome e il numero vuoto.
//!
//! `AL_ESTRAI` scrive in cartelle nuove sotto la cartella temporanea, tolte a
//! fine test: mai file o impostazioni dell'utente.

use super::{Message, ModalKind, OpenCADStudio};
use crate::bim::extract::{file_name, Elaborato};
use crate::bim::fixtures::{case_a, Form, LAYER_MURI};
use crate::bim::record;
use crate::bim::report::Notice;
use crate::bim::rooms::{rooms_in, Phase, Room, RoomPart};
use crate::bim::units::DrawingUnit;
use crate::bim::zone::{missing_layers, read_zone, zones_in, Zone, ZoneState};
use crate::bim::{
    ACI_GIALLO, ACI_ROSSO, APP_LOCALE, APP_ZONA, LAYER_DEMOLIZIONI, LAYER_LOCALI,
    LAYER_LOCALI_PROGETTO, LAYER_NUOVE,
};
use crate::command::StepInput;
use crate::ui::window::bim_extract::{BimMsg, OVERWRITE_NOTE};
use codec::entities::{Line, LwPolyline, MText};
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{CadDocument, Color, EntityType, Handle, Vector3};
use std::io::Cursor;
use std::path::{Path, PathBuf};

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
/// La riga va nel JSON con le sue sequenze di escape (i percorsi di Windows
/// hanno le barre rovesciate).
fn run(app: &mut OpenCADStudio, cmd: &str) {
    app.automation_op(&serde_json::json!({ "op": "run", "cmd": cmd }).to_string());
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
        ("AL_LOCALE 2,1.5 A", 6),
        ("AL_LOCALE", 0),
        ("AL_AGGIORNA", 6),
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

/// Dopo un PURGE un disegno con sole zone demolite non ha più `AL-NUOVE` (e
/// viceversa): `AL_STATO` che sposta una zona su un layer assente lo ricrea nello
/// stesso passo di annullamento, e un solo annullamento riporta zona e tabella
/// dei layer come prima.
#[test]
fn stato_recreates_a_purged_target_layer_in_the_same_undo_step() {
    for (command, to_state, purged, aci) in [
        ("AL_STATO N", ZoneState::Nuovo, LAYER_NUOVE, ACI_ROSSO),
        (
            "AL_STATO D",
            ZoneState::Demolito,
            LAYER_DEMOLIZIONI,
            ACI_GIALLO,
        ),
    ] {
        let (mut app, demolita, nuova) = app_with_two_zones();
        // Resta una sola zona, dell'altro stato: il layer di arrivo è vuoto.
        let (handle, gone) = match to_state {
            ZoneState::Nuovo => (demolita, nuova),
            ZoneState::Demolito => (nuova, demolita),
        };
        let i = app.active_tab;
        app.tabs[i].scene.document.remove_entity(gone);
        let _ = app.tabs[i].scene.document.layers.remove(purged);
        assert!(
            doc(&app).layers.get(purged).is_none(),
            "{command}: layer tolto"
        );
        let before = entity_of(&app, handle);
        let was_missing = missing_layers(doc(&app));
        assert!(was_missing);
        select(&mut app, &[handle]);
        let steps = undo_steps(&app);

        run(&mut app, command);

        assert_eq!(
            zone_of(&app, handle).state,
            to_state,
            "{command}: storia: {}",
            history(&app)
        );
        assert_eq!(
            layer_color(&app, purged),
            Some(Color::from_index(aci)),
            "{command}: il layer c'è di nuovo, col suo colore"
        );
        assert_eq!(
            doc(&app).get_entity(handle).unwrap().common().layer,
            purged,
            "{command}: la zona sta sul layer"
        );
        assert!(
            !missing_layers(doc(&app)),
            "{command}: layer e APPID a posto"
        );
        assert_eq!(undo_steps(&app), steps + 1, "{command}: un solo passo");

        undo(&mut app);
        assert_eq!(entity_of(&app, handle), before, "{command}: zona com'era");
        assert!(
            doc(&app).layers.get(purged).is_none(),
            "{command}: lo stesso annullamento toglie il layer ricreato"
        );
        assert_eq!(missing_layers(doc(&app)), was_missing);
    }
}

/// Se nessuna zona cambia, il layer mancante non si ricrea e non nasce un passo.
#[test]
fn stato_without_a_change_does_not_create_layers() {
    let (mut app, demolita, _) = app_with_two_zones();
    let i = app.active_tab;
    let _ = app.tabs[i].scene.document.layers.remove(LAYER_NUOVE);
    select(&mut app, &[demolita]);
    let steps = undo_steps(&app);

    run(&mut app, "AL_STATO D");

    assert!(doc(&app).layers.get(LAYER_NUOVE).is_none());
    assert_eq!(undo_steps(&app), steps);
}

// --------------------------------------------------------------- AL_ESTRAI

/// Una cartella nuova sotto la cartella temporanea, tolta a fine test. Il
/// percorso non ha spazi: il runner degli script divide la riga sugli spazi.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "archline_bim_app_{tag}_{}_{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(
            !dir.to_string_lossy().contains(' '),
            "cartella temporanea con spazi: {}",
            dir.display()
        );
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// I tre DWG estratti per il disegno `base`, nell'ordine in cui si scrivono:
/// stato di fatto, demolizioni e costruzioni, stato di progetto.
fn extracted_paths(dir: &Path, base: &str) -> Vec<PathBuf> {
    [
        Elaborato::StatoDiFatto,
        Elaborato::DemolizioniCostruzioni,
        Elaborato::StatoDiProgetto,
    ]
    .into_iter()
    .map(|elaborato| dir.join(file_name(base, elaborato)))
    .collect()
}

/// Il nome del tab attivo: il nome di base dei file di un disegno mai salvato.
fn tab_title(app: &OpenCADStudio) -> String {
    app.tabs[app.active_tab].tab_title.clone()
}

fn dwg_count(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dwg"))
        })
        .count()
}

const MARKER: &[u8] = b"segno: da non sovrascrivere";

/// Sostituisce il contenuto dei file con un segno: un file riscritto non lo ha
/// più, anche se la data di modifica non cambiasse.
fn mark(paths: &[PathBuf]) {
    for path in paths {
        std::fs::write(path, MARKER).unwrap();
    }
}

fn marked(path: &Path) -> bool {
    std::fs::read(path).unwrap() == MARKER
}

fn mtimes(paths: &[PathBuf]) -> Vec<std::time::SystemTime> {
    paths
        .iter()
        .map(|path| std::fs::metadata(path).unwrap().modified().unwrap())
        .collect()
}

/// Un messaggio dell'interfaccia, come da un clic nella finestra.
fn send(app: &mut OpenCADStudio, message: Message) {
    let _ = app.update(message);
}

fn bim(app: &mut OpenCADStudio, message: BimMsg) {
    send(app, Message::Bim(message));
}

fn pick_folder(app: &mut OpenCADStudio, dir: &Path) {
    bim(app, BimMsg::Picked(Some(dir.to_path_buf())));
}

#[test]
fn scripted_extract_writes_three_files() {
    let tmp = TempDir::new("scritti");
    let (mut app, _, _) = app_with_two_zones();
    let before = rilievo_snapshot(&app);
    let zones = zones_in(doc(&app)).len();
    let steps = undo_steps(&app);
    forget_history(&mut app);

    run(&mut app, &format!("AL_ESTRAI {}", tmp.path().display()));

    let history = history(&app);
    for path in extracted_paths(tmp.path(), &tab_title(&app)) {
        assert!(path.is_file(), "{}: storia: {history}", path.display());
        crate::io::load_file(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
    assert_eq!(dwg_count(tmp.path()), 3, "storia: {history}");
    assert!(
        history.contains(&format!(
            "AL_ESTRAI: scritti 3 file in {}.",
            tmp.path().display()
        )),
        "storia: {history}"
    );
    // Il layer proposto (il più frequente dei muri) non si ricorda da script.
    assert_eq!(record::read(doc(&app)).new_walls_layer, None);
    assert!(!history.contains("esistono già"), "un giro solo: {history}");
    // Da script nessuna finestra e nessun comando che resta aperto.
    assert_eq!(app.active_modal, None);
    assert_eq!(active_command(&app), None);
    // Estrarre non tocca il disegno.
    assert_eq!(rilievo_snapshot(&app), before);
    assert_eq!(zones_in(doc(&app)).len(), zones);
    assert_eq!(undo_steps(&app), steps);
}

#[test]
fn scripted_extract_never_overwrites() {
    let tmp = TempDir::new("rifiuto");
    let (mut app, _, _) = app_with_two_zones();
    let line = format!("AL_ESTRAI {}", tmp.path().display());
    run(&mut app, &line);
    let paths = extracted_paths(tmp.path(), &tab_title(&app));
    assert!(
        paths.iter().all(|path| path.is_file()),
        "storia: {}",
        history(&app)
    );
    mark(&paths);
    let before = mtimes(&paths);
    forget_history(&mut app);

    run(&mut app, &line);

    let history = history(&app);
    assert!(
        history.contains("AL_ESTRAI: i file esistono già: "),
        "storia: {history}"
    );
    assert!(
        history.contains("Niente è stato scritto."),
        "storia: {history}"
    );
    assert!(!history.contains("AL_ESTRAI: scritti"), "storia: {history}");
    assert_eq!(mtimes(&paths), before, "date di modifica invariate");
    assert!(paths.iter().all(|path| marked(path)), "file non riscritti");
}

#[test]
fn dialog_flow_overwrite_needs_second_ok() {
    let tmp = TempDir::new("finestra");
    let (mut app, _, _) = app_with_two_zones();
    let paths = extracted_paths(tmp.path(), &tab_title(&app));

    // Senza script AL_ESTRAI apre la finestra, con i suoi valori iniziali.
    dispatch_line(&mut app, "AL_ESTRAI");
    assert_eq!(app.active_modal, Some(ModalKind::BimExtract));
    {
        let dialog = app.bim_extract.as_ref().expect("finestra aperta");
        assert_eq!(dialog.dir, None, "disegno mai salvato: nessuna cartella");
        assert!(dialog.fatto && dialog.comparativa && dialog.progetto);
        assert_eq!(
            dialog.layer, LAYER_MURI,
            "nessun layer nel record: il più frequente dei muri"
        );
        assert!(
            dialog.layers.iter().any(|layer| layer == LAYER_MURI),
            "{:?}",
            dialog.layers
        );
        assert!(
            !dialog.layers.iter().any(|layer| layer.starts_with("AL-")),
            "{:?}",
            dialog.layers
        );
        assert!(!dialog.confirm_overwrite);
    }
    pick_folder(&mut app, tmp.path());
    bim(&mut app, BimMsg::Ok);
    assert!(
        paths.iter().all(|path| path.is_file()),
        "storia: {}",
        history(&app)
    );
    assert_eq!(app.active_modal, None, "OK chiude la finestra");
    assert!(app.bim_extract.is_none());
    mark(&paths);

    // Nuova apertura, stessa cartella: il primo OK chiede conferma e non scrive.
    dispatch_line(&mut app, "AL_ESTRAI");
    pick_folder(&mut app, tmp.path());
    forget_history(&mut app);
    bim(&mut app, BimMsg::Ok);
    assert_eq!(
        app.active_modal,
        Some(ModalKind::BimExtract),
        "resta aperta"
    );
    let dialog = app.bim_extract.as_ref().expect("finestra aperta");
    assert!(dialog.confirm_overwrite);
    assert_eq!(dialog.note(), Some(OVERWRITE_NOTE));
    assert!(paths.iter().all(|path| marked(path)), "niente scritto");
    assert!(
        !history(&app).contains("esistono già"),
        "l'avviso sta nella finestra: {}",
        history(&app)
    );

    // Il secondo OK sovrascrive e chiude.
    bim(&mut app, BimMsg::Ok);
    assert!(
        paths.iter().all(|path| !marked(path)),
        "riscritti; storia: {}",
        history(&app)
    );
    assert_eq!(app.active_modal, None);
    assert!(app.bim_extract.is_none());
}

/// Le caselle scelgono i file; cambiarle dopo l'avviso chiede di nuovo conferma.
#[test]
fn dialog_toggles_choose_the_files() {
    let tmp = TempDir::new("caselle");
    let (mut app, _, _) = app_with_two_zones();
    let paths = extracted_paths(tmp.path(), &tab_title(&app));
    dispatch_line(&mut app, "AL_ESTRAI");
    pick_folder(&mut app, tmp.path());
    for elaborato in [
        Elaborato::StatoDiFatto,
        Elaborato::DemolizioniCostruzioni,
        Elaborato::StatoDiProgetto,
    ] {
        bim(&mut app, BimMsg::Toggle(elaborato));
    }
    // Nessuna casella: OK non fa nulla.
    bim(&mut app, BimMsg::Ok);
    assert_eq!(app.active_modal, Some(ModalKind::BimExtract));
    assert_eq!(dwg_count(tmp.path()), 0);

    bim(&mut app, BimMsg::Toggle(Elaborato::DemolizioniCostruzioni));
    bim(&mut app, BimMsg::Ok);
    assert!(!paths[0].exists(), "stato di fatto non chiesto");
    assert!(paths[1].is_file(), "storia: {}", history(&app));
    assert_eq!(app.active_modal, None);

    // Tutte e tre: il file della comparativa c'è già, si chiede conferma; una
    // casella cambiata dopo l'avviso la ritira.
    dispatch_line(&mut app, "AL_ESTRAI");
    pick_folder(&mut app, tmp.path());
    bim(&mut app, BimMsg::Ok);
    assert!(app.bim_extract.as_ref().unwrap().confirm_overwrite);
    assert!(!paths[0].exists(), "niente scritto prima della conferma");
    bim(&mut app, BimMsg::Toggle(Elaborato::StatoDiProgetto));
    assert!(!app.bim_extract.as_ref().unwrap().confirm_overwrite);
    assert_eq!(app.bim_extract.as_ref().unwrap().note(), None);
}

#[test]
fn dialog_remembers_the_new_walls_layer() {
    let tmp = TempDir::new("layer");
    let (mut app, _, _) = app_with_two_zones();
    let steps = undo_steps(&app);

    dispatch_line(&mut app, "AL_ESTRAI");
    pick_folder(&mut app, tmp.path());
    bim(&mut app, BimMsg::Layer(LAYER_MURI.to_string()));
    bim(&mut app, BimMsg::Ok);
    assert!(app.bim_extract.is_none(), "storia: {}", history(&app));
    assert_eq!(
        record::read(doc(&app)).new_walls_layer.as_deref(),
        Some(LAYER_MURI)
    );
    assert_eq!(undo_steps(&app), steps + 1, "un passo di annullamento a sé");

    // Riaperta, la finestra lo propone; lo stesso layer non apre un altro passo.
    let other = TempDir::new("layer-di-nuovo");
    dispatch_line(&mut app, "AL_ESTRAI");
    assert_eq!(app.bim_extract.as_ref().unwrap().layer, LAYER_MURI);
    pick_folder(&mut app, other.path());
    bim(&mut app, BimMsg::Ok);
    assert!(app.bim_extract.is_none(), "storia: {}", history(&app));
    assert_eq!(dwg_count(other.path()), dwg_count(tmp.path()));
    assert_eq!(undo_steps(&app), steps + 1);

    undo(&mut app);
    assert_eq!(record::read(doc(&app)).new_walls_layer, None);
}

#[test]
fn dialog_cancel_writes_nothing() {
    let tmp = TempDir::new("annulla");
    let (mut app, _, _) = app_with_two_zones();
    let steps = undo_steps(&app);

    dispatch_line(&mut app, "AL_ESTRAI");
    pick_folder(&mut app, tmp.path());
    bim(&mut app, BimMsg::Layer(LAYER_MURI.to_string()));
    bim(&mut app, BimMsg::Cancel);
    assert_eq!(app.active_modal, None);
    assert!(app.bim_extract.is_none());

    // La X e Esc passano da CloseModal: stessa pulizia.
    dispatch_line(&mut app, "AL_ESTRAI");
    assert_eq!(app.active_modal, Some(ModalKind::BimExtract));
    send(&mut app, Message::CloseModal);
    assert_eq!(app.active_modal, None);
    assert!(app.bim_extract.is_none());

    // A finestra chiusa una cartella scelta in ritardo e OK non fanno nulla.
    pick_folder(&mut app, tmp.path());
    bim(&mut app, BimMsg::Ok);
    assert!(app.bim_extract.is_none());
    assert_eq!(dwg_count(tmp.path()), 0);
    assert_eq!(record::read(doc(&app)).new_walls_layer, None);
    assert_eq!(undo_steps(&app), steps);
}

/// Un disegno salvato: la finestra parte dalla sua cartella e i file prendono
/// il suo nome.
#[test]
fn dialog_starts_in_the_drawing_folder() {
    let tmp = TempDir::new("cartella");
    let (mut app, _, _) = app_with_two_zones();
    let i = app.active_tab;
    app.tabs[i].current_path = Some(tmp.path().join("casa rilievo.dwg"));

    dispatch_line(&mut app, "AL_ESTRAI");
    assert_eq!(
        app.bim_extract.as_ref().unwrap().dir.as_deref(),
        Some(tmp.path())
    );
    bim(&mut app, BimMsg::Ok);

    for path in extracted_paths(tmp.path(), "casa rilievo") {
        assert!(
            path.is_file(),
            "{}: storia: {}",
            path.display(),
            history(&app)
        );
    }
}

/// Una linea nello spazio carta del layout: l'estrazione non la porta e lo
/// dice con un avviso `NonCopiata` («Layout <nome> <handle> non copiata…»).
/// Restituisce la parte finale, fissa, di quella riga.
fn add_paper_space_line(app: &mut OpenCADStudio) -> String {
    let i = app.active_tab;
    let doc = &mut app.tabs[i].scene.document;
    let paper = doc.header.paper_space_block_handle;
    assert!(!paper.is_null(), "il disegno ha uno spazio carta");
    let mut note = Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
    note.common.owner_handle = paper;
    doc.add_entity(EntityType::Line(note)).unwrap();
    format!("{paper:X} non copiata nel DWG estratto.")
}

/// `then` compare nella storia dopo `first`.
fn comes_after(history: &str, first: &str, then: &str) -> bool {
    matches!((history.find(first), history.find(then)), (Some(a), Some(b)) if a < b)
}

/// Da riga di comando, dopo «scritti n file» una riga per ogni avviso.
#[test]
fn scripted_extract_reports_the_notices() {
    let tmp = TempDir::new("avvisi");
    let (mut app, _, _) = app_with_two_zones();
    let notice = add_paper_space_line(&mut app);
    forget_history(&mut app);

    run(&mut app, &format!("AL_ESTRAI {}", tmp.path().display()));

    let history = history(&app);
    assert!(
        comes_after(&history, "AL_ESTRAI: scritti ", &notice),
        "storia: {history}"
    );
    assert!(history.contains("Layout "), "storia: {history}");
}

/// Dalla finestra lo stesso rapporto, con i suoi avvisi.
#[test]
fn dialog_extract_reports_the_notices() {
    let tmp = TempDir::new("avvisi-finestra");
    let (mut app, _, _) = app_with_two_zones();
    let notice = add_paper_space_line(&mut app);
    dispatch_line(&mut app, "AL_ESTRAI");
    pick_folder(&mut app, tmp.path());
    forget_history(&mut app);

    bim(&mut app, BimMsg::Ok);

    assert!(app.bim_extract.is_none());
    let history = history(&app);
    assert!(
        comes_after(&history, "AL_ESTRAI: scritti ", &notice),
        "storia: {history}"
    );
}

/// Un file che non si può scrivere (al suo posto c'è una cartella). Se si
/// ferma dopo il primo file, la riga di comando dice quale file e quali sono
/// già stati scritti, e gli avvisi seguono: valgono per i file rimasti. Se si
/// ferma al primo, niente è stato scritto e non ci sono avvisi da dare.
#[test]
fn dialog_reports_a_write_error() {
    let tmp = TempDir::new("errore");
    let (mut app, _, _) = app_with_two_zones();
    let notice = add_paper_space_line(&mut app);
    let paths = extracted_paths(tmp.path(), &tab_title(&app));
    std::fs::create_dir(&paths[1]).unwrap();

    dispatch_line(&mut app, "AL_ESTRAI");
    pick_folder(&mut app, tmp.path());
    bim(&mut app, BimMsg::Ok);
    assert!(app.bim_extract.as_ref().unwrap().confirm_overwrite);
    assert!(!paths[0].exists());
    forget_history(&mut app);
    bim(&mut app, BimMsg::Ok);

    assert!(app.bim_extract.is_none());
    assert_eq!(app.active_modal, None);
    assert!(paths[0].is_file());
    let partial = history(&app);
    let error = format!(
        "AL_ESTRAI: scrittura non riuscita: {}: ",
        paths[1].display()
    );
    assert!(comes_after(&partial, &error, &notice), "storia: {partial}");
    let written = paths[0].file_name().unwrap().to_string_lossy();
    assert!(
        partial.contains(&format!("(già scritti prima dell'errore: {written}).")),
        "storia: {partial}"
    );
    assert!(!partial.contains("AL_ESTRAI: scritti"), "storia: {partial}");

    // Si ferma al primo file: niente scritto, nessun avviso.
    std::fs::remove_file(&paths[0]).unwrap();
    std::fs::create_dir(&paths[0]).unwrap();
    std::fs::remove_dir(&paths[1]).unwrap();
    dispatch_line(&mut app, "AL_ESTRAI");
    pick_folder(&mut app, tmp.path());
    bim(&mut app, BimMsg::Ok);
    assert!(app.bim_extract.as_ref().unwrap().confirm_overwrite);
    forget_history(&mut app);
    bim(&mut app, BimMsg::Ok);

    assert!(app.bim_extract.is_none());
    assert!(!paths[1].exists(), "il secondo file non si scrive");
    let refused = history(&app);
    assert!(
        refused.contains(&format!(
            "AL_ESTRAI: scrittura non riuscita: {}: ",
            paths[0].display()
        )),
        "storia: {refused}"
    );
    assert!(
        refused.contains("Niente è stato scritto."),
        "storia: {refused}"
    );
    assert!(!refused.contains(&notice), "storia: {refused}");
}

#[test]
fn extract_refuses_a_missing_folder() {
    let tmp = TempDir::new("manca");
    let missing = tmp.path().join("non-esiste");
    let (mut app, _, _) = app_with_two_zones();
    forget_history(&mut app);

    run(&mut app, &format!("AL_ESTRAI {}", missing.display()));

    let history = history(&app);
    assert!(
        history.contains(&format!(
            "AL_ESTRAI: la cartella {} non esiste.",
            missing.display()
        )),
        "storia: {history}"
    );
    assert!(!missing.exists(), "nessuna cartella creata");
    assert_eq!(dwg_count(tmp.path()), 0);
}

/// Da script, `AL_ESTRAI` senza cartella la chiede in riga di comando; un Invio
/// a vuoto non scrive nulla.
#[test]
fn script_prompts_for_the_folder() {
    let (mut app, _, _) = app_with_two_zones();
    app.scripted_dispatch = true;
    forget_history(&mut app);

    dispatch_line(&mut app, "AL_ESTRAI");
    assert_eq!(active_command(&app), Some("AL_ESTRAI"));
    assert_eq!(app.active_modal, None);
    assert!(
        history(&app).contains("Cartella di destinazione:"),
        "storia: {}",
        history(&app)
    );

    feed(&mut app, StepInput::Enter);
    app.scripted_dispatch = false;
    assert_eq!(active_command(&app), None);
    assert!(
        history(&app).contains("AL_ESTRAI: nessuna cartella indicata."),
        "storia: {}",
        history(&app)
    );
}

/// In un file .scr la riga dopo `AL_ESTRAI` è la cartella intera, spazi
/// compresi.
#[test]
fn script_file_line_is_the_whole_folder() {
    let tmp = TempDir::new("scr");
    let dir = tmp.path().join("Casa Rossi");
    std::fs::create_dir(&dir).unwrap();
    let (mut app, _, _) = app_with_two_zones();

    for line in ["AL_ESTRAI".to_string(), dir.display().to_string()] {
        let task = app.feed_scripted_line(&line);
        app.drive_headless_task(task).expect("nessun dialogo");
    }

    for path in extracted_paths(&dir, &tab_title(&app)) {
        assert!(
            path.is_file(),
            "{}: storia: {}",
            path.display(),
            history(&app)
        );
    }
    assert_eq!(active_command(&app), None);
}

/// Senza unità (INSUNITS = 0) la si chiede prima della finestra.
#[test]
fn extract_asks_the_unit_before_the_dialog() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Centimetri), 0);

    dispatch_line(&mut app, "AL_ESTRAI");
    assert_eq!(active_command(&app), Some("AL_UNITA"));
    assert_eq!(app.active_modal, None);

    answer(&mut app, "CM");
    assert_eq!(record::read(doc(&app)).unit, Some(DrawingUnit::Centimetri));
    assert_eq!(app.active_modal, Some(ModalKind::BimExtract));
}

/// Uno script risponde all'unità e poi dà la cartella, nella stessa riga.
#[test]
fn script_answers_the_unit_then_the_folder() {
    let tmp = TempDir::new("unita");
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Millimetri), 0);

    run(&mut app, &format!("AL_ESTRAI MM {}", tmp.path().display()));

    assert_eq!(record::read(doc(&app)).unit, Some(DrawingUnit::Millimetri));
    for path in extracted_paths(tmp.path(), &tab_title(&app)) {
        assert!(
            path.is_file(),
            "{}: storia: {}",
            path.display(),
            history(&app)
        );
    }
}

#[test]
fn extract_refuses_inches() {
    let tmp = TempDir::new("pollici");
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 1);

    run(&mut app, &format!("AL_ESTRAI {}", tmp.path().display()));
    dispatch_line(&mut app, "AL_ESTRAI");

    assert!(
        history(&app).contains("Unità del disegno non supportata (INSUNITS = 1)"),
        "storia: {}",
        history(&app)
    );
    assert_eq!(app.active_modal, None, "nessuna finestra");
    assert_eq!(dwg_count(tmp.path()), 0);
}

// ------------------------------------------------- AL_ESTRAI, stato di progetto

/// Il record con il layer dei muri nuovi `layer`, scritto senza passare dai
/// comandi (come in un disegno riaperto).
fn remember_layer(app: &mut OpenCADStudio, layer: &str) {
    let i = app.active_tab;
    let doc = &mut app.tabs[i].scene.document;
    let mut rec = record::read(doc);
    rec.new_walls_layer = Some(layer.to_string());
    record::write(doc, &rec);
}

/// Le `LINE` del DWG `path` sul layer `layer` (senza badare alle maiuscole).
fn lines_on_layer(path: &Path, layer: &str) -> usize {
    crate::io::load_file(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .entities()
        .filter(|e| {
            matches!(e, EntityType::Line(_)) && e.common().layer.eq_ignore_ascii_case(layer)
        })
        .count()
}

/// Una polilinea chiusa per muro, tramezzo demolito: `AL_ESTRAI` scrive anche
/// lo stato di progetto e la riga di comando elenca le due giunzioni da
/// verificare. Il rilievo, il record e la storia degli annullamenti non cambiano.
#[test]
fn scripted_extract_writes_the_project_and_lists_the_junctions() {
    let tmp = TempDir::new("progetto");
    let mut app = fresh_app();
    load(&mut app, case_a(Form::ChiusaPerMuro, DrawingUnit::Metri), 6);
    run(&mut app, "AL_DEMOLISCI 4.05,0 4.05,3 N");
    assert_eq!(zones_in(doc(&app)).len(), 1, "storia: {}", history(&app));
    let before = rilievo_snapshot(&app);
    assert_eq!(before.len(), 5, "cinque polilinee chiuse");
    let steps = undo_steps(&app);
    let rec = record::read(doc(&app));
    forget_history(&mut app);

    run(&mut app, &format!("AL_ESTRAI {}", tmp.path().display()));

    let history = history(&app);
    let paths = extracted_paths(tmp.path(), &tab_title(&app));
    for path in &paths {
        assert!(path.is_file(), "{}: storia: {history}", path.display());
    }
    for at in [[4.05, 0.0], [4.05, 3.0]] {
        let junction = Notice::GiunzioneDaVerificare { at }.text();
        assert!(
            comes_after(&history, "AL_ESTRAI: scritti 3 file", &junction),
            "manca «{junction}»; storia: {history}"
        );
    }
    let progetto = crate::io::load_file(&paths[2]).unwrap();
    assert!(
        !progetto
            .entities()
            .any(|e| matches!(e, EntityType::Hatch(_))),
        "zone nel progetto"
    );
    // Sud, nord, ovest, est intere e le due teste rimaste del tramezzo.
    let polylines = progetto
        .entities()
        .filter(|e| matches!(e, EntityType::LwPolyline(_)))
        .count();
    assert_eq!(polylines, 6);
    assert_eq!(rilievo_snapshot(&app), before, "il rilievo non si tocca");
    assert_eq!(undo_steps(&app), steps);
    assert_eq!(record::read(doc(&app)), rec);
    assert_eq!(zones_in(doc(&app)).len(), 1);
}

/// La finestra propone il layer del record; senza record, il più frequente dei
/// muri; senza muri, nessuno.
#[test]
fn dialog_proposes_the_record_layer_then_the_walls_layer() {
    let (mut app, _, _) = app_with_two_zones();
    dispatch_line(&mut app, "AL_ESTRAI");
    assert_eq!(app.bim_extract.as_ref().unwrap().layer, LAYER_MURI);
    bim(&mut app, BimMsg::Cancel);

    remember_layer(&mut app, "TRAMEZZI");
    dispatch_line(&mut app, "AL_ESTRAI");
    assert_eq!(app.bim_extract.as_ref().unwrap().layer, "TRAMEZZI");
    bim(&mut app, BimMsg::Cancel);

    let mut empty = fresh_app();
    load(&mut empty, Vec::new(), 6);
    dispatch_line(&mut empty, "AL_ESTRAI");
    assert_eq!(empty.bim_extract.as_ref().unwrap().layer, "");
}

/// Da script, senza layer nel record e senza muri, i muri nuovi vanno sul
/// layer 0 e la riga di comando lo dice; nulla si ricorda nel disegno.
#[test]
fn script_without_walls_uses_layer_zero_and_says_so() {
    let tmp = TempDir::new("layer-zero");
    let mut app = fresh_app();
    load(&mut app, Vec::new(), 6);
    let steps = undo_steps(&app);
    forget_history(&mut app);

    run(&mut app, &format!("AL_ESTRAI {}", tmp.path().display()));

    let history = history(&app);
    assert!(
        history.contains("i muri nuovi vanno sul layer 0"),
        "storia: {history}"
    );
    assert_eq!(dwg_count(tmp.path()), 3, "storia: {history}");
    assert_eq!(record::read(doc(&app)).new_walls_layer, None);
    assert_eq!(undo_steps(&app), steps);
}

/// Da script vale il layer ricordato nel record, anche se nel disegno non c'è:
/// le linee del muro nuovo stanno lì nel DWG di progetto.
#[test]
fn script_uses_the_remembered_layer() {
    let tmp = TempDir::new("layer-record");
    let (mut app, _, _) = app_with_two_zones();
    remember_layer(&mut app, "MURI-NUOVI");
    let steps = undo_steps(&app);

    run(&mut app, &format!("AL_ESTRAI {}", tmp.path().display()));

    let paths = extracted_paths(tmp.path(), &tab_title(&app));
    assert!(paths[2].is_file(), "storia: {}", history(&app));
    // Il muro nuovo da 0,12 nel locale B: lati lunghi e teste sulle facce del
    // locale (le teste coincidono con i muri esistenti: giunzioni).
    assert_eq!(lines_on_layer(&paths[2], "MURI-NUOVI"), 2);
    assert_eq!(undo_steps(&app), steps);
    assert_eq!(
        record::read(doc(&app)).new_walls_layer.as_deref(),
        Some("MURI-NUOVI")
    );
}

/// Un layer scritto a mano che nel disegno non c'è si accetta: lo si crea nel
/// DWG di progetto con colore e tipo di linea del layer dei muri (R46), la riga
/// di comando lo dice e il record lo ricorda.
#[test]
fn dialog_accepts_a_new_layer_for_the_new_walls() {
    let tmp = TempDir::new("layer-scritto");
    let (mut app, _, _) = app_with_two_zones();
    {
        let i = app.active_tab;
        let muri = app.tabs[i]
            .scene
            .document
            .layers
            .get_mut(LAYER_MURI)
            .expect("layer dei muri nella tabella");
        muri.color = Color::from_index(3);
    }
    dispatch_line(&mut app, "AL_ESTRAI");
    pick_folder(&mut app, tmp.path());
    bim(&mut app, BimMsg::Layer("  MURI-NUOVI ".to_string()));
    forget_history(&mut app);
    bim(&mut app, BimMsg::Ok);

    assert!(app.bim_extract.is_none(), "storia: {}", history(&app));
    let history = history(&app);
    assert!(
        history.contains(
            "il layer MURI-NUOVI dei muri nuovi non c'è nel disegno: creato nello stato di \
             progetto con colore, tipo di linea e spessore del layer MURI."
        ),
        "storia: {history}"
    );
    let paths = extracted_paths(tmp.path(), &tab_title(&app));
    assert_eq!(lines_on_layer(&paths[2], "MURI-NUOVI"), 2);
    let progetto = crate::io::load_file(&paths[2]).unwrap();
    let layer = progetto.layers.get("MURI-NUOVI").expect("layer nel DWG");
    assert_eq!(layer.color, Color::from_index(3));
    assert_eq!(
        record::read(doc(&app)).new_walls_layer.as_deref(),
        Some("MURI-NUOVI")
    );
    assert!(
        doc(&app).layers.get("MURI-NUOVI").is_none(),
        "non nel disegno"
    );
}

/// Con lo stato di progetto spuntato serve un layer valido per i muri nuovi:
/// vuoto, o con un carattere che AutoCAD non accetta, niente si scrive e la
/// finestra resta aperta. Senza progetto il layer non serve e non si ricorda.
#[test]
fn dialog_refuses_a_missing_or_invalid_layer_for_the_project() {
    let tmp = TempDir::new("layer-vuoto");
    let (mut app, _, _) = app_with_two_zones();
    let steps = undo_steps(&app);
    dispatch_line(&mut app, "AL_ESTRAI");
    pick_folder(&mut app, tmp.path());
    for (typed, error) in [
        ("   ", "AL_ESTRAI: indica il layer dei muri nuovi"),
        (
            "MURI/NUOVI",
            "AL_ESTRAI: «MURI/NUOVI» non è un nome di layer valido",
        ),
    ] {
        bim(&mut app, BimMsg::Layer(typed.to_string()));
        forget_history(&mut app);
        bim(&mut app, BimMsg::Ok);

        assert_eq!(
            app.active_modal,
            Some(ModalKind::BimExtract),
            "resta aperta"
        );
        assert!(
            history(&app).contains(error),
            "{typed:?}: storia: {}",
            history(&app)
        );
        assert!(
            history(&app).contains("Niente è stato scritto."),
            "storia: {}",
            history(&app)
        );
        assert_eq!(dwg_count(tmp.path()), 0);
    }

    bim(&mut app, BimMsg::Toggle(Elaborato::StatoDiProgetto));
    bim(&mut app, BimMsg::Ok);
    assert!(app.bim_extract.is_none(), "storia: {}", history(&app));
    assert_eq!(dwg_count(tmp.path()), 2);
    assert_eq!(record::read(doc(&app)).new_walls_layer, None);
    assert_eq!(undo_steps(&app), steps);
}

// ------------------------------------------------------- AL_LOCALE e AL_AGGIORNA

/// Le entità sul layer `layer`, con il loro handle, in ordine di handle.
fn on_layer(app: &OpenCADStudio, layer: &str) -> Vec<(Handle, EntityType)> {
    let mut entities: Vec<(Handle, EntityType)> = doc(app)
        .entities()
        .filter(|entity| entity.common().layer == layer)
        .map(|entity| (entity.common().handle, entity.clone()))
        .collect();
    entities.sort_by_key(|(handle, _)| handle.value());
    entities
}

/// Il contorno e il testo dell'unico locale di fatto (`AL-LOCALI`).
fn room_parts(app: &OpenCADStudio) -> ((Handle, EntityType), (Handle, EntityType)) {
    let parts = on_layer(app, LAYER_LOCALI);
    assert_eq!(parts.len(), 2, "contorno e testo: {parts:?}");
    let find = |is: fn(&EntityType) -> bool| {
        parts
            .iter()
            .find(|(_, entity)| is(entity))
            .cloned()
            .expect("parte del locale")
    };
    (
        find(|e| matches!(e, EntityType::LwPolyline(_))),
        find(|e| matches!(e, EntityType::MText(_))),
    )
}

fn polyline(entity: &EntityType) -> &LwPolyline {
    let EntityType::LwPolyline(polyline) = entity else {
        panic!("non è una LWPOLYLINE: {entity:?}")
    };
    polyline
}

fn mtext(entity: &EntityType) -> &MText {
    let EntityType::MText(text) = entity else {
        panic!("non è un MTEXT: {entity:?}")
    };
    text
}

/// L'area racchiusa dai vertici (lati dritti).
fn polyline_area(polyline: &LwPolyline) -> f64 {
    let v = &polyline.vertices;
    let twice: f64 = (0..v.len())
        .map(|k| {
            let (a, b) = (v[k].location, v[(k + 1) % v.len()].location);
            a.x * b.y - b.x * a.y
        })
        .sum();
    twice.abs() / 2.0
}

fn near(p: Vector3, q: [f64; 2]) -> bool {
    (p.x - q[0]).abs() < 1e-9 && (p.y - q[1]).abs() < 1e-9
}

/// L'handle della `LINE` del rilievo da `a` a `b`.
fn survey_line(app: &OpenCADStudio, a: [f64; 2], b: [f64; 2]) -> Handle {
    doc(app)
        .entities()
        .find(|e| matches!(e, EntityType::Line(l) if near(l.start, a) && near(l.end, b)))
        .map(|e| e.common().handle)
        .expect("linea del rilievo")
}

/// Il tramezzo del caso A (linee): la faccia di A passa da x = 4,00 a 3,90,
/// fuori dai comandi (come una modifica a mano del rilievo).
fn move_tramezzo_face(app: &mut OpenCADStudio) {
    let face = survey_line(app, [4.0, 0.0], [4.0, 3.0]);
    let i = app.active_tab;
    let Some(EntityType::Line(line)) = app.tabs[i].scene.document.get_entity_mut(face) else {
        panic!("faccia del tramezzo")
    };
    line.start.x = 3.9;
    line.end.x = 3.9;
}

#[test]
fn locale_fatto_writes_outline_and_label() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 6);
    let before = rilievo_snapshot(&app);
    let steps = undo_steps(&app);

    run(&mut app, "AL_LOCALE 2,1.5 A");

    let history = history(&app);
    assert!(
        comes_after(
            &history,
            "AL_LOCALE (Fatto)  Punto interno del locale o [Fase]:",
            "Superficie netta 12,00 m². Nome del locale:"
        ),
        "storia: {history}"
    );
    assert!(
        comes_after(&history, "Superficie netta 12,00 m²", "Numero <nessuno>:"),
        "storia: {history}"
    );
    assert_eq!(active_command(&app), None);
    let ((_, outline), (_, text)) = room_parts(&app);
    let outline = polyline(&outline);
    assert!(outline.is_closed);
    assert_eq!(outline.vertices.len(), 4);
    assert!((polyline_area(outline) - 12.0).abs() < 1e-9);
    let text = mtext(&text);
    assert_eq!(text.value, "A\\PS = 12,00 m²");
    assert_eq!(text.height, doc(&app).header.text_height);
    assert!(near(text.insertion_point, [2.0, 1.5]));
    let room = Room {
        phase: Phase::Fatto,
        name: "A".into(),
        number: String::new(),
        seed: [2.0, 1.5],
    };
    let rooms = rooms_in(doc(&app));
    let parts: Vec<RoomPart> = rooms.iter().map(|(_, _, part)| *part).collect();
    assert_eq!(parts, [RoomPart::Contorno, RoomPart::Testo]);
    assert!(rooms.iter().all(|(_, r, _)| *r == room), "{rooms:?}");
    assert!(doc(&app).app_ids.contains(APP_LOCALE));
    assert!(on_layer(&app, LAYER_LOCALI_PROGETTO).is_empty());
    assert_eq!(rilievo_snapshot(&app), before, "il rilievo non si tocca");

    // Il locale è un passo di annullamento; i layer, la prima volta, un altro.
    assert_eq!(undo_steps(&app), steps + 2);
    undo(&mut app);
    assert!(on_layer(&app, LAYER_LOCALI).is_empty());
    assert!(!missing_layers(doc(&app)), "i layer restano");
    undo(&mut app);
    assert!(missing_layers(doc(&app)));
    assert_eq!(rilievo_snapshot(&app), before);
}

#[test]
fn locale_progetto_stores_only_the_seed() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 6);
    let before = rilievo_snapshot(&app);

    run(&mut app, "AL_LOCALE F 3,1 Soggiorno");

    let history = history(&app);
    assert!(
        comes_after(
            &history,
            "AL_LOCALE (Progetto)  Punto interno del locale o [Fase]:",
            "Superficie netta 12,00 m². Nome del locale:"
        ),
        "storia: {history}"
    );
    assert!(
        on_layer(&app, LAYER_LOCALI).is_empty(),
        "né contorno né testo"
    );
    let seeds = on_layer(&app, LAYER_LOCALI_PROGETTO);
    assert_eq!(seeds.len(), 1, "storia: {history}");
    assert_eq!(mtext(&seeds[0].1).value, "Soggiorno");
    let room = Room {
        phase: Phase::Progetto,
        name: "Soggiorno".into(),
        number: String::new(),
        seed: [3.0, 1.0],
    };
    assert_eq!(
        rooms_in(doc(&app)),
        vec![(seeds[0].0, room, RoomPart::Seme)]
    );
    assert_eq!(rilievo_snapshot(&app), before);
}

#[test]
fn aggiorna_follows_the_survey() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 6);
    run(&mut app, "AL_LOCALE 2,1.5 A");
    let ((outline_handle, outline), (text_handle, _)) = room_parts(&app);
    move_tramezzo_face(&mut app);
    let survey = rilievo_snapshot(&app);
    let steps = undo_steps(&app);
    forget_history(&mut app);

    run(&mut app, "AL_AGGIORNA");

    let log = history(&app);
    assert!(
        log.contains("AL_AGGIORNA: 1 locali aggiornati."),
        "storia: {log}"
    );
    let ((new_outline_handle, new_outline), (new_text_handle, text)) = room_parts(&app);
    assert_eq!(
        (new_outline_handle, new_text_handle),
        (outline_handle, text_handle),
        "stessi handle"
    );
    assert_eq!(mtext(&text).value, "A\\PS = 11,70 m²");
    let new_outline = polyline(&new_outline);
    assert!((polyline_area(new_outline) - 11.7).abs() < 1e-9);
    assert!(new_outline
        .vertices
        .iter()
        .any(|v| (v.location.x - 3.9).abs() < 1e-9));
    assert!(!new_outline
        .vertices
        .iter()
        .any(|v| (v.location.x - 4.0).abs() < 1e-9));
    assert_eq!(rilievo_snapshot(&app), survey, "il rilievo non si tocca");
    assert_eq!(undo_steps(&app), steps + 1, "un solo passo");

    // Niente da cambiare: nessun passo.
    forget_history(&mut app);
    run(&mut app, "AL_AGGIORNA");
    assert!(
        history(&app).contains("AL_AGGIORNA: 0 locali aggiornati."),
        "storia: {}",
        history(&app)
    );
    assert_eq!(undo_steps(&app), steps + 1);

    // Un annullamento riporta contorno e testo insieme.
    undo(&mut app);
    let ((_, old_outline), (_, old_text)) = room_parts(&app);
    assert_eq!(old_outline, outline);
    assert_eq!(mtext(&old_text).value, "A\\PS = 12,00 m²");
}

#[test]
fn aggiorna_reports_open_room() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 6);
    run(&mut app, "AL_LOCALE 2,1.5 A");
    let (outline, text) = room_parts(&app);
    let south = survey_line(&app, [0.0, 0.0], [4.0, 0.0]);
    let i = app.active_tab;
    app.tabs[i].scene.erase_entities(&[south]);
    let steps = undo_steps(&app);
    forget_history(&mut app);

    run(&mut app, "AL_AGGIORNA");

    let history = history(&app);
    assert!(
        history.contains("Il locale A non è più chiuso: lasciato com'era."),
        "storia: {history}"
    );
    assert!(
        history.contains("AL_AGGIORNA: 0 locali aggiornati."),
        "storia: {history}"
    );
    assert_eq!(
        room_parts(&app),
        (outline, text),
        "polilinea e testo invariati"
    );
    assert_eq!(undo_steps(&app), steps, "nessun passo");
}

/// Review Focus 3, lato app: un vuoto di 2 mm all'angolo (0;0) di A, sulla
/// faccia ovest. Senza `HPGAPTOL` il locale non è chiuso; con 5 mm misura
/// 12,00, con l'avviso che lo chiude solo la tolleranza.
#[test]
fn gap_closed_by_hpgaptol() {
    let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
    let west = entities
        .iter_mut()
        .find_map(|e| match e {
            EntityType::Line(l) if near(l.start, [0.0, 3.0]) && near(l.end, [0.0, 0.0]) => Some(l),
            _ => None,
        })
        .expect("faccia ovest di A");
    west.end = Vector3::new(0.0, 0.002, 0.0);
    let mut app = fresh_app();
    load(&mut app, entities, 6);
    let before = rilievo_snapshot(&app);

    run(&mut app, "AL_LOCALE 2,1.5 A");
    assert!(
        history(&app).contains(
            "Nessun locale chiuso attorno al punto (chiudi i vani con una linea di soglia o usa HPGAPTOL)."
        ),
        "storia: {}",
        history(&app)
    );
    assert!(on_layer(&app, LAYER_LOCALI).is_empty());
    assert_eq!(active_command(&app), None, "l'Invio finale chiude");

    dispatch_line(&mut app, "HPGAPTOL 0.005");
    forget_history(&mut app);
    run(&mut app, "AL_LOCALE 2,1.5 A");

    let log = history(&app);
    let tolerance = Notice::ChiusoPerTolleranza { name: "A".into() }.text();
    assert!(
        comes_after(&log, "Superficie netta 12,00 m²", &tolerance),
        "manca «{tolerance}»; storia: {log}"
    );
    let ((_, outline), (_, text)) = room_parts(&app);
    assert_eq!(mtext(&text).value, "A\\PS = 12,00 m²");
    let outline = polyline(&outline);
    assert!(outline.vertices.iter().all(|v| v.bulge == 0.0));
    assert!((polyline_area(outline) - 12.0).abs() < 1e-3);

    // AL_AGGIORNA lo ritrova uguale e lo ridice.
    forget_history(&mut app);
    run(&mut app, "AL_AGGIORNA");
    let log = history(&app);
    assert!(
        log.contains("AL_AGGIORNA: 0 locali aggiornati."),
        "storia: {log}"
    );
    assert!(log.contains(&tolerance), "storia: {log}");
    assert_eq!(rilievo_snapshot(&app), before);
    dispatch_line(&mut app, "HPGAPTOL 0");
}

/// R54: il tramezzo (una polilinea chiusa per muro) si ferma 2 mm prima del
/// muro nord dopo che A e B sono stati scritti. La regione di A contiene il
/// seme di B: `AL_AGGIORNA` lo dice, una volta sola.
#[test]
fn aggiorna_reports_fused_rooms() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::ChiusaPerMuro, DrawingUnit::Metri), 6);
    run(&mut app, "AL_LOCALE 2,1.5 A");
    run(&mut app, "AL_LOCALE 6,1.5 B");
    let labels: Vec<String> = on_layer(&app, LAYER_LOCALI)
        .iter()
        .filter(|(_, e)| matches!(e, EntityType::MText(_)))
        .map(|(_, e)| mtext(e).value.clone())
        .collect();
    assert_eq!(labels, ["A\\PS = 12,00 m²", "B\\PS = 11,70 m²"]);
    let tramezzo = doc(&app)
        .entities()
        .find(|e| {
            matches!(e, EntityType::LwPolyline(p) if e.common().layer == LAYER_MURI
                && p.vertices.iter().any(|v| near(Vector3::new(v.location.x, v.location.y, 0.0), [4.1, 3.0])))
        })
        .map(|e| e.common().handle)
        .expect("tramezzo");
    let i = app.active_tab;
    if let Some(EntityType::LwPolyline(p)) = app.tabs[i].scene.document.get_entity_mut(tramezzo) {
        for v in &mut p.vertices {
            if (v.location.y - 3.0).abs() < 1e-9 {
                v.location.y = 2.998;
            }
        }
    }
    forget_history(&mut app);

    run(&mut app, "AL_AGGIORNA");

    let history = history(&app);
    let fused = "Locali A + B fusi in uno (23,70 m²): nome da confermare.";
    assert_eq!(history.matches(fused).count(), 1, "storia: {history}");
    // L'avviso è l'unica differenza: i due locali seguono la regione trovata.
    assert!(
        history.contains("AL_AGGIORNA: 2 locali aggiornati."),
        "storia: {history}"
    );
}

/// In un disegno in centimetri senza unità (INSUNITS = 0) e senza `TEXTSIZE`:
/// lo script risponde all'unità, la superficie esce in m² e il testo è alto
/// 0,25 m, detto in riga di comando.
#[test]
fn locale_asks_the_unit_and_falls_back_on_the_text_height() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Centimetri), 0);
    let i = app.active_tab;
    app.tabs[i].scene.document.header.text_height = 0.0;

    run(&mut app, "AL_LOCALE CM 200,150 A");

    let history = history(&app);
    assert!(
        history.contains(
            "AL_LOCALE: TEXTSIZE del disegno non è positiva: i testi dei locali sono alti 0,25 m."
        ),
        "storia: {history}"
    );
    let (_, (_, text)) = room_parts(&app);
    let text = mtext(&text);
    assert_eq!(text.value, "A\\PS = 12,00 m²");
    assert!((text.height - 25.0).abs() < 1e-9, "{}", text.height);
    assert_eq!(record::read(doc(&app)).unit, Some(DrawingUnit::Centimetri));
    assert_eq!(doc(&app).header.insertion_units, 0);
}

#[test]
fn aggiorna_skips_rooms_on_locked_layers() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 6);
    run(&mut app, "AL_LOCALE 2,1.5 A");
    let parts = room_parts(&app);
    move_tramezzo_face(&mut app);
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .document
        .layers
        .get_mut(LAYER_LOCALI)
        .expect("layer dei locali")
        .lock();
    let steps = undo_steps(&app);
    forget_history(&mut app);

    run(&mut app, "AL_AGGIORNA");

    let history = history(&app);
    assert!(
        history.contains("AL_AGGIORNA: 0 locali aggiornati."),
        "storia: {history}"
    );
    assert!(
        history.contains("Locali su layer bloccati ignorati: 1."),
        "storia: {history}"
    );
    assert_eq!(room_parts(&app), parts);
    assert_eq!(undo_steps(&app), steps);
}

/// Un locale senza contorno (cancellato a mano): il testo si aggiorna e la
/// riga di comando dice che il contorno manca.
#[test]
fn aggiorna_updates_a_lone_text_and_says_so() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 6);
    run(&mut app, "AL_LOCALE 2,1.5 A");
    let ((outline, _), (text, _)) = room_parts(&app);
    let i = app.active_tab;
    app.tabs[i].scene.erase_entities(&[outline]);
    move_tramezzo_face(&mut app);
    forget_history(&mut app);

    run(&mut app, "AL_AGGIORNA");

    let history = history(&app);
    assert!(
        history.contains("Locale A: c'è il testo ma non il contorno."),
        "storia: {history}"
    );
    assert!(
        history.contains("AL_AGGIORNA: 1 locali aggiornati."),
        "storia: {history}"
    );
    assert_eq!(mtext(&entity_of(&app, text)).value, "A\\PS = 11,70 m²");
    assert_eq!(
        on_layer(&app, LAYER_LOCALI).len(),
        1,
        "nessun contorno nuovo"
    );
}
