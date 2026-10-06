# Pagina iniziale in stile AutoCAD

Stato: approvata dall'utente in chat il 2026-10-07 (disegno breve). Branch `lavoro`.

## 1. Scopo

Nel workspace classico (`ARCHLINE_WORKSPACE` diverso da `ribbon`) la pagina iniziale ha la struttura della pagina
iniziale di AutoCAD 2025: barra laterale a sinistra, «Recent» al centro con schede grandi, colonna facoltativa a
destra. Il contenuto è quello di ArchLine: niente Autodesk Projects, Insights, Sign in, Send feedback. Offline di
default (`ARCHLINE_ONLINE` e `ARCHLINE_PROMO` spenti).

## 2. Struttura

- **Sinistra (280 px).** Titolo «ArchLine»; due pulsanti larghi, Open… e New (niente menu a tendina: non ci sono
  modelli). Navigazione: **Recent** (predefinita) e **Learning** (video tutorial; solo con `ARCHLINE_ONLINE=1`). In
  basso Options e Plugins; con `ARCHLINE_ONLINE=1` anche Online help e Community forum.
- **Centro, «Recent».** Barra con commutatore griglia/elenco, «Sort by» (Last opened, Name, Modified) e campo
  Search; sotto, le schede (circa 196 px): anteprima del DWG, nome su due righe, data di modifica del file. Senza
  anteprima: segnaposto. Clic = apre; la ✕ che toglie il file dall'elenco compare al passaggio del mouse. In fondo
  resta il controllo «Keep N recent files».
- **Destra.** Vuota di default (nessuna colonna). `ARCHLINE_ONLINE=1`: Discussions. `ARCHLINE_PROMO=1`: Supporters.
- **Finestra stretta** (< `MIN_WIDTH`): si ricade sulla pagina a schede già esistente, invariata.
- **`ARCHLINE_WORKSPACE=ribbon`:** pagina di prima.

## 3. Dati e stato

- «Last opened» è l'ordine della lista dei recenti (la più recente in cima). «Modified» e la data sulla scheda sono
  la data di modifica del file, letta dal disco **fuori dal `view`** (come le miniature) e tenuta in cache; se il
  file non c'è, nessuna data (non si inventa nulla). Il fuso è quello corrente (`utc_offset_days`).
- Ordinamento, ricerca, vista e voce di navigazione stanno in memoria (`StartUi`), non nel `settings.json`.
- Una sola variante di messaggio, `Message::Start(StartMsg)`; un solo campo nell'app, `start_ui`.

## 4. File

- `src/ui/classic_start.rs` (nuovo): modello (`StartUi`, `RecentSort`, filtro e ordinamento puri, formato data) e
  viste. Funzioni `#[inline(never)]`.
- `src/app/update/start.rs` (nuovo): gestione di `StartMsg`.
- Punti di aggancio in codice upstream, minimi: `src/app/view/mod.rs` (`start_page_view`: un argomento e una
  diramazione), `src/app/mod.rs` (campo e variante di messaggio), `src/app/recent.rs` (caricamento delle date).

## 5. Test

Filtro e ordinamento come funzioni pure; formato data; un test sul widget reale (una scheda per file, il filtro
della ricerca la toglie, il segnaposto senza anteprima); misura delle schede e della barra laterale. La resa a
schermo la verifica l'utente con uno screenshot (nessuna GPU qui).
