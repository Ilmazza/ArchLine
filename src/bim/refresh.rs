//! `AL_AGGIORNA` (spec §4.2): il ricalcolo dei locali di fatto già scritti nel
//! disegno ([`refresh_rooms`]), puro. Le modifiche ([`RoomEdit`]) le applica
//! chi chiama: l'app sul disegno, l'estrazione sulla sua copia.

use codec::entities::{LwPolyline, LwVertex, MText};
use codec::{CadDocument, EntityType, Handle};

use super::report::Notice;
use super::rooms::{
    label, measure_at, quality_notices, region_holds, room_entities, rooms_in, Computed, Measured,
    Phase, Room, RoomPart,
};
use super::units::DrawingUnit;
use super::walls::{faces as wall_faces, wall_set_without_rooms};

/// Una modifica di `AL_AGGIORNA` a un'entità di un locale di fatto, che resta
/// con il suo handle.
#[derive(Clone, Debug, PartialEq)]
pub enum RoomEdit {
    /// I vertici (con i bulge) e la chiusura del contorno.
    Contorno {
        vertices: Vec<LwVertex>,
        closed: bool,
    },
    /// Il testo dell'etichetta.
    Testo { value: String },
}

impl RoomEdit {
    /// Scrive la modifica nell'entità, se è del tipo giusto (`LWPOLYLINE` per
    /// il contorno, `MTEXT` per il testo); un altro tipo resta com'è.
    pub fn apply_to(&self, entity: &mut EntityType) {
        match (self, entity) {
            (Self::Contorno { vertices, closed }, EntityType::LwPolyline(polyline)) => {
                polyline.vertices = vertices.clone();
                polyline.is_closed = *closed;
            }
            (Self::Testo { value }, EntityType::MText(text)) => text.value = value.clone(),
            _ => {}
        }
    }
}

/// Un locale di fatto che `AL_AGGIORNA` riscrive.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomChange {
    pub name: String,
    /// L'area che diceva l'etichetta di prima, in m² (R60: netta, come la
    /// nuova; il contorno è solo l'anello esterno, lordo delle isole). `None`
    /// senza testo o se il testo non la dice.
    pub old_area_m2: Option<f64>,
    pub new_area_m2: f64,
    /// L'etichetta cambia (altrimenti cambia solo il contorno).
    pub label_changed: bool,
    /// Le modifiche alle sue entità, ognuna col suo handle.
    pub edits: Vec<(Handle, RoomEdit)>,
}

impl RoomChange {
    /// La riga che dice quale locale cambia e da quanto a quanto (R58, R60):
    /// [`Notice::LocaleAggiornato`], o [`Notice::ContornoAggiornato`] quando
    /// l'etichetta resta la stessa.
    pub fn notice(&self) -> Notice {
        if self.label_changed {
            Notice::LocaleAggiornato {
                name: self.name.clone(),
                old_m2: self.old_area_m2,
                new_m2: self.new_area_m2,
            }
        } else {
            Notice::ContornoAggiornato {
                name: self.name.clone(),
                area_m2: self.new_area_m2,
            }
        }
    }
}

/// Il ricalcolo dei locali di fatto di un disegno ([`refresh_rooms`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoomsRefresh {
    /// I locali in cui qualcosa cambia, nell'ordine di disegno.
    pub changed: Vec<RoomChange>,
    /// Gli avvisi, locale per locale, nell'ordine di disegno.
    pub notices: Vec<Notice>,
}

/// Un locale di fatto nel disegno: il contorno e il testo accoppiati per seme.
struct FattoParts<'a> {
    room: Room,
    outline: Option<(Handle, &'a LwPolyline)>,
    text: Option<(Handle, &'a MText)>,
}

/// Le parti dei locali di fatto dello spazio modello, accoppiate per seme
/// entro `tol` nell'ordine di disegno: ogni parte va al primo locale con il
/// seme vicino che non ne ha ancora una di quel tipo. Un contorno che non è
/// una `LWPOLYLINE`, o un testo che non è un `MTEXT`, non conta.
fn fatto_parts(doc: &CadDocument, tol: f64) -> Vec<FattoParts<'_>> {
    let mut rooms: Vec<FattoParts> = Vec::new();
    for (handle, room, part) in rooms_in(doc) {
        if room.phase != Phase::Fatto {
            continue;
        }
        let seed = room.seed;
        let near = |parts: &FattoParts| distance(parts.room.seed, seed) <= tol;
        match (part, doc.get_entity(handle)) {
            (RoomPart::Contorno, Some(EntityType::LwPolyline(polyline))) => {
                match rooms
                    .iter_mut()
                    .find(|parts| parts.outline.is_none() && near(parts))
                {
                    Some(parts) => parts.outline = Some((handle, polyline)),
                    None => rooms.push(FattoParts {
                        room,
                        outline: Some((handle, polyline)),
                        text: None,
                    }),
                }
            }
            (RoomPart::Testo, Some(EntityType::MText(text))) => {
                match rooms
                    .iter_mut()
                    .find(|parts| parts.text.is_none() && near(parts))
                {
                    Some(parts) => parts.text = Some((handle, text)),
                    None => rooms.push(FattoParts {
                        room,
                        outline: None,
                        text: Some((handle, text)),
                    }),
                }
            }
            _ => {}
        }
    }
    rooms
}

/// `AL_AGGIORNA` (spec §4.2): ogni locale di fatto si rimisura sui muri
/// attuali attorno al suo seme, alla tolleranza dei locali `tol`
/// (`rooms::measure_at`). Se il locale c'è, il contorno prende i vertici
/// nuovi e il testo l'etichetta nuova, solo dove cambiano: il contorno si
/// confronta alla tolleranza di base, e uno uguale partendo da un altro
/// vertice non cambia. Ogni locale cambiato porta nome, area di prima (letta
/// dall'etichetta di prima, R60) e area nuova ([`RoomChange::notice`], R58). Se non è più chiuso,
/// [`Notice::LocaleNonChiuso`] e niente cambia. Un locale senza contorno o
/// senza testo lo dice ([`Notice::ContornoMancante`],
/// [`Notice::TestoMancante`]) e aggiorna la parte che ha. Se la regione di un
/// locale contiene il seme di un altro (un vuoto li ha uniti), nessuno dei
/// due cambia: [`Notice::LocaleFuso`] e [`Notice::LocaliFusiLasciati`], una
/// volta per gruppo (R54, R55). Per gli altri, gli avvisi di qualità
/// (`rooms::quality_notices`). In testa,
/// [`Notice::FuoriPiano`] se ci sono muri fuori dal piano XY. Senza locali di
/// fatto non c'è niente da dire. Il disegno non si tocca: le modifiche le
/// applica chi chiama ([`RoomEdit::apply_to`]).
pub fn refresh_rooms(doc: &CadDocument, unit: DrawingUnit, tol: f64) -> RoomsRefresh {
    let mut out = RoomsRefresh::default();
    let rooms = fatto_parts(doc, tol);
    if rooms.is_empty() {
        return out;
    }
    let base = unit.tolerance();
    // R67: le parti dei locali (anche spostate su un layer non `AL-*`) non
    // sono muri.
    let walls = wall_set_without_rooms(doc, base);
    let faces = wall_faces(&walls.sources, tol);
    // R47: le facce alla tolleranza di base, una volta per tutti i locali.
    let base_faces = (tol > base * (1.0 + 1.0e-9)).then(|| wall_faces(&walls.sources, base));
    if walls.off_plane > 0 {
        out.notices.push(Notice::FuoriPiano {
            count: walls.off_plane,
        });
    }
    let found: Vec<Option<Measured>> = rooms
        .iter()
        .map(|parts| {
            measure_at(
                &faces,
                base_faces.as_deref(),
                &walls.sources,
                parts.room.seed,
                unit,
                tol,
            )
        })
        .collect();
    // R54/R55: i locali la cui regione contiene il seme di un altro fanno un
    // gruppo (anche a catena), che resta com'è.
    let mut group: Vec<usize> = (0..rooms.len()).collect();
    for (index, measured) in found.iter().enumerate() {
        let Some(measured) = measured else {
            continue;
        };
        for other in 0..rooms.len() {
            if other != index && region_holds(&measured.region, rooms[other].room.seed, tol) {
                let (keep, drop) = (group[index], group[other]);
                for label in &mut group {
                    if *label == drop {
                        *label = keep;
                    }
                }
            }
        }
    }
    let mut reported: Vec<usize> = Vec::new();
    for (index, parts) in rooms.iter().enumerate() {
        let name = &parts.room.name;
        if parts.outline.is_none() {
            out.notices
                .push(Notice::ContornoMancante { name: name.clone() });
        }
        if parts.text.is_none() {
            out.notices
                .push(Notice::TestoMancante { name: name.clone() });
        }
        let members: Vec<usize> = (0..rooms.len())
            .filter(|&other| group[other] == group[index])
            .collect();
        if members.len() > 1 {
            // Un vuoto ha unito due locali: l'area unita sostituirebbe dati
            // giusti. Niente cambia, e lo si dice una volta per gruppo.
            if !reported.contains(&group[index]) {
                reported.push(group[index]);
                let names: Vec<String> = members
                    .iter()
                    .map(|&member| rooms[member].room.name.clone())
                    .collect();
                if let Some(area_m2) = members
                    .iter()
                    .find_map(|&member| found[member].as_ref().map(|m| m.area_m2))
                {
                    out.notices.push(Notice::LocaleFuso {
                        names: names.clone(),
                        area_m2,
                    });
                }
                out.notices.push(Notice::LocaliFusiLasciati { names });
            }
            continue;
        }
        let Some(measured) = &found[index] else {
            out.notices
                .push(Notice::LocaleNonChiuso { name: name.clone() });
            continue;
        };
        out.notices.extend(quality_notices(
            name,
            measured.exact,
            measured.by_tolerance,
            measured.islands,
        ));
        let mut edits = Vec::new();
        if let Some((handle, old)) = parts.outline {
            let computed = Computed {
                name: name.clone(),
                number: parts.room.number.clone(),
                area_m2: measured.area_m2,
                region: measured.region.clone(),
                confirm: false,
                exact: measured.exact,
                islands: measured.islands,
                seed: parts.room.seed,
            };
            let fresh = room_entities(&computed, Phase::Fatto, &walls.sources, 0.0, tol);
            if let Some(EntityType::LwPolyline(new)) = fresh.into_iter().next() {
                // R59: alla tolleranza di base, non a `HPGAPTOL`: il testo si
                // confronta per intero e il contorno deve seguirlo.
                if !same_outline(old, &new, base) {
                    edits.push((
                        handle,
                        RoomEdit::Contorno {
                            vertices: new.vertices,
                            closed: new.is_closed,
                        },
                    ));
                }
            }
        }
        let mut label_changed = false;
        if let Some((handle, old)) = parts.text {
            let value = label(name, measured.area_m2, false);
            if old.value != value {
                label_changed = true;
                edits.push((handle, RoomEdit::Testo { value }));
            }
        }
        if !edits.is_empty() {
            out.changed.push(RoomChange {
                name: name.clone(),
                // R60: «prima» è ciò che il disegno diceva, cioè l'etichetta.
                old_area_m2: parts.text.and_then(|(_, old)| label_area_m2(&old.value)),
                new_area_m2: measured.area_m2,
                label_changed,
                edits,
            });
        }
    }
    out
}

/// L'area che dice un'etichetta di locale, in m²: il numero dopo l'ultimo `=`
/// e prima di ` m²` in fondo, con la virgola decimale, come la scrive
/// `rooms::label` (`A (da confermare)\PS = 23,70 m²`). Si legge dalla fine,
/// così il nome (con le sue barre e graffe, o un `=`) e la nota «da
/// confermare» non contano. `None` se il testo non finisce così.
fn label_area_m2(value: &str) -> Option<f64> {
    let (_, number) = value
        .trim_end()
        .strip_suffix("m²")?
        .trim_end()
        .rsplit_once('=')?;
    let number = number.trim();
    let digits = number.chars().filter(char::is_ascii_digit).count();
    let well_formed = digits > 0
        && number.matches(',').count() <= 1
        && number.chars().all(|c| c.is_ascii_digit() || c == ',');
    if !well_formed {
        return None;
    }
    number.replace(',', ".").parse::<f64>().ok()
}

/// I locali di fatto del disegno, uno per locale: contorno e testo accoppiati
/// per seme entro `tol`, come li accoppia [`refresh_rooms`]. Per i locali di
/// progetto dell'estrazione (`rooms::project_rooms`).
pub fn fatto_rooms(doc: &CadDocument, tol: f64) -> Vec<Room> {
    fatto_parts(doc, tol)
        .into_iter()
        .map(|parts| parts.room)
        .collect()
}

/// Il disegno ha locali di fatto (un contorno o un testo) nello spazio
/// modello?
pub fn has_fatto_rooms(doc: &CadDocument) -> bool {
    rooms_in(doc)
        .iter()
        .any(|(_, room, part)| room.phase == Phase::Fatto && *part != RoomPart::Seme)
}

/// Lo stesso contorno: stessa chiusura, stessi vertici entro `tol` e stessi
/// bulge, anche partendo da un altro vertice. Le larghezze non contano.
fn same_outline(old: &LwPolyline, new: &LwPolyline, tol: f64) -> bool {
    let n = new.vertices.len();
    if old.is_closed != new.is_closed || old.vertices.len() != n {
        return false;
    }
    let same = |a: &LwVertex, b: &LwVertex| {
        (a.location.x - b.location.x).hypot(a.location.y - b.location.y) <= tol
            && (a.bulge - b.bulge).abs() <= 1.0e-9
    };
    n == 0
        || (0..n)
            .any(|shift| (0..n).all(|k| same(&old.vertices[(k + shift) % n], &new.vertices[k])))
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

#[cfg(test)]
mod tests {
    //! Il ricalcolo di `AL_AGGIORNA` ([`refresh_rooms`]) sui casi di prova,
    //! senza l'app.
    use super::*;
    use crate::bim::fixtures::{
        case_a, move_to_layer, pilastro, shift_tramezzo, Form, LAYER_PILASTRI,
    };
    use crate::bim::room_command::test_support::*;
    use crate::bim::room_command::{room_snapshot, RoomCommand};
    use crate::command::CadCommand;
    use codec::entities::LwPolyline;
    use codec::Vector2;

    /// L'handle della `LINE` da `a` a `b`.
    fn line_handle(doc: &CadDocument, a: [f64; 2], b: [f64; 2]) -> Handle {
        doc.entities()
            .find(|e| matches!(e, EntityType::Line(l) if near(l.start, a) && near(l.end, b)))
            .map(|e| e.common().handle)
            .expect("linea del rilievo")
    }

    #[test]
    fn refresh_rooms_edits_only_what_changes() {
        let mut scene = scene_of(case_a(Form::Continua, M));
        assert!(!has_fatto_rooms(&scene.document));
        assert_eq!(
            refresh_rooms(&scene.document, M, TOL),
            RoomsRefresh::default()
        );
        let (outline, text) = write_room(&mut scene, "A", [2.0, 1.5]);
        assert!(has_fatto_rooms(&scene.document));
        assert_eq!(
            refresh_rooms(&scene.document, M, TOL),
            RoomsRefresh::default(),
            "niente da cambiare"
        );

        // Il tramezzo da x = 4,00 a x = 3,90.
        let east = line_handle(&scene.document, [4.0, 0.0], [4.0, 3.0]);
        if let Some(EntityType::Line(line)) = scene.document.get_entity_mut(east) {
            line.start.x = 3.9;
            line.end.x = 3.9;
        }
        let refresh = refresh_rooms(&scene.document, M, TOL);
        assert!(refresh.notices.is_empty(), "{:?}", refresh.notices);
        let [change] = &refresh.changed[..] else {
            panic!("{:?}", refresh.changed)
        };
        // R58: quale locale, da quanto a quanto (il prima dall'etichetta).
        assert_eq!(change.name, "A");
        assert!((change.old_area_m2.expect("etichetta") - 12.0).abs() < 1e-9);
        assert!((change.new_area_m2 - 11.7).abs() < 1e-9);
        assert_eq!(change.notice().text(), "Locale A: 12,00 → 11,70 m².");
        let edits = &change.edits;
        assert_eq!(edits.len(), 2, "{edits:?}");
        assert_eq!(edits[0].0, outline);
        let RoomEdit::Contorno { vertices, closed } = &edits[0].1 else {
            panic!("{:?}", edits[0])
        };
        assert!(*closed);
        assert!((polyline_area(vertices) - 11.7).abs() < 1e-9);
        assert_eq!(
            edits[1],
            (
                text,
                RoomEdit::Testo {
                    value: "A\\PS = 11,70 m²".into()
                }
            )
        );
        // Applicate, non resta niente.
        for (handle, edit) in edits {
            edit.apply_to(scene.document.get_entity_mut(*handle).unwrap());
        }
        assert_eq!(
            refresh_rooms(&scene.document, M, TOL),
            RoomsRefresh::default()
        );

        // Senza la faccia sud di A il locale non è più chiuso: niente cambia.
        let south = line_handle(&scene.document, [0.0, 0.0], [4.0, 0.0]);
        scene.erase_entities(&[south]);
        let refresh = refresh_rooms(&scene.document, M, TOL);
        assert!(refresh.changed.is_empty(), "{:?}", refresh.changed);
        assert_eq!(
            refresh.notices,
            [Notice::LocaleNonChiuso { name: "A".into() }]
        );
    }

    #[test]
    fn refresh_rooms_pairs_by_seed_and_reports_lone_parts() {
        let mut scene = scene_of(case_a(Form::Continua, M));
        // Le parti di A e B in ordine sparso: contorno di A, di B, testo di
        // B, di A.
        let a = room_of(&scene, "A", [2.0, 1.5]);
        let b = room_of(&scene, "B", [6.0, 1.5]);
        let a_outline = scene.add_entity(a[0].clone());
        scene.add_entity(b[0].clone());
        let b_text = scene.add_entity(b[1].clone());
        let a_text = scene.add_entity(a[1].clone());
        assert_eq!(
            refresh_rooms(&scene.document, M, TOL),
            RoomsRefresh::default(),
            "accoppiati per seme"
        );

        // Senza il contorno di A e il testo di B, con il testo di A vecchio.
        scene.erase_entities(&[a_outline, b_text]);
        if let Some(EntityType::MText(text)) = scene.document.get_entity_mut(a_text) {
            text.value = "A\\PS = 99,00 m²".into();
        }
        let refresh = refresh_rooms(&scene.document, M, TOL);
        assert_eq!(
            refresh.notices,
            [
                Notice::TestoMancante { name: "B".into() },
                Notice::ContornoMancante { name: "A".into() },
            ]
        );
        let [change] = &refresh.changed[..] else {
            panic!("{:?}", refresh.changed)
        };
        assert_eq!(
            change.edits,
            [(
                a_text,
                RoomEdit::Testo {
                    value: "A\\PS = 12,00 m²".into()
                }
            )]
        );
        // R60: l'area di prima è quella che l'etichetta diceva.
        assert_eq!(change.old_area_m2, Some(99.0));
        assert_eq!(change.notice().text(), "Locale A: 99,00 → 12,00 m².");
    }

    /// R59: il contorno si confronta alla tolleranza di base anche con
    /// `HPGAPTOL`: un muro spostato di 4 mm con la tolleranza a 5 mm cambia
    /// testo e contorno insieme.
    #[test]
    fn refresh_rooms_compares_outlines_at_the_base_tolerance() {
        const GAP: f64 = 0.005;
        let mut scene = scene_of(case_a(Form::ChiusaPerMuro, M));
        let mut c = RoomCommand::new(room_snapshot(&scene.document, M, GAP).0);
        need(c.on_point(pt(2.0, 1.5)));
        need_text(c.on_text_input("A"));
        let handles: Vec<Handle> = committed(c.on_enter())
            .into_iter()
            .map(|entity| scene.add_entity(entity))
            .collect();
        assert_eq!(
            refresh_rooms(&scene.document, M, GAP),
            RoomsRefresh::default()
        );
        // Tutto il tramezzo 4 mm più a est.
        let tramezzo = scene
            .document
            .entities()
            .find(|entity| is_tramezzo(entity))
            .map(|entity| entity.common().handle)
            .expect("tramezzo");
        if let Some(EntityType::LwPolyline(polyline)) = scene.document.get_entity_mut(tramezzo) {
            for vertex in &mut polyline.vertices {
                vertex.location.x += 0.004;
            }
        }
        let refresh = refresh_rooms(&scene.document, M, GAP);
        let [change] = &refresh.changed[..] else {
            panic!("{:?}", refresh.changed)
        };
        assert_eq!(change.notice().text(), "Locale A: 12,00 → 12,01 m².");
        let [(outline, RoomEdit::Contorno { vertices, .. }), (text, RoomEdit::Testo { value })] =
            &change.edits[..]
        else {
            panic!("contorno e testo: {:?}", change.edits)
        };
        assert_eq!((*outline, *text), (handles[0], handles[1]));
        assert_eq!(value, "A\\PS = 12,01 m²");
        assert!(vertices.iter().any(|v| (v.location.x - 4.004).abs() < 1e-9));
    }

    /// R54 e R55 in `AL_AGGIORNA`: una volta per gruppo, e i locali fusi
    /// restano com'erano (l'area unita sostituirebbe dati giusti).
    #[test]
    fn refresh_rooms_reports_fused_rooms_once() {
        let mut scene = scene_of(case_a(Form::ChiusaPerMuro, M));
        write_room(&mut scene, "A", [2.0, 1.5]);
        write_room(&mut scene, "B", [6.0, 1.5]);
        assert_eq!(
            refresh_rooms(&scene.document, M, TOL),
            RoomsRefresh::default()
        );
        let tramezzo = scene
            .document
            .entities()
            .find(|entity| is_tramezzo(entity))
            .map(|entity| entity.common().handle)
            .expect("tramezzo");
        if let Some(EntityType::LwPolyline(polyline)) = scene.document.get_entity_mut(tramezzo) {
            open_tramezzo(polyline);
        }
        let refresh = refresh_rooms(&scene.document, M, TOL);
        let [Notice::LocaleFuso { names, area_m2 }, Notice::LocaliFusiLasciati { names: left }] =
            &refresh.notices[..]
        else {
            panic!("{:?}", refresh.notices)
        };
        assert_eq!(names, &["A", "B"]);
        assert_eq!(left, &["A", "B"]);
        assert!((area_m2 - 23.7002).abs() < 1e-9, "{area_m2}");
        assert!(refresh.changed.is_empty(), "{:?}", refresh.changed);

        // Un secondo A scritto dopo il vuoto, sullo stesso seme: un gruppo
        // solo di tre, un avviso solo, niente cambia.
        write_room(&mut scene, "A", [2.0, 1.5]);
        let refresh = refresh_rooms(&scene.document, M, TOL);
        let fused: Vec<&Notice> = refresh
            .notices
            .iter()
            .filter(|n| matches!(n, Notice::LocaleFuso { .. }))
            .collect();
        assert_eq!(fused.len(), 1, "{:?}", refresh.notices);
        let Notice::LocaleFuso { names, .. } = fused[0] else {
            unreachable!()
        };
        assert_eq!(names, &["A", "B", "A"]);
        assert!(refresh.changed.is_empty(), "{:?}", refresh.changed);
    }

    #[test]
    fn outline_rules() {
        // Lo stesso contorno da un altro vertice non è una modifica.
        let polyline = |ring: &[[f64; 2]], closed: bool| {
            let mut p = LwPolyline::new();
            for &[x, y] in ring {
                p.add_vertex(LwVertex::new(Vector2::new(x, y)));
            }
            p.is_closed = closed;
            p
        };
        let ring = rect(0.0, 0.0, 4.0, 3.0);
        let mut rotated = ring.clone();
        rotated.rotate_left(1);
        let a = polyline(&ring, true);
        assert!(same_outline(&a, &polyline(&rotated, true), TOL));
        let nudged = [[0.0, 5e-7], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]];
        assert!(same_outline(&a, &polyline(&nudged, true), TOL), "entro tol");
        assert!(!same_outline(
            &a,
            &polyline(&rect(0.0, 0.0, 3.9, 3.0), true),
            TOL
        ));
        assert!(!same_outline(&a, &polyline(&ring, false), TOL));
        assert!(!same_outline(&a, &polyline(&ring[..3], true), TOL));
        let mut arc = polyline(&ring, true);
        arc.vertices[1].bulge = 0.2;
        assert!(!same_outline(&a, &arc, TOL));
    }

    /// La faccia est di A (la `LINE` del caso a linee, o il lato ovest del
    /// tramezzo a polilinea) portata a `x`.
    fn move_east_face_of_a(doc: &mut CadDocument, x: f64) {
        let handles: Vec<Handle> = doc
            .entities()
            .filter(|e| {
                is_tramezzo(e)
                    || matches!(e, EntityType::Line(l)
                        if near(l.start, [4.0, 0.0]) && near(l.end, [4.0, 3.0]))
            })
            .map(|e| e.common().handle)
            .collect();
        assert_eq!(handles.len(), 1, "una faccia est di A");
        match doc.get_entity_mut(handles[0]) {
            Some(EntityType::Line(line)) => {
                line.start.x = x;
                line.end.x = x;
            }
            Some(EntityType::LwPolyline(polyline)) => {
                for vertex in &mut polyline.vertices {
                    if (vertex.location.x - 4.0).abs() < 1e-9 {
                        vertex.location.x = x;
                    }
                }
            }
            other => panic!("{other:?}"),
        }
    }

    /// La riga dell'unico locale cambiato, e le sue modifiche.
    #[track_caller]
    fn only_change(doc: &CadDocument) -> (String, Vec<(Handle, RoomEdit)>) {
        let refresh = refresh_rooms(doc, M, TOL);
        let [change] = &refresh.changed[..] else {
            panic!("{:?}", refresh.changed)
        };
        (change.notice().text(), change.edits.clone())
    }

    /// R60: con un'isola l'etichetta dice l'area netta, mentre il contorno è
    /// solo l'anello esterno (lordo). La riga di `AL_AGGIORNA` legge «prima»
    /// dall'etichetta, mai dal contorno.
    #[test]
    fn refresh_line_reads_the_old_area_from_the_label() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let with_pillar = || {
                let mut entities = case_a(form, M);
                entities.push(pilastro(M));
                let mut scene = scene_of(entities);
                let (_, text) = write_room(&mut scene, "A", [2.0, 1.5]);
                let Some(EntityType::MText(label)) = scene.document.get_entity(text) else {
                    panic!("testo")
                };
                assert_eq!(label.value, "A\\PS = 11,91 m²", "{form:?}");
                (scene, text)
            };

            // Il tramezzo da x = 4,00 a 3,90: 11,91 → 11,61.
            let (mut scene, _) = with_pillar();
            move_east_face_of_a(&mut scene.document, 3.9);
            let (line, _) = only_change(&scene.document);
            assert_eq!(line, "Locale A: 11,91 → 11,61 m².", "{form:?}");

            // Il layer del pilastro spento: l'isola sparisce, 11,91 → 12,00.
            let (mut scene, _) = with_pillar();
            scene
                .document
                .layers
                .get_mut(LAYER_PILASTRI)
                .expect("layer del pilastro")
                .turn_off();
            let (line, edits) = only_change(&scene.document);
            assert_eq!(line, "Locale A: 11,91 → 12,00 m².", "{form:?}");
            assert!(
                matches!(&edits[..], [(_, RoomEdit::Testo { .. })]),
                "il contorno esterno è lo stesso: {edits:?}"
            );

            // Un'etichetta che non dice l'area: solo quella nuova.
            let (mut scene, text) = with_pillar();
            if let Some(EntityType::MText(label)) = scene.document.get_entity_mut(text) {
                label.value = "A".into();
            }
            move_east_face_of_a(&mut scene.document, 3.9);
            let (line, _) = only_change(&scene.document);
            assert_eq!(line, "Locale A: aggiornato a 11,61 m².", "{form:?}");

            // 1 mm: il contorno cambia, l'etichetta no (11,907 → «11,91»).
            let (mut scene, _) = with_pillar();
            move_east_face_of_a(&mut scene.document, 3.999);
            let (line, edits) = only_change(&scene.document);
            assert_eq!(
                line, "Locale A: contorno aggiornato (11,91 m²).",
                "{form:?}"
            );
            assert!(
                matches!(&edits[..], [(_, RoomEdit::Contorno { .. })]),
                "solo il contorno: {edits:?}"
            );
        }
    }

    /// R67: le parti dei locali non sono mai muri, su qualunque layer.
    /// Contorno e testo di A spostati a mano sul layer LOCALI, poi il rilievo
    /// corretto (A da 12,00 a 12,60 m²): `AL_AGGIORNA` porta A all'area nuova,
    /// nelle due forme. Prima il contorno vecchio faceva da muro e la faccia
    /// attorno al seme restava 12,00, «esatta», senza righe né avvisi.
    #[test]
    fn moved_room_outline_is_not_a_wall() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let mut scene = scene_of(case_a(form, M));
            let (outline, text) = write_room(&mut scene, "A", [2.0, 1.5]);
            for handle in [outline, text] {
                move_to_layer(&mut scene.document, handle, "LOCALI");
            }
            assert_eq!(
                refresh_rooms(&scene.document, M, TOL),
                RoomsRefresh::default(),
                "{form:?}: niente da cambiare"
            );

            shift_tramezzo(&mut scene.document, M, 0.2);
            let refresh = refresh_rooms(&scene.document, M, TOL);
            assert!(
                refresh.notices.is_empty(),
                "{form:?}: {:?}",
                refresh.notices
            );
            let [change] = &refresh.changed[..] else {
                panic!("{form:?}: {:?}", refresh.changed)
            };
            assert_eq!(
                change.notice().text(),
                "Locale A: 12,00 → 12,60 m².",
                "{form:?}"
            );
            let [(h0, RoomEdit::Contorno { vertices, closed }), (h1, RoomEdit::Testo { value })] =
                &change.edits[..]
            else {
                panic!("{form:?}: contorno e testo: {:?}", change.edits)
            };
            assert_eq!((*h0, *h1), (outline, text), "{form:?}: stessi handle");
            assert!(*closed);
            assert!((polyline_area(vertices) - 12.6).abs() < 1e-9, "{form:?}");
            assert_eq!(value, "A\\PS = 12,60 m²", "{form:?}");

            // Applicate, non resta niente (il contorno, sempre su LOCALI, ora
            // coincide con le facce).
            for (handle, edit) in &change.edits {
                edit.apply_to(scene.document.get_entity_mut(*handle).unwrap());
            }
            assert_eq!(
                refresh_rooms(&scene.document, M, TOL),
                RoomsRefresh::default(),
                "{form:?}: dopo"
            );
        }
    }

    #[test]
    fn label_area_reads_what_label_wrote() {
        for (value, want) in [
            ("A\\PS = 11,91 m²", Some(11.91)),
            ("A (da confermare)\\PS = 23,70 m²", Some(23.7)),
            ("WC\\\\Doccia \\{2\\}\\PS = 3,05 m²", Some(3.05)),
            ("Sala = grande\\PS = 1234,50 m²", Some(1234.5)),
            ("A\\PS = 12 m²", Some(12.0)),
            ("A", None),
            ("A\\PS = dodici m²", None),
            ("A\\PS = 12,00", None),
            ("", None),
        ] {
            assert_eq!(label_area_m2(value), want, "{value:?}");
        }
        assert_eq!(label_area_m2(&label("A", 11.905, true)), Some(11.91));
    }
}
