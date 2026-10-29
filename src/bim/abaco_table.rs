//! Le tabelle d'abaco come entità `TABLE` (spec parte 2, §5.2).

#[cfg(test)]
mod tests {
    use codec::entities::TableBuilder;
    use codec::tables::AppId;
    use codec::xdata::{ExtendedDataRecord, XDataValue};
    use codec::{Document, DxfVersion, EntityType, Vector3};

    use crate::bim::APP_ABACO;

    /// Una cartella nuova sotto la cartella temporanea, tolta a fine test.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir().join(format!(
                "archline_bim_abaco_{tag}_{}_{nanos}",
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

    /// Spike di fattibilità (spec §5.2, passo 1 del Task 7): una `TABLE` con
    /// celle (anche unite) e l'XDATA `ARCHLINE_ABACO`, salvata in DWG e riletta
    /// come fanno i test di `extract.rs` (`io::save_as_version`, poi
    /// `io::load_file`): il testo delle celle e l'XDATA tornano, nelle versioni
    /// DWG che hanno le tabelle (2004 in poi).
    #[test]
    fn table_survives_dwg_roundtrip() {
        let tmp = TempDir::new("roundtrip");
        for version in [
            DxfVersion::AC1018,
            DxfVersion::AC1021,
            DxfVersion::AC1024,
            DxfVersion::AC1027,
            DxfVersion::AC1032,
        ] {
            let mut doc = Document::new();
            doc.version = version;
            let mut app = AppId::new(APP_ABACO);
            app.handle = doc.allocate_handle();
            doc.app_ids.add(app).unwrap();
            let mut table = TableBuilder::new(3, 3)
                .at(Vector3::new(20.0, 0.0, 0.0))
                .build();
            table.set_cell_text(0, 0, "Demolizioni (m³)");
            table.set_cell_text(1, 0, "D1");
            table.set_cell_text(1, 2, "0,81");
            table.set_cell_text(2, 0, "Varchi (fuori dal totale delle demolizioni)");
            table.merge_cells(codec::entities::table::CellRange::new(2, 0, 2, 2));
            let mut rec = ExtendedDataRecord::new(APP_ABACO);
            for value in [
                XDataValue::Integer16(1),
                XDataValue::String("tabella".into()),
                XDataValue::String("demolizioni".into()),
            ] {
                rec.add_value(value);
            }
            table.common.extended_data.upsert_record(rec);
            let handle = doc.add_entity(EntityType::Table(Box::new(table))).unwrap();

            let path = tmp.0.join(format!("{version:?}.dwg"));
            crate::io::save_as_version(&doc, &path, version)
                .unwrap_or_else(|e| panic!("{version:?}: {e}"));
            let loaded =
                crate::io::load_file(&path).unwrap_or_else(|e| panic!("{version:?}: {e}"));

            let tables: Vec<_> = loaded
                .entities()
                .filter_map(|entity| match entity {
                    EntityType::Table(table) => Some(table.as_ref()),
                    _ => None,
                })
                .collect();
            let [table] = tables[..] else {
                panic!("{version:?}: {} tabelle", tables.len())
            };
            assert_eq!(table.common.handle, handle, "{version:?}");
            assert_eq!(table.cell_text(0, 0), Some("Demolizioni (m³)"), "{version:?}");
            assert_eq!(table.cell_text(1, 0), Some("D1"), "{version:?}");
            assert_eq!(table.cell_text(1, 2), Some("0,81"), "{version:?}");
            assert_eq!(
                table.cell_text(2, 0),
                Some("Varchi (fuori dal totale delle demolizioni)"),
                "{version:?}"
            );
            assert!(
                table.cell(2, 0).is_some_and(|cell| cell.merge_width == 3),
                "{version:?}: cella unita"
            );
            assert!(
                (table.insertion_point.x - 20.0).abs() < 1e-9
                    && table.insertion_point.y.abs() < 1e-9,
                "{version:?}: {:?}",
                table.insertion_point
            );
            let rec = table
                .common
                .extended_data
                .get_record(APP_ABACO)
                .unwrap_or_else(|| panic!("{version:?}: XDATA perso"));
            assert_eq!(
                rec.values,
                [
                    XDataValue::Integer16(1),
                    XDataValue::String("tabella".into()),
                    XDataValue::String("demolizioni".into()),
                ],
                "{version:?}"
            );
        }
    }
}
