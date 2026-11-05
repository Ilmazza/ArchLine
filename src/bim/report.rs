//! Avvisi del modulo BIM: i comandi li raccolgono e li scrivono in riga di
//! comando. Coordinate in unità del disegno, aree già in m².

use codec::Handle;

use super::units::{format_decimal, format_m2};

#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    /// Entità del rilievo fuori dal piano XY, ignorate.
    FuoriPiano {
        count: usize,
    },
    ZonaSenzaMuri,
    NuovoSuEsistente,
    StessoStatoSovrapposte,
    ConfiniNonRimossi {
        at: [f64; 2],
    },
    GiunzioneDaVerificare {
        at: [f64; 2],
    },
    TaglioNonChiuso {
        at: [f64; 2],
    },
    NuoveSovrapposte {
        at: [f64; 2],
    },
    VarcoNonSupportato {
        at: [f64; 2],
    },
    /// Un tratto di faccia del muro demolito resta: facce non parallele
    /// all'asse della zona (o tra loro).
    FacciaNonTolta {
        at: [f64; 2],
    },
    NonTagliata {
        handle: Handle,
        kind: String,
    },
    Esclusa {
        handle: Handle,
        kind: String,
    },
    NonCopiata {
        handle: Handle,
        kind: String,
    },
    LocaleFuso {
        names: Vec<String>,
        area_m2: f64,
    },
    /// Contorno ed etichetta di questi locali di fatto non si abbinano con
    /// certezza: dopo una copia, due parti con lo stesso record non si
    /// distinguono più (`rooms::ambiguous_parts`). Nomi e altezze vengono dal
    /// primo abbinamento possibile: da verificare.
    LocaleAmbiguo {
        names: Vec<String>,
    },
    /// Locali di fatto fusi in una regione sola (R55): non si aggiornano.
    LocaliFusiLasciati {
        names: Vec<String>,
    },
    LocaleSenzaNome {
        area_m2: f64,
    },
    SemeInMuroNuovo {
        name: String,
    },
    /// Un seme di progetto nella faccia di un varco, che non è un locale.
    SemeNelVarco {
        name: String,
    },
    /// Un seme in una faccia tutta dentro una zona demolita.
    SemeInDemolizione {
        name: String,
    },
    /// Un locale che si chiude solo con la tolleranza `HPGAPTOL`.
    ChiusoPerTolleranza {
        name: String,
    },
    LocaleNonChiuso {
        name: String,
    },
    /// Un locale di fatto riscritto da `AL_AGGIORNA`: l'area di prima (quella
    /// che diceva l'etichetta, se si legge) e quella nuova, in m².
    LocaleAggiornato {
        name: String,
        old_m2: Option<f64>,
        new_m2: f64,
    },
    /// Un locale di fatto di cui `AL_AGGIORNA` cambia solo il contorno:
    /// l'etichetta (e la sua area, in m²) resta quella.
    ContornoAggiornato {
        name: String,
        area_m2: f64,
    },
    /// Un locale di fatto la cui etichetta nel disegno di lavoro non è quella
    /// dell'estratto (il disegno è vecchio: lo aggiorna `AL_AGGIORNA`).
    /// L'area che dice l'etichetta del disegno, se si legge, e quella
    /// dell'estratto, in m². È un'informazione, non un errore.
    EtichettaDaAggiornare {
        name: String,
        drawing_m2: Option<f64>,
        extract_m2: f64,
    },
    /// Un locale di fatto di cui nel disegno c'è il testo ma non il contorno.
    ContornoMancante {
        name: String,
    },
    /// Un locale di fatto di cui nel disegno c'è il contorno ma non il testo.
    TestoMancante {
        name: String,
    },
    Isole {
        name: String,
        count: usize,
    },
    Varco {
        area_m2: f64,
    },
    ContornoApprossimato {
        name: String,
    },
    /// Un'altezza digitata fuori da 0,50–10 m (in metri): un avviso, non un blocco.
    AltezzaFuoriIntervallo {
        value_m: f64,
    },
    /// Locali di fatto fusi con altezze diverse (o non tutte note): l'altezza
    /// del locale di progetto è `n.d.`. Nomi e altezze in metri, nello stesso ordine.
    AltezzeDiscordi {
        names: Vec<String>,
        values_m: Vec<Option<f64>>,
    },
    /// Lo spessore dichiarato di una zona non è quello misurato dalla geometria.
    SpessoreDiverso {
        code: String,
        declared_m: f64,
        measured_m: f64,
    },
    /// Zone dello stesso stato sovrapposte in modo sostanziale.
    ZoneSovrapposteSostanzialmente {
        codes: Vec<String>,
    },
    /// Una riga di abaco a cui manca un dato: il motivo.
    RigaIncompleta {
        code: String,
        reason: String,
    },
    /// Un elemento che la sezione non disegna (parte 4, R111): `what` dice
    /// quale (un muro a una distanza, un locale, un'apertura), `reason` perché.
    SezioneEsclusa {
        what: String,
        reason: String,
    },
    /// Una parte che il modello 3D non estrude (parte 5, R123): `what` dice
    /// quale (una massa muraria, un'apertura), `reason` perché.
    ModelloEscluso {
        what: String,
        reason: String,
    },
    /// Un elemento con una stratigrafia che il disegno non ha più (parte 6):
    /// `what` è l'elemento (codice della riga o descrizione).
    StratigrafiaMancante {
        what: String,
        name: String,
    },
    /// Una stratigrafia del tipo sbagliato per l'elemento (una parete su un
    /// locale, un solaio su una zona): nessuna riga di strato.
    StratigrafiaTipoDiverso {
        what: String,
        name: String,
        kind: String,
    },
    /// La somma degli strati di una parete non è lo spessore della muratura
    /// (in cm, già formattati).
    StratigrafiaDiversa {
        code: String,
        name: String,
        sum_cm: String,
        thickness_cm: String,
    },
    /// Tabelle d'abaco scritte in un disegno R2000 o precedente: la `TABLE` è
    /// provata solo dal DWG 2004 in poi (spike del Task 7, Ruling X).
    TabellaNonProvataR2000,
}

/// `(4,05; 0,00)`: coordinate con due decimali e virgola, come le aree.
fn point(at: &[f64; 2]) -> String {
    format!("({}; {})", format_decimal(at[0]), format_decimal(at[1]))
}

impl Notice {
    pub fn text(&self) -> String {
        match self {
            Self::FuoriPiano { count } => format!("{count} entità fuori dal piano XY ignorate."),
            Self::ZonaSenzaMuri => {
                "La zona demolita non incrocia nessuna linea del rilievo.".into()
            }
            Self::NuovoSuEsistente => {
                "La zona nuova si sovrappone a un muro esistente non demolito.".into()
            }
            Self::StessoStatoSovrapposte => {
                "La zona si sovrappone in parte a un'altra zona dello stesso stato.".into()
            }
            Self::ConfiniNonRimossi { at } => format!(
                "Zona senza asse in {}: i confini non sono stati rimossi.",
                point(at)
            ),
            Self::GiunzioneDaVerificare { at } => {
                format!("Giunzione da verificare in {}.", point(at))
            }
            Self::TaglioNonChiuso { at } => {
                format!("Taglio non chiuso in {}.", point(at))
            }
            Self::NuoveSovrapposte { at } => {
                format!("Zone nuove sovrapposte solo in parte in {}.", point(at))
            }
            Self::VarcoNonSupportato { at } => format!(
                "Varco non supportato in {}: serve un rettangolo che attraversi il muro.",
                point(at)
            ),
            Self::FacciaNonTolta { at } => format!(
                "Faccia del muro demolito non tolta in {}: facce non parallele all'asse.",
                point(at)
            ),
            Self::NonTagliata { handle, kind } => {
                format!("{kind} {handle:X} attraversa una zona demolita: lasciata intera.")
            }
            Self::Esclusa { handle, kind } => {
                format!("{kind} {handle:X} dentro una zona demolita: esclusa dal progetto.")
            }
            Self::NonCopiata { handle, kind } => {
                format!("{kind} {handle:X} non copiata nel DWG estratto.")
            }
            Self::LocaleFuso { names, area_m2 } => format!(
                "Locali {} fusi in uno ({}): nome da confermare.",
                names.join(" + "),
                format_m2(*area_m2)
            ),
            Self::LocaliFusiLasciati { names } => {
                format!("Locali {} lasciati com'erano.", names.join(" + "))
            }
            Self::LocaleAmbiguo { names } => format!(
                "Locali {}: contorno ed etichetta non si abbinano con certezza (copia o spostamento \
                 delle etichette). Nome e altezza da verificare; AL_ALTEZZA li rifiuta.",
                names.join(" + ")
            ),
            Self::LocaleSenzaNome { area_m2 } => {
                format!("Locale senza nome ({}).", format_m2(*area_m2))
            }
            Self::SemeInMuroNuovo { name } => {
                format!("Il locale di progetto {name} cade dentro un muro nuovo.")
            }
            Self::SemeNelVarco { name } => {
                format!("Il locale di progetto {name} cade nel varco: non nomina nessun locale.")
            }
            Self::SemeInDemolizione { name } => {
                format!("Il locale {name} cade in una zona demolita: non nomina nessun locale.")
            }
            Self::ChiusoPerTolleranza { name } => format!(
                "Locale {name}: chiuso solo dalla tolleranza HPGAPTOL, superficie da verificare."
            ),
            Self::LocaleNonChiuso { name } => {
                format!("Il locale {name} non è più chiuso: lasciato com'era.")
            }
            Self::LocaleAggiornato {
                name,
                old_m2: Some(old),
                new_m2,
            } => format!(
                "Locale {name}: {} → {}.",
                format_decimal(*old),
                format_m2(*new_m2)
            ),
            Self::LocaleAggiornato {
                name,
                old_m2: None,
                new_m2,
            } => format!("Locale {name}: aggiornato a {}.", format_m2(*new_m2)),
            Self::ContornoAggiornato { name, area_m2 } => format!(
                "Locale {name}: contorno aggiornato ({}).",
                format_m2(*area_m2)
            ),
            Self::EtichettaDaAggiornare {
                name,
                drawing_m2,
                extract_m2,
            } => match drawing_m2
                .map(format_decimal)
                .filter(|old| *old != format_decimal(*extract_m2))
            {
                Some(old) => format!(
                    "Locale {name}: etichetta nel disegno {old} → {} nell'estratto.",
                    format_m2(*extract_m2)
                ),
                // L'etichetta non dice l'area, o dice la stessa (cambia la
                // nota «da confermare»).
                None => format!(
                    "Locale {name}: etichetta nel disegno da aggiornare ({} nell'estratto).",
                    format_m2(*extract_m2)
                ),
            },
            Self::ContornoMancante { name } => {
                format!("Locale {name}: c'è il testo ma non il contorno.")
            }
            Self::TestoMancante { name } => {
                format!("Locale {name}: c'è il contorno ma non il testo.")
            }
            Self::Isole { name, count: 1 } => format!("Locale {name}: 1 isola sottratta."),
            Self::Isole { name, count } => format!("Locale {name}: {count} isole sottratte."),
            Self::Varco { area_m2 } => format!("Varco {} escluso dai locali.", format_m2(*area_m2)),
            Self::ContornoApprossimato { name } => {
                format!("Locale {name}: contorno curvo approssimato.")
            }
            Self::AltezzaFuoriIntervallo { value_m } => format!(
                "Altezza {} m fuori dall'intervallo 0,50–10 m: da verificare.",
                format_decimal(*value_m)
            ),
            Self::AltezzeDiscordi { names, values_m } => {
                let values: Vec<String> = values_m
                    .iter()
                    .map(|v| v.map_or("n.d.".to_string(), format_decimal))
                    .collect();
                format!(
                    "Altezze discordi nei locali {} ({}): altezza non disponibile.",
                    names.join(" + "),
                    values.join("; ")
                )
            }
            Self::SpessoreDiverso {
                code,
                declared_m,
                measured_m,
            } => format!(
                "Zona {code}: spessore dichiarato {} m, misurato {} m.",
                format_decimal(*declared_m),
                format_decimal(*measured_m)
            ),
            Self::ZoneSovrapposteSostanzialmente { codes } => format!(
                "Zone {} sovrapposte in modo sostanziale: i totali possono contare due volte.",
                codes.join(" + ")
            ),
            // Un motivo che finisce con `n.d.` ha già il suo punto.
            Self::RigaIncompleta { code, reason } if reason.ends_with('.') => {
                format!("Riga {code} incompleta: {reason}")
            }
            Self::RigaIncompleta { code, reason } => {
                format!("Riga {code} incompleta: {reason}.")
            }
            Self::SezioneEsclusa { what, reason } => {
                format!("Sezione: {what} non disegnato: {reason}.")
            }
            Self::ModelloEscluso { what, reason } => {
                format!("Modello 3D: {what} non estruso: {reason}.")
            }
            Self::StratigrafiaMancante { what, name } => {
                format!("{what}: la stratigrafia «{name}» non c'è più nel disegno.")
            }
            Self::StratigrafiaTipoDiverso { what, name, kind } => {
                format!("{what}: la stratigrafia «{name}» è di tipo {kind}, non va bene qui.")
            }
            Self::StratigrafiaDiversa {
                code,
                name,
                sum_cm,
                thickness_cm,
            } => format!(
                "{code}: gli strati di «{name}» sommano {sum_cm} cm, la muratura è spessa {thickness_cm} cm."
            ),
            Self::TabellaNonProvataR2000 => {
                "TABLE non provata in formato R2000: da verificare in AutoCAD.".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// I testi sono quelli della spec, parola per parola.
    #[test]
    fn notice_texts() {
        let at = [1.0, 2.5];
        let h = Handle::new(0x2A);
        let kind = || "LINE".to_string();
        let cases = [
            (
                Notice::FuoriPiano { count: 3 },
                "3 entità fuori dal piano XY ignorate.",
            ),
            (
                Notice::ZonaSenzaMuri,
                "La zona demolita non incrocia nessuna linea del rilievo.",
            ),
            (
                Notice::NuovoSuEsistente,
                "La zona nuova si sovrappone a un muro esistente non demolito.",
            ),
            (
                Notice::StessoStatoSovrapposte,
                "La zona si sovrappone in parte a un'altra zona dello stesso stato.",
            ),
            (
                Notice::ConfiniNonRimossi { at },
                "Zona senza asse in (1,00; 2,50): i confini non sono stati rimossi.",
            ),
            (
                Notice::GiunzioneDaVerificare { at },
                "Giunzione da verificare in (1,00; 2,50).",
            ),
            (
                Notice::TaglioNonChiuso { at },
                "Taglio non chiuso in (1,00; 2,50).",
            ),
            (
                Notice::NuoveSovrapposte { at },
                "Zone nuove sovrapposte solo in parte in (1,00; 2,50).",
            ),
            (
                Notice::VarcoNonSupportato { at },
                "Varco non supportato in (1,00; 2,50): serve un rettangolo che attraversi il muro.",
            ),
            (
                Notice::FacciaNonTolta { at },
                "Faccia del muro demolito non tolta in (1,00; 2,50): facce non parallele all'asse.",
            ),
            (
                Notice::NonTagliata {
                    handle: h,
                    kind: kind(),
                },
                "LINE 2A attraversa una zona demolita: lasciata intera.",
            ),
            (
                Notice::Esclusa {
                    handle: h,
                    kind: kind(),
                },
                "LINE 2A dentro una zona demolita: esclusa dal progetto.",
            ),
            (
                Notice::NonCopiata {
                    handle: h,
                    kind: kind(),
                },
                "LINE 2A non copiata nel DWG estratto.",
            ),
            (
                Notice::LocaleFuso {
                    names: vec!["Cucina".into(), "Pranzo".into()],
                    area_m2: 11.905,
                },
                "Locali Cucina + Pranzo fusi in uno (11,91 m²): nome da confermare.",
            ),
            (
                Notice::LocaleAmbiguo {
                    names: vec!["Camera".into()],
                },
                "Locali Camera: contorno ed etichetta non si abbinano con certezza (copia o spostamento \
                 delle etichette). Nome e altezza da verificare; AL_ALTEZZA li rifiuta.",
            ),
            (
                Notice::LocaliFusiLasciati {
                    names: vec!["Cucina".into(), "Pranzo".into()],
                },
                "Locali Cucina + Pranzo lasciati com'erano.",
            ),
            (
                Notice::LocaleSenzaNome { area_m2: 4.0 },
                "Locale senza nome (4,00 m²).",
            ),
            (
                Notice::SemeInMuroNuovo {
                    name: "Bagno".into(),
                },
                "Il locale di progetto Bagno cade dentro un muro nuovo.",
            ),
            (
                Notice::SemeNelVarco {
                    name: "Disimpegno".into(),
                },
                "Il locale di progetto Disimpegno cade nel varco: non nomina nessun locale.",
            ),
            (
                Notice::SemeInDemolizione {
                    name: "Ripostiglio".into(),
                },
                "Il locale Ripostiglio cade in una zona demolita: non nomina nessun locale.",
            ),
            (
                Notice::ChiusoPerTolleranza {
                    name: "Bagno".into(),
                },
                "Locale Bagno: chiuso solo dalla tolleranza HPGAPTOL, superficie da verificare.",
            ),
            (
                Notice::LocaleNonChiuso {
                    name: "Bagno".into(),
                },
                "Il locale Bagno non è più chiuso: lasciato com'era.",
            ),
            (
                Notice::LocaleAggiornato {
                    name: "Bagno".into(),
                    old_m2: Some(12.0),
                    new_m2: 11.706,
                },
                "Locale Bagno: 12,00 → 11,71 m².",
            ),
            (
                Notice::LocaleAggiornato {
                    name: "Bagno".into(),
                    old_m2: None,
                    new_m2: 11.7,
                },
                "Locale Bagno: aggiornato a 11,70 m².",
            ),
            (
                Notice::ContornoAggiornato {
                    name: "Bagno".into(),
                    area_m2: 11.91,
                },
                "Locale Bagno: contorno aggiornato (11,91 m²).",
            ),
            (
                Notice::EtichettaDaAggiornare {
                    name: "A".into(),
                    drawing_m2: Some(12.0),
                    extract_m2: 11.7,
                },
                "Locale A: etichetta nel disegno 12,00 → 11,70 m² nell'estratto.",
            ),
            (
                Notice::EtichettaDaAggiornare {
                    name: "A".into(),
                    drawing_m2: None,
                    extract_m2: 11.704,
                },
                "Locale A: etichetta nel disegno da aggiornare (11,70 m² nell'estratto).",
            ),
            (
                Notice::EtichettaDaAggiornare {
                    name: "B".into(),
                    drawing_m2: Some(23.7),
                    extract_m2: 23.7002,
                },
                "Locale B: etichetta nel disegno da aggiornare (23,70 m² nell'estratto).",
            ),
            (
                Notice::ContornoMancante {
                    name: "Bagno".into(),
                },
                "Locale Bagno: c'è il testo ma non il contorno.",
            ),
            (
                Notice::TestoMancante {
                    name: "Bagno".into(),
                },
                "Locale Bagno: c'è il contorno ma non il testo.",
            ),
            (
                Notice::Isole {
                    name: "Bagno".into(),
                    count: 2,
                },
                "Locale Bagno: 2 isole sottratte.",
            ),
            (
                Notice::Isole {
                    name: "Bagno".into(),
                    count: 1,
                },
                "Locale Bagno: 1 isola sottratta.",
            ),
            (
                Notice::Varco { area_m2: 0.09 },
                "Varco 0,09 m² escluso dai locali.",
            ),
            (
                Notice::ContornoApprossimato {
                    name: "Bagno".into(),
                },
                "Locale Bagno: contorno curvo approssimato.",
            ),
            (
                Notice::AltezzaFuoriIntervallo { value_m: 0.4 },
                "Altezza 0,40 m fuori dall'intervallo 0,50–10 m: da verificare.",
            ),
            (
                Notice::AltezzeDiscordi {
                    names: vec!["A".into(), "B".into(), "C".into()],
                    values_m: vec![Some(2.7), Some(3.0), None],
                },
                "Altezze discordi nei locali A + B + C (2,70; 3,00; n.d.): altezza non disponibile.",
            ),
            (
                Notice::SpessoreDiverso {
                    code: "D01".into(),
                    declared_m: 0.12,
                    measured_m: 0.1,
                },
                "Zona D01: spessore dichiarato 0,12 m, misurato 0,10 m.",
            ),
            (
                Notice::ZoneSovrapposteSostanzialmente {
                    codes: vec!["D01".into(), "D02".into()],
                },
                "Zone D01 + D02 sovrapposte in modo sostanziale: i totali possono contare due volte.",
            ),
            (
                Notice::RigaIncompleta {
                    code: "D01".into(),
                    reason: "altezza non disponibile".into(),
                },
                "Riga D01 incompleta: altezza non disponibile.",
            ),
            (
                Notice::TabellaNonProvataR2000,
                "TABLE non provata in formato R2000: da verificare in AutoCAD.",
            ),
        ];
        for (notice, text) in cases {
            assert_eq!(notice.text(), text);
        }
        // Un motivo che finisce con un punto (`n.d.`) non ne prende un altro.
        for reason in ["altezza n.d.", "superficie n.d."] {
            assert_eq!(
                Notice::RigaIncompleta {
                    code: "D1".into(),
                    reason: reason.into(),
                }
                .text(),
                format!("Riga D1 incompleta: {reason}")
            );
        }
        // Virgola decimale anche per le coordinate; negative e arrotondate.
        assert_eq!(
            Notice::GiunzioneDaVerificare { at: [4.05, 0.0] }.text(),
            "Giunzione da verificare in (4,05; 0,00)."
        );
        assert_eq!(
            Notice::TaglioNonChiuso {
                at: [-1250.5, 0.004]
            }
            .text(),
            "Taglio non chiuso in (-1250,50; 0,00)."
        );
    }
}
