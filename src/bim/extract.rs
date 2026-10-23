//! Estrazione degli elaborati in DWG 2D ordinari (spec §5.5): lo stato di
//! fatto (il rilievo senza zone) e demolizioni e costruzioni (il rilievo con le
//! campiture gialle e rosse). Lo stato di progetto arriva con il Task 12.
//!
//! Il file estratto si apre senza ArchLine: solo spazio modello, nessun XDATA
//! e nessun APPID `ARCHLINE_*`, nessun record `ARCHLINE_PROGETTO` (il
//! documento è nuovo e la copia non porta le voci del dizionario radice). La
//! copia è quella di `WBLOCK` (`extract_entities_into`): handle nuovi, con i
//! layer, i tipi di linea, gli stili e i blocchi che le entità usano. Gli
//! handle che la copia non restituisce sono elencati con `Notice::NonCopiata`.
//!
//! Limite noto: la copia restituisce anche entità che rimandano a ciò che non
//! porta con sé (la definizione di un'immagine, il blocco di un riferimento
//! esterno, gli stili di multileader, multilinea e tabella). Arrivano nel file
//! senza il loro riferimento e qui non sono elencate.
//!
//! Il disegno di lavoro non si tocca: si lavora su una sua copia.

use std::path::{Path, PathBuf};

use codec::xdata::ExtendedData;
use codec::{CadDocument, DxfVersion, EntityType, Handle};
use rustc_hash::FxHashSet;

use crate::app::copy_to_clipboard_kernel;
use crate::modules::insert::wblock::extract_entities_into;
use crate::scene::view::dispatch::set_entity_xdata;

use super::report::Notice;
use super::units::DrawingUnit;
use super::zone::zones_in;
use super::{LAYER_LOCALI, LAYER_LOCALI_PROGETTO};

/// Prefisso degli APPID di ArchLine (`ARCHLINE_ZONA`, `ARCHLINE_LOCALE`).
const APP_PREFIX: &str = "ARCHLINE_";

/// Il tipo nell'avviso quando l'handle non porta a nessuna entità.
const UNKNOWN_KIND: &str = "Entità";

/// Gli elaborati in pianta che si estraggono.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elaborato {
    StatoDiFatto,
    DemolizioniCostruzioni,
    StatoDiProgetto,
}

impl Elaborato {
    /// La parte del nome del file dopo il nome del disegno.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::StatoDiFatto => "stato-di-fatto",
            Self::DemolizioniCostruzioni => "demolizioni-costruzioni",
            Self::StatoDiProgetto => "stato-di-progetto",
        }
    }
}

/// Cosa estrarre e con quali impostazioni.
pub struct ExtractOptions {
    pub elaborati: Vec<Elaborato>,
    /// Layer dei muri nuovi dello stato di progetto.
    pub new_walls_layer: String,
    pub unit: DrawingUnit,
    /// Tolleranza geometrica, in unità del disegno.
    pub tol: f64,
}

/// I documenti estratti, nell'ordine chiesto, e gli avvisi.
pub struct Extracted {
    pub docs: Vec<(Elaborato, CadDocument)>,
    pub notices: Vec<Notice>,
}

/// Un documento per elaborato chiesto (una volta sola, nell'ordine chiesto),
/// più gli avvisi senza doppioni.
///
/// - **fatto**: lo spazio modello senza le zone e senza i layer
///   `AL-LOCALI-PROGETTO`;
/// - **comparativa**: lo spazio modello senza i layer `AL-LOCALI` e
///   `AL-LOCALI-PROGETTO`, cioè con le zone.
///
/// `StatoDiProgetto` per ora si salta, senza documento né avviso (Task 12).
pub fn extract(src: &CadDocument, opts: &ExtractOptions) -> Extracted {
    let mut ex = Extracted {
        docs: Vec::new(),
        notices: Vec::new(),
    };
    for &elaborato in &opts.elaborati {
        if ex.docs.iter().any(|(done, _)| *done == elaborato) {
            continue;
        }
        let handles = match elaborato {
            Elaborato::StatoDiFatto => {
                let zones: FxHashSet<Handle> =
                    zones_in(src).into_iter().map(|zone| zone.handle).collect();
                model_space_handles(src, |handle, entity| {
                    !zones.contains(&handle) && !on_layers(entity, &[LAYER_LOCALI_PROGETTO])
                })
            }
            Elaborato::DemolizioniCostruzioni => model_space_handles(src, |_, entity| {
                !on_layers(entity, &[LAYER_LOCALI, LAYER_LOCALI_PROGETTO])
            }),
            Elaborato::StatoDiProgetto => continue,
        };
        let mut work = src.clone();
        let (out, notices) = copy_out(src, &mut work, &handles);
        ex.docs.push((elaborato, out));
        for notice in notices {
            if !ex.notices.contains(&notice) {
                ex.notices.push(notice);
            }
        }
    }
    ex
}

/// Gli handle dello spazio modello, in ordine di disegno, che `keep` accetta.
/// Un handle senza entità resta: la copia non lo restituirà e finirà tra le
/// non copiate.
fn model_space_handles(
    doc: &CadDocument,
    keep: impl Fn(Handle, &EntityType) -> bool,
) -> Vec<Handle> {
    doc.block_records
        .iter()
        .filter(|record| record.is_model_space())
        .flat_map(|record| record.entity_handles.iter().copied())
        .filter(|&handle| {
            doc.get_entity(handle)
                .map_or(true, |entity| keep(handle, entity))
        })
        .collect()
}

/// L'entità sta su uno dei layer dati? Come tutti i nomi dei layer, senza
/// distinguere le maiuscole.
fn on_layers(entity: &EntityType, layers: &[&str]) -> bool {
    let layer = &entity.common().layer;
    layers.iter().any(|name| layer.eq_ignore_ascii_case(name))
}

/// Passi 3-6 per gli handle scelti in `work` (copia di `src`): via i nostri
/// record, copia in un documento nuovo con versione e unità di `src`, tipi di
/// linea dei layer, avvisi per ciò che non si copia.
fn copy_out(
    src: &CadDocument,
    work: &mut CadDocument,
    handles: &[Handle],
) -> (CadDocument, Vec<Notice>) {
    strip_our_xdata(work, handles);

    let mut out = CadDocument::new();
    out.version = output_version(src.version);
    // L'errore dice solo che non c'era nulla da copiare (nessun handle, o
    // nessuno copiabile): il documento resta vuoto e gli avvisi sotto
    // elencano gli handle non copiati.
    let _ = extract_entities_into(work, handles, &mut out);
    out.header.insertion_units = src.header.insertion_units;
    copy_layer_linetypes(src, &mut out);
    scrub_our_data(&mut out);

    // La copia di WBLOCK passa a `copy_to_clipboard_kernel` gli handle senza
    // `BLOCK`/`ENDBLK`; ciò che il kernel non restituisce non è nel documento.
    let copyable: Vec<Handle> = handles
        .iter()
        .copied()
        .filter(|&handle| {
            !matches!(
                work.get_entity(handle),
                Some(EntityType::Block(_) | EntityType::BlockEnd(_))
            )
        })
        .collect();
    let (copied, _) = copy_to_clipboard_kernel(work, &copyable);
    let copied: FxHashSet<Handle> = copied.iter().map(|e| e.common().handle).collect();
    let notices = handles
        .iter()
        .filter(|handle| !copied.contains(handle))
        .map(|&handle| Notice::NonCopiata {
            handle,
            kind: work
                .get_entity(handle)
                .map_or(UNKNOWN_KIND, |entity| entity.as_entity().entity_type())
                .to_string(),
        })
        .collect();
    (out, notices)
}

/// La versione del disegno di partenza, se sta tra AC1014 (R14) e AC1032
/// (2018); altrimenti AC1032.
fn output_version(version: DxfVersion) -> DxfVersion {
    if (DxfVersion::AC1014..=DxfVersion::AC1032).contains(&version) {
        version
    } else {
        DxfVersion::AC1032
    }
}

/// APPID di ArchLine (`ARCHLINE_*`, senza distinguere le maiuscole come la
/// tabella degli APPID).
fn is_ours(app: &str) -> bool {
    app.get(..APP_PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(APP_PREFIX))
}

/// Passo 3: toglie da `handles` i record `ARCHLINE_ZONA`, `ARCHLINE_LOCALE`
/// (e ogni altro `ARCHLINE_*`), compresi i blocchi grezzi letti da un DWG.
/// Fatto prima della copia, né i record né i loro APPID passano in `out`.
fn strip_our_xdata(work: &mut CadDocument, handles: &[Handle]) {
    let our_apps: Vec<(String, u64)> = work
        .app_ids
        .iter()
        .filter(|app| is_ours(&app.name))
        .map(|app| (app.name.clone(), app.handle.value()))
        .collect();
    for &handle in handles {
        let Some(entity) = work.get_entity(handle) else {
            continue;
        };
        let xdata = &entity.common().extended_data;
        let mut apps: Vec<String> = xdata
            .records()
            .iter()
            .map(|record| record.application_name.clone())
            .filter(|name| is_ours(name))
            .collect();
        apps.extend(
            our_apps
                .iter()
                .filter(|(_, app)| xdata.raw_dwg_eed.iter().any(|(raw, _)| raw == app))
                .map(|(name, _)| name.clone()),
        );
        apps.sort();
        apps.dedup();
        for app in apps {
            set_entity_xdata(work, handle, &app, None);
        }
    }
}

/// Dopo la copia: le entità delle definizioni di blocco copiate come
/// dipendenze (e gli attributi) possono ancora portare XDATA nostro, e con
/// esso il suo APPID. Via record, blocchi grezzi e APPID `ARCHLINE_*`.
fn scrub_our_data(out: &mut CadDocument) {
    let our_apps: Vec<(String, u64)> = out
        .app_ids
        .iter()
        .filter(|app| is_ours(&app.name))
        .map(|app| (app.name.clone(), app.handle.value()))
        .collect();
    let raw_apps: FxHashSet<u64> = our_apps.iter().map(|(_, handle)| *handle).collect();
    let scrub = |xdata: &mut ExtendedData| {
        let names: Vec<String> = xdata
            .records()
            .iter()
            .map(|record| record.application_name.clone())
            .filter(|name| is_ours(name))
            .collect();
        for name in names {
            xdata.remove_record(&name);
        }
        xdata.raw_dwg_eed.retain(|(app, _)| !raw_apps.contains(app));
    };
    for entity in out.entities_mut() {
        scrub(&mut entity.common_mut().extended_data);
        if let EntityType::Insert(insert) = entity {
            for attribute in &mut insert.attributes {
                scrub(&mut attribute.common.extended_data);
            }
        }
    }
    for (name, _) in our_apps {
        out.app_ids.remove(&name);
    }
}

/// Passo 5: `ClipboardDeps` copia i tipi di linea delle entità, non quelli dei
/// layer. Un layer di `out` con un tipo di linea che `out` non ha lo riceve da
/// `src`, con un handle nuovo.
fn copy_layer_linetypes(src: &CadDocument, out: &mut CadDocument) {
    let names: Vec<String> = out
        .layers
        .iter()
        .map(|layer| layer.line_type.clone())
        .filter(|name| !name.is_empty())
        .collect();
    for name in names {
        if out.line_types.contains(&name) {
            continue;
        }
        let Some(line_type) = src.line_types.get(&name) else {
            continue;
        };
        let mut line_type = line_type.clone();
        line_type.handle = out.allocate_handle();
        let _ = out.line_types.add(line_type);
    }
}

/// `"{base}_{suffisso}.dwg"`.
pub fn file_name(base: &str, e: Elaborato) -> String {
    format!("{base}_{}.dwg", e.suffix())
}

#[derive(Debug, PartialEq)]
pub enum WriteError {
    /// I file che esistono già: non si è scritto nulla.
    Exists(Vec<PathBuf>),
    Io(String),
}

/// Scrive un DWG per documento in `dir`, con il nome di [`file_name`] e la
/// versione del documento. Senza `overwrite`, se anche uno solo dei file
/// esiste non scrive nulla e restituisce tutti quelli che esistono. Un errore
/// di scrittura ferma il giro: i file già scritti restano (ognuno è scritto
/// accanto e poi sostituito, quindi mai a metà).
pub fn write(
    ex: &Extracted,
    dir: &Path,
    base: &str,
    overwrite: bool,
) -> Result<Vec<PathBuf>, WriteError> {
    let paths: Vec<PathBuf> = ex
        .docs
        .iter()
        .map(|(elaborato, _)| dir.join(file_name(base, *elaborato)))
        .collect();
    if !overwrite {
        let mut existing = Vec::new();
        for path in &paths {
            if path
                .try_exists()
                .map_err(|e| WriteError::Io(format!("{}: {e}", path.display())))?
            {
                existing.push(path.clone());
            }
        }
        if !existing.is_empty() {
            return Err(WriteError::Exists(existing));
        }
    }
    for ((_, doc), path) in ex.docs.iter().zip(&paths) {
        crate::io::save_as_version(doc, path, doc.version)
            .map_err(|e| WriteError::Io(format!("{}: {e}", path.display())))?;
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::{Insert, LwPolyline, MText};
    use codec::objects::ObjectType;
    use codec::tables::{Layer, LineType};
    use codec::xdata::{ExtendedDataRecord, XDataValue};
    use codec::{Color, DxfVersion, EntityType, Handle, Vector2, Vector3};

    use crate::bim::fixtures::{case_a, Form, LAYER_MURI};
    use crate::bim::record::{self, ProjectRecord};
    use crate::bim::walls::{rect_from_axis, rect_from_axis_thickness, wall_set};
    use crate::bim::zone::{ensure_layers, zone_hatch, Zone, ZoneOrigin, ZoneState};
    use crate::bim::{
        APP_LOCALE, APP_ZONA, LAYER_LOCALI, LAYER_LOCALI_PROGETTO, MAX_WALL_THICKNESS_M,
        RECORD_PROGETTO,
    };

    const M: DrawingUnit = DrawingUnit::Metri;

    fn opts(e: &[Elaborato]) -> ExtractOptions {
        ExtractOptions {
            elaborati: e.to_vec(),
            new_walls_layer: "MURI".into(),
            unit: DrawingUnit::Metri,
            tol: 1e-6,
        }
    }

    /// Caso A in metri (linee continue, INSUNITS 6), una zona demolita sul
    /// tramezzo (asse (4,05; 0)-(4,05; 3)) e una zona nuova da 0,12 sull'asse
    /// (2; 0,5)-(2; 2,5), più il record `ARCHLINE_PROGETTO`.
    fn doc_with_zones() -> CadDocument {
        let mut doc = CadDocument::new();
        doc.header.insertion_units = 6;
        let mut muri = Layer::new(LAYER_MURI);
        muri.handle = doc.allocate_handle();
        doc.layers.add(muri).unwrap();
        for entity in case_a(Form::Continua, M) {
            doc.add_entity(entity).unwrap();
        }
        ensure_layers(&mut doc);

        let tol = M.tolerance();
        let walls = wall_set(&doc, tol);
        let tramezzo = rect_from_axis(
            &walls.sources,
            [4.05, 0.0],
            [4.05, 3.0],
            M.metres(MAX_WALL_THICKNESS_M),
            tol,
        )
        .unwrap();
        let demolita = Zone {
            state: ZoneState::Demolito,
            origin: ZoneOrigin::MuroDuePunti,
            axis: Some(tramezzo.axis),
            thickness: Some(tramezzo.thickness),
            height: None,
            varco: false,
        };
        doc.add_entity(zone_hatch(&demolita, &[tramezzo.ring()]))
            .unwrap();

        let muro = rect_from_axis_thickness([2.0, 0.5], [2.0, 2.5], 0.12).unwrap();
        let nuova = Zone {
            state: ZoneState::Nuovo,
            origin: ZoneOrigin::LineaSpessore,
            axis: Some(muro.axis),
            thickness: Some(0.12),
            height: None,
            varco: false,
        };
        doc.add_entity(zone_hatch(&nuova, &[muro.ring()])).unwrap();

        record::write(
            &mut doc,
            &ProjectRecord {
                unit: None,
                new_walls_layer: Some(LAYER_MURI.into()),
            },
        );
        doc
    }

    fn hatches(doc: &CadDocument) -> Vec<&codec::entities::Hatch> {
        doc.entities()
            .filter_map(|e| match e {
                EntityType::Hatch(h) => Some(h.as_ref()),
                _ => None,
            })
            .collect()
    }

    fn lines_on<'a>(doc: &'a CadDocument, layer: &str) -> Vec<&'a codec::entities::Line> {
        doc.entities()
            .filter_map(|e| match e {
                EntityType::Line(l) if l.common.layer == layer => Some(l),
                _ => None,
            })
            .collect()
    }

    fn model_space_count(doc: &CadDocument) -> usize {
        doc.block_records
            .iter()
            .filter(|r| r.is_model_space())
            .map(|r| r.entity_handles.len())
            .sum()
    }

    /// Nessun XDATA, nessun APPID e nessun record del dizionario radice di
    /// ArchLine.
    #[track_caller]
    fn assert_no_archline_data(out: &CadDocument) {
        let our_record = |e: &EntityType| {
            e.common()
                .extended_data
                .records()
                .iter()
                .any(|r| r.application_name.starts_with("ARCHLINE"))
        };
        assert!(!out.entities().any(our_record), "XDATA ARCHLINE_* rimasto");
        assert!(
            !out.app_ids.iter().any(|a| a.name.starts_with("ARCHLINE")),
            "APPID ARCHLINE_* rimasto"
        );
        assert_eq!(record::read(out), ProjectRecord::default());
        let Some(ObjectType::Dictionary(root)) =
            out.objects.get(&out.header.named_objects_dict_handle)
        else {
            panic!("dizionario radice mancante");
        };
        assert!(
            !root
                .entries
                .iter()
                .any(|(key, _)| key.to_ascii_uppercase().starts_with("ARCHLINE")),
            "voce ARCHLINE_* nel dizionario radice"
        );
        assert!(
            !out.objects.values().any(|o| matches!(
                o,
                ObjectType::XRecord(x) if x.name.eq_ignore_ascii_case(RECORD_PROGETTO)
            )),
            "XRecord {RECORD_PROGETTO} copiato"
        );
    }

    /// Una cartella nuova sotto la cartella temporanea, cancellata alla fine.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir().join(format!(
                "archline_bim_extract_{tag}_{}_{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn both() -> [Elaborato; 2] {
        [Elaborato::StatoDiFatto, Elaborato::DemolizioniCostruzioni]
    }

    #[test]
    fn suffixes_and_file_names() {
        assert_eq!(Elaborato::StatoDiFatto.suffix(), "stato-di-fatto");
        assert_eq!(
            Elaborato::DemolizioniCostruzioni.suffix(),
            "demolizioni-costruzioni"
        );
        assert_eq!(Elaborato::StatoDiProgetto.suffix(), "stato-di-progetto");
        assert_eq!(
            file_name("Rilievo Via Roma", Elaborato::StatoDiFatto),
            "Rilievo Via Roma_stato-di-fatto.dwg"
        );
        assert_eq!(
            file_name("casa", Elaborato::DemolizioniCostruzioni),
            "casa_demolizioni-costruzioni.dwg"
        );
        assert_eq!(
            file_name("casa", Elaborato::StatoDiProgetto),
            "casa_stato-di-progetto.dwg"
        );
    }

    #[test]
    fn comparativa_has_yellow_and_red_without_our_data() {
        let src = doc_with_zones();
        let ex = extract(&src, &opts(&[Elaborato::DemolizioniCostruzioni]));
        assert_eq!(ex.docs.len(), 1);
        let (e, out) = &ex.docs[0];
        assert_eq!(*e, Elaborato::DemolizioniCostruzioni);
        let hatches: Vec<_> = out
            .entities()
            .filter_map(|e| {
                if let EntityType::Hatch(h) = e {
                    Some(h)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(hatches.len(), 2);
        assert!(hatches.iter().any(|h| h.common.layer == "AL-DEMOLIZIONI"
            && h.common.color == Color::from_index(2)
            && h.is_solid));
        assert!(hatches.iter().any(|h| h.common.layer == "AL-NUOVE"
            && h.common.color == Color::from_index(1)
            && h.is_solid));
        assert!(out.entities().all(|e| e
            .common()
            .extended_data
            .records()
            .iter()
            .all(|r| !r.application_name.starts_with("ARCHLINE"))));
        assert!(!out.app_ids.iter().any(|a| a.name.starts_with("ARCHLINE")));
        assert_no_archline_data(out);
        // Tutto il rilievo c'è, con le zone: 12 LINE e 2 HATCH.
        assert_eq!(lines_on(out, LAYER_MURI).len(), 12);
        assert_eq!(model_space_count(out), 14);
        assert!(ex.notices.is_empty(), "{:?}", ex.notices);
    }

    #[test]
    fn fatto_has_no_zones_and_same_lines() {
        let src = doc_with_zones();
        let ex = extract(&src, &opts(&[Elaborato::StatoDiFatto]));
        assert_eq!(ex.docs.len(), 1);
        let (e, out) = &ex.docs[0];
        assert_eq!(*e, Elaborato::StatoDiFatto);
        assert!(
            !hatches(out)
                .iter()
                .any(|h| h.common.layer.to_ascii_uppercase().starts_with("AL-")),
            "zone nello stato di fatto"
        );
        assert!(hatches(out).is_empty());
        let src_lines = lines_on(&src, LAYER_MURI);
        let out_lines = lines_on(out, LAYER_MURI);
        assert_eq!(src_lines.len(), 12);
        assert_eq!(out_lines.len(), src_lines.len());
        let close = |a: Vector3, b: Vector3| (a - b).length() <= 1e-6;
        for line in &src_lines {
            assert!(
                out_lines
                    .iter()
                    .any(|o| close(o.start, line.start) && close(o.end, line.end)),
                "manca la LINE {:?}-{:?}",
                line.start,
                line.end
            );
        }
        assert_eq!(model_space_count(out), 12);
        assert_no_archline_data(out);
        assert!(ex.notices.is_empty(), "{:?}", ex.notices);
    }

    #[test]
    fn source_is_never_modified() {
        let src = doc_with_zones();
        let snapshot = |doc: &CadDocument| {
            (
                doc.entities().cloned().collect::<Vec<EntityType>>(),
                doc.app_ids
                    .iter()
                    .map(|a| a.name.clone())
                    .collect::<Vec<_>>(),
                doc.objects.len(),
                record::read(doc),
            )
        };
        let before = snapshot(&src);
        let ex = extract(&src, &opts(&both()));
        assert_eq!(ex.docs.len(), 2);
        assert!(snapshot(&src) == before, "il disegno di lavoro è cambiato");
        // Le zone di src hanno ancora il loro record.
        assert_eq!(crate::bim::zone::zones_in(&src).len(), 2);
    }

    #[test]
    fn rooms_layers_follow_the_elaborato_and_lose_our_xdata() {
        let mut src = doc_with_zones();
        let locale = |values: &[&str]| {
            let mut rec = ExtendedDataRecord::new(APP_LOCALE);
            rec.add_value(XDataValue::Integer16(1));
            for v in values {
                rec.add_value(XDataValue::String((*v).into()));
            }
            rec
        };
        let mut contorno = LwPolyline::from_points(vec![
            Vector2::new(0.5, 0.5),
            Vector2::new(3.5, 0.5),
            Vector2::new(3.5, 2.5),
            Vector2::new(0.5, 2.5),
        ]);
        contorno.is_closed = true;
        contorno.common.layer = LAYER_LOCALI.into();
        contorno
            .common
            .extended_data
            .add_record(locale(&["fase", "fatto"]));
        src.add_entity(EntityType::LwPolyline(contorno)).unwrap();
        let mut testo = MText::with_value("A\\PS = 12,00 m²", Vector3::new(2.0, 1.5, 0.0));
        testo.common.layer = LAYER_LOCALI_PROGETTO.into();
        testo
            .common
            .extended_data
            .add_record(locale(&["fase", "progetto"]));
        src.add_entity(EntityType::MText(Box::new(testo))).unwrap();
        // Il layer dei locali si confronta senza badare alle maiuscole.
        let mut minuscolo = LwPolyline::from_points(vec![
            Vector2::new(4.5, 0.5),
            Vector2::new(7.5, 0.5),
            Vector2::new(7.5, 2.5),
        ]);
        minuscolo.common.layer = LAYER_LOCALI_PROGETTO.to_ascii_lowercase();
        src.add_entity(EntityType::LwPolyline(minuscolo)).unwrap();

        let ex = extract(&src, &opts(&both()));
        let (_, fatto_out) = &ex.docs[0];
        let (_, comparativa) = &ex.docs[1];
        let on = |doc: &CadDocument, layer: &str| {
            doc.entities()
                .filter(|e| e.common().layer.eq_ignore_ascii_case(layer))
                .count()
        };
        // Fatto: il contorno di fatto resta (senza il nostro record), quello
        // di progetto no.
        assert_eq!(on(fatto_out, LAYER_LOCALI), 1);
        assert_eq!(on(fatto_out, LAYER_LOCALI_PROGETTO), 0);
        assert_no_archline_data(fatto_out);
        // Comparativa: niente locali.
        assert_eq!(on(comparativa, LAYER_LOCALI), 0);
        assert_eq!(on(comparativa, LAYER_LOCALI_PROGETTO), 0);
        assert_no_archline_data(comparativa);
        assert!(ex.notices.is_empty(), "{:?}", ex.notices);
    }

    #[test]
    fn our_xdata_is_dropped_but_other_apps_stay() {
        let mut src = doc_with_zones();
        // Una LINE del rilievo con un record di un'altra applicazione e uno
        // nostro (di una versione futura).
        let mut altra = codec::tables::AppId::new("ALTRA_APP");
        altra.handle = src.allocate_handle();
        src.app_ids.add(altra).unwrap();
        let mut futura = codec::tables::AppId::new("ARCHLINE_FUTURA");
        futura.handle = src.allocate_handle();
        src.app_ids.add(futura).unwrap();
        let line = src
            .entities()
            .find_map(|e| match e {
                EntityType::Line(l) if l.common.layer == LAYER_MURI => Some(l.common.handle),
                _ => None,
            })
            .unwrap();
        let entity = src.get_entity_mut(line).unwrap();
        let mut other = ExtendedDataRecord::new("ALTRA_APP");
        other.add_value(XDataValue::Integer16(7));
        entity.common_mut().extended_data.add_record(other.clone());
        let mut future = ExtendedDataRecord::new("ARCHLINE_FUTURA");
        future.add_value(XDataValue::Integer16(1));
        entity.common_mut().extended_data.add_record(future);

        let ex = extract(&src, &opts(&both()));
        for (_, out) in &ex.docs {
            assert_no_archline_data(out);
            let kept: Vec<_> = out
                .entities()
                .filter_map(|e| e.common().extended_data.get_record("ALTRA_APP"))
                .collect();
            assert_eq!(kept, vec![&other]);
            assert!(out.app_ids.contains("ALTRA_APP"));
        }
        // src conserva tutto.
        let entity = src.get_entity(line).unwrap();
        assert!(entity
            .common()
            .extended_data
            .get_record("ARCHLINE_FUTURA")
            .is_some());
    }

    #[test]
    fn zones_inside_a_copied_block_lose_our_data_too() {
        let mut src = doc_with_zones();
        let ring = vec![[10.0, 0.0], [11.0, 0.0], [11.0, 1.0], [10.0, 1.0]];
        let zona = Zone {
            state: ZoneState::Nuovo,
            origin: ZoneOrigin::Rettangolo,
            axis: None,
            thickness: None,
            height: None,
            varco: false,
        };
        crate::app::define_block_raw_doc(
            &mut src,
            "BLOCCO_ZONA",
            Vector3::ZERO,
            &[zone_hatch(&zona, &[ring])],
        );
        src.add_entity(EntityType::Insert(Insert::new(
            "BLOCCO_ZONA",
            Vector3::ZERO,
        )))
        .unwrap();
        let ex = extract(&src, &opts(&both()));
        for (_, out) in &ex.docs {
            assert!(out.block_records.get("BLOCCO_ZONA").is_some());
            assert_no_archline_data(out);
        }
    }

    #[test]
    fn version_units_and_layer_linetype_follow_source() {
        let mut src = doc_with_zones();
        src.version = DxfVersion::AC1015;
        let mut dashed = LineType::new("DASHED");
        dashed.description = "__ __ __".into();
        dashed.handle = src.allocate_handle();
        src.line_types.add(dashed).unwrap();
        src.layers.get_mut(LAYER_MURI).unwrap().line_type = "DASHED".into();

        let ex = extract(&src, &opts(&both()));
        assert_eq!(ex.docs.len(), 2);
        for (_, out) in &ex.docs {
            assert_eq!(out.version, DxfVersion::AC1015);
            assert_eq!(out.header.insertion_units, 6);
            let lt = out.line_types.get("DASHED").expect("tipo di linea DASHED");
            assert_eq!(lt.description, "__ __ __");
            assert!(!lt.handle.is_null());
            assert!(
                out.line_types
                    .iter()
                    .filter(|l| l.handle == lt.handle)
                    .count()
                    == 1,
                "handle doppio"
            );
            assert_eq!(out.layers.get(LAYER_MURI).unwrap().line_type, "DASHED");
        }

        // Fuori da AC1014..AC1032 si scrive AC1032; dentro, la versione resta.
        for (from, to) in [
            (DxfVersion::AC1009, DxfVersion::AC1032),
            (DxfVersion::AC1012, DxfVersion::AC1032),
            (DxfVersion::Unknown, DxfVersion::AC1032),
            (DxfVersion::AC1014, DxfVersion::AC1014),
            (DxfVersion::AC1027, DxfVersion::AC1027),
        ] {
            src.version = from;
            let ex = extract(&src, &opts(&[Elaborato::StatoDiFatto]));
            assert_eq!(ex.docs[0].1.version, to, "{from:?}");
        }
        // Le unità seguono il disegno, anche 0.
        src.header.insertion_units = 0;
        let ex = extract(&src, &opts(&[Elaborato::StatoDiFatto]));
        assert_eq!(ex.docs[0].1.header.insertion_units, 0);
    }

    #[test]
    fn progetto_is_skipped_for_now_and_duplicates_once() {
        let src = doc_with_zones();
        let ex = extract(
            &src,
            &opts(&[
                Elaborato::DemolizioniCostruzioni,
                Elaborato::StatoDiProgetto,
                Elaborato::StatoDiFatto,
                Elaborato::DemolizioniCostruzioni,
            ]),
        );
        let kinds: Vec<_> = ex.docs.iter().map(|(e, _)| *e).collect();
        assert_eq!(
            kinds,
            vec![Elaborato::DemolizioniCostruzioni, Elaborato::StatoDiFatto]
        );
        assert!(ex.notices.is_empty());
        assert!(extract(&src, &opts(&[])).docs.is_empty());
    }

    #[test]
    fn handles_that_do_not_copy_are_listed_once() {
        let mut src = doc_with_zones();
        let ghost = Handle::new(0xDEAD);
        assert!(src.get_entity(ghost).is_none());
        src.block_records
            .iter_mut()
            .find(|r| r.is_model_space())
            .unwrap()
            .entity_handles
            .push(ghost);
        let ex = extract(&src, &opts(&both()));
        assert_eq!(ex.docs.len(), 2);
        assert_eq!(
            ex.notices,
            vec![Notice::NonCopiata {
                handle: ghost,
                kind: "Entità".into(),
            }]
        );
        assert_eq!(
            ex.notices[0].text(),
            "Entità DEAD non copiata nel DWG estratto."
        );
        // Il resto è copiato.
        assert_eq!(model_space_count(&ex.docs[0].1), 12);
        assert_eq!(model_space_count(&ex.docs[1].1), 14);
    }

    #[test]
    fn empty_drawing_gives_empty_documents() {
        let mut src = CadDocument::new();
        src.header.insertion_units = 5;
        let ex = extract(&src, &opts(&both()));
        assert_eq!(ex.docs.len(), 2);
        for (_, out) in &ex.docs {
            assert_eq!(model_space_count(out), 0);
            assert_eq!(out.header.insertion_units, 5);
        }
        assert!(ex.notices.is_empty());
    }

    fn mtimes(paths: &[PathBuf]) -> Vec<std::time::SystemTime> {
        paths
            .iter()
            .map(|p| std::fs::metadata(p).unwrap().modified().unwrap())
            .collect()
    }

    #[test]
    fn write_refuses_existing_files() {
        let tmp = TempDir::new("refuse");
        let ex = extract(&doc_with_zones(), &opts(&both()));

        let written = write(&ex, &tmp.0, "casa", false).unwrap();
        assert_eq!(
            written,
            vec![
                tmp.0.join("casa_stato-di-fatto.dwg"),
                tmp.0.join("casa_demolizioni-costruzioni.dwg"),
            ]
        );
        assert!(written.iter().all(|p| p.is_file()));
        let before = mtimes(&written);
        let lens: Vec<u64> = written
            .iter()
            .map(|p| std::fs::metadata(p).unwrap().len())
            .collect();

        // Senza conferma: rifiuto, con tutti i file esistenti, e nulla scritto.
        assert_eq!(
            write(&ex, &tmp.0, "casa", false),
            Err(WriteError::Exists(written.clone()))
        );
        assert_eq!(mtimes(&written), before);

        // Uno solo esiste: rifiuto con quello solo, l'altro non si crea.
        std::fs::remove_file(&written[0]).unwrap();
        assert_eq!(
            write(&ex, &tmp.0, "casa", false),
            Err(WriteError::Exists(vec![written[1].clone()]))
        );
        assert!(
            !written[0].exists(),
            "scritto un file nonostante il rifiuto"
        );
        assert_eq!(mtimes(&written[1..]), before[1..]);

        // Con conferma: si scrive tutto.
        assert_eq!(write(&ex, &tmp.0, "casa", true), Ok(written.clone()));
        assert!(written.iter().all(|p| p.is_file()));
        let after: Vec<u64> = written
            .iter()
            .map(|p| std::fs::metadata(p).unwrap().len())
            .collect();
        assert_eq!(after, lens);
        // Nessun file temporaneo rimasto nella cartella.
        assert_eq!(std::fs::read_dir(&tmp.0).unwrap().count(), 2);
    }

    #[test]
    fn write_reports_io_errors() {
        let tmp = TempDir::new("io");
        let ex = extract(&doc_with_zones(), &opts(&[Elaborato::StatoDiFatto]));
        let missing = tmp.0.join("non-esiste");
        assert!(matches!(
            write(&ex, &missing, "casa", false),
            Err(WriteError::Io(_))
        ));
    }

    #[test]
    fn written_dwg_reloads() {
        let tmp = TempDir::new("reload");
        let mut src = doc_with_zones();
        src.version = DxfVersion::AC1018;
        let ex = extract(&src, &opts(&both()));
        let written = write(&ex, &tmp.0, "casa", false).unwrap();
        assert_eq!(written.len(), ex.docs.len());
        for ((elaborato, doc), path) in ex.docs.iter().zip(&written) {
            let loaded =
                crate::io::load_file(path).unwrap_or_else(|e| panic!("{elaborato:?}: {e}"));
            assert_eq!(
                loaded.entities().count(),
                doc.entities().count(),
                "{elaborato:?}"
            );
            assert_eq!(model_space_count(&loaded), model_space_count(doc));
            assert_eq!(loaded.version, DxfVersion::AC1018, "{elaborato:?}");
            assert_eq!(loaded.header.insertion_units, 6);
            assert_no_archline_data(&loaded);
            assert!(!loaded.app_ids.contains(APP_ZONA));
            let expected = match elaborato {
                Elaborato::DemolizioniCostruzioni => 2,
                _ => 0,
            };
            assert_eq!(hatches(&loaded).len(), expected, "{elaborato:?}");
            assert_eq!(lines_on(&loaded, LAYER_MURI).len(), 12);
        }
    }

    /// Un disegno letto da DWG porta anche i blocchi grezzi dell'XDATA: neanche
    /// quelli devono arrivare nel file estratto.
    #[test]
    fn source_read_from_dwg_loses_raw_xdata_too() {
        let bytes = codec::DwgWriter::write_to_vec(&doc_with_zones()).unwrap();
        let src = codec::DwgReader::from_stream(std::io::Cursor::new(bytes))
            .read()
            .unwrap();
        let zones = crate::bim::zone::zones_in(&src);
        assert_eq!(zones.len(), 2);
        let zona = src.app_ids.get(APP_ZONA).unwrap().handle.value();
        assert!(
            zones.iter().any(|z| src
                .get_entity(z.handle)
                .unwrap()
                .common()
                .extended_data
                .raw_dwg_eed
                .iter()
                .any(|(app, _)| *app == zona)),
            "la lettura DWG non ha lasciato blocchi grezzi: il test non prova nulla"
        );

        let tmp = TempDir::new("raw");
        let ex = extract(&src, &opts(&both()));
        for (_, out) in &ex.docs {
            assert_no_archline_data(out);
            assert!(out
                .entities()
                .all(|e| e.common().extended_data.raw_dwg_eed.is_empty()));
        }
        let written = write(&ex, &tmp.0, "rilievo", false).unwrap();
        for path in &written {
            let loaded = crate::io::load_file(path).unwrap();
            assert_no_archline_data(&loaded);
        }
        let comparativa = crate::io::load_file(&written[1]).unwrap();
        assert_eq!(hatches(&comparativa).len(), 2);
        assert!(crate::bim::zone::zones_in(&comparativa).is_empty());
    }
}
