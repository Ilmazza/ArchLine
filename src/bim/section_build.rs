//! Le sezioni di un disegno (spec parte 4 §5.1): dai tagli ai tre disegni, per
//! ogni taglio, dello stato di fatto, delle demolizioni e costruzioni e dello
//! stato di progetto. Logica pura: legge il documento, non scrive nulla.
//!
//! - **fatto**: i muri del rilievo con le soglie delle porte esistenti o da
//!   demolire, i locali di fatto con le loro altezze (`quantities_rooms`),
//!   le aperture esistenti e quelle da demolire;
//! - **progetto**: la geometria di progetto (`project::project_geometry`), i
//!   locali di progetto, le aperture esistenti e nuove;
//! - **demolizioni e costruzioni** (R115): il confronto dei due profili lungo
//!   lo stesso taglio. Dove c'è massa muraria nel fatto e nel progetto è
//!   `Plain`; solo nel fatto è demolita (`Tint::Demolished`, giallo, con
//!   l'altezza del fatto); solo nel progetto è nuova (`Tint::New`, rosso, con
//!   l'altezza del progetto). I locali sono quelli di progetto (il soffitto).
//!
//! Il taglio si legge da sinistra a destra come lo vede chi guarda
//! ([`Cut::oriented`]). Ogni esclusione (massa o locale senza altezza, vano
//! senza dati) è un avviso con il nome del taglio, senza doppioni.

use codec::{Document, EntityType};
use rustc_hash::FxHashMap;

use super::aperture::{apertures_in, ApertureGeom, ApertureState};
use super::cut::CutGeom;
use super::extract::push_unique;
use super::height::Height;
use super::project::project_geometry;
use super::quantities_rooms::{fatto_room_rows, project_room_rows, RoomRow};
use super::record;
use super::report::Notice;
use super::elevation::{draw_elevation, elevation, ElevationAperture};
use super::section::{elements, profile, Element, SectionAperture, Span};
use super::section_draw::{draw_section, Tint};
use super::units::DrawingUnit;
use super::walls::{faces, sources_of, wall_set_without_rooms};
use super::zone::zones_in;

/// Altezza dei testi del disegno, in metri.
const TEXT_HEIGHT_M: f64 = 0.25;

/// Quale dei tre disegni di un taglio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionPhase {
    Fatto,
    Comparativa,
    Progetto,
}

impl SectionPhase {
    pub fn key(self) -> &'static str {
        match self {
            Self::Fatto => "fatto",
            Self::Comparativa => "demolizioni-costruzioni",
            Self::Progetto => "progetto",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Fatto => "stato di fatto",
            Self::Comparativa => "demolizioni e costruzioni",
            Self::Progetto => "stato di progetto",
        }
    }
}

/// Unità e tolleranze del calcolo, come per le quantità.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionOptions {
    pub unit: DrawingUnit,
    pub tol: f64,
    pub room_tol: f64,
}

/// Che cosa disegna un [`SectionDrawing`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawingKind {
    Sezione,
    Prospetto,
}

impl DrawingKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Sezione => "sezione",
            Self::Prospetto => "prospetto",
        }
    }

    pub fn from_key(text: &str) -> Option<Self> {
        match text {
            "sezione" => Some(Self::Sezione),
            "prospetto" => Some(Self::Prospetto),
            _ => None,
        }
    }
}

/// Un disegno di sezione o di prospetto, con le entità già a partire da
/// `(0; 0)`. `cut_name` è il nome del taglio o del prospetto.
#[derive(Clone, Debug)]
pub struct SectionDrawing {
    pub kind: DrawingKind,
    pub cut_name: String,
    pub phase: SectionPhase,
    pub entities: Vec<EntityType>,
}

#[derive(Clone, Debug, Default)]
pub struct SectionBuild {
    pub drawings: Vec<SectionDrawing>,
    pub notices: Vec<Notice>,
}

/// La pianta di una fase: facce dei muri, contorni dei locali con nome e
/// altezza, aperture viste dal taglio.
struct Plan {
    faces: Vec<Vec<[f64; 2]>>,
    outlines: Vec<Vec<[f64; 2]>>,
    rooms: Vec<(String, Height)>,
    apertures: Vec<SectionAperture>,
}

/// I codici delle aperture come nell'abaco: `P1`, `F1`, `PF1`, per tipo e
/// nell'ordine degli handle.
fn codes_of(found: &[ApertureGeom]) -> Vec<(usize, String)> {
    let mut order: Vec<usize> = (0..found.len()).collect();
    let rank = |kind| match kind {
        super::aperture::ApertureKind::Porta => 0,
        super::aperture::ApertureKind::Finestra => 1,
        super::aperture::ApertureKind::Portafinestra => 2,
    };
    order.sort_by_key(|&i| (rank(found[i].aperture.kind), found[i].handle.value()));
    let mut counts = [0_usize; 3];
    order
        .into_iter()
        .map(|i| {
            let kind = found[i].aperture.kind;
            let count = &mut counts[rank(kind)];
            *count += 1;
            (i, format!("{}{count}", kind.code_prefix()))
        })
        .collect()
}

fn apertures_for(
    found: &[ApertureGeom],
    keep: impl Fn(ApertureState) -> bool,
    per_metre: f64,
) -> Vec<SectionAperture> {
    codes_of(found)
        .into_iter()
        .filter(|(i, _)| keep(found[*i].aperture.state))
        .map(|(i, code)| {
            let geom = &found[i];
            SectionAperture {
                code,
                kind: geom.aperture.kind,
                ring: geom.ring.clone(),
                height_m: geom.aperture.height.map(|h| h / per_metre),
                sill_m: geom.aperture.sill.map(|s| s / per_metre),
            }
        })
        .collect()
}

fn rooms_of(rows: &[&RoomRow]) -> (Vec<Vec<[f64; 2]>>, Vec<(String, Height)>) {
    rows.iter()
        .filter(|row| row.outline.len() >= 3)
        .map(|row| (row.outline.clone(), (row.name.clone(), row.height.clone())))
        .unzip()
}

fn plans(doc: &Document, opts: &SectionOptions) -> (Plan, Plan) {
    let per_metre = opts.unit.per_metre();
    let default_height = record::read(doc).default_height;
    let found = apertures_in(doc);
    let walls = wall_set_without_rooms(doc, opts.tol);
    let mut survey = walls.sources.clone();
    super::aperture::add_fatto_thresholds(doc, &mut survey, opts.tol);
    let (fatto_rows, _) = fatto_room_rows(doc, opts.unit, opts.room_tol, default_height);
    let (project_rows, _) = project_room_rows(
        doc,
        opts.unit,
        opts.tol,
        opts.room_tol,
        default_height,
        &fatto_rows,
    );
    let (outlines_f, rooms_f) = rooms_of(&fatto_rows.iter().collect::<Vec<_>>());
    let (outlines_p, rooms_p) = rooms_of(&project_rows.iter().map(|p| &p.row).collect::<Vec<_>>());
    let zones = zones_in(doc);
    let pg = project_geometry(doc, &walls, &zones, "0", opts.tol);
    let (pg_sources, _): (FxHashMap<_, _>, usize) = sources_of(&pg.room_entities, opts.tol);
    let fatto = Plan {
        faces: faces(&survey, opts.tol),
        outlines: outlines_f,
        rooms: rooms_f,
        apertures: apertures_for(
            &found,
            |s| matches!(s, ApertureState::Esistente | ApertureState::Demolita),
            per_metre,
        ),
    };
    let progetto = Plan {
        faces: faces(&pg_sources, opts.tol),
        outlines: outlines_p,
        rooms: rooms_p,
        apertures: apertures_for(
            &found,
            |s| matches!(s, ApertureState::Esistente | ApertureState::Nuova),
            per_metre,
        ),
    };
    (fatto, progetto)
}

fn section_of(
    plan: &Plan,
    from: [f64; 2],
    to: [f64; 2],
    opts: &SectionOptions,
) -> (Vec<Element>, Vec<Notice>) {
    let spans = profile(&plan.faces, &plan.outlines, from, to, opts.tol);
    elements(
        &spans,
        &plan.rooms,
        &plan.apertures,
        from,
        to,
        opts.unit.per_metre(),
        opts.tol,
    )
}

/// L'elemento che copre il punto `d` (distanza dal primo punto del taglio).
fn covering(list: &[Element], d: f64) -> Option<&Element> {
    list.iter().find(|e| {
        let s = e.span();
        d >= s.from && d <= s.to
    })
}

/// `element` ristretto a `[a, b]`: lo stesso tipo e la stessa altezza, i vani
/// tagliati al nuovo tratto (quelli più corti di `tol` spariscono).
fn clipped(element: &Element, a: f64, b: f64, tol: f64) -> Element {
    match element {
        Element::Room { index, height, span } => Element::Room {
            span: Span { from: a, to: b, kind: span.kind },
            index: *index,
            height: height.clone(),
        },
        Element::Mass { height, holes, span } => Element::Mass {
            span: Span { from: a, to: b, kind: span.kind },
            height: height.clone(),
            holes: holes
                .iter()
                .filter_map(|hole| {
                    let (from, to) = (hole.from.max(a), hole.to.min(b));
                    (to - from > tol).then(|| super::section::Hole {
                        from,
                        to,
                        ..hole.clone()
                    })
                })
                .collect(),
        },
    }
}

/// Il disegno delle demolizioni e costruzioni: i due profili confrontati
/// lungo lo stesso taglio (R115).
fn comparison(
    fatto: &[Element],
    progetto: &[Element],
    tol: f64,
) -> Vec<(Element, Tint)> {
    let mut cuts: Vec<f64> = fatto
        .iter()
        .chain(progetto)
        .flat_map(|e| [e.span().from, e.span().to])
        .collect();
    cuts.sort_by(f64::total_cmp);
    let mut points: Vec<f64> = Vec::new();
    for d in cuts {
        if points.last().map_or(true, |&last| d - last > tol) {
            points.push(d);
        }
    }
    let mut out: Vec<(Element, Tint)> = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let mid = (a + b) / 2.0;
        let (in_fatto, in_progetto) = (covering(fatto, mid), covering(progetto, mid));
        let piece = match (in_fatto, in_progetto) {
            (Some(Element::Mass { .. }), Some(p @ Element::Mass { .. })) => {
                Some((clipped(p, a, b, tol), Tint::Plain))
            }
            (Some(f @ Element::Mass { .. }), _) => Some((clipped(f, a, b, tol), Tint::Demolished)),
            (_, Some(p @ Element::Mass { .. })) => Some((clipped(p, a, b, tol), Tint::New)),
            (_, Some(p @ Element::Room { .. })) => Some((clipped(p, a, b, tol), Tint::Plain)),
            _ => None,
        };
        let Some((piece, tint)) = piece else { continue };
        // Pezzi contigui dello stesso tipo, tinta e altezza, senza vani, si
        // uniscono: la campitura non deve avere cuciture.
        if let Some((Element::Mass { span, height, holes }, last_tint)) = out.last_mut() {
            if let Element::Mass { height: h2, holes: holes2, span: s2 } = &piece {
                if *last_tint == tint
                    && holes.is_empty()
                    && holes2.is_empty()
                    && height.value_m == h2.value_m
                    && (s2.from - span.to).abs() <= tol
                {
                    span.to = s2.to;
                    continue;
                }
            }
        }
        out.push((piece, tint));
    }
    out
}

/// Le sezioni di tutti i tagli, nell'ordine dei tagli e per ognuno fatto,
/// demolizioni e costruzioni, progetto. Un taglio senza elementi disegnabili
/// non dà il disegno di quella fase.
pub fn build_sections(doc: &Document, cuts: &[CutGeom], opts: &SectionOptions) -> SectionBuild {
    let mut build = SectionBuild::default();
    if cuts.is_empty() {
        return build;
    }
    let per_metre = opts.unit.per_metre();
    let text_height = TEXT_HEIGHT_M * per_metre;
    let (fatto, progetto) = plans(doc, opts);
    for geom in cuts {
        let name = &geom.cut.name;
        let (from, to) = geom.cut.oriented();
        let (elements_f, notices_f) = section_of(&fatto, from, to, opts);
        let (elements_p, notices_p) = section_of(&progetto, from, to, opts);
        for notice in notices_f.into_iter().chain(notices_p) {
            let notice = match notice {
                Notice::SezioneEsclusa { what, reason } => Notice::SezioneEsclusa {
                    what: format!("{what} (taglio {name})"),
                    reason,
                },
                other => other,
            };
            push_unique(&mut build.notices, vec![notice]);
        }
        let plain = |list: &[Element]| -> Vec<(Element, Tint)> {
            list.iter().cloned().map(|e| (e, Tint::Plain)).collect()
        };
        let drawings = [
            (SectionPhase::Fatto, plain(&elements_f)),
            (
                SectionPhase::Comparativa,
                comparison(&elements_f, &elements_p, opts.tol),
            ),
            (SectionPhase::Progetto, plain(&elements_p)),
        ];
        for (phase, list) in drawings {
            let title = format!("Sezione {name} - {}", phase.label());
            let entities = draw_section(&title, &list, [0.0, 0.0], per_metre, text_height);
            if !entities.is_empty() {
                build.drawings.push(SectionDrawing {
                    kind: DrawingKind::Sezione,
                    cut_name: name.clone(),
                    phase,
                    entities,
                });
            }
        }
    }
    build
}

fn elevation_apertures(
    found: &[ApertureGeom],
    keep: impl Fn(ApertureState) -> bool,
    per_metre: f64,
) -> Vec<ElevationAperture> {
    codes_of(found)
        .into_iter()
        .filter(|(i, _)| keep(found[*i].aperture.state))
        .map(|(i, code)| {
            let a = &found[i].aperture;
            ElevationAperture {
                code,
                kind: a.kind,
                state: a.state,
                axis: a.axis,
                thickness: a.thickness,
                height_m: a.height.map(|h| h / per_metre),
                sill_m: a.sill.map(|s| s / per_metre),
            }
        })
        .collect()
}

/// I tre prospetti con normale uscente `normal` e nome `name` (R120): fatto
/// (facciate del rilievo, aperture esistenti e da demolire), progetto
/// (facciate di progetto, aperture esistenti e nuove) e demolizioni e
/// costruzioni (le facciate del fatto con **tutte** le aperture, le demolite in
/// giallo e le nuove in rosso). Un prospetto senza facciate disegnabili non dà
/// il disegno di quella fase.
pub fn build_elevations(
    doc: &Document,
    name: &str,
    normal: [f64; 2],
    opts: &SectionOptions,
) -> SectionBuild {
    let mut build = SectionBuild::default();
    let per_metre = opts.unit.per_metre();
    let text_height = TEXT_HEIGHT_M * per_metre;
    let (fatto, progetto) = plans(doc, opts);
    let found = apertures_in(doc);
    let sets = [
        (
            SectionPhase::Fatto,
            &fatto,
            elevation_apertures(
                &found,
                |s| matches!(s, ApertureState::Esistente | ApertureState::Demolita),
                per_metre,
            ),
        ),
        (
            SectionPhase::Comparativa,
            &fatto,
            elevation_apertures(&found, |_| true, per_metre),
        ),
        (
            SectionPhase::Progetto,
            &progetto,
            elevation_apertures(
                &found,
                |s| matches!(s, ApertureState::Esistente | ApertureState::Nuova),
                per_metre,
            ),
        ),
    ];
    for (phase, plan, apertures) in sets {
        let (result, notices) = elevation(
            &plan.faces,
            &plan.outlines,
            &plan.rooms,
            &apertures,
            normal,
            per_metre,
            opts.tol,
        );
        for notice in notices {
            let notice = match notice {
                Notice::SezioneEsclusa { what, reason } => Notice::SezioneEsclusa {
                    what: format!("{what} (prospetto {name})"),
                    reason,
                },
                other => other,
            };
            push_unique(&mut build.notices, vec![notice]);
        }
        let title = format!("Prospetto {name} - {}", phase.label());
        let entities = draw_elevation(&title, &result, [0.0, 0.0], per_metre, text_height);
        if !entities.is_empty() {
            build.drawings.push(SectionDrawing {
                kind: DrawingKind::Prospetto,
                cut_name: name.to_string(),
                phase,
                entities,
            });
        }
    }
    build
}

#[cfg(test)]
mod tests {
    use super::*;

    use codec::entities::Hatch;

    use crate::bim::aperture::{aperture_polyline, ensure_layer, Aperture, ApertureKind};
    use crate::bim::cut::Cut;
    use crate::bim::fixtures::{case_a_with_door, scene_with, Form};
    use crate::bim::rooms::{measure_at, room_entities, Computed, Phase};
    use crate::bim::zone::{ensure_layers, zone_hatch, Zone, ZoneOrigin, ZoneState};
    use crate::bim::walls::rect_from_axis;
    use crate::bim::MAX_WALL_THICKNESS_M;
    use crate::scene::Scene;
    use codec::Handle;

    const M: DrawingUnit = DrawingUnit::Metri;
    const TOL: f64 = 1e-6;
    const OPTS: SectionOptions = SectionOptions {
        unit: M,
        tol: TOL,
        room_tol: TOL,
    };

    fn door(state: ApertureState) -> Aperture {
        Aperture {
            kind: ApertureKind::Porta,
            state,
            axis: ([4.05, 1.0], [4.05, 1.9]),
            thickness: 0.10,
            height: Some(2.1),
            sill: None,
            threshold: true,
            lintel: false,
            bearing: None,
        }
    }

    /// Caso A con la porta esistente, i locali A e B e l'altezza 2,70.
    fn scene() -> Scene {
        let mut scene = scene_with(case_a_with_door(Form::Continua, M), M.insunits());
        ensure_layers(&mut scene.document);
        ensure_layer(&mut scene.document);
        scene.add_entity(aperture_polyline(&door(ApertureState::Esistente)).unwrap());
        let rec = record::ProjectRecord {
            default_height: Some(2.7),
            ..record::read(&scene.document)
        };
        record::write(&mut scene.document, &rec);
        for (name, seed) in [("A", [2.0, 1.5]), ("B", [6.0, 1.5])] {
            let walls = wall_set_without_rooms(&scene.document, TOL);
            let mut sources = walls.sources.clone();
            crate::bim::aperture::add_fatto_thresholds(&scene.document, &mut sources, TOL);
            let all = faces(&sources, TOL);
            let found = measure_at(&all, None, &sources, seed, M, TOL).expect("locale");
            let computed = Computed {
                name: name.into(),
                number: String::new(),
                area_m2: found.area_m2,
                region: found.region,
                confirm: false,
                exact: found.exact,
                islands: found.islands,
                seed,
            };
            for entity in room_entities(&computed, Phase::Fatto, &sources, 0.25, TOL) {
                scene.add_entity(entity);
            }
        }
        scene
    }

    fn demolish_tramezzo(scene: &mut Scene) {
        let walls = wall_set_without_rooms(&scene.document, TOL);
        let rect = rect_from_axis(
            &walls.sources,
            [4.05, 0.0],
            [4.05, 3.0],
            MAX_WALL_THICKNESS_M,
            TOL,
        )
        .expect("tramezzo");
        let zone = Zone {
            state: ZoneState::Demolito,
            origin: ZoneOrigin::MuroDuePunti,
            axis: Some(rect.axis),
            thickness: Some(rect.thickness),
            height: None,
            varco: false,
        };
        scene.add_entity(zone_hatch(&zone, &[rect.ring()]));
    }

    /// Il taglio orizzontale a y = 1,5 con il punto di vista a nord (a sinistra
    /// del verso est): da ovest a est.
    fn cut_east() -> CutGeom {
        CutGeom {
            handle: Handle::NULL,
            cut: Cut {
                name: "A-A".into(),
                from: [-1.0, 1.5],
                to: [9.0, 1.5],
                view: [4.0, 3.0],
            },
        }
    }

    fn hatch_colours(entities: &[EntityType]) -> Vec<codec::Color> {
        entities
            .iter()
            .filter_map(|e| match e {
                EntityType::Hatch(h) => Some((**h).clone()),
                _ => None,
            })
            .map(|h: Hatch| h.common.color)
            .collect()
    }

    /// Tre disegni per taglio (fatto, demolizioni e costruzioni, progetto),
    /// dal profilo del caso A con la porta: stesso disegno in fatto e in
    /// progetto, tutto `ByLayer`.
    #[test]
    fn a_cut_gives_three_drawings_and_fatto_equals_progetto_without_works() {
        let scene = scene();
        let build = build_sections(&scene.document, &[cut_east()], &OPTS);
        let phases: Vec<SectionPhase> = build.drawings.iter().map(|d| d.phase).collect();
        assert_eq!(
            phases,
            [
                SectionPhase::Fatto,
                SectionPhase::Comparativa,
                SectionPhase::Progetto
            ]
        );
        assert!(build.notices.is_empty(), "{:?}", build.notices);
        for drawing in &build.drawings {
            assert_eq!(drawing.cut_name, "A-A");
            assert!(
                hatch_colours(&drawing.entities)
                    .iter()
                    .all(|c| *c == codec::Color::ByLayer),
                "{:?}: senza lavori tutto ByLayer",
                drawing.phase
            );
        }
        let count = |d: &SectionDrawing| hatch_colours(&d.entities).len();
        // Muro ovest, muro est, e il tramezzo diviso dalla porta (solo sopra).
        assert_eq!(count(&build.drawings[0]), 3);
        assert_eq!(count(&build.drawings[2]), 3);
    }

    /// Demolito il tramezzo: nel fatto c'è, nel progetto no, nella
    /// comparativa è giallo; i locali A e B diventano uno.
    #[test]
    fn a_demolished_wall_is_yellow_in_the_comparison_and_gone_in_the_project() {
        let mut scene = scene();
        demolish_tramezzo(&mut scene);
        let build = build_sections(&scene.document, &[cut_east()], &OPTS);
        let by = |p| build.drawings.iter().find(|d| d.phase == p).unwrap();
        let yellow = codec::Color::from_index(2);
        let comparison = hatch_colours(&by(SectionPhase::Comparativa).entities);
        assert!(comparison.contains(&yellow), "{comparison:?}");
        assert!(!comparison.contains(&codec::Color::from_index(1)));
        assert!(
            !hatch_colours(&by(SectionPhase::Progetto).entities).contains(&yellow),
            "il progetto non è colorato"
        );
        assert!(
            hatch_colours(&by(SectionPhase::Fatto).entities).len()
                > hatch_colours(&by(SectionPhase::Progetto).entities).len() - 1,
            "il fatto ha il tramezzo"
        );
    }

    /// Il punto di vista dal lato opposto rovescia il disegno: l'ordine dei
    /// tratti si legge da est a ovest, ma la geometria del caso A è
    /// simmetrica per i muri d'ambito: cambiano le posizioni dei locali.
    #[test]
    fn the_view_side_flips_the_section() {
        let scene = scene();
        let mut cut = cut_east();
        let normal = build_sections(&scene.document, &[cut.clone()], &OPTS);
        cut.cut.view = [4.0, 0.0];
        let flipped = build_sections(&scene.document, &[cut], &OPTS);
        let first_x = |b: &SectionBuild| -> f64 {
            b.drawings[0]
                .entities
                .iter()
                .filter_map(|e| match e {
                    EntityType::LwPolyline(p) => {
                        Some(p.vertices.iter().map(|v| v.location.x).fold(f64::MAX, f64::min))
                    }
                    _ => None,
                })
                .fold(f64::MAX, f64::min)
        };
        // Muro ovest da 0,70 a 1,00: nel rovescio il primo muro è quello est.
        assert!((first_x(&normal) - 0.7).abs() < 1e-9, "{}", first_x(&normal));
        assert!((first_x(&flipped) - 0.7).abs() < 1e-9, "{}", first_x(&flipped));
        // Il tramezzo sta a 5,0..5,1 da ovest e a 4,9..5,0 da est (10 − 5,1).
        let tramezzo_x = |b: &SectionBuild| -> f64 {
            b.drawings[0]
                .entities
                .iter()
                .filter_map(|e| match e {
                    EntityType::LwPolyline(p) => {
                        let xs: Vec<f64> = p.vertices.iter().map(|v| v.location.x).collect();
                        let (lo, hi) = (
                            xs.iter().cloned().fold(f64::MAX, f64::min),
                            xs.iter().cloned().fold(f64::MIN, f64::max),
                        );
                        ((hi - lo - 0.1).abs() < 1e-9).then_some(lo)
                    }
                    _ => None,
                })
                .next()
                .unwrap_or(f64::NAN)
        };
        assert!((tramezzo_x(&normal) - 5.0).abs() < 1e-9, "{}", tramezzo_x(&normal));
        assert!((tramezzo_x(&flipped) - 4.9).abs() < 1e-9, "{}", tramezzo_x(&flipped));
    }

    /// Nessun taglio, nessuna sezione; un locale senza altezza lascia gli
    /// avvisi con il nome del taglio, senza doppioni.
    #[test]
    fn no_cuts_no_sections_and_notices_carry_the_cut_name() {
        let scene = scene();
        assert!(build_sections(&scene.document, &[], &OPTS).drawings.is_empty());

        let mut no_height = self::scene();
        let rec = record::ProjectRecord {
            default_height: None,
            ..record::read(&no_height.document)
        };
        record::write(&mut no_height.document, &rec);
        let build = build_sections(&no_height.document, &[cut_east()], &OPTS);
        let texts: Vec<String> = build.notices.iter().map(Notice::text).collect();
        assert!(!texts.is_empty());
        assert!(
            texts.iter().all(|t| t.contains("taglio A-A") || t.contains("locale")),
            "{texts:?}"
        );
        let mut unique = texts.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), texts.len(), "senza doppioni: {texts:?}");
    }

    /// I codici delle aperture sono quelli dell'abaco.
    #[test]
    fn aperture_codes_match_the_abaco() {
        let mut scene = scene();
        scene.add_entity(
            aperture_polyline(&Aperture {
                kind: ApertureKind::Finestra,
                axis: ([4.05, 2.0], [4.05, 2.9]),
                threshold: false,
                sill: Some(0.9),
                ..door(ApertureState::Nuova)
            })
            .unwrap(),
        );
        let found = apertures_in(&scene.document);
        let mut mine: Vec<String> = codes_of(&found).into_iter().map(|(_, c)| c).collect();
        mine.sort();
        let abaco = crate::bim::abaco::compute_abaco(
            &scene.document,
            &crate::bim::abaco::AbacoOptions {
                unit: M,
                tol: TOL,
                room_tol: TOL,
            },
        );
        let mut theirs: Vec<String> = abaco.aperture.rows.iter().map(|r| r.code.clone()).collect();
        theirs.sort();
        assert_eq!(mine, theirs);
    }

    /// I tre prospetti del fronte sud del caso A con la porta: la facciata, la
    /// porta non cade sul fronte; nessun avviso. Il nord ha la stessa
    /// larghezza; con un'apertura nuova sul fronte, nella comparativa è rossa.
    #[test]
    fn elevations_come_in_three_phases() {
        let mut scene = scene();
        let south = build_elevations(&scene.document, "Sud", [0.0, -1.0], &OPTS);
        let phases: Vec<SectionPhase> = south.drawings.iter().map(|d| d.phase).collect();
        assert_eq!(
            phases,
            [SectionPhase::Fatto, SectionPhase::Comparativa, SectionPhase::Progetto]
        );
        assert!(south.drawings.iter().all(|d| d.kind == DrawingKind::Prospetto && d.cut_name == "Sud"));
        assert!(south.notices.is_empty(), "{:?}", south.notices);

        scene.add_entity(
            aperture_polyline(&Aperture {
                kind: ApertureKind::Finestra,
                axis: ([1.0, -0.15], [2.2, -0.15]),
                thickness: 0.30,
                height: Some(1.2),
                sill: Some(0.9),
                threshold: false,
                ..door(ApertureState::Nuova)
            })
            .unwrap(),
        );
        let with_window = build_elevations(&scene.document, "Sud", [0.0, -1.0], &OPTS);
        let red = |phase: SectionPhase| -> usize {
            with_window
                .drawings
                .iter()
                .find(|d| d.phase == phase)
                .map(|d| {
                    d.entities
                        .iter()
                        .filter(|e| matches!(e, EntityType::LwPolyline(p) if p.common.color == codec::Color::from_index(1)))
                        .count()
                })
                .unwrap_or(0)
        };
        assert_eq!(red(SectionPhase::Fatto), 0, "la nuova non c'è nel fatto");
        assert_eq!(red(SectionPhase::Comparativa), 1, "nella comparativa è rossa");
        assert_eq!(red(SectionPhase::Progetto), 1, "nel progetto c'è, rossa");
    }
}
