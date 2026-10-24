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
            Self::ContornoMancante { name } => {
                format!("Locale {name}: c'è il testo ma non il contorno.")
            }
            Self::TestoMancante { name } => {
                format!("Locale {name}: c'è il contorno ma non il testo.")
            }
            Self::Isole { name, count } => format!("Locale {name}: {count} isole sottratte."),
            Self::Varco { area_m2 } => format!("Varco {} escluso dai locali.", format_m2(*area_m2)),
            Self::ContornoApprossimato { name } => {
                format!("Locale {name}: contorno curvo approssimato.")
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
                Notice::Varco { area_m2: 0.09 },
                "Varco 0,09 m² escluso dai locali.",
            ),
            (
                Notice::ContornoApprossimato {
                    name: "Bagno".into(),
                },
                "Locale Bagno: contorno curvo approssimato.",
            ),
        ];
        for (notice, text) in cases {
            assert_eq!(notice.text(), text);
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
