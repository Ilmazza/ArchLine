//! Altezza a tre livelli (spec parte 2, §4.1): propria, predefinita del
//! disegno, `n.d.`. Logica pura. I valori arrivano e si salvano in unità del
//! disegno (come lo spessore); qui si convertono in metri, che è ciò che
//! leggono abachi e quantità. Un valore non finito o ≤ 0 conta come assente.

use super::report::Notice;
use super::units::DrawingUnit;

/// Da dove viene un'altezza.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeightState {
    /// Scritta da Mauro sull'oggetto (o su un locale di fatto che la dà).
    Inserita,
    /// Presa dalla predefinita del disegno.
    Ereditata,
    /// Nessun valore: i volumi e le pareti che la usano sono `n.d.`.
    NonDisponibile,
}

/// Un'altezza risolta, in metri, con stato e, se serve, il motivo.
#[derive(Clone, Debug, PartialEq)]
pub struct Height {
    pub value_m: Option<f64>,
    pub state: HeightState,
    pub note: Option<String>,
}

impl Height {
    pub fn missing(note: Option<&str>) -> Height {
        Height {
            value_m: None,
            state: HeightState::NonDisponibile,
            note: note.map(str::to_string),
        }
    }

    fn known(value_m: f64, state: HeightState, note: Option<&str>) -> Height {
        Height {
            value_m: Some(value_m),
            state,
            note: note.map(str::to_string),
        }
    }
}

/// Altezze uguali entro un micrometro: sono lo stesso valore.
const SAME_HEIGHT_M: f64 = 1.0e-6;
/// Intervallo plausibile di un'altezza, in metri (spec §3).
const MIN_PLAUSIBLE_M: f64 = 0.50;
const MAX_PLAUSIBLE_M: f64 = 10.0;

/// Un valore utilizzabile: finito e > 0.
fn valid(v: Option<f64>) -> Option<f64> {
    v.filter(|v| v.is_finite() && *v > 0.0)
}

fn to_metres(value: f64, unit: DrawingUnit) -> f64 {
    value / unit.per_metre()
}

/// Altezza propria, altrimenti la predefinita, altrimenti `n.d.`.
pub fn resolve(own: Option<f64>, default: Option<f64>, unit: DrawingUnit) -> Height {
    if let Some(v) = valid(own) {
        return Height::known(to_metres(v, unit), HeightState::Inserita, None);
    }
    match valid(default) {
        Some(v) => Height::known(to_metres(v, unit), HeightState::Ereditata, None),
        None => Height::missing(None),
    }
}

/// Un varco non eredita mai: l'altezza di un'apertura la dice solo Mauro.
pub fn resolve_varco(own: Option<f64>, unit: DrawingUnit) -> Height {
    match valid(own) {
        Some(v) => Height::known(to_metres(v, unit), HeightState::Inserita, None),
        None => Height::missing(Some("altezza del varco da inserire")),
    }
}

/// Altezza di un locale di progetto: il seme proprio; altrimenti i locali di
/// fatto della sua faccia, se concordi (con lo stato di quelli); se discordi o
/// con uno `n.d.`, `n.d.` con l'avviso; senza locali di fatto, la predefinita.
pub fn resolve_project_room(
    seed_own: Option<f64>,
    fatto: &[(String, Height)],
    default: Option<f64>,
    unit: DrawingUnit,
) -> (Height, Option<Notice>) {
    if valid(seed_own).is_some() {
        return (resolve(seed_own, default, unit), None);
    }
    let Some((_, first)) = fatto.first() else {
        return (resolve(None, default, unit), None);
    };
    let concordant = match first.value_m {
        Some(first_m) => fatto.iter().all(|(_, h)| {
            h.value_m
                .is_some_and(|m| (m - first_m).abs() <= SAME_HEIGHT_M)
        }),
        None => false,
    };
    if !concordant {
        let notice = Notice::AltezzeDiscordi {
            names: fatto.iter().map(|(name, _)| name.clone()).collect(),
            values_m: fatto.iter().map(|(_, h)| h.value_m).collect(),
        };
        return (
            Height::missing(Some("altezze dei locali di fatto discordi")),
            Some(notice),
        );
    }
    let state = if fatto.iter().any(|(_, h)| h.state == HeightState::Inserita) {
        HeightState::Inserita
    } else {
        HeightState::Ereditata
    };
    let value_m = first.value_m.expect("concordi implica valore noto");
    (
        Height::known(value_m, state, Some("dal locale di fatto")),
        None,
    )
}

/// Avviso se l'altezza digitata (in unità del disegno) sta fuori da 0,50–10 m.
pub fn range_warning(value: f64, unit: DrawingUnit) -> Option<Notice> {
    let value_m = to_metres(value, unit);
    if (MIN_PLAUSIBLE_M..=MAX_PLAUSIBLE_M).contains(&value_m) {
        None
    } else {
        Some(Notice::AltezzaFuoriIntervallo { value_m })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use DrawingUnit::{Centimetri, Metri};

    fn h(value_m: f64, state: HeightState) -> Height {
        Height {
            value_m: Some(value_m),
            state,
            note: None,
        }
    }

    fn named(name: &str, height: Height) -> (String, Height) {
        (name.to_string(), height)
    }

    #[test]
    fn own_height_wins_over_default() {
        let r = resolve(Some(3.0), Some(2.7), Metri);
        assert_eq!(r.value_m, Some(3.0));
        assert_eq!(r.state, HeightState::Inserita);
    }

    #[test]
    fn default_is_inherited() {
        let r = resolve(None, Some(2.7), Metri);
        assert_eq!(r.value_m, Some(2.7));
        assert_eq!(r.state, HeightState::Ereditata);
    }

    #[test]
    fn missing_everywhere_is_not_available() {
        let r = resolve(None, None, Metri);
        assert_eq!(r.value_m, None);
        assert_eq!(r.state, HeightState::NonDisponibile);
        assert_eq!(r, Height::missing(None));
    }

    #[test]
    fn invalid_own_falls_back_to_default() {
        for bad in [f64::NAN, 0.0, -1.0, f64::INFINITY] {
            let r = resolve(Some(bad), Some(2.7), Metri);
            assert_eq!(r.value_m, Some(2.7), "{bad}");
            assert_eq!(r.state, HeightState::Ereditata, "{bad}");
            let r = resolve(Some(bad), None, Metri);
            assert_eq!(r.value_m, None, "{bad}");
            assert_eq!(r.state, HeightState::NonDisponibile, "{bad}");
        }
        // Anche una predefinita non valida conta come assente.
        assert_eq!(
            resolve(None, Some(0.0), Metri).state,
            HeightState::NonDisponibile
        );
    }

    #[test]
    fn centimetre_drawing_converts() {
        let r = resolve(Some(270.0), None, Centimetri);
        assert!((r.value_m.unwrap() - 2.70).abs() < 1e-12);
        assert_eq!(r.state, HeightState::Inserita);
        let r = resolve(None, Some(270.0), Centimetri);
        assert!((r.value_m.unwrap() - 2.70).abs() < 1e-12);
        assert_eq!(r.state, HeightState::Ereditata);
    }

    #[test]
    fn varco_never_inherits() {
        let r = resolve_varco(None, Metri);
        assert_eq!(r.value_m, None);
        assert_eq!(r.state, HeightState::NonDisponibile);
        assert_eq!(r.note.as_deref(), Some("altezza del varco da inserire"));
        let r = resolve_varco(Some(2.1), Metri);
        assert_eq!(r.value_m, Some(2.1));
        assert_eq!(r.state, HeightState::Inserita);
        assert_eq!(
            resolve_varco(Some(f64::NAN), Metri).state,
            HeightState::NonDisponibile
        );
    }

    #[test]
    fn project_room_own_seed_wins() {
        let fatto = [named("A", h(2.7, HeightState::Ereditata))];
        let (r, notice) = resolve_project_room(Some(3.2), &fatto, Some(2.7), Metri);
        assert_eq!(r.value_m, Some(3.2));
        assert_eq!(r.state, HeightState::Inserita);
        assert_eq!(notice, None);
        // Un seme con altezza non valida passa ai locali di fatto.
        let (r, _) = resolve_project_room(Some(0.0), &fatto, None, Metri);
        assert_eq!(r.value_m, Some(2.7));
    }

    #[test]
    fn project_room_concordant_fatto_inherits_state_and_note() {
        let fatto = [
            named("A", h(2.7, HeightState::Ereditata)),
            named("B", h(2.7, HeightState::Ereditata)),
        ];
        let (r, notice) = resolve_project_room(None, &fatto, Some(2.7), Metri);
        assert_eq!(r.value_m, Some(2.7));
        assert_eq!(r.state, HeightState::Ereditata);
        assert_eq!(r.note.as_deref(), Some("dal locale di fatto"));
        assert_eq!(notice, None);

        let fatto = [
            named("A", h(3.0, HeightState::Inserita)),
            named("B", h(3.0, HeightState::Ereditata)),
        ];
        let (r, notice) = resolve_project_room(None, &fatto, Some(2.7), Metri);
        assert_eq!(r.value_m, Some(3.0));
        assert_eq!(r.state, HeightState::Inserita);
        assert_eq!(r.note.as_deref(), Some("dal locale di fatto"));
        assert_eq!(notice, None);
    }

    #[test]
    fn project_room_discordant_fatto_is_not_available_with_notice() {
        let fatto = [
            named("A", h(2.7, HeightState::Ereditata)),
            named("B", h(3.0, HeightState::Inserita)),
        ];
        let (r, notice) = resolve_project_room(None, &fatto, Some(2.7), Metri);
        assert_eq!(r.value_m, None);
        assert_eq!(r.state, HeightState::NonDisponibile);
        assert_eq!(
            notice,
            Some(Notice::AltezzeDiscordi {
                names: vec!["A".into(), "B".into()],
                values_m: vec![Some(2.7), Some(3.0)],
            })
        );
        // Un locale di fatto senza altezza rende discorde anche il resto.
        let fatto = [
            named("A", h(2.7, HeightState::Ereditata)),
            named("B", Height::missing(None)),
        ];
        let (r, notice) = resolve_project_room(None, &fatto, Some(2.7), Metri);
        assert_eq!(r.state, HeightState::NonDisponibile);
        assert!(matches!(notice, Some(Notice::AltezzeDiscordi { .. })));
    }

    #[test]
    fn project_room_without_fatto_uses_default() {
        let (r, notice) = resolve_project_room(None, &[], Some(2.7), Metri);
        assert_eq!(r.value_m, Some(2.7));
        assert_eq!(r.state, HeightState::Ereditata);
        assert_eq!(notice, None);
        let (r, notice) = resolve_project_room(None, &[], None, Metri);
        assert_eq!(r.state, HeightState::NonDisponibile);
        assert_eq!(notice, None);
    }

    #[test]
    fn range_warning_bounds() {
        assert!(range_warning(0.4, Metri).is_some());
        assert!(range_warning(11.0, Metri).is_some());
        assert_eq!(range_warning(2.7, Metri), None);
        assert_eq!(range_warning(0.5, Metri), None);
        assert_eq!(range_warning(10.0, Metri), None);
        // In un disegno in cm, digitare 2,7 vuol dire 2,7 cm.
        match range_warning(2.7, Centimetri) {
            Some(Notice::AltezzaFuoriIntervallo { value_m }) => {
                assert!((value_m - 0.027).abs() < 1e-12)
            }
            other => panic!("atteso l'avviso, trovato {other:?}"),
        }
        assert_eq!(range_warning(270.0, Centimetri), None);
    }
}
