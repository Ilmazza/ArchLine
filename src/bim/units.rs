//! Unità del disegno: millimetri, centimetri, metri. `INSUNITS` si legge e
//! non si scrive mai; con 0 vale l'unità scelta da Mauro, nel record.

use codec::Document;

use super::{record, BASE_TOLERANCE_M};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawingUnit {
    Millimetri,
    Centimetri,
    Metri,
}

impl DrawingUnit {
    /// Codice `INSUNITS`: 4 = mm, 5 = cm, 6 = m.
    pub fn from_insunits(code: i16) -> Option<Self> {
        match code {
            4 => Some(Self::Millimetri),
            5 => Some(Self::Centimetri),
            6 => Some(Self::Metri),
            _ => None,
        }
    }

    pub fn insunits(self) -> i16 {
        match self {
            Self::Millimetri => 4,
            Self::Centimetri => 5,
            Self::Metri => 6,
        }
    }

    /// Unità del disegno in un metro.
    pub fn per_metre(self) -> f64 {
        match self {
            Self::Millimetri => 1000.0,
            Self::Centimetri => 100.0,
            Self::Metri => 1.0,
        }
    }

    /// Tolleranza geometrica di base, in unità del disegno.
    pub fn tolerance(self) -> f64 {
        BASE_TOLERANCE_M * self.per_metre()
    }

    /// `MM` / `CM` / `M`, maiuscole o minuscole.
    pub fn from_keyword(text: &str) -> Option<Self> {
        if text.eq_ignore_ascii_case("MM") {
            Some(Self::Millimetri)
        } else if text.eq_ignore_ascii_case("CM") {
            Some(Self::Centimetri)
        } else if text.eq_ignore_ascii_case("M") {
            Some(Self::Metri)
        } else {
            None
        }
    }

    /// Metri → unità del disegno.
    pub fn metres(self, m: f64) -> f64 {
        m * self.per_metre()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitLookup {
    /// Unità nota: da `INSUNITS` (4/5/6) o, con `INSUNITS` = 0, dal record.
    Known(DrawingUnit),
    /// `INSUNITS` = 0 e nessuna unità nel record: va chiesta.
    Unspecified,
    /// `INSUNITS` con un codice non supportato (es. 1 = pollici): si rifiuta.
    Unsupported(i16),
}

pub fn resolve(doc: &Document) -> UnitLookup {
    let code = doc.header.insertion_units;
    if let Some(unit) = DrawingUnit::from_insunits(code) {
        return UnitLookup::Known(unit);
    }
    if code != 0 {
        return UnitLookup::Unsupported(code);
    }
    match record::read(doc).unit {
        Some(unit) => UnitLookup::Known(unit),
        None => UnitLookup::Unspecified,
    }
}

/// Area in unità del disegno al quadrato → metri quadrati.
pub fn area_m2(area: f64, unit: DrawingUnit) -> f64 {
    let k = unit.per_metre();
    area / (k * k)
}

/// `12,00`: due decimali, virgola. L'arrotondamento va sui centesimi (mezzo
/// via dallo zero), non sulla rappresentazione binaria di `{:.2}`. Serve alle
/// aree (`format_m2`) e alle coordinate degli avvisi.
pub fn format_decimal(value: f64) -> String {
    let cents = (value * 100.0).round() as i64;
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("{sign}{},{:02}", abs / 100, abs % 100)
}

/// `12,00 m²`.
pub fn format_m2(m2: f64) -> String {
    format!("{} m²", format_decimal(m2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bim::record::ProjectRecord;

    #[test]
    fn insunits_codes() {
        assert_eq!(DrawingUnit::from_insunits(4), Some(DrawingUnit::Millimetri));
        assert_eq!(DrawingUnit::from_insunits(5), Some(DrawingUnit::Centimetri));
        assert_eq!(DrawingUnit::from_insunits(6), Some(DrawingUnit::Metri));
        assert_eq!(DrawingUnit::from_insunits(1), None);
    }

    #[test]
    fn resolve_reads_insunits_then_record() {
        let mut doc = codec::Document::new();
        doc.header.insertion_units = 0;
        assert_eq!(resolve(&doc), UnitLookup::Unspecified);
        crate::bim::record::write(
            &mut doc,
            &ProjectRecord {
                unit: Some(DrawingUnit::Centimetri),
                ..Default::default()
            },
        );
        assert_eq!(resolve(&doc), UnitLookup::Known(DrawingUnit::Centimetri));
        assert_eq!(doc.header.insertion_units, 0, "INSUNITS non si scrive mai");
        doc.header.insertion_units = 6;
        assert_eq!(resolve(&doc), UnitLookup::Known(DrawingUnit::Metri));
        doc.header.insertion_units = 1;
        assert_eq!(resolve(&doc), UnitLookup::Unsupported(1));
    }

    #[test]
    fn areas_and_format() {
        assert!((area_m2(120_000.0, DrawingUnit::Centimetri) - 12.0).abs() < 1e-12);
        assert_eq!(format_m2(12.0), "12,00 m²");
        assert_eq!(format_m2(0.09), "0,09 m²");
        assert_eq!(format_m2(11.905), "11,91 m²");
        assert_eq!(format_decimal(4.05), "4,05");
        assert_eq!(format_decimal(0.0), "0,00");
        assert_eq!(format_decimal(-1250.5), "-1250,50");
        assert_eq!(format_decimal(-0.001), "0,00");
    }
}
