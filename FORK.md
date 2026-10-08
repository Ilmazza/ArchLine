# ArchLine — fork di Open CAD Studio

ArchLine è un fork di [Open CAD Studio](https://github.com/HakanSeven12/OpenCADStudio)
(HakanSeven12), mantenuto da Mauro Mazzarelli per il flusso di lavoro di uno studio di
architettura (ristrutturazioni, computi, pratiche edilizie) in ambito normativo italiano.

Progetto indipendente, non affiliato ad Autodesk. "AutoCAD" e "DWG" sono marchi di Autodesk, Inc.

## Licenze e attribuzione
- Codice dell'applicazione: GPL-3.0 (vedi `LICENSE`), invariata rispetto a upstream.
- Il codec DWG/DXF (`opencadcodec`) è distribuito da upstream con licenza MPL-2.0: le sue
  condizioni restano valide per i relativi file.
- Il copyright del codice originale resta ai rispettivi autori. Le modifiche di ArchLine
  sono rilasciate con la stessa licenza.

## Politica di fork
1. `upstream` = HakanSeven12/OpenCADStudio. Merge a ogni release settimanale, mai rebase del `main`.
2. Modifiche proprie confinate, dove possibile, in: `src/ui/ribbon/*`,
   `src/ui/style/fusion_theme.rs`, `src/app/view` (pagina iniziale), `plugins/`, `tests/conformance/`.
3. Correzioni e funzioni generiche vanno proposte a monte come PR separate.

## Piano
| Fase | Contenuto | Stato |
|---|---|---|
| F1 | Rebrand (nome, logo), pagina iniziale senza sponsor/Donate/contenuti da rete, aggiornamenti disattivabili | fatto |
| F2 | Analisi di gap vs AutoCAD (`docs/gap-ocs-vs-autocad.md`) | fatto |
| F3 | Workspace "AutoCAD classico": barra dei menu, barre strumenti Draw/Modify/Layers/Properties, riga di comando ancorata in basso, palette Properties/Layers, tema scuro; ribbon opzionale | fatto |
| F4 | Banco di conformità (`tests/conformance/`, oracolo ezdxf, corpus reale) | parziale: banco attivo con disegni sintetici; mancano i disegni reali e il cancello in CI |
| F5 | Strato architettonico come plugin: locali, abachi, IFC, quantità con fonte e stato | da fare |
| F6 | Contributi a monte (es. codepage R2000: accenti corrotti con header ANSI_1252) | da fare |

Analisi di partenza: repository `Ilmazza/Autocad_clone`, `docs/analisi-open-cad-studio.md`
e `docs/baseline-ocs-f0.md`.

## Decisioni
- Modello di interfaccia: **AutoCAD classico** (scelta dell'utente).
- Nome: ArchLine (scelta dell'utente; verifica di marchio a carico dell'utente).
