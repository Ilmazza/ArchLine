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
use crate::bim::fixtures::{case_a, move_to_layer, shift_tramezzo, Form, LAYER_MURI};
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
use crate::ui::window::bim_extract::{BimMsg, OVERWRITE_BUTTON, OVERWRITE_NOTE};
use codec::entities::{Arc, Line, LwPolyline, MText};
use codec::xdata::{ExtendedDataRecord, XDataValue};
use codec::{Document, Color, EntityType, Handle, Vector3};
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

fn doc(app: &OpenCADStudio) -> &Document {
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

/// Fix finale, punto 2: un disegno senza dizionario radice non può tenere il
/// record. `AL_UNITA` lo dice e non lascia un passo di annullamento vuoto né
/// segna il disegno come modificato.
#[test]
fn unit_record_that_cannot_be_written_leaves_no_undo_step() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 0);
    let i = app.active_tab;
    app.tabs[i].scene.document.header.named_objects_dict_handle = Handle::NULL;
    app.tabs[i].dirty = false;
    let steps = undo_steps(&app);

    run(&mut app, "AL_UNITA M");

    assert!(
        history(&app).contains("AL_UNITA: impossibile salvare l'unità nel disegno."),
        "storia: {}",
        history(&app)
    );
    assert_eq!(record::read(doc(&app)).unit, None);
    assert_eq!(undo_steps(&app), steps, "nessun passo vuoto");
    assert!(!app.tabs[i].dirty, "il disegno non è cambiato");
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

/// Fix finale, punto 2: con file esistenti il primo OK avvisa, mostra la
/// casella «Sovrascrivi i file esistenti» (spenta) e il pulsante diventa
/// «Sovrascrivi»; un secondo OK senza casella (doppio clic) non scrive nulla.
/// Solo con la casella accesa i file si riscrivono.
#[test]
fn dialog_flow_overwrite_needs_the_checkbox() {
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
        assert!(!dialog.files_exist && !dialog.overwrite);
        assert_eq!(dialog.ok_label(), "OK");
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
    let before = mtimes(&paths);

    // Nuova apertura, stessa cartella: il primo OK avvisa e non scrive.
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
    assert!(dialog.files_exist);
    assert!(!dialog.overwrite, "la casella nasce spenta");
    assert_eq!(dialog.note(), Some(OVERWRITE_NOTE));
    assert_eq!(dialog.ok_label(), OVERWRITE_BUTTON);
    assert!(!dialog.can_ok(), "senza casella «Sovrascrivi» è spento");
    assert!(paths.iter().all(|path| marked(path)), "niente scritto");
    assert!(
        !history(&app).contains("esistono già"),
        "l'avviso sta nella finestra: {}",
        history(&app)
    );

    // Il secondo OK, senza la casella (doppio clic): ancora niente.
    bim(&mut app, BimMsg::Ok);
    assert_eq!(app.active_modal, Some(ModalKind::BimExtract));
    assert!(paths.iter().all(|path| marked(path)), "niente scritto");
    assert_eq!(mtimes(&paths), before, "date di modifica invariate");
    assert!(!history(&app).contains("AL_ESTRAI: scritti"));

    // Con la casella accesa «Sovrascrivi» riscrive e chiude.
    bim(&mut app, BimMsg::Overwrite(true));
    assert!(app.bim_extract.as_ref().unwrap().can_ok());
    bim(&mut app, BimMsg::Ok);
    assert!(
        paths.iter().all(|path| !marked(path)),
        "riscritti; storia: {}",
        history(&app)
    );
    assert_eq!(app.active_modal, None);
    assert!(app.bim_extract.is_none());
}

/// La casella si spegne quando cambia ciò che si scrive: un'altra cartella
/// (avviso via, pulsante di nuovo OK), un elaborato in più o in meno (lo
/// stesso), un altro layer dei muri nuovi (l'avviso resta: stessi file).
#[test]
fn dialog_overwrite_checkbox_resets_when_the_choice_changes() {
    let tmp = TempDir::new("casella");
    let other = TempDir::new("casella-altrove");
    let (mut app, _, _) = app_with_two_zones();
    let paths = extracted_paths(tmp.path(), &tab_title(&app));
    run(&mut app, &format!("AL_ESTRAI {}", tmp.path().display()));
    assert!(paths.iter().all(|path| path.is_file()));
    mark(&paths);

    let dialog = |app: &OpenCADStudio| app.bim_extract.clone().expect("finestra aperta");
    dispatch_line(&mut app, "AL_ESTRAI");
    pick_folder(&mut app, tmp.path());
    bim(&mut app, BimMsg::Ok);
    bim(&mut app, BimMsg::Overwrite(true));
    assert!(dialog(&app).overwrite);

    // Altra cartella: casella spenta, avviso via.
    pick_folder(&mut app, other.path());
    let d = dialog(&app);
    assert!(!d.overwrite && !d.files_exist, "{d:?}");
    assert_eq!(d.ok_label(), "OK");

    // Di nuovo la cartella con i file: l'avviso torna, con la casella spenta.
    pick_folder(&mut app, tmp.path());
    bim(&mut app, BimMsg::Ok);
    assert!(dialog(&app).files_exist && !dialog(&app).overwrite);
    assert!(paths.iter().all(|path| marked(path)), "niente scritto");

    // Un altro layer: la casella si spegne, l'avviso resta.
    bim(&mut app, BimMsg::Overwrite(true));
    bim(&mut app, BimMsg::Layer("MURI-NUOVI".to_string()));
    let d = dialog(&app);
    assert!(d.files_exist && !d.overwrite, "{d:?}");
    bim(&mut app, BimMsg::Ok);
    assert!(paths.iter().all(|path| marked(path)), "niente scritto");

    // Un elaborato tolto: casella e avviso via.
    bim(&mut app, BimMsg::Overwrite(true));
    bim(&mut app, BimMsg::Toggle(Elaborato::StatoDiFatto));
    let d = dialog(&app);
    assert!(!d.overwrite && !d.files_exist, "{d:?}");
    assert!(paths.iter().all(|path| marked(path)), "niente scritto");
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
    assert!(app.bim_extract.as_ref().unwrap().files_exist);
    assert!(!paths[0].exists(), "niente scritto prima della conferma");
    bim(&mut app, BimMsg::Toggle(Elaborato::StatoDiProgetto));
    assert!(!app.bim_extract.as_ref().unwrap().files_exist);
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
    assert!(app.bim_extract.as_ref().unwrap().files_exist);
    assert!(!paths[0].exists());
    forget_history(&mut app);
    bim(&mut app, BimMsg::Overwrite(true));
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
    assert!(app.bim_extract.as_ref().unwrap().files_exist);
    forget_history(&mut app);
    bim(&mut app, BimMsg::Overwrite(true));
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
    // Il documento nuovo ha TEXTSIZE 2,5, non plausibile in metri: 0,25 m
    // (fix finale, punto 6).
    assert_eq!(doc(&app).header.text_height, 2.5);
    assert_eq!(text.height, 0.25);
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
    // R58: quale locale e da quanto a quanto, prima del riepilogo.
    assert!(
        comes_after(
            &log,
            "Locale A: 12,00 → 11,70 m².",
            "AL_AGGIORNA: 1 locali aggiornati."
        ),
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

/// R67: il caso A con il locale A scritto da `AL_LOCALE`, contorno e testo
/// spostati a mano sul layer LOCALI, poi il rilievo corretto: il tramezzo
/// 0,20 m più a est, A da 12,00 a 12,60 m². Il lato est del contorno vecchio
/// (x = 4,00) ora cade dentro A. Gli handle di contorno e testo.
fn app_with_a_moved_room_outline() -> (OpenCADStudio, Handle, Handle) {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 6);
    run(&mut app, "AL_LOCALE 2,1.5 A");
    let ((outline, _), (text, _)) = room_parts(&app);
    let i = app.active_tab;
    let document = &mut app.tabs[i].scene.document;
    for handle in [outline, text] {
        move_to_layer(document, handle, "LOCALI");
    }
    shift_tramezzo(document, DrawingUnit::Metri, 0.2);
    forget_history(&mut app);
    (app, outline, text)
}

/// R67 in `AL_AGGIORNA` dall'app: il contorno spostato non fa da muro, A
/// passa a 12,60 m² sugli stessi handle (prima restava 12,00 e il comando
/// diceva «0 locali aggiornati»).
#[test]
fn aggiorna_ignores_a_moved_room_outline() {
    let (mut app, outline, text) = app_with_a_moved_room_outline();

    run(&mut app, "AL_AGGIORNA");

    let log = history(&app);
    assert!(
        comes_after(
            &log,
            "Locale A: 12,00 → 12,60 m².",
            "AL_AGGIORNA: 1 locali aggiornati."
        ),
        "storia: {log}"
    );
    let Some(EntityType::MText(label)) = doc(&app).get_entity(text) else {
        panic!("testo di A")
    };
    assert_eq!(label.value, "A\\PS = 12,60 m²");
    assert_eq!(label.common.layer, "LOCALI", "resta sul suo layer");
    let Some(EntityType::LwPolyline(contorno)) = doc(&app).get_entity(outline) else {
        panic!("contorno di A")
    };
    assert!((polyline_area(contorno) - 12.6).abs() < 1e-9);
}

/// R67 nei comandi delle zone (`zone_snapshot`): il lato est del contorno
/// vecchio non è un muro. Un muro nuovo lungo x = 4,00 nel locale A non dà
/// «nuovo su esistente», e il punto interno a x = 4,10 trova il locale A
/// intero (12,60 m²), non la striscia tra il contorno vecchio e la faccia
/// nuova (0,60 m²).
#[test]
fn zone_commands_ignore_a_moved_room_outline() {
    let (mut app, _, _) = app_with_a_moved_room_outline();

    run(&mut app, "AL_NUOVO 4,0 4,3 0.12");

    let log = history(&app);
    assert!(
        !log.contains(&Notice::NuovoSuEsistente.text()) && !log.contains("Confermi la zona?"),
        "storia: {log}"
    );
    assert_eq!(active_command(&app), None, "storia: {log}");
    let zones = zones_in(doc(&app));
    assert_eq!(zones.len(), 1, "storia: {log}");
    assert_eq!(zones[0].zone.state, ZoneState::Nuovo);

    let (mut app, _, _) = app_with_a_moved_room_outline();

    run(&mut app, "AL_DEMOLISCI I 4.1,1.5 N");

    let log = history(&app);
    let zones = zones_in(doc(&app));
    assert_eq!(zones.len(), 1, "storia: {log}");
    assert_eq!(zones[0].zone.state, ZoneState::Demolito);
    let outer = &zones[0].rings[0];
    let twice: f64 = (0..outer.len())
        .map(|k| {
            let (a, b) = (outer[k], outer[(k + 1) % outer.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    assert!(
        (twice.abs() / 2.0 - 12.6).abs() < 1e-9,
        "area {} invece di 12,60 m²; storia: {log}",
        twice.abs() / 2.0
    );
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
    // Since upstream v2026.43 the ring closes a 2 mm gap by joining the two
    // ends, so the outline loses the corner triangle (4 mm²).
    assert!((polyline_area(outline) - 12.0).abs() < 1e-2);

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

/// R54 e R55: il tramezzo (una polilinea chiusa per muro) si ferma 2 mm prima
/// del muro nord dopo che A e B sono stati scritti. La regione di A contiene il
/// seme di B: `AL_AGGIORNA` lo dice una volta sola e lascia i due locali
/// com'erano, senza passi di annullamento.
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
    let rooms_before = on_layer(&app, LAYER_LOCALI);
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
    let steps = undo_steps(&app);
    forget_history(&mut app);

    run(&mut app, "AL_AGGIORNA");

    let history = history(&app);
    let fused = "Locali A + B fusi in uno (23,70 m²): nome da confermare.";
    assert_eq!(history.matches(fused).count(), 1, "storia: {history}");
    assert!(
        comes_after(&history, fused, "Locali A + B lasciati com'erano."),
        "storia: {history}"
    );
    assert!(
        history.contains("AL_AGGIORNA: 0 locali aggiornati."),
        "storia: {history}"
    );
    // Contorni e testi di A e B identici: 12,00 e 11,70 restano.
    assert_eq!(on_layer(&app, LAYER_LOCALI), rooms_before);
    assert_eq!(undo_steps(&app), steps, "nessun passo");
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

/// Fix finale, punto 6: un disegno in metri con `TEXTSIZE` 2,5 (nato da un
/// modello in millimetri come acadiso): il testo del locale è alto 0,25 m, non
/// 2,5 m, e la riga di comando dice perché.
#[test]
fn locale_in_metres_with_an_implausible_textsize_falls_back() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 6);
    let i = app.active_tab;
    app.tabs[i].scene.document.header.text_height = 2.5;

    run(&mut app, "AL_LOCALE 2,1.5 A");

    let history = history(&app);
    assert!(
        history.contains(
            "AL_LOCALE: TEXTSIZE del disegno (2,50) è implausibile (oltre 1 m): i testi dei \
             locali sono alti 0,25 m."
        ),
        "storia: {history}"
    );
    let (_, (_, text)) = room_parts(&app);
    let text = mtext(&text);
    assert_eq!(text.value, "A\\PS = 12,00 m²");
    assert!((text.height - 0.25).abs() < 1e-12, "{}", text.height);
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
    // R60: l'area di prima viene dall'etichetta, anche senza contorno.
    assert!(
        history.contains("Locale A: 12,00 → 11,70 m²."),
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

/// R59: senza locali di fatto `AL_AGGIORNA` lo dice, senza chiedere l'unità
/// (INSUNITS = 0) e senza passi di annullamento. Un seme di progetto non è un
/// locale di fatto.
#[test]
fn aggiorna_without_rooms_says_so() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::Continua, DrawingUnit::Metri), 0);
    let steps = undo_steps(&app);
    // Un rilancio lasciato a metà non vale più, come per le altre uscite.
    app.bim_pending = Some("AL_DEMOLISCI".into());

    run(&mut app, "AL_AGGIORNA");

    let log = history(&app);
    assert!(
        log.contains("AL_AGGIORNA: nessun locale di fatto nel disegno."),
        "storia: {log}"
    );
    assert!(!log.contains("Unità del disegno"), "storia: {log}");
    assert_eq!(active_command(&app), None);
    assert_eq!(app.bim_pending, None);
    assert_eq!(record::read(doc(&app)).unit, None);
    assert_eq!(undo_steps(&app), steps);

    // Con il solo seme di un locale di progetto è lo stesso.
    run(&mut app, "AL_UNITA M");
    run(&mut app, "AL_LOCALE F 3,1 Soggiorno");
    assert_eq!(on_layer(&app, LAYER_LOCALI_PROGETTO).len(), 1);
    forget_history(&mut app);
    run(&mut app, "AL_AGGIORNA");
    assert!(
        history(&app).contains("AL_AGGIORNA: nessun locale di fatto nel disegno."),
        "storia: {}",
        history(&app)
    );
}

/// R59: con `HPGAPTOL` a 5 mm, un tramezzo spostato di 4 mm cambia l'etichetta
/// e anche il contorno (confrontato alla tolleranza di base).
#[test]
fn aggiorna_with_hpgaptol_moves_the_outline_too() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::ChiusaPerMuro, DrawingUnit::Metri), 6);
    dispatch_line(&mut app, "HPGAPTOL 0.005");
    run(&mut app, "AL_LOCALE 2,1.5 A");
    let ((outline_handle, _), (text_handle, _)) = room_parts(&app);
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
            v.location.x += 0.004;
        }
    }
    forget_history(&mut app);

    run(&mut app, "AL_AGGIORNA");

    let history = history(&app);
    assert!(
        history.contains("Locale A: 12,00 → 12,01 m²."),
        "storia: {history}"
    );
    let ((outline, new_outline), (text, new_text)) = room_parts(&app);
    assert_eq!((outline, text), (outline_handle, text_handle));
    assert_eq!(mtext(&new_text).value, "A\\PS = 12,01 m²");
    assert!(
        polyline(&new_outline)
            .vertices
            .iter()
            .any(|v| (v.location.x - 4.004).abs() < 1e-9),
        "il contorno segue il tramezzo"
    );
    dispatch_line(&mut app, "HPGAPTOL 0");
}

/// R59: un locale con una parete ad arco (mezzo disco sopra un rettangolo
/// 2 × 1). Il contorno porta l'arco vero; rimisurato uguale non cambia (nessun
/// passo); con la parete sud spostata l'arco resta e l'area di prima tiene
/// conto dell'arco.
#[test]
fn aggiorna_keeps_true_arcs() {
    let wall = |a: [f64; 2], b: [f64; 2]| {
        let mut line =
            Line::from_points(Vector3::new(a[0], a[1], 0.0), Vector3::new(b[0], b[1], 0.0));
        line.common.layer = LAYER_MURI.to_string();
        EntityType::Line(line)
    };
    let mut arc = Arc::from_center_radius_angles(Vector3::ZERO, 1.0, 0.0, std::f64::consts::PI);
    arc.common.layer = LAYER_MURI.to_string();
    let entities = vec![
        wall([-1.0, -1.0], [1.0, -1.0]),
        wall([1.0, -1.0], [1.0, 0.0]),
        EntityType::Arc(arc),
        wall([-1.0, 0.0], [-1.0, -1.0]),
    ];
    let mut app = fresh_app();
    load(&mut app, entities, 6);
    run(&mut app, "AL_LOCALE 0,-0.5 Abside");
    let ((_, outline), (_, text)) = room_parts(&app);
    // 2 × 1 + π/2 = 3,5708.
    assert_eq!(mtext(&text).value, "Abside\\PS = 3,57 m²");
    let has_arc = |entity: &EntityType| {
        polyline(entity)
            .vertices
            .iter()
            .any(|v| v.bulge.abs() > 1e-6)
    };
    assert!(has_arc(&outline), "arco vero nel contorno");

    let steps = undo_steps(&app);
    forget_history(&mut app);
    run(&mut app, "AL_AGGIORNA");
    assert!(
        history(&app).contains("AL_AGGIORNA: 0 locali aggiornati."),
        "storia: {}",
        history(&app)
    );
    assert_eq!(undo_steps(&app), steps, "nessun passo");
    assert_eq!(room_parts(&app).0 .1, outline);

    // La parete sud da y = -1 a y = -0,9: 1,8 + π/2 = 3,3708.
    let south = survey_line(&app, [-1.0, -1.0], [1.0, -1.0]);
    let i = app.active_tab;
    if let Some(EntityType::Line(line)) = app.tabs[i].scene.document.get_entity_mut(south) {
        line.start.y = -0.9;
        line.end.y = -0.9;
    }
    forget_history(&mut app);
    run(&mut app, "AL_AGGIORNA");
    let history = history(&app);
    assert!(
        history.contains("Locale Abside: 3,57 → 3,37 m²."),
        "storia: {history}"
    );
    let ((_, new_outline), (_, new_text)) = room_parts(&app);
    assert_eq!(mtext(&new_text).value, "Abside\\PS = 3,37 m²");
    assert!(has_arc(&new_outline), "l'arco resta");
    assert_eq!(undo_steps(&app), steps + 1);
}

/// R60: con il pilastro l'etichetta dice l'area netta (11,91). Spento il layer
/// del pilastro, la riga dice da quanto a quanto: 11,91 → 12,00, non 12,00 →
/// 12,00 (il contorno, solo l'anello esterno, è lordo).
#[test]
fn aggiorna_reports_the_net_area_with_a_pillar() {
    let mut entities = case_a(Form::Continua, DrawingUnit::Metri);
    entities.push(crate::bim::fixtures::pilastro(DrawingUnit::Metri));
    let mut app = fresh_app();
    load(&mut app, entities, 6);
    run(&mut app, "AL_LOCALE 2,1.5 A");
    let ((_, outline), (_, text)) = room_parts(&app);
    assert_eq!(mtext(&text).value, "A\\PS = 11,91 m²");
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .document
        .layers
        .get_mut(crate::bim::fixtures::LAYER_PILASTRI)
        .expect("layer del pilastro")
        .turn_off();
    forget_history(&mut app);

    run(&mut app, "AL_AGGIORNA");

    let history = history(&app);
    assert!(
        comes_after(
            &history,
            "Locale A: 11,91 → 12,00 m².",
            "AL_AGGIORNA: 1 locali aggiornati."
        ),
        "storia: {history}"
    );
    let ((_, new_outline), (_, new_text)) = room_parts(&app);
    assert_eq!(mtext(&new_text).value, "A\\PS = 12,00 m²");
    assert_eq!(new_outline, outline, "l'anello esterno è lo stesso");
}

/// R55 e R59 con un locale scritto «da confermare»: B nasce dentro la regione
/// unita da un vuoto (il seme di A ci sta dentro); finché il vuoto c'è,
/// `AL_AGGIORNA` lascia com'erano A e B; chiuso il vuoto, B torna 11,70 e
/// perde la nota, A resta com'era.
#[test]
fn aggiorna_drops_the_note_when_the_gap_closes() {
    let mut app = fresh_app();
    load(&mut app, case_a(Form::ChiusaPerMuro, DrawingUnit::Metri), 6);
    run(&mut app, "AL_LOCALE 2,1.5 A");
    let tramezzo = doc(&app)
        .entities()
        .find(|e| {
            matches!(e, EntityType::LwPolyline(p) if e.common().layer == LAYER_MURI
                && p.vertices.iter().any(|v| near(Vector3::new(v.location.x, v.location.y, 0.0), [4.1, 3.0])))
        })
        .map(|e| e.common().handle)
        .expect("tramezzo");
    let set_top = |app: &mut OpenCADStudio, from: f64, to: f64| {
        let i = app.active_tab;
        if let Some(EntityType::LwPolyline(p)) = app.tabs[i].scene.document.get_entity_mut(tramezzo)
        {
            for v in &mut p.vertices {
                if (v.location.y - from).abs() < 1e-9 {
                    v.location.y = to;
                }
            }
        }
    };
    set_top(&mut app, 3.0, 2.998);
    run(&mut app, "AL_LOCALE 6,1.5 B");
    let label_of = |app: &OpenCADStudio, name: &str| {
        on_layer(app, LAYER_LOCALI)
            .iter()
            .filter_map(|(_, e)| match e {
                EntityType::MText(t) if t.value.starts_with(name) => Some(t.value.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(label_of(&app, "B"), ["B (da confermare)\\PS = 23,70 m²"]);
    let rooms_before = on_layer(&app, LAYER_LOCALI);
    forget_history(&mut app);

    run(&mut app, "AL_AGGIORNA");

    let fused = history(&app);
    assert!(
        fused.contains("Locali A + B lasciati com'erano."),
        "storia: {fused}"
    );
    assert_eq!(on_layer(&app, LAYER_LOCALI), rooms_before, "niente cambia");

    // Il vuoto si chiude.
    set_top(&mut app, 2.998, 3.0);
    forget_history(&mut app);
    run(&mut app, "AL_AGGIORNA");

    let closed = history(&app);
    assert!(
        comes_after(
            &closed,
            "Locale B: 23,70 → 11,70 m².",
            "AL_AGGIORNA: 1 locali aggiornati."
        ),
        "storia: {closed}"
    );
    assert_eq!(label_of(&app, "B"), ["B\\PS = 11,70 m²"]);
    assert_eq!(label_of(&app, "A"), ["A\\PS = 12,00 m²"]);
}

// ------------------------------------------------- Locali negli elaborati

/// I testi (`MTEXT`) sul layer `layer` di un documento, in ordine alfabetico.
fn labels_in(doc: &Document, layer: &str) -> Vec<String> {
    let mut labels: Vec<String> = doc
        .entities()
        .filter(|e| e.common().layer.eq_ignore_ascii_case(layer))
        .filter_map(|e| match e {
            EntityType::MText(t) => Some(t.value.clone()),
            _ => None,
        })
        .collect();
    labels.sort();
    labels
}

/// Le polilinee del rilievo (layer `MURI`) di `doc` sono quelle di `survey`:
/// stesse in numero, e per ognuna una con la stessa chiusura e gli stessi
/// vertici (entro 1e-6) e bulge.
#[track_caller]
fn same_survey_polylines(doc: &Document, survey: &[(Handle, EntityType)], what: &str) {
    let found: Vec<&LwPolyline> = doc
        .entities()
        .filter_map(|e| match e {
            EntityType::LwPolyline(p) if p.common.layer == LAYER_MURI => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(found.len(), survey.len(), "{what}");
    for (_, entity) in survey {
        let original = polyline(entity);
        let same = |p: &&LwPolyline| {
            p.is_closed == original.is_closed
                && p.vertices.len() == original.vertices.len()
                && p.vertices.iter().zip(&original.vertices).all(|(a, b)| {
                    (a.location.x - b.location.x).abs() <= 1e-6
                        && (a.location.y - b.location.y).abs() <= 1e-6
                        && (a.bulge - b.bulge).abs() <= 1e-9
                })
        };
        assert!(
            found.iter().any(same),
            "{what}: manca la polilinea {:?}",
            original.vertices
        );
    }
}

/// Spec §7, «Originali intatti», per tutto il giro: caso A a polilinee, un
/// varco nel tramezzo (`AL_DEMOLISCI`, poi `AL_STATO V`), un muro nuovo in B,
/// i locali di fatto A e B, un locale di progetto, `AL_AGGIORNA` e
/// `AL_ESTRAI`. Dopo ogni comando le entità del rilievo hanno lo stesso
/// handle, la stessa geometria e gli stessi attributi; i tre DWG si rileggono,
/// nel fatto e nella comparativa il rilievo è lo stesso al micron, nel
/// progetto ci sono i locali ricalcolati.
#[test]
fn full_flow_never_touches_the_survey() {
    let tmp = TempDir::new("giro-completo");
    let mut app = fresh_app();
    load(&mut app, case_a(Form::ChiusaPerMuro, DrawingUnit::Metri), 6);
    let before = rilievo_snapshot(&app);
    assert_eq!(before.len(), 5, "cinque polilinee chiuse");
    let step = |app: &mut OpenCADStudio, cmd: &str| {
        run(app, cmd);
        assert_eq!(active_command(app), None, "{cmd}: storia: {}", history(app));
        assert_eq!(
            rilievo_snapshot(app),
            before,
            "{cmd}: il rilievo è cambiato; storia: {}",
            history(app)
        );
    };

    step(&mut app, "AL_DEMOLISCI 4.05,1 4.05,1.9 N");
    step(&mut app, "AL_NUOVO 6,0 6,3 0.12");
    let demolita = zones_in(doc(&app))
        .iter()
        .find(|z| z.zone.state == ZoneState::Demolito)
        .map(|z| z.handle)
        .expect("zona demolita");
    select(&mut app, &[demolita]);
    step(&mut app, "AL_STATO V");
    assert!(zone_of(&app, demolita).varco, "storia: {}", history(&app));
    select(&mut app, &[]);
    step(&mut app, "AL_LOCALE 2,1.5 A");
    step(&mut app, "AL_LOCALE 7,1.5 B");
    step(&mut app, "AL_LOCALE F 5,1.5 Studio");
    assert!(
        history(&app).contains("Superficie netta 5,52 m²"),
        "storia: {}",
        history(&app)
    );
    assert_eq!(
        labels_in(doc(&app), LAYER_LOCALI),
        ["A\\PS = 12,00 m²", "B\\PS = 11,70 m²"]
    );
    forget_history(&mut app);
    step(&mut app, "AL_AGGIORNA");
    assert!(
        history(&app).contains("AL_AGGIORNA: 0 locali aggiornati."),
        "storia: {}",
        history(&app)
    );
    let rooms = on_layer(&app, LAYER_LOCALI);
    let seeds = on_layer(&app, LAYER_LOCALI_PROGETTO);
    let steps = undo_steps(&app);
    forget_history(&mut app);
    step(&mut app, &format!("AL_ESTRAI {}", tmp.path().display()));

    let log = history(&app);
    assert!(
        log.contains(&format!(
            "AL_ESTRAI: scritti 3 file in {}.",
            tmp.path().display()
        )),
        "storia: {log}"
    );
    assert!(
        log.contains(&Notice::Varco { area_m2: 0.09 }.text()),
        "storia: {log}"
    );
    // L'estrazione non tocca né i locali né la storia degli annullamenti.
    assert_eq!(on_layer(&app, LAYER_LOCALI), rooms);
    assert_eq!(on_layer(&app, LAYER_LOCALI_PROGETTO), seeds);
    assert_eq!(undo_steps(&app), steps);

    let paths = extracted_paths(tmp.path(), &tab_title(&app));
    let loaded: Vec<Document> = paths
        .iter()
        .map(|path| {
            assert!(path.is_file(), "{}: storia: {log}", path.display());
            crate::io::load_file(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        })
        .collect();
    let (fatto, comparativa, progetto) = (&loaded[0], &loaded[1], &loaded[2]);
    same_survey_polylines(fatto, &before, "stato di fatto");
    same_survey_polylines(comparativa, &before, "demolizioni e costruzioni");
    assert_eq!(
        labels_in(fatto, LAYER_LOCALI),
        ["A\\PS = 12,00 m²", "B\\PS = 11,70 m²"]
    );
    assert!(labels_in(comparativa, LAYER_LOCALI).is_empty());
    assert_eq!(
        comparativa
            .entities()
            .filter(|e| matches!(e, EntityType::Hatch(_)))
            .count(),
        2,
        "le due zone"
    );
    // Progetto: A resta (il varco non lo unisce), B diviso dal muro nuovo,
    // la parte senza seme di fatto prende il nome del seme di progetto.
    assert_eq!(
        labels_in(progetto, LAYER_LOCALI),
        [
            "A\\PS = 12,00 m²",
            "B\\PS = 5,82 m²",
            "Studio\\PS = 5,52 m²"
        ]
    );
    assert!(labels_in(progetto, LAYER_LOCALI_PROGETTO).is_empty());
    for (path, out) in paths.iter().zip(&loaded) {
        assert!(
            !out.app_ids.iter().any(|a| a.name.starts_with("ARCHLINE")),
            "{}: APPID nostro",
            path.display()
        );
        assert!(
            out.entities().all(|e| e
                .common()
                .extended_data
                .records()
                .iter()
                .all(|r| !r.application_name.starts_with("ARCHLINE"))),
            "{}: XDATA nostro",
            path.display()
        );
    }
}

/// R61, lato app: `AL_ESTRAI` misura i locali con `HPGAPTOL`, come
/// `AL_LOCALE` e `AL_AGGIORNA`. Un vuoto di 2 mm all'angolo di A, A scritto
/// con HPGAPTOL 5 mm, poi la faccia del tramezzo spostata a mano: l'estratto
/// dice 11,70 (fatto e progetto), con le righe dell'etichetta vecchia e della
/// tolleranza; il disegno di lavoro resta a 12,00. Con HPGAPTOL 0 A non è
/// chiuso e resta com'era.
#[test]
fn extract_measures_rooms_with_hpgaptol() {
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
    dispatch_line(&mut app, "HPGAPTOL 0.005");
    run(&mut app, "AL_LOCALE 2,1.5 A");
    let (_, (text, _)) = room_parts(&app);
    assert_eq!(mtext(&entity_of(&app, text)).value, "A\\PS = 12,00 m²");
    move_tramezzo_face(&mut app);
    let survey = rilievo_snapshot(&app);
    let tmp = TempDir::new("hpgaptol");
    forget_history(&mut app);

    run(&mut app, &format!("AL_ESTRAI {}", tmp.path().display()));

    let log = history(&app);
    let stale = "Locale A: etichetta nel disegno 12,00 → 11,70 m² nell'estratto.";
    assert!(
        comes_after(&log, "AL_ESTRAI: scritti 3 file", stale),
        "storia: {log}"
    );
    let tolerance = Notice::ChiusoPerTolleranza { name: "A".into() }.text();
    assert_eq!(log.matches(&tolerance).count(), 1, "storia: {log}");
    let paths = extracted_paths(tmp.path(), &tab_title(&app));
    let load_dwg = |path: &Path| crate::io::load_file(path).unwrap();
    assert_eq!(
        labels_in(&load_dwg(&paths[0]), LAYER_LOCALI),
        ["A\\PS = 11,70 m²"]
    );
    assert_eq!(
        labels_in(&load_dwg(&paths[2]), LAYER_LOCALI),
        ["A\\PS = 11,70 m²"]
    );
    assert_eq!(mtext(&entity_of(&app, text)).value, "A\\PS = 12,00 m²");
    assert_eq!(rilievo_snapshot(&app), survey);

    dispatch_line(&mut app, "HPGAPTOL 0");
    let tmp = TempDir::new("senza-hpgaptol");
    forget_history(&mut app);
    run(&mut app, &format!("AL_ESTRAI {}", tmp.path().display()));
    let log = history(&app);
    assert!(
        log.contains("Il locale A non è più chiuso: lasciato com'era."),
        "storia: {log}"
    );
    assert!(!log.contains(stale), "storia: {log}");
    let paths = extracted_paths(tmp.path(), &tab_title(&app));
    assert_eq!(
        labels_in(&load_dwg(&paths[0]), LAYER_LOCALI),
        ["A\\PS = 12,00 m²"]
    );
}
