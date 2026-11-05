//! Scheda «Ristrutturazione» del ribbon (workspace OpenCAD Studio): gli stessi
//! quindici comandi `AL_*` di Tools > Ristrutturazione e della barra omonima del
//! workspace AutoCAD Classic, con le stesse icone (`assets/icons/ristr_*.svg`).
//! Modulo proprio di ArchLine: nessun comando nuovo, solo l'accesso dal ribbon.

use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

pub struct RistrutturazioneModule;

/// I quindici comandi in ordine di gruppo: id, etichetta, icona.
fn command(id: &'static str, label: &'static str, icon: &'static [u8]) -> RibbonItem {
    RibbonItem::LargeTool(ToolDef {
        id,
        label,
        icon: IconKind::Svg(icon),
        event: ModuleEvent::Command(id.to_string()),
    })
}

impl CadModule for RistrutturazioneModule {
    fn id(&self) -> &'static str {
        "ristrutturazione"
    }

    fn title(&self) -> &'static str {
        "Ristrutturazione"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                RibbonGroup {
                    title: "Stato",
                    tools: vec![
                        command(
                            "AL_DEMOLISCI",
                            "Demolisci",
                            include_bytes!("../../../assets/icons/ristr_demolisci.svg"),
                        ),
                        command(
                            "AL_NUOVO",
                            "Nuovo",
                            include_bytes!("../../../assets/icons/ristr_nuovo.svg"),
                        ),
                        command(
                            "AL_STATO",
                            "Cambia stato",
                            include_bytes!("../../../assets/icons/ristr_stato.svg"),
                        ),
                    ],
                },
                RibbonGroup {
                    title: "Locali",
                    tools: vec![
                        command(
                            "AL_LOCALE",
                            "Locale",
                            include_bytes!("../../../assets/icons/ristr_locale.svg"),
                        ),
                        command(
                            "AL_AGGIORNA",
                            "Aggiorna",
                            include_bytes!("../../../assets/icons/ristr_aggiorna.svg"),
                        ),
                        command(
                            "AL_ALTEZZA",
                            "Altezza",
                            include_bytes!("../../../assets/icons/ristr_altezza.svg"),
                        ),
                    ],
                },
                RibbonGroup {
                    title: "Aperture",
                    tools: vec![
                        command(
                            "AL_PORTA",
                            "Porta",
                            include_bytes!("../../../assets/icons/ristr_porta.svg"),
                        ),
                        command(
                            "AL_FINESTRA",
                            "Finestra",
                            include_bytes!("../../../assets/icons/ristr_finestra.svg"),
                        ),
                        command(
                            "AL_APERTURA_DATI",
                            "Dati apertura",
                            include_bytes!("../../../assets/icons/ristr_apertura_dati.svg"),
                        ),
                    ],
                },
                RibbonGroup {
                    title: "Sezioni",
                    tools: vec![
                        command(
                            "AL_TAGLIO",
                            "Taglio",
                            include_bytes!("../../../assets/icons/ristr_taglio.svg"),
                        ),
                        command(
                            "AL_SEZIONE",
                            "Sezione",
                            include_bytes!("../../../assets/icons/ristr_sezione.svg"),
                        ),
                        command(
                            "AL_PROSPETTO",
                            "Prospetto",
                            include_bytes!("../../../assets/icons/ristr_prospetto.svg"),
                        ),
                    ],
                },
                RibbonGroup {
                    title: "Elaborati",
                    tools: vec![
                        command(
                            "AL_ESTRAI",
                            "Estrai elaborati",
                            include_bytes!("../../../assets/icons/ristr_estrai.svg"),
                        ),
                        command(
                            "AL_ABACO",
                            "Abaco",
                            include_bytes!("../../../assets/icons/ristr_abaco.svg"),
                        ),
                        command(
                            "AL_ABACO_CSV",
                            "Abaco CSV",
                            include_bytes!("../../../assets/icons/ristr_abaco_csv.svg"),
                        ),
                    ],
                },
            ]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tab_carries_the_fifteen_commands_of_the_menu() {
        let ids: Vec<&str> = RistrutturazioneModule
            .ribbon_groups()
            .iter()
            .flat_map(|g| g.tools.iter())
            .filter_map(|item| match item {
                RibbonItem::LargeTool(t) => Some(t.id),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids,
            [
                "AL_DEMOLISCI",
                "AL_NUOVO",
                "AL_STATO",
                "AL_LOCALE",
                "AL_AGGIORNA",
                "AL_ALTEZZA",
                "AL_PORTA",
                "AL_FINESTRA",
                "AL_APERTURA_DATI",
                "AL_TAGLIO",
                "AL_SEZIONE",
                "AL_PROSPETTO",
                "AL_ESTRAI",
                "AL_ABACO",
                "AL_ABACO_CSV",
            ]
        );
    }
}
