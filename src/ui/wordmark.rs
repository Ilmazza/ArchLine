//! Logotipo ArchLine: "Arch" in arancio, "Line" nel colore del testo del tema.
//!
//! Due SVG con lo stesso viewBox (contorni di Jost 600/300, niente font a
//! runtime): il secondo viene tinto, così la scritta resta leggibile su
//! qualsiasi tema. Il file proprio evita conflitti col merge da upstream.

use iced::widget::{row, svg};
use iced::{Element, Theme};

static ARCH: &[u8] = include_bytes!("../../assets/wordmark-arch.svg");
static LINE: &[u8] = include_bytes!("../../assets/wordmark-line.svg");

// Dimensioni dei viewBox degli SVG (unità del font).
const VIEW_H: f32 = 820.0;
const ARCH_W: f32 = 2076.0;
const LINE_W: f32 = 1597.0;

/// La scritta alta `height` px (ascendenti incluse).
pub fn wordmark<'a, M: 'a>(height: f32) -> Element<'a, M> {
    let scale = height / VIEW_H;
    row![
        svg(svg::Handle::from_memory(ARCH))
            .width(ARCH_W * scale)
            .height(height),
        svg(svg::Handle::from_memory(LINE))
            .width(LINE_W * scale)
            .height(height)
            .style(|theme: &Theme, _| svg::Style {
                color: Some(theme.palette().background.base.text),
            }),
    ]
    .into()
}
