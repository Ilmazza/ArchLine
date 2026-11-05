//! Modello 3D, terzo passo (spec parte 5 §4.3, §4.4): i due modelli, stato di
//! fatto e stato di progetto, come facce piane. Logica pura: legge il documento,
//! non scrive nulla.
//!
//! - **fatto**: i muri del rilievo con le soglie delle porte esistenti o da
//!   demolire, i locali di fatto, le aperture esistenti e da demolire;
//! - **progetto**: la geometria di progetto (`project::project_geometry`), i
//!   locali di progetto, le aperture esistenti e nuove (R116: le esistenti
//!   dentro una zona demolita non ci sono);
//! - per ogni fase: le masse e i vani come prismi
//!   ([`super::mass3d_prism::extrude`]) e, per ogni locale misurato, il
//!   **pavimento** a quota 0 (il suo contorno, triangolato);
//! - la demolizione e la costruzione non hanno un modello a sé (spec §2): la
//!   differenza è la differenza fra i due.
//!
//! Gli avvisi sono quelli di classificazione e di estrusione, senza doppioni.

use codec::Document;

use super::aperture::{present_apertures, ApertureGeom, ApertureKind};
use super::extract::push_unique;
use super::mass3d::{classify, VanoRing};
use super::mass3d_prism::{cover, extrude, Face3, Tone};
use super::quantities_rooms::{fatto_room_rows, project_room_rows, RoomRow};
use super::record;
use super::report::Notice;
use super::rooms::Phase;
use super::section_build::{codes_of, SectionOptions};
use super::walls::{rect_from_axis_thickness, sources_of, wall_set_without_rooms};
use super::zone::zones_in;
use super::project::project_geometry;

/// Un modello 3D di una fase.
#[derive(Clone, Debug, Default)]
pub struct Model3d {
    pub faces: Vec<Face3>,
    pub notices: Vec<Notice>,
    /// Quante masse con altezza sono state estruse.
    pub prisms: usize,
    pub floors: usize,
}

fn vani_of(found: &[ApertureGeom], per_metre: f64) -> Vec<VanoRing> {
    codes_of(found)
        .into_iter()
        .map(|(i, code)| {
            let geom = &found[i];
            let a = &geom.aperture;
            let through = rect_from_axis_thickness(a.axis.0, a.axis.1, a.thickness)
                .map(|rect| rect.long_sides().to_vec())
                .unwrap_or_default();
            VanoRing {
                code,
                ring: geom.ring.clone(),
                through,
                bottom_m: match a.kind {
                    ApertureKind::Finestra => a.sill.map(|s| s / per_metre),
                    ApertureKind::Porta | ApertureKind::Portafinestra => Some(0.0),
                },
                height_m: a.height.map(|h| h / per_metre),
            }
        })
        .collect()
}

fn build_one(
    sources: &rustc_hash::FxHashMap<codec::Handle, crate::scene::BoundarySource>,
    found: &[ApertureGeom],
    rows: &[&RoomRow],
    opts: &SectionOptions,
) -> Model3d {
    let per_metre = opts.unit.per_metre();
    let vani = vani_of(found, per_metre);
    let measured: Vec<&&RoomRow> = rows.iter().filter(|row| row.outline.len() >= 3).collect();
    let outlines: Vec<Vec<[f64; 2]>> = measured.iter().map(|row| row.outline.clone()).collect();
    let rooms: Vec<(String, super::height::Height)> = measured
        .iter()
        .map(|row| (row.name.clone(), row.height.clone()))
        .collect();
    let (cells, mut notices) = classify(sources, &vani, &outlines, &rooms, per_metre, opts.tol);
    let prisms = cells
        .iter()
        .filter(|c| c.kind == super::mass3d::CellKind::Mass && c.height.value_m.is_some())
        .count();
    let (mut faces, more) = extrude(&cells, &vani, per_metre, opts.tol);
    push_unique(&mut notices, more);
    let mut floors = 0;
    for outline in &outlines {
        faces.extend(cover(outline, &[], 0.0, Tone::Floor));
        floors += 1;
    }
    Model3d {
        faces,
        notices,
        prisms,
        floors,
    }
}

/// I modelli di fatto e di progetto di `doc`.
pub fn build_models(doc: &Document, opts: &SectionOptions) -> (Model3d, Model3d) {
    let per_metre = opts.unit.per_metre();
    let default_height = record::read(doc).default_height;
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
    let zones = zones_in(doc);
    let pg = project_geometry(doc, &walls, &zones, "0", opts.tol);
    let (pg_sources, _) = sources_of(&pg.room_entities, opts.tol);
    let fatto = build_one(
        &survey,
        &present_apertures(doc, Phase::Fatto),
        &fatto_rows.iter().collect::<Vec<_>>(),
        opts,
    );
    let progetto = build_one(
        &pg_sources,
        &present_apertures(doc, Phase::Progetto),
        &project_rows.iter().map(|p| &p.row).collect::<Vec<_>>(),
        opts,
    );
    let _ = per_metre;
    (fatto, progetto)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::bim::aperture::{aperture_polyline, ensure_layer, Aperture, ApertureState};
    use crate::bim::fixtures::{case_a_with_door, scene_with, Form};
    use crate::bim::rooms::{measure_at, room_entities, Computed};
    use crate::bim::units::DrawingUnit;
    use crate::bim::walls::{faces, rect_from_axis};
    use crate::bim::zone::{ensure_layers, zone_hatch, Zone, ZoneOrigin, ZoneState};
    use crate::bim::MAX_WALL_THICKNESS_M;
    use crate::scene::Scene;
    use kernel::geom2d::signed_area;

    const M: DrawingUnit = DrawingUnit::Metri;
    const TOL: f64 = 1e-6;
    const OPTS: SectionOptions = SectionOptions {
        unit: M,
        tol: TOL,
        room_tol: TOL,
    };

    /// Caso A con la porta esistente, locali A e B e altezza 2,70.
    fn scene(form: Form, height: Option<f64>) -> Scene {
        let mut scene = scene_with(case_a_with_door(form, M), M.insunits());
        ensure_layers(&mut scene.document);
        ensure_layer(&mut scene.document);
        scene.add_entity(
            aperture_polyline(&Aperture {
                kind: ApertureKind::Porta,
                state: ApertureState::Esistente,
                axis: ([4.05, 1.0], [4.05, 1.9]),
                thickness: 0.10,
                height: Some(2.1),
                sill: None,
                threshold: true,
                lintel: false,
                bearing: None,
            })
            .unwrap(),
        );
        let rec = record::ProjectRecord {
            default_height: height,
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
        let rect = rect_from_axis(&walls.sources, [4.05, 0.0], [4.05, 3.0], MAX_WALL_THICKNESS_M, TOL)
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

    fn area(face: &Face3) -> f64 {
        let ring: Vec<[f64; 2]> = face.quad[..3].iter().map(|p| [p[0], p[1]]).collect();
        signed_area(&ring).abs()
    }

    fn at_z(model: &Model3d, z: f64, tone: Tone) -> f64 {
        model
            .faces
            .iter()
            .filter(|f| f.tone == tone && f.quad.iter().all(|p| (p[2] - z).abs() < 1e-9))
            .map(area)
            .sum()
    }

    /// Senza lavori fatto e progetto sono lo stesso modello: massa 7,26 m²
    /// (7,17 di muro + 0,09 di testa della porta) a 2,70, pavimenti 23,70 m²
    /// a quota 0, nessun avviso.
    #[test]
    fn without_works_fatto_and_progetto_are_the_same_model() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let scene = scene(form, Some(2.7));
            let (fatto, progetto) = build_models(&scene.document, &OPTS);
            for (model, what) in [(&fatto, "fatto"), (&progetto, "progetto")] {
                assert!(model.notices.is_empty(), "{form:?} {what}: {:?}", model.notices);
                assert_eq!(model.floors, 2, "{form:?} {what}");
                assert!(model.prisms >= 1, "{form:?} {what}");
                let top = at_z(model, 2.7, Tone::Mass);
                assert!((top - 7.26).abs() < 1e-9, "{form:?} {what}: coperchi {top}");
                let floor = at_z(model, 0.0, Tone::Floor);
                assert!((floor - 23.7).abs() < 1e-9, "{form:?} {what}: pavimenti {floor}");
            }
            assert_eq!(fatto.faces.len(), progetto.faces.len(), "{form:?}");
        }
    }

    /// Demolito il tramezzo (con la porta dentro): il fatto lo ha, il progetto
    /// no; i due locali diventano uno di 24,00 m² (23,70 + 0,30) e la massa
    /// scende a 30,96 − 24,00 = 6,96 m².
    #[test]
    fn demolishing_the_partition_changes_only_the_project() {
        for form in [Form::Continua, Form::ChiusaPerMuro] {
            let mut scene = scene(form, Some(2.7));
            demolish_tramezzo(&mut scene);
            let (fatto, progetto) = build_models(&scene.document, &OPTS);
            let top_f = at_z(&fatto, 2.7, Tone::Mass);
            let top_p = at_z(&progetto, 2.7, Tone::Mass);
            assert!((top_f - 7.26).abs() < 1e-9, "{form:?}: fatto {top_f}");
            assert!((top_p - 6.96).abs() < 1e-9, "{form:?}: progetto {top_p}");
            let floor_p = at_z(&progetto, 0.0, Tone::Floor);
            assert!((floor_p - 24.0).abs() < 1e-9, "{form:?}: pavimento di progetto {floor_p}");
            assert_eq!(progetto.floors, 1, "{form:?}");
        }
    }

    /// Senza altezza nessun prisma: avvisi, solo i pavimenti.
    #[test]
    fn without_heights_there_are_no_prisms_only_notices_and_floors() {
        let scene = scene(Form::Continua, None);
        let (fatto, _) = build_models(&scene.document, &OPTS);
        assert_eq!(fatto.prisms, 0);
        assert!(!fatto.notices.is_empty());
        assert!(fatto.faces.iter().all(|f| f.tone == Tone::Floor), "{:?}", fatto.faces.len());
    }
}
