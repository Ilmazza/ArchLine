//! Le quantità per strato (spec parte 6 §5.3): per ogni elemento con una
//! stratigrafia valida, una riga per strato, codice `<elemento>-S<n>` (N1-S1,
//! LP1-S2…), con la **quantità dell'elemento** (superficie della nuova muratura
//! L × H, superficie netta del locale di progetto) e il suo stato. Lo spessore
//! dello strato dà solo il volume informativo (m² × spessore).
//!
//! - una riga dell'elemento incompleta dà righe di strato incomplete, con lo
//!   stesso motivo (mai una superficie di ripiego);
//! - stratigrafia che non c'è più: `StratigrafiaMancante`, nessuna riga;
//! - tipo sbagliato (letto da un disegno cambiato altrove):
//!   `StratigrafiaTipoDiverso`, nessuna riga;
//! - parete: somma degli strati diversa dallo spessore effettivo della
//!   muratura oltre la tolleranza: `StratigrafiaDiversa` (le righe restano:
//!   il pacchetto è di chi progetta, il disegno non si corregge).
//!
//! Logica pura: legge le assegnazioni dal documento e le righe già calcolate
//! dell'abaco.

use codec::{Document, Handle};

use super::height::Height;
use super::quantities::ZoneRow;
use super::quantities_rooms::ProjectRoomRow;
use super::report::Notice;
use super::rooms::{self, Phase, RoomPart};
use super::strata::{self, cm, Kind};
use super::strata_assign::{assignments_in, Element};
use super::units::DrawingUnit;
use super::BASE_TOLERANCE_M;

/// Una riga di strato.
#[derive(Clone, Debug, PartialEq)]
pub struct StrataRow {
    /// `N1-S1`, `LP1-S2`…
    pub code: String,
    /// Il codice della riga dell'elemento (`N1`, `LP1`).
    pub owner_code: String,
    pub owner: Element,
    /// L'entità dell'elemento (zona o seme), per la fonte.
    pub owner_handle: Handle,
    pub stratigraphy: String,
    pub material: String,
    /// Spessore dello strato, in metri.
    pub thickness_m: f64,
    /// Spessore come «1,5» (cm), per testi e note.
    pub thickness_cm: String,
    /// La quantità dell'elemento; `None` se la sua riga è incompleta.
    pub area_m2: Option<f64>,
    /// Superficie × spessore, solo informativo.
    pub volume_m3: Option<f64>,
    pub height: Height,
    /// Il motivo di una riga incompleta (quello dell'elemento).
    pub reason: Option<String>,
}

impl StrataRow {
    pub fn is_complete(&self) -> bool {
        self.area_m2.is_some()
    }
}

/// Le righe di strato e gli avvisi.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StrataQuantities {
    pub rows: Vec<StrataRow>,
    pub notices: Vec<Notice>,
}

/// Le righe di strato del disegno, nell'ordine delle nuove murature e poi dei
/// locali di progetto, gli strati nell'ordine della stratigrafia.
pub fn strata_quantities(
    doc: &Document,
    nuove: &[ZoneRow],
    progetto: &[ProjectRoomRow],
    unit: DrawingUnit,
) -> StrataQuantities {
    let mut out = StrataQuantities::default();
    let all = strata::read_all(doc);
    let assigned = assignments_in(doc);
    if assigned.is_empty() {
        return out;
    }
    // Il seme di ogni entità seme di progetto, per ritrovare la sua riga.
    let seeds: Vec<(Handle, [f64; 2])> = rooms::rooms_in(doc)
        .into_iter()
        .filter(|(_, room, part)| room.phase == Phase::Progetto && *part == RoomPart::Seme)
        .map(|(handle, room, _)| (handle, room.seed))
        .collect();
    let per_metre = unit.per_metre();

    // Gli elementi nell'ordine delle righe dell'abaco.
    struct Owner<'a> {
        code: &'a str,
        handle: Handle,
        element: Element,
        area: Option<f64>,
        height: &'a Height,
        reason: Option<String>,
        thickness_m: Option<f64>,
    }
    let mut owners: Vec<Owner> = Vec::new();
    for row in nuove {
        owners.push(Owner {
            code: &row.code,
            handle: row.handle,
            element: Element::NewWall,
            area: row.surface_m2,
            height: &row.height,
            reason: row.incomplete_reason(),
            thickness_m: row.thickness_m,
        });
    }
    for project in progetto {
        let Some(&(handle, _)) = seeds.iter().find(|(_, seed)| *seed == project.row.seed) else {
            continue;
        };
        owners.push(Owner {
            code: &project.row.code,
            handle,
            element: Element::ProjectRoom,
            area: project.row.area_m2,
            height: &project.row.height,
            reason: project.row.incomplete_reason(),
            thickness_m: None,
        });
    }

    for owner in owners {
        let Some((_, _, name)) = assigned
            .iter()
            .find(|(handle, element, _)| *handle == owner.handle && *element == Some(owner.element))
        else {
            continue;
        };
        let Some(s) = strata::find(&all, name) else {
            out.notices.push(Notice::StratigrafiaMancante {
                what: owner.code.to_string(),
                name: name.clone(),
            });
            continue;
        };
        if s.kind != owner.element.kind() {
            out.notices.push(Notice::StratigrafiaTipoDiverso {
                what: owner.code.to_string(),
                name: s.name.clone(),
                kind: s.kind.word().to_string(),
            });
            continue;
        }
        let total_m = s.total() / per_metre;
        if let (Kind::Parete, Some(thickness)) = (s.kind, owner.thickness_m) {
            if (total_m - thickness).abs() > BASE_TOLERANCE_M {
                out.notices.push(Notice::StratigrafiaDiversa {
                    code: owner.code.to_string(),
                    name: s.name.clone(),
                    sum_cm: cm(s.total(), unit),
                    thickness_cm: super::abaco::format_fixed(thickness * 100.0, 1),
                });
            }
        }
        for (n, layer) in s.layers.iter().enumerate() {
            let thickness_m = layer.thickness / per_metre;
            let area = owner.area.filter(|a| a.is_finite());
            out.rows.push(StrataRow {
                code: format!("{}-S{}", owner.code, n + 1),
                owner_code: owner.code.to_string(),
                owner: owner.element,
                owner_handle: owner.handle,
                stratigraphy: s.name.clone(),
                material: layer.material.clone(),
                thickness_m,
                thickness_cm: cm(layer.thickness, unit),
                area_m2: area,
                volume_m3: area.map(|a| a * thickness_m),
                height: owner.height.clone(),
                reason: if area.is_some() {
                    None
                } else {
                    Some(owner.reason.clone().unwrap_or_else(|| "superficie n.d.".into()))
                },
            });
        }
    }
    out
}
