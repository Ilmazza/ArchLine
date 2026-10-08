# Finestra "Hatch and Gradient": pattern, solido e sfumato, in creazione e in modifica

Stato: spec da approvare. Estende `2026-10-08-finestra-hatch-design.md` (finestra in creazione, browser dei pattern,
modalità modifica), tutto già nel branch `claude/finestra-hatch`.

## 0. Intesa (da correggere se sbagliata)

**Cosa si vuole.** La stessa finestra deve servire i tre tipi di riempimento, sia quando si crea (HATCH, GRADIENT)
sia quando si modifica (doppio clic, HATCHEDIT). Oggi la scheda Gradient è solo un'etichetta, il colore di un
retino è sempre ByLayer e solidi/sfumati non aprono la finestra in modifica.

**Successo.** (a) HATCH crea pattern o solido con colore scelto; (b) la scheda Gradient crea uno sfumato con forma,
colori, tinta, centratura e angolo; (c) doppio clic/HATCHEDIT aprono la finestra su qualunque retino e OK cambia solo
i campi toccati, anche passando da un tipo all'altro, con un solo undo; (d) il pannello Proprietà e la finestra
scrivono il gradiente con la stessa funzione.

**Dal design approvato** (non rimesso in discussione): due schede = due modi di riempire (Hatch = pattern e solido,
Gradient = sfumato); decide la scheda attiva a OK, non il nome del pattern; 9 forme; "One color" con Tint/Shade;
colore "Use Current" come default; `-GRADIENT` = comando di oggi.

**Cambia rispetto a oggi** (da dire all'utente nella PR):
1. I retini creati dalla finestra prendono il **colore corrente** (`ribbon.active_color`), come ogni altro comando di
   disegno (`commit_entity_handle_with_policies`, `app/properties.rs`). Oggi `add_hatch(.., None)` lascia ByLayer. Con
   colore corrente = ByLayer (il valore di partenza) non cambia nulla; cambia solo se si è scelto un colore esplicito.
2. GRADIENT apre la finestra (prima: comando a un clic, solo piano XY, nessuna area/isola/associativo). Il gradiente
   creato dalla finestra segue il piano di lavoro (UCS) come HATCH.
3. Doppio clic/HATCHEDIT su solidi e sfumati aprono la finestra (prima: una riga in riga di comando).

## 1. Scelte prese dove il design lasciava aperto (da confermare)

| # | Scelta | Motivo |
|---|---|---|
| a | `-HATCH` resta com'è, **anche nel colore** (ByLayer, o quello del retino ereditato). | Gli script contano su quel risultato; il design dice "come oggi". Differenza dichiarata con HATCH. |
| b | HATCH apre sempre sulla scheda Hatch, GRADIENT sulla scheda Gradient. La scheda non è ricordata tra una sessione d'uso e l'altra. | Prevedibile; AutoCAD ricorda l'ultima, ma qui il comando scelto dice cosa si vuole. |
| c | L'angolo dello sfumato è un campo **separato** da quello del pattern (ognuno con la sua scheda). | Come in AutoCAD; il valore del pattern (es. 45°) non deve finire su uno sfumato. |
| d | Le 9 forme sono un **elenco a discesa** + un'anteprima della forma scelta (non una griglia 3×3). | Come nel design ("da elenco con anteprima"); sta nell'altezza esistente. |
| e | In modifica, la scheda **non** corrispondente al retino parte dai valori predefiniti (non da `hatch_last`). | Risultato deterministico e testabile. |
| f | In modifica, **cambiare scheda è già una modifica**: aprire un pattern, passare a Gradient e premere OK lo converte in uno sfumato con i valori mostrati. | "Conta la scheda". Tornare alla scheda di partenza annulla la conversione. |
| g | Convertendo via dal gradiente, `gradient_color` si azzera (poi riscritto se si torna). | Evita colori/nome vecchi in un retino non sfumato che il DXF scriverebbe comunque. |
| h | Il colore del retino in modifica mostra il colore **reale** dell'entità (ByLayer incluso); "Use Current" non esiste in modifica. | In modifica non c'è un "colore corrente" da seguire. |
| i | `ColorPickTarget::Hatch(slot)` accetta per Color 1/2 solo Index/Rgb; ByLayer/ByBlock/None sono ignorati. | Le fermate di un gradiente sono colori veri. |

## 2. Modello

### 2.1 Tipo di riempimento di un retino esistente

`entities::hatch_fill::FillKind { Pattern, Solid, Gradient }` e `FillKind::of(&Hatch)`:
- `Gradient` se `gradient_color.enabled`;
- altrimenti `Solid` se `is_solid` o il nome del pattern è `SOLID` (come l'attuale `is_pattern_hatch`);
- altrimenti `Pattern`.

`is_pattern_hatch` (`hatch_edit_settings.rs`) sparisce: i suoi chiamanti passano a `FillKind`.

### 2.2 Stato della finestra (`modules/draw/draw/hatch_settings.rs`, dati puri)

```
FillTab        = Hatch | Gradient
HatchColor     = UseCurrent | Color(AcadColor)
GradientSettings {
    shape: usize,          // indice in GradientKind::CHOICES (0..9)
    one_color: bool,
    color1: AcadColor, color2: AcadColor,   // solo Index/Rgb; predefiniti: gli attuali di GradientCommand
    tint: f32,             // 0..1, solo con one_color; predefinito 1.0 (come "fill_type" nel pannello Proprietà)
    centered: bool,        // predefinito true  (shift < 0.5)
    angle: String,         // gradi, testo grezzo come Angle del pattern; predefinito "0"
}
HatchSettings += { tab: FillTab, color: HatchColor, gradient: GradientSettings }
```

Predefiniti dei due colori: i valori che `GradientCommand::make_hatch` usa oggi (`[0.30,0.60,0.95]` e `[0.18,0.18,0.18]`
→ RGB 77,153,242 e 46,46,46), così lo sfumato di default non cambia aspetto.

`resolve()` **dipende dalla scheda**: con `tab == Hatch` pretende pattern in catalogo, angolo e scala validi (come
oggi); con `tab == Gradient` pretende solo l'angolo dello sfumato valido (un pattern sparito dal catalogo non deve
impedire di creare uno sfumato). Restituisce
`ResolvedSettings { common: {associative, separate, retain, island_style}, fill: ResolvedFill }` con
`ResolvedFill::Hatch { pattern, angle_rad, scale, color } | ResolvedFill::Gradient(GradientSpec)`. `fields_valid()`,
`can_ok()`, i messaggi di errore e l'abilitazione di Add/Preview/OK/Invio seguono `resolve()`, quindi la scheda attiva.

`hatch_last` conserva **tutto** (entrambe le schede, colore, forma, colori, tinta, centratura, angolo) e si aggiorna
come oggi: solo con Add e OK, solo con valori validi, mai con Preview. `tab` viene sempre sovrascritto all'apertura (b).

`Field` guadagna: `Tab(FillTab)`, `Color(HatchColor)`, `GradientShape(usize)`, `GradientOneColor(bool)`,
`GradientColor1(AcadColor)`, `GradientColor2(AcadColor)`, `GradientTint(f32)`, `GradientCentered(bool)`,
`GradientAngle(String)`, e per il solo stato di vista `ColorList(Option<HatchColorSlot>)` (elenco colori aperto).
`State::apply` li gestisce; in modifica valgono le stesse regole di oggi (Separate/Retain ignorati,
Associative solo per spegnere).

## 3. Vista

Stesso stile (`group`, `labelled`, `dialog_button_styled_opt`). Le due etichette "Hatch"/"Gradient" diventano
pulsanti-scheda (`Field::Tab`). Tre colonne come oggi; **l'altezza massima resta `sized_flow(ex, 940, 760, …)`**.

**Scheda Hatch.** Come oggi, con: la riga "Color" (oggi grigia) diventa il selettore di §4 più un pulsante piccolo
"Use Current" (disabilitato se già selezionato, assente in modifica). La riga prende il posto di quella grigia: l'altezza
della colonna sinistra non aumenta.

**Scheda Gradient.**
- Colonna sinistra: gruppo *Color* (radio "One color" / "Two colors"; Color 1; con "Two colors" Color 2, con
  "One color" il cursore *Tint/Shade* da 0 a 1 con il valore accanto); gruppo *Gradient pattern* (elenco delle 9
  forme, etichette `GradientKind::choice_label`, + swatch §8 della forma e dei colori scelti); gruppo *Orientation*
  (Centered, Angle con la stessa validazione e lo stesso messaggio d'errore di Angle del pattern).
- Colonna centrale e destra: invariate (Boundaries, Options, Islands, Boundary retention, grigi). Add: Pick points /
  Select objects funzionano come nella scheda Hatch.
- Non compaiono: Type and pattern, Angle and scale, Hatch origin (lo sfumato non ha pattern né origine).
- La colonna sinistra stimata è ~440 px, ben dentro 760: nessun rischio di tagliare OK/Preview. Resta comunque un
  punto da verificare a schermo dall'utente (il difetto precedente è nato da una stima).

## 4. Selettore colore e finestra "Select Color"

Si riusa `ui::color_select::color_selector_with_name` (dropdown con ACI 1-9 + "Select Color...") e la finestra
condivisa (`Message::OpenColorWindow` / `ColorWindowPick`, già usata da ModalKind::Layers).

- `app::ColorPickTarget` guadagna `Hatch(HatchColorSlot)`, con `HatchColorSlot { Fill, Gradient1, Gradient2 }`
  (definito in `ui/window/hatch_dialog.rs`). Un ramo in `on_color_window_pick` (`app/update/style.rs`) di poche righe
  chiama `hatch_dialog_color_picked(slot, colore)` (file proprio, `#[inline(never)]`), che emette il `Field` giusto.
  Se la finestra Hatch è già chiusa, non fa nulla.
- Voci dell'elenco: `Fill` → ByLayer e ByBlock sì; `Gradient1/2` → no (vedi scelta i).
- "Select Color..." nell'elenco: chiude l'elenco (`Field::ColorList(None)`) e apre la finestra con
  `OpenColorWindow(Hatch(slot), colore corrente dello slot)`. Per `Fill` con "Use Current", il colore mostrato nella
  finestra è `ribbon.active_color`.
- **Tastiera con la finestra Hatch aperta** (`app/update/mod.rs`, poche righe che delegano a una funzione
  `#[inline(never)]` nel file proprio):
  - Esc: se `color_pick_target` è aperto → `CloseColorPicker` e basta; altrimenti se un elenco colori è aperto →
    lo chiude e basta; altrimenti come oggi (palette, poi finestra).
  - Invio: se "Select Color" è aperta nella pagina *Index* → `ColorWindowPick(colore in sospeso)`; nella pagina
    *True Color* → ignorato; altrimenti se un elenco colori è aperto → lo chiude e basta; altrimenti OK come oggi.
  - La regola vale **solo** per ModalKind::Hatch (le altre finestre che aprono "Select Color" hanno lo stesso
    difetto potenziale: non si toccano, segnalato).
- **Cicli di vita.** `hatch_dialog_cancel` (Cancel, X, Esc, cambio/chiusura tab, comando nuovo, scarto di tab sporco)
  chiude anche la finestra "Select Color" se il suo target è `Hatch(_)`. `OK` non può partire con "Select Color"
  aperta (la finestra è un overlay modale), ma il guard sul target resta.

## 5. Creazione

### 5.1 Comandi

- `HATCH` (non programmatico): `hatch_dialog_open(FillTab::Hatch)`. `GRADIENT` (non programmatico):
  `hatch_dialog_open(FillTab::Gradient)`, `last_cmd = "GRADIENT"`.
- `-HATCH` e `-GRADIENT` = il corpo di oggi. L'arm `"GRADIENT"` in `app/commands/draw.rs` segue lo schema di HATCH
  (`"GRADIENT" if !self.scripted_dispatch` → finestra; `"GRADIENT" | "-GRADIENT"` → `GradientCommand`). I canali
  programmatici (stessi di HATCH: plugin host, control/MCP/REST, automazione, `.scr`) ottengono `-GRADIENT`.
  Registrazione: `names: &["GRADIENT", "-GRADIENT"]` e `"-GRADIENT"` nell'elenco di `app/commands/mod.rs`.
- `GradientCommand` non cambia (resta il comando da riga di comando, piano XY).

### 5.2 `HatchCommand` e dove si risolve il colore

**Contratto.** `ResolvedSettings` è dato puro: non conosce `ribbon.active_color`, la trasparenza corrente, i layer né
il documento, e non li acquisisce. La risoluzione a runtime sta **solo** nell'app (`hatch_dialog_ok`,
`hatch_dialog_preview`), che legge il tab proprietario (`owner_tab_id`) e il suo documento.

- `hatch_command_from_state(state, document_origin)` resta pura (stato + origine). Costruisce il comando con
  `with_settings(&ResolvedSettings)`, che imposta soltanto `fill: FillChoice { Pattern(override) | Gradient(GradientSpec) }`
  e le opzioni geometriche (associativo, separati, contorni, stile isole, angolo, scala). `pattern_override` esistente
  resta per `-HATCH` (P).
- Due builder separati, chiamati solo dall'app, nessuno dei quali tocca `ResolvedSettings`:
  - `HatchCommand::with_creation_style(Option<(Color, Transparency)>)`, chiamato da `hatch_dialog_ok` dopo la
    costruzione. Il colore è: `UseCurrent` → `ribbon.active_color`; `Color(c)` → `c`. La trasparenza è
    `scene.document.current_entity_transparency()` del tab proprietario. Per `Gradient` si passa `None`: il colore
    dell'entità non entra nel disegno.
  - `HatchCommand::with_preview_color(Option<[f32;4]>)`, chiamato da `hatch_dialog_preview`: colore RGBA con alpha 0.75
    (`UseCurrent` → `ribbon.active_color`; `ByLayer` → colore del layer corrente del tab proprietario; `ByBlock` → lo
    stesso colore di ripiego del rendering per un ByBlock fuori da un blocco). `None` lascia il blu di oggi (solo
    `-HATCH`). Per il gradiente non serve: i colori vengono dal `GradientSpec`.
- **Il comando di Add** (`HatchCommand::collecting`) non riceve né stile né colore di anteprima: raccoglie aree e basta.
- `make_hatch` con `Gradient`: stesso modello di oggi (anelli, fonti, percorsi, piano di riempimento, stile isole) ma
  `pattern: HatchPattern::Gradient{..}` con `color2 = spec.effective_color2()` (vedi §7: **mai** il `color2` grezzo),
  `one_color` e `tint` della spec, `name = kind.dxf_name(invert)`, `color = colore 1`, `angle_offset =` angolo dello
  sfumato, `pattern_origin = None`, nessuna regolazione di famiglie.
- `on_enter` con `creation_style`: tutti i rami restituiscono le varianti con stile (`CommitStyledHatch`,
  `CommitHatches{entity_style}`, `CommitHatchWithBoundaries{entity_style}`); senza stile restano come oggi.
- **Anteprima.** `preview_models` smette di forzare `[0.15,0.55,1.0,0.75]`: per pattern/solido usa il colore di
  `with_preview_color`; per il gradiente usa `effective_color2()` e colore 1 con alpha 0.75 (anche il secondo, che in
  `GradientCommand` oggi ha alpha 0). In "One color" il colore 2 dell'anteprima è quindi lo stesso che l'entità
  ricostruita mostrerà dopo il commit. Il filtro anelli per stile isole è invariato.

### 5.3 `Scene::add_hatch`

`HatchPattern::Gradient` guadagna `one_color: bool` e `tint: f32` (i siti che costruiscono la variante a mano sono
pochi: `GradientCommand::make_hatch`, `hatch_model_from_dxf`; gli altri usano `..`; lo dirà il compilatore).
Il blocco gradiente di `add_hatch` (oggi ~2947-2987) sparisce e chiama `hatch_fill::apply_gradient` (scrittura completa, §7) con la
`GradientSpec` ricavata dal modello (colore 1 = `model.color`, `one_color`, `tint`, `color2` = quello del modello, già
effettivo): così `is_single_color`, `color_tint` e le fermate si scrivono **sempre** qui, con la scrittura canonica
completa. `gradient_tint_color` si sposta (pubblica) in `hatch_fill.rs`, dove vive l'unico `effective_color2`.

## 6. Modifica

### 6.1 Apertura

`hatch_dialog_open_edit` accetta qualunque `EntityType::Hatch`; sparisce il messaggio "solid and gradient hatches are
edited from the Properties panel…". `hatchedit_window_target`, `hatch_double_click_on_grip`,
`hot_pattern_hatch_grip` smettono di filtrare per `is_pattern_hatch` (un retino solido/sfumato associativo ha
comunque il grip centrale: `Grippable for Hatch`, grip 0 se `is_associative`). `-HATCHEDIT` e il suo comando restano.

`EditTarget::from_hatch` guadagna `kind: FillKind` e riempie `initial: HatchSettings`:
- `tab` = Hatch per Pattern/Solid, Gradient per Gradient;
- scheda del tipo del retino: dai valori del retino (pattern, angolo, scala come oggi; per un solido il pattern è
  `SOLID`; per uno sfumato `GradientSettings` da `hatch_fill::read_gradient`);
- l'altra scheda: valori predefiniti (scelta e);
- `color` = `HatchColor::Color(colore dell'entità)`, mai UseCurrent (scelta h).

### 6.2 Cosa cambia a OK (`EditTarget::changes`)

```
HatchEditChanges {
    fill: Option<FillEdit>,
    color: Option<AcadColor>,           // solo scheda Hatch, se diverso da quello di partenza
    style: Option<HatchStyleType>, disassociate: bool, origin: Option<[f64;2]>,
}
FillEdit =
    Pattern { pattern: Option<String>, scale: Option<f32>, angle_deg: Option<f32> }   // retino Pattern/Solid, scheda Hatch
  | ToPattern { name, scale, angle_deg }          // retino Gradient, scheda Hatch (con name == SOLID diventa solido)
  | Gradient(GradientPatch)                       // retino Gradient, scheda Gradient: solo i campi cambiati e visibili
  | ToGradient(GradientSpec)                      // retino Pattern/Solid, scheda Gradient: completo
```
Regole: conta la scheda attiva rispetto a `kind` (scelta f). `fill == None` solo se la scheda è quella del tipo del
retino e nessun suo campo è cambiato. Le modifiche valgono per la scheda attiva; quelle fatte sull'altra scheda
sono ignorate. Stile isole, associativo e origine valgono per tutti i tipi (l'origine solo dalla scheda Hatch: lo
sfumato non ne ha). `can_ok` = campi validi della scheda attiva e `changes` non vuoto.

**Campi nascosti non contano.** Il modo *finale* (`one_color` della scheda, non quello di partenza) decide quali
campi dello sfumato sono visibili: con "One color" una differenza di `color2` è ignorata, con "Two colors" lo è una
differenza di `tint`. Cambiare solo un campo nascosto non produce alcuna modifica e non abilita OK. Il passaggio
One color ↔ Two colors è invece una modifica a sé (`one_color: Some(..)`); in quel caso la `GradientPatch` porta anche
i campi che diventano visibili solo se differiscono dal valore di partenza.

### 6.3 Applicazione: un'operazione, un undo

Nuova `HatchEditOperation::Window(Box<HatchWindowEdit>)` (`command.rs`), con `HatchWindowEdit` = i campi di
`HatchEditChanges` più i valori di partenza che servono a non toccare ciò che non è cambiato. Il ramo in
`app/command_driver/modify.rs` fa: controlli di layer bloccato/entità sparita (già presenti), **un solo**
`push_undo_snapshot("HATCHEDIT")`, `hatch_fill::apply_window_edit(&mut Hatch, &edit)`, `bump_entities(Modified)`,
`refresh_properties`. `EditTarget::apply_result` produce `CmdResult::HatcheditApply` con `Window(..)` (nome/scala/
angolo di contorno = quelli del retino; non letti).

`Appearance` e `Update` restano per `-HATCHEDIT`; il corpo di `Update` (pattern, scala, angolo, origine, disassocia,
stile) viene estratto in `hatch_fill::apply_pattern_update` e richiamato da `Update` **e** da `Window`: nessuna copia.

## 7. Funzione condivisa: `src/entities/hatch_fill.rs` (file nuovo, solo su `&mut Hatch`)

```
FillKind
GradientSpec { kind, invert, one_color, color1, color2, tint, angle_rad, centered }
    effective_color2(&self) -> AcadColor     // one_color: gradient_tint_color(color1, tint) in Rgb; altrimenti color2
GradientPatch { kind: Option<(GradientKind,bool)>, one_color: Option<bool>, color1/color2: Option<Color>,
                tint: Option<f64>, angle_rad: Option<f64>, centered: Option<bool> }
read_gradient(&Hatch) -> GradientSpec                 // default per i campi mancanti

apply_gradient(&mut Hatch, &GradientSpec)             // scrittura CANONICA COMPLETA: creazione e ToGradient
apply_gradient_patch(&mut Hatch, &GradientPatch)      // SOLO i campi presenti e i loro campi fisici dipendenti

set_catalog_pattern(&mut Hatch, &PatternEntry, scale, angle)   // pattern o SOLID; spegne il gradiente (scelta g)
apply_pattern_update(&mut Hatch, name, scale, angle, origin, disassociate, style)
apply_window_edit(&mut Hatch, &HatchWindowEdit)
gradient_tint_color(base, target) -> [f32;4]          // spostata da scene/entity.rs
gradient_profile(kind, invert, t) -> f32              // stessa curva dello shader, per swatch e test
```

**Un solo calcolo del colore 2.** `GradientSpec::effective_color2` è l'unico posto che decide il colore 2 reale:
lo usano `HatchCommand::make_hatch` (modello e anteprima), `apply_gradient` (seconda fermata), lo swatch e
`hatch_model_from_dxf` (lettore). Così l'anteprima di un "One color" non può mostrare un vecchio Color 2 mentre
l'entità ricostruita mostra la tinta.

**`apply_gradient` (completa)** scrive tutto da zero: `gradient_color.enabled = true`, `name = kind.dxf_name(invert)`,
**entrambi** gli angoli (`gradient_color.angle` e `pattern_angle`), `shift = if centered {0} else {1}`,
`is_single_color`, `color_tint`, le due fermate (`value` 0 e 1: colore 1 e `effective_color2()`; per Linear invertito si
scambiano, come fa oggi `add_hatch`), `is_solid = true`, e se il retino non era uno sfumato sostituisce `pattern` col
pattern pieno (`Hatch::solid()`).

**`apply_gradient_patch` (parziale)** non ricostruisce né normalizza nulla: per ogni campo presente scrive solo ciò che
quel campo possiede, e lascia intatti gli altri byte dell'entità.

| Campo della patch | Scrive |
|---|---|
| `kind` | solo `name` (come `"gradient_type"` oggi) |
| `one_color` | `is_single_color`; passando a "One color" senza `tint` nella patch, `color_tint = 1.0` (come `"fill_type"` oggi) |
| `color1` / `color2` | solo la fermata corrispondente (creandola se manca, come il ciclo di oggi); `color2` resta memorizzato anche in "One color" |
| `tint` | solo `color_tint` (clamp 0..1) |
| `angle_rad` | `gradient_color.angle` **e** `pattern_angle` (i due restano allineati) |
| `centered` | solo `shift` |

Se il retino non è uno sfumato (`gradient_color.enabled == false`) la patch non converte: la conversione è sempre
`apply_gradient` con una `GradientSpec` completa. Usi: creazione e `ToGradient` → `apply_gradient`; modifica di uno
sfumato esistente (`FillEdit::Gradient`) e pannello Proprietà → `apply_gradient_patch`.

I quattro siti esistenti passano da qui:
1. `src/entities/hatch.rs` `apply_geom_prop`: `fill_type`, `gradient_type`, `gradient_centered`, `gradient_tint`,
   `pattern_angle` (ramo gradiente) → `apply_gradient_patch` con un solo campo.
2. `src/app/update/mod.rs` ~7268 (colori 1/2 dal pannello Proprietà) → `apply_gradient_patch(GradientPatch{color1|color2})`; scompare il
   ciclo che spinge fermate a mano. La riga `populate_hatches_from_document` che segue resta.
3. `src/app/update/command.rs` `on_prop_hatch_pattern_changed` → `set_catalog_pattern`.
4. `src/app/command_driver/modify.rs` ramo `Update` → `apply_pattern_update`; ramo `Window` → `apply_window_edit`.

## 8. Swatch (`src/ui/properties.rs`, `HatchPatternPreview`)

Si estende sul posto: `with_colors(color1, color2: Option<..>)` e `with_gradient(kind, invert, angle_deg, centered)`.
- Pattern: linee nel colore scelto (oggi colore del testo del tema); `UseCurrent` → colore corrente risolto.
- Solido: riempimento nel colore scelto (oggi grigio).
- Il colore 2 dello swatch è sempre `GradientSpec::effective_color2()` (in "One color" la tinta, non il Color 2 nascosto).
- Gradiente lineare: `canvas::Gradient::Linear` esatto, con le fermate che seguono `gradient_profile` (Linear: 2;
  Cylinder: 3 a 0/0.5/1; Curved: ~8 per approssimare t²).
- Sferico/emisferico: `canvas::Gradient` è solo lineare, quindi si disegnano ~16 ellissi concentriche dall'esterno al
  centro, ritagliate al riquadro, con t = distanza/raggio (emisferico: √t) e fermate esterno→centro come nello shader
  (il colore 2 al centro). **Approssimazione dichiarata**: va confermata a schermo dall'utente.
- Mai riquadro vuoto: con colori non validi o dimensioni nulle si cade su un riempimento tinto, come per i pattern.
- Il limite di 400 segmenti e la tinta per scale minuscole dei pattern restano.

## 9. Modifiche al codice

Nuovi: `src/entities/hatch_fill.rs`.
Estesi, nei file propri: `modules/draw/draw/hatch_settings.rs` (dati puri), `hatch_edit_settings.rs`,
`hatch.rs` (`HatchCommand`, `GradientCommand::make_hatch` per i campi nuovi), `ui/window/hatch_dialog.rs`,
`app/commands/hatch_dialog.rs`.
Punti d'aggancio in codice upstream (minimi, ognuno con test): `app/mod.rs` (`ColorPickTarget::Hatch`),
`app/update/style.rs` (ramo), `app/update/mod.rs` (Esc/Invio, poche righe), `app/commands/draw.rs`
(arm GRADIENT), `app/commands/mod.rs` (`-GRADIENT`), `command.rs` (`HatchEditOperation::Window`),
`app/command_driver/modify.rs`, `scene/entity.rs` (`add_hatch`, `gradient_tint_color`), `scene/model/hatch_model.rs`
(due campi nella variante), `entities/hatch.rs`, `ui/properties.rs`.
Nessuna modifica a `locale_catalog.rs`: le stringhe nuove (`t!` in inglese) ricadono sull'inglese.

## 10. Rischi

1. **Stack di `rustc`** (trappola n. 1): `view_main`, `update_message`, `apply_cmd_result_inner`,
   `on_viewport_left_press/release`: solo poche righe che delegano a `#[inline(never)]` nei file propri. `.cargo/config.toml`
   non si tocca.
2. **Altezza della finestra**: la scheda Gradient deve stare in 760; non si può verificare senza GPU.
3. **Modello scena dopo la conversione**: `bump_entities(Modified)` deve ricostruire il `HatchModel` quando il tipo
   cambia (pattern↔solido↔sfumato). Il piano lo verifica con un test sul `model.pattern` dopo OK; se non basta,
   si usa `populate_hatches_from_document` come fa già il pannello Proprietà per i colori.
4. **`HatchPattern::Gradient` più largo**: i `match` senza `..` non compilano finché non si aggiornano (voluto).
5. **Gradiente sul piano UCS**: `fill_plane` ora c'è anche per lo sfumato creato dalla finestra. Il comportamento del
   renderer su un piano ruotato va visto dall'utente.
6. **Formato**: il gradiente esiste nel DWG/DXF solo da R2004; il PDF lo esporta come media dei due colori (già così).

## 11. Test

Regola per tutti: prima rosso, poi verde; ogni test di conteggio deve anche verificare il **contenuto** (valori dei
campi dell'entità, non solo "c'è un retino"); per i punti segnati (M) si dichiara la mutazione che deve farlo fallire.

**Dati puri** (`hatch_settings`, `hatch_edit_settings`, `hatch_fill`)
- `resolve()` per scheda: con Gradient un pattern assente dal catalogo non blocca; con Hatch sì; angolo non valido
  blocca la scheda attiva e non l'altra. (M: ignorare `tab`.)
- `FillKind::of` per i quattro casi (gradiente con `is_solid` vero, `SOLID` per nome, pattern, solido).
- `apply_gradient` (completa): scrive `is_single_color` e `color_tint` (M: non scriverli), entrambi gli angoli,
  `shift`, nome, due fermate con la seconda = `effective_color2()`; Linear invertito scambia le fermate; da pattern
  converte e azzera le linee del pattern.
- `apply_gradient_patch` (parziale): per **ogni** campo della patch, preso da solo, l'entità resta identica byte per
  byte tranne i campi della tabella di §7 (M: una patch che riscrive fermate/tinta/angoli); "One color" senza tinta
  → 1.0; `angle_rad` aggiorna entrambi gli angoli; su un retino non sfumato non converte.
- `effective_color2`: Two colors → `color2`; One color → `gradient_tint_color(color1, tint)` in Rgb, per tinta
  0 / 0.5 / 1 (M: restituire sempre `color2`).
- `set_catalog_pattern` da gradiente: `gradient_color.enabled == false` e azzerato; da pattern a SOLID: `is_solid`.
- `changes`: stessa scheda senza modifiche → vuoto; cambio scheda → `ToGradient`/`ToPattern` (anche senza altri
  campi); ritorno alla scheda di partenza → vuoto; un solo campo gradiente → `Gradient` con quel solo campo;
  colore cambiato solo con scheda Hatch; campi dell'altra scheda ignorati. (M: calcolare le modifiche senza guardare `tab`.)
- `changes`, campi nascosti: in One color una differenza di `color2` è vuota e `can_ok` resta falso; in Two colors
  una differenza di `tint` idem; il passaggio One↔Two è una modifica. (M: confrontare tutti i campi.)
- `ResolvedSettings` non dipende dall'app: `resolve()` si chiama senza `OpenCADStudio` (garantito dalla firma) e non
  contiene colore corrente, trasparenza né layer.
- `gradient_profile` e `gradient_tint_color`: valori noti (cylinder a 0/0.5/1, curved t², tinta 0/0.5/1).

**Creazione** (flusso `app.update(Message::…)`)
- Scheda Hatch: pattern e SOLID con "Use Current" → `common.color == ribbon.active_color`; con colore esplicito → quello;
  con colore corrente ByLayer → ByLayer come prima. (M: ignorare il colore.)
- Scheda Gradient: una forma per ciascuna delle 9, "One color" con tinta (`is_single_color`, `color_tint`), Two colors
  con i due colori, Centered vero/falso, angolo; il documento contiene `gradient_color` esatto e il `HatchModel` ricostruito
  ha la variante `Gradient` con gli stessi valori. Il nome "SOLID" **non** decide: scheda Hatch+SOLID → solido,
  scheda Gradient → sfumato. (M: decidere dal nome.)
- GRADIENT apre sulla scheda Gradient; HATCH su Hatch; `-GRADIENT`, `-HATCH` e i canali programmatici
  (`assert_runs_dialog_free`) non aprono finestra; `-GRADIENT` produce lo stesso sfumato di oggi.
- Add: Pick points / Select objects dalla scheda Gradient raccolgono aree; OK con isole (Normal/Outer/Ignore),
  associativo e "separati" creano il numero giusto di sfumati; undo a un passo.
- Anteprima: i colori del modello sono quelli scelti (non il blu), anche il secondo con alpha 0.75; pattern con
  "Use Current" e ribbon rosso → anteprima rossa; con `ByLayer` → colore del layer corrente del tab proprietario.
- Anteprima **One color**: il `color2` del modello di anteprima è uguale a `gradient_tint_color(color1, tint)` ed è
  uguale al `color2` del `HatchModel` ricostruito dall'entità dopo OK; cambiando il Color 2 nascosto l'anteprima non
  cambia. (M: usare `color2` grezzo.)
- Il comando di Add non porta stile né colore di anteprima; OK passa lo stile risolto dal tab proprietario.
- `hatch_last` conserva entrambe le schede dopo Add/OK; Preview non lo tocca.

**Modifica**
- Invertire: `a_solid_or_gradient_hatch_does_not_open_the_window`, `hatchedit_on_a_solid_hatch_runs_the_command_line`,
  `double_clicking_a_solid_hatch_only_writes_a_line`, `double_clicking_a_gradient_hatch_only_writes_a_line`
  (`app/commands/hatch_dialog.rs` ~2983, ~3116, ~3237, ~3248) e `only_pattern_fills_are_edited_in_the_window`
  (`hatch_edit_settings.rs` ~274): ora aprono la finestra sulla scheda giusta, con i valori del retino.
- Apertura su solido e sfumato da doppio clic, HATCHEDIT, doppio clic sul grip centrale e sul grip già "caldo" (retino
  associativo senza linee di pattern).
- OK applica solo i campi cambiati: per ognuno dei tre tipi un campo alla volta (colore, forma, un colore, tinta,
  angolo, centratura, pattern, scala, angolo, stile isole) e verifica che **tutto il resto** sia byte per byte quello di prima.
- Conversioni: pattern→solido, pattern→sfumato, solido→pattern, solido→sfumato, sfumato→pattern, sfumato→solido
  (stesso nome "SOLID": il caso che oggi non funziona); ognuna con **un solo undo** (profondità dello stack +1) e con
  undo che ripristina l'entità identica (M: due snapshot).
- Modifica con "separati"/"retain" ancora ignorati; origine specificata solo dalla scheda Hatch.
- `-HATCHEDIT` invariato (stessi test di oggi).
- Pannello Proprietà: ogni campo gradiente produce lo stesso risultato di prima (test di non regressione) e passa
  da `apply_gradient_patch`; cambiare solo la forma non tocca fermate, tinta e angoli.
- Modifica di uno sfumato: cambiare solo la forma (o solo la centratura) lascia fermate, tinta e angoli identici.

**Select Color e tastiera**
- Esc con "Select Color" aperta chiude solo quella (la finestra Hatch e il suo stato restano); Esc con elenco colori
  aperto chiude solo l'elenco; Esc senza nulla chiude la finestra come oggi.
- Invio con "Select Color" aperta (pagina Index) applica il colore alla fessura giusta e **non** crea né modifica
  retini; nella pagina True Color non fa nulla; Invio con l'elenco aperto lo chiude e non crea nulla.
- La scelta arriva alla fessura giusta (Fill, Color 1, Color 2); un Color 1/2 ByLayer/ByBlock è ignorato.
- Cancel, cambio tab e chiusura del tab proprietario con "Select Color" aperta la chiudono e scartano lo stato.
- La finestra "Select Color" aperta da un'altra parte (Layers) non è toccata dalle regole Hatch.

**Swatch**
- Pattern/solido con colore: colore usato; gradiente lineare: fermate di `canvas::Gradient::Linear` coerenti con
  `gradient_profile`; sferico/emisferico: numero di anelli e ordine dei colori; mai vuoto con input non validi.

## 12. Fuori da questo giro e problemi noti non toccati

Fuori: Transparency, Layer, Draw order, Annotative, Inherit Properties, Gap tolerance, Boundary set, Remove/Recreate
boundaries e View selections, Double/Spacing/ISO pen width, tipi User defined e Custom, Preview in modifica, griglia 3×3
delle forme, colore di sfondo del pattern (XDATA `HATCHBACKGROUNDCOLOR`), linetype/lineweight correnti per i retini
(gli altri comandi li applicano, HATCH non li ha mai applicati).

Segnalati, non risolti qui: prompt di HATCHEDIT con segnaposti `%__ocs_fmt_0__`; HATCHEDIT senza selezione cliccando sul
contorno prende la linea; doppio clic con 1 px di jitter su retino già selezionato; con "separati" dopo OK resta
selezionato solo l'ultimo retino; Esc/Invio con "Select Color" aperta in altre finestre (Layers…).

## 13. Documentazione da aggiornare alla fine

`CLAUDE.md`, paragrafo della finestra Hatch: togliere "tab Gradient, Color/…" da "Fuori dal giro", aggiungere
`hatch_fill.rs`, `-GRADIENT`, il colore "Use Current" e `ColorPickTarget::Hatch`.

## 14. Controlli manuali su Windows (lista da consegnare a fine lavoro)

1. HATCH: scheda Hatch, riga Color con "Use Current"; scegliere rosso, creare: il retino è rosso; "Use Current": segue il colore corrente del ribbon.
2. GRADIENT: apre sulla scheda Gradient; la finestra mostra OK/Preview/Cancel interi, senza tagli.
3. Le 9 forme, "One color" con cursore, "Two colors", Centered, Angle: confrontare swatch e disegno reale, soprattutto sferico/emisferico/curved.
4. Preview di uno sfumato e di un pattern colorato: i colori mostrati sono quelli scelti.
5. Add: Pick points / Select objects dalla scheda Gradient; isole; associativo; "separati".
6. Doppio clic su pattern, solido e sfumato (e sul punto centrale se il retino è già selezionato); HATCHEDIT.
7. Conversioni in modifica nei sei sensi + un solo Ctrl+Z ciascuna.
8. "Select Color" aperta dalla finestra: Esc chiude solo quella, Invio la conferma; poi OK/Esc della finestra Hatch come prima.
9. Sfumato su UCS ruotato; salvataggio in DXF R2004+ e riapertura (forma, colori, tinta, centratura, angolo).
10. Pannello Proprietà su uno sfumato creato dalla finestra: tipo di colore, tinta, colori e forma coerenti.
