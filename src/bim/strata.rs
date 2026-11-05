//! Stratigrafie del disegno (spec parte 6 §3.1): pacchetti con nome, di tipo
//! **parete** o **solaio**, fatti di strati (materiale e spessore) inseriti da
//! chi progetta. Nessun catalogo e nessuno spessore proposto: ogni numero
//! viene dal disegno.
//!
//! Stanno tutte in un XRecord `ARCHLINE_STRATIGRAFIE` del dizionario radice:
//! 90 (Int32) versione; per ogni stratigrafia 1 (String) nome e 70 (Int16)
//! tipo (0 parete, 1 solaio); per ogni strato 3 (String) materiale e 40
//! (Double) spessore in **unità del disegno**. La lettura va per codice e salta
//! ciò che non conosce; uno strato senza stratigrafia prima si ignora.
//!
//! Per una parete gli strati vanno da una faccia all'altra, per un solaio
//! dall'alto (la finitura) in basso. Logica pura: niente undo, lo apre il
//! comando.

use codec::objects::{XRecordEntry, XRecordValue};
use codec::Document;

use super::abaco::format_fixed;
use super::record::{named_xrecord_entries, write_named_xrecord};
use super::units::DrawingUnit;
use super::RECORD_STRATIGRAFIE;

const VERSION: i32 = 1;
const CODE_VERSION: i32 = 90;
const CODE_NAME: i32 = 1;
const CODE_KIND: i32 = 70;
const CODE_MATERIAL: i32 = 3;
const CODE_THICKNESS: i32 = 40;

/// Dove va un pacchetto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Su una nuova muratura (zona `AL_NUOVO`).
    Parete,
    /// Su un locale di progetto (il suo seme).
    Solaio,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Parete => "parete",
            Kind::Solaio => "solaio",
        }
    }

    /// `Parete`/`P` o `Solaio`/`S`, senza distinzione di maiuscole.
    pub fn parse(text: &str) -> Option<Kind> {
        match text.to_ascii_uppercase().as_str() {
            "PARETE" | "P" => Some(Kind::Parete),
            "SOLAIO" | "S" => Some(Kind::Solaio),
            _ => None,
        }
    }

    fn code(self) -> i16 {
        match self {
            Kind::Parete => 0,
            Kind::Solaio => 1,
        }
    }

    fn from_code(code: i16) -> Option<Kind> {
        match code {
            0 => Some(Kind::Parete),
            1 => Some(Kind::Solaio),
            _ => None,
        }
    }
}

/// Uno strato: materiale e spessore in unità del disegno.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub material: String,
    pub thickness: f64,
}

/// Una stratigrafia.
#[derive(Clone, Debug, PartialEq)]
pub struct Stratigraphy {
    pub name: String,
    pub kind: Kind,
    pub layers: Vec<Layer>,
}

impl Stratigraphy {
    /// Lo spessore totale, in unità del disegno.
    pub fn total(&self) -> f64 {
        self.layers.iter().map(|l| l.thickness).sum()
    }
}

/// Un nome valido: non vuoto, senza spazi né `;` (sta in un token della riga
/// di comando).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|c| c.is_whitespace() || c == ';')
}

/// Gli strati di `text`: `materiale spessore; materiale spessore; …`. L'ultimo
/// token di ogni strato è lo spessore (punto o virgola decimale), il resto il
/// materiale. Un `;` finale si ignora. Ogni errore è un messaggio, mai uno
/// strato indovinato.
pub fn parse_layers(text: &str) -> Result<Vec<Layer>, String> {
    let mut layers = Vec::new();
    let parts: Vec<&str> = text.split(';').map(str::trim).collect();
    for (n, part) in parts.iter().enumerate() {
        if part.is_empty() {
            if n + 1 == parts.len() && n > 0 {
                continue;
            }
            return Err(format!("strato {}: vuoto.", n + 1));
        }
        let Some((material, number)) = part.rsplit_once(char::is_whitespace) else {
            return Err(format!(
                "strato {}: «{part}» non ha materiale e spessore (es. «Intonaco 0.015»).",
                n + 1
            ));
        };
        let material = material.trim();
        let thickness: f64 = number.replace(',', ".").parse().map_err(|_| {
            format!("strato {}: spessore «{number}» non è un numero.", n + 1)
        })?;
        if material.is_empty() {
            return Err(format!("strato {}: manca il materiale.", n + 1));
        }
        if !thickness.is_finite() || thickness <= 0.0 {
            return Err(format!("strato {}: lo spessore deve essere positivo.", n + 1));
        }
        layers.push(Layer {
            material: material.to_string(),
            thickness,
        });
    }
    if layers.is_empty() {
        return Err("nessuno strato.".into());
    }
    Ok(layers)
}

/// Lo spessore in centimetri con un decimale e la virgola («1,5»).
pub fn cm(thickness: f64, unit: DrawingUnit) -> String {
    format_fixed(thickness / unit.per_metre() * 100.0, 1)
}

/// Le stratigrafie del disegno, nell'ordine in cui sono scritte.
pub fn read_all(doc: &Document) -> Vec<Stratigraphy> {
    let mut out: Vec<Stratigraphy> = Vec::new();
    let Some(entries) = named_xrecord_entries(doc, RECORD_STRATIGRAFIE) else {
        return out;
    };
    let mut material: Option<String> = None;
    for entry in entries {
        match (entry.code, &entry.value) {
            (CODE_NAME, XRecordValue::String(name)) => {
                out.push(Stratigraphy {
                    name: name.clone(),
                    kind: Kind::Parete,
                    layers: Vec::new(),
                });
                material = None;
            }
            (CODE_KIND, XRecordValue::Int16(code)) => {
                if let (Some(last), Some(kind)) = (out.last_mut(), Kind::from_code(*code)) {
                    last.kind = kind;
                }
            }
            (CODE_MATERIAL, XRecordValue::String(text)) => material = Some(text.clone()),
            (CODE_THICKNESS, XRecordValue::Double(t)) => {
                if let (Some(last), Some(m)) = (out.last_mut(), material.take()) {
                    if t.is_finite() && *t > 0.0 {
                        last.layers.push(Layer {
                            material: m,
                            thickness: *t,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Riscrive tutte le stratigrafie del disegno.
pub fn write_all(doc: &mut Document, all: &[Stratigraphy]) {
    let mut entries = vec![XRecordEntry::int32(CODE_VERSION, VERSION)];
    for s in all {
        entries.push(XRecordEntry::string(CODE_NAME, s.name.clone()));
        entries.push(XRecordEntry::int16(CODE_KIND, s.kind.code()));
        for layer in &s.layers {
            entries.push(XRecordEntry::string(CODE_MATERIAL, layer.material.clone()));
            entries.push(XRecordEntry::double(CODE_THICKNESS, layer.thickness));
        }
    }
    write_named_xrecord(doc, RECORD_STRATIGRAFIE, entries);
}

/// La stratigrafia `name` (senza distinzione di maiuscole).
pub fn find<'a>(all: &'a [Stratigraphy], name: &str) -> Option<&'a Stratigraphy> {
    all.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

/// Aggiunge `s` se il nome è valido e libero; altrimenti dice perché.
pub fn add(all: &mut Vec<Stratigraphy>, s: Stratigraphy) -> Result<(), String> {
    if !valid_name(&s.name) {
        return Err(format!("nome «{}» non valido (niente spazi né «;»).", s.name));
    }
    if find(all, &s.name).is_some() {
        return Err(format!(
            "la stratigrafia «{}» c'è già: per cambiarla Rimuovi e poi Nuova.",
            s.name
        ));
    }
    if s.layers.is_empty() {
        return Err("nessuno strato.".into());
    }
    all.push(s);
    Ok(())
}

/// Toglie la stratigrafia `name`; `false` se non c'era.
pub fn remove(all: &mut Vec<Stratigraphy>, name: &str) -> bool {
    let before = all.len();
    all.retain(|s| !s.name.eq_ignore_ascii_case(name));
    all.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t12() -> Stratigraphy {
        Stratigraphy {
            name: "T12".into(),
            kind: Kind::Parete,
            layers: parse_layers("Intonaco di calce 0.015; Laterizio forato 0,09; Intonaco di calce 0.015;")
                .unwrap(),
        }
    }

    #[test]
    fn layers_parse_with_spaces_and_both_decimal_marks() {
        let s = t12();
        assert_eq!(s.layers.len(), 3);
        assert_eq!(s.layers[0].material, "Intonaco di calce");
        assert_eq!(s.layers[1].material, "Laterizio forato");
        assert_eq!(s.layers[1].thickness, 0.09);
        assert!((s.total() - 0.12).abs() < 1e-12);
        assert_eq!(cm(s.total(), DrawingUnit::Metri), "12,0");
        assert_eq!(cm(15.0, DrawingUnit::Millimetri), "1,5");
    }

    #[test]
    fn bad_layers_are_refused_with_a_reason() {
        assert!(parse_layers("").is_err());
        assert!(parse_layers("Intonaco").unwrap_err().contains("strato 1"));
        assert!(parse_layers("Intonaco x").unwrap_err().contains("non è un numero"));
        assert!(parse_layers("Intonaco 0").unwrap_err().contains("positivo"));
        assert!(parse_layers("Intonaco -1").is_err());
        assert!(parse_layers("Intonaco 0.01;; Massetto 0.05").unwrap_err().contains("strato 2"));
        assert!(parse_layers("0.01").is_err(), "manca il materiale");
    }

    #[test]
    fn names_are_unique_and_tokens() {
        let mut all = Vec::new();
        add(&mut all, t12()).unwrap();
        let mut again = t12();
        again.name = "t12".into();
        assert!(add(&mut all, again).unwrap_err().contains("c'è già"));
        let mut spaced = t12();
        spaced.name = "T 12".into();
        assert!(add(&mut all, spaced).is_err());
        assert!(find(&all, "t12").is_some());
        assert!(remove(&mut all, "T12"));
        assert!(!remove(&mut all, "T12"));
        assert_eq!(Kind::parse("s"), Some(Kind::Solaio));
        assert_eq!(Kind::parse("Parete"), Some(Kind::Parete));
        assert_eq!(Kind::parse("muro"), None);
    }

    #[test]
    fn xrecord_roundtrip_and_rewrite() {
        let mut doc = Document::new();
        assert!(read_all(&doc).is_empty());
        let floor = Stratigraphy {
            name: "P1".into(),
            kind: Kind::Solaio,
            layers: parse_layers("Ceramica 0.01; Massetto 0.05").unwrap(),
        };
        write_all(&mut doc, &[t12(), floor.clone()]);
        assert_eq!(read_all(&doc), vec![t12(), floor.clone()]);
        // Riscritto con una sola: un solo XRecord, una sola stratigrafia.
        write_all(&mut doc, &[floor.clone()]);
        assert_eq!(read_all(&doc), vec![floor]);
        // Il record di progetto convive.
        super::super::record::write(
            &mut doc,
            &super::super::record::ProjectRecord {
                unit: Some(DrawingUnit::Metri),
                ..Default::default()
            },
        );
        assert_eq!(read_all(&doc).len(), 1);
        assert_eq!(super::super::record::read(&doc).unit, Some(DrawingUnit::Metri));
    }

    #[test]
    fn survives_dxf_and_dwg() {
        use std::io::Cursor;
        let mut doc = Document::new();
        write_all(&mut doc, &[t12()]);
        let bytes = codec::DxfWriter::new(&doc).write_to_vec().unwrap();
        let read = codec::DxfReader::from_reader(Cursor::new(bytes)).unwrap().read().unwrap();
        assert_eq!(read_all(&read), vec![t12()], "DXF");
        let bytes = codec::DwgWriter::write_to_vec(&doc).unwrap();
        let read = codec::DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
        assert_eq!(read_all(&read), vec![t12()], "DWG");
    }
}
