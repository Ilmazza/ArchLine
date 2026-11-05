# BIM parte 6: stratigrafie e particolari — piano

Spec: `docs/superpowers/specs/2026-11-05-bim-particolari-design.md`. Branch `claude/bim-particolari` da
`claude/bim-3d`. Registro: `docs/superpowers/ledgers/2026-11-05-bim-particolari.md` (da R132).

Ogni task: test prima, poi codice; `cargo test --locked --lib bim::<modulo>`; commit e push sul branch.

1. **Modello** (`src/bim/strata.rs`): `Kind {Parete, Solaio}`, `Layer {material, thickness}`, `Stratigraphy {name,
   kind, layers}`, `validate`, `total`; XRecord `ARCHLINE_STRATIGRAFIE` (`read_all`, `write_all`) con l'aiuto comune
   di `record.rs` (`write_named_xrecord`, `named_xrecord`); `parse_layers("a 0.015; b 0.09")`. Test: andata e
   ritorno, DXF/DWG, nomi duplicati, spessori non validi, virgola decimale.
2. **Assegnazione** (`src/bim/strata_assign.rs`): APPID `ARCHLINE_STRATIGRAFIA`, `encode`/`decode`,
   `assignments_in(doc)` → (handle, nome, elemento: zona nuova | seme di progetto); `target_at(doc, point)`;
   controlli `StratigrafiaMancante`, `StratigrafiaTipoDiverso` in `report.rs`.
3. **`AL_STRATIGRAFIA` Nuova/Elenco/Rimuovi** (`src/app/commands/bim_strati.rs`), gancio della riga intera in
   `command_driver/utilities.rs`, registrazione in `bim.rs`; record in un passo a sé.
4. **Assegna/Togli**: selezione o punto, tipo controllato, layer bloccati, un passo di annullamento.
5. **Particolare** (`src/bim/detail.rs`): bande, richiami, testi, titolo; posa e `details_in`, layer
   `AL-PARTICOLARI`, APPID `ARCHLINE_PARTICOLARE`.
6. **`AL_PARTICOLARE`** (`src/app/commands/bim_strati.rs`): posa e sostituzione.
7. **Quantità** (`src/bim/strata_quantities.rs`, `abaco_strata.rs`): righe di strato, `StratigrafiaDiversa`,
   `TableKind::Strati`, CSV.
8. **Estrazione, menu, barra, ribbon, icone** (`ristr_stratigrafia.svg`, `ristr_particolare.svg`; diciotto comandi).
9. **Banco** (sesto giro «strati, »), build release, `CLAUDE.md`, registro chiuso.
