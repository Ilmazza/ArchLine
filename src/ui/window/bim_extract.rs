//! ArchLine (BIM): la finestra di `AL_ESTRAI` (spec §5.5 e §6). Minima di
//! proposito: cartella, i tre elaborati, il layer dei muri nuovi, OK e
//! Annulla; il resto si fa da riga di comando. Qui stato, messaggi e vista; le
//! azioni stanno in `app/commands/bim.rs` (`on_bim`).

use std::path::PathBuf;

use iced::widget::{button, checkbox, column, container, pick_list, row, text, text_input, Space};
use iced::{Element, Fill};

use crate::app::Message;
use crate::bim::extract::Elaborato;
use crate::ui::modal::ModalSizing;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{dialog_button, dialog_button_styled_opt, field_style};

/// Che cosa dice la finestra quando il primo OK ha trovato file esistenti.
pub const OVERWRITE_NOTE: &str = "I file esistono già: premi di nuovo OK per sovrascriverli.";

/// Stato della finestra aperta (`app.bim_extract`); `None` quando è chiusa.
#[derive(Clone, Debug)]
pub struct ExtractDialog {
    /// Il disegno da cui si estrae (`DocumentTab::id`): la finestra resta sua
    /// anche se nel frattempo si passa a un altro tab.
    pub tab_id: u64,
    pub dir: Option<PathBuf>,
    pub fatto: bool,
    pub comparativa: bool,
    pub progetto: bool,
    /// Layer dei muri nuovi dello stato di progetto.
    pub layer: String,
    /// I layer del disegno da proporre, esclusi gli `AL-*`.
    pub layers: Vec<String>,
    /// Il primo OK ha trovato file esistenti: il prossimo li sovrascrive.
    pub confirm_overwrite: bool,
}

impl ExtractDialog {
    /// Gli elaborati spuntati, nell'ordine della finestra.
    pub fn elaborati(&self) -> Vec<Elaborato> {
        [
            (self.fatto, Elaborato::StatoDiFatto),
            (self.comparativa, Elaborato::DemolizioniCostruzioni),
            (self.progetto, Elaborato::StatoDiProgetto),
        ]
        .into_iter()
        .filter_map(|(on, elaborato)| on.then_some(elaborato))
        .collect()
    }

    /// Spunta o toglie un elaborato. I file da scrivere cambiano: una
    /// conferma data per i file di prima non vale più.
    pub fn toggle(&mut self, elaborato: Elaborato) {
        let on = match elaborato {
            Elaborato::StatoDiFatto => &mut self.fatto,
            Elaborato::DemolizioniCostruzioni => &mut self.comparativa,
            Elaborato::StatoDiProgetto => &mut self.progetto,
        };
        *on = !*on;
        self.confirm_overwrite = false;
    }

    /// OK si può premere: c'è una cartella e almeno un elaborato.
    pub fn can_ok(&self) -> bool {
        self.dir.is_some() && (self.fatto || self.comparativa || self.progetto)
    }

    /// La riga d'avviso sopra i pulsanti, se c'è.
    pub fn note(&self) -> Option<&'static str> {
        self.confirm_overwrite.then_some(OVERWRITE_NOTE)
    }
}

/// Tutto ciò che la finestra può chiedere; un solo `Message::Bim` lo porta.
#[derive(Debug, Clone)]
pub enum BimMsg {
    /// «Sfoglia…»: sceglie la cartella.
    Browse,
    /// La cartella scelta; `None` se la scelta è stata annullata.
    Picked(Option<PathBuf>),
    Toggle(Elaborato),
    /// Il layer scritto o scelto dall'elenco.
    Layer(String),
    Ok,
    Cancel,
}

fn msg(m: BimMsg) -> Message {
    Message::Bim(m)
}

#[inline(never)]
pub(crate) fn view(state: &ExtractDialog, sizing: ModalSizing) -> Element<'_, Message> {
    let folder = match &state.dir {
        // Un percorso non ha spazi dove andare a capo: va a capo anche a metà.
        Some(dir) => text(dir.display().to_string())
            .size(12)
            .wrapping(iced::advanced::text::Wrapping::WordOrGlyph),
        None => text("Nessuna cartella scelta").size(12).style(muted_style),
    };
    let check = |label: &'static str, on: bool, elaborato: Elaborato| {
        checkbox(on)
            .label(label)
            .on_toggle(move |_| msg(BimMsg::Toggle(elaborato)))
            .size(14)
            .text_size(12)
    };
    let layer_field = text_input("Layer dei muri nuovi", &state.layer)
        .on_input(|name| msg(BimMsg::Layer(name)))
        .style(field_style);
    // L'elenco suggerisce: il campo resta libero (un layer che non c'è ancora
    // è ammesso); se il nome scritto è nell'elenco, l'elenco lo mostra.
    let selected = state
        .layers
        .iter()
        .find(|name| **name == state.layer)
        .cloned();
    let layer_pick = pick_list(selected, state.layers.as_slice(), |name: &String| {
        name.clone()
    })
    .on_select(|name| msg(BimMsg::Layer(name)))
    .placeholder("Layer del disegno".to_string())
    .width(Fill);
    container(
        column![
            text("Cartella di destinazione").size(12),
            row![
                container(folder).width(Fill),
                dialog_button("Sfoglia…", msg(BimMsg::Browse), false),
            ]
            .spacing(8)
            .align_y(iced::Center),
            check("Stato di fatto", state.fatto, Elaborato::StatoDiFatto),
            check(
                "Demolizioni e costruzioni",
                state.comparativa,
                Elaborato::DemolizioniCostruzioni
            ),
            check(
                "Stato di progetto",
                state.progetto,
                Elaborato::StatoDiProgetto
            ),
            text("Layer dei muri nuovi (stato di progetto)").size(12),
            row![layer_field, layer_pick].spacing(8),
            text(state.note().unwrap_or_default())
                .size(11)
                .style(text::danger),
            Space::new().height(Fill),
            row![
                Space::new().width(Fill),
                dialog_button_styled_opt(
                    "OK",
                    state.can_ok().then(|| msg(BimMsg::Ok)),
                    button::primary
                ),
                dialog_button("Annulla", msg(BimMsg::Cancel), false),
            ]
            .spacing(8),
        ]
        // Tutto entra nei 300 px di altezza iniziali (`sized_flow`), anche con
        // un percorso lungo su due righe: oltre, i pulsanti verrebbero tagliati.
        .spacing(6),
    )
    .padding(16)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> ExtractDialog {
        ExtractDialog {
            tab_id: 1,
            dir: Some(PathBuf::from("C:/estratti")),
            fatto: true,
            comparativa: true,
            progetto: true,
            layer: String::new(),
            layers: Vec::new(),
            confirm_overwrite: false,
        }
    }

    #[test]
    fn toggles_choose_the_elaborati_and_drop_the_confirmation() {
        let mut d = dialog();
        assert_eq!(
            d.elaborati(),
            vec![
                Elaborato::StatoDiFatto,
                Elaborato::DemolizioniCostruzioni,
                Elaborato::StatoDiProgetto
            ]
        );
        d.confirm_overwrite = true;
        assert_eq!(d.note(), Some(OVERWRITE_NOTE));
        d.toggle(Elaborato::StatoDiFatto);
        assert_eq!(
            d.elaborati(),
            vec![
                Elaborato::DemolizioniCostruzioni,
                Elaborato::StatoDiProgetto
            ]
        );
        assert!(!d.confirm_overwrite);
        assert_eq!(d.note(), None);
        d.toggle(Elaborato::DemolizioniCostruzioni);
        d.toggle(Elaborato::StatoDiProgetto);
        assert!(d.elaborati().is_empty());
        assert!(!d.can_ok(), "nessun elaborato");
        d.toggle(Elaborato::StatoDiProgetto);
        assert!(d.can_ok());
        d.dir = None;
        assert!(!d.can_ok(), "nessuna cartella");
    }
}
