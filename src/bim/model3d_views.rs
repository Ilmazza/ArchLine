//! Modello 3D, quinto passo (spec parte 5 §4.5): le cinque **viste con nome**
//! che inquadrano il modello di progetto: quattro assonometrie ortogonali
//! (`Assonometria SE|SO|NE|NO`) e una prospettiva (`Prospettiva`).
//!
//! Non cambiano la camera: sono voci della tabella VIEW del disegno, che si
//! richiamano con VIEW. Le assonometrie guardano da 35,264° sull'orizzonte
//! (isometrica vera: `atan(1/√2)`), la prospettiva da 30° dal sud-est con
//! obiettivo 50 mm. L'inquadratura è la sfera che contiene il modello: nessuna
//! vista lo taglia, qualunque sia il lato da cui guarda.
//!
//! Logica pura: costruisce le viste e le mette nella tabella del documento.

use codec::tables::View;
use codec::types::Vector3;
use codec::Document;

/// Nome, direzione orizzontale dell'occhio (x, y), prospettiva?
const VIEWS: [(&str, [f64; 2], bool); 5] = [
    ("Assonometria SE", [1.0, -1.0], false),
    ("Assonometria SO", [-1.0, -1.0], false),
    ("Assonometria NE", [1.0, 1.0], false),
    ("Assonometria NO", [-1.0, 1.0], false),
    ("Prospettiva", [1.0, -1.0], true),
];

/// Elevazione dell'occhio nelle assonometrie: `atan(1/√2)`, in gradi 35,264.
pub fn isometric_elevation() -> f64 {
    (1.0 / 2f64.sqrt()).atan()
}

/// Elevazione dell'occhio nella prospettiva.
pub const PERSPECTIVE_ELEVATION_DEG: f64 = 30.0;

/// Obiettivo della prospettiva, in millimetri.
pub const LENS_MM: f64 = 50.0;

/// Margine intorno alla sfera che contiene il modello.
pub const MARGIN: f64 = 1.15;

/// Rapporto larghezza/altezza delle viste (solo informativo: ArchLine
/// ricava l'inquadratura dall'altezza).
pub const ASPECT: f64 = 16.0 / 9.0;

/// I nomi delle cinque viste, nell'ordine in cui si creano.
pub fn names() -> [&'static str; 5] {
    [VIEWS[0].0, VIEWS[1].0, VIEWS[2].0, VIEWS[3].0, VIEWS[4].0]
}

/// Il parallelepipedo che contiene i punti, se ce ne sono.
pub fn bounds<'a>(points: impl IntoIterator<Item = &'a [f64; 3]>) -> Option<([f64; 3], [f64; 3])> {
    let mut it = points.into_iter();
    let first = *it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| {
        (
            [lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])],
            [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])],
        )
    }))
}

/// Le cinque viste (senza handle) che inquadrano il parallelepipedo `lo`–`hi`.
pub fn build(lo: [f64; 3], hi: [f64; 3]) -> Vec<View> {
    let target = [
        (lo[0] + hi[0]) / 2.0,
        (lo[1] + hi[1]) / 2.0,
        (lo[2] + hi[2]) / 2.0,
    ];
    let diagonal = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt();
    let height = diagonal * MARGIN;
    VIEWS
        .iter()
        .map(|(name, horizontal, perspective)| {
            let elevation = if *perspective {
                PERSPECTIVE_ELEVATION_DEG.to_radians()
            } else {
                isometric_elevation()
            };
            let length = (horizontal[0].powi(2) + horizontal[1].powi(2)).sqrt();
            let mut view = View::new(*name);
            view.center = Vector3::new(0.0, 0.0, 0.0);
            view.target = Vector3::new(target[0], target[1], target[2]);
            view.direction = Vector3::new(
                elevation.cos() * horizontal[0] / length,
                elevation.cos() * horizontal[1] / length,
                elevation.sin(),
            );
            view.height = height;
            view.width = height * ASPECT;
            view.lens_length = LENS_MM;
            view.front_clip = 0.0;
            view.back_clip = 0.0;
            view.twist_angle = 0.0;
            view.perspective = *perspective;
            view
        })
        .collect()
}

/// Mette le viste nella tabella VIEW: una con lo stesso nome si **sostituisce**
/// tenendo il suo handle, le altre ne prendono uno nuovo. Dà quante erano
/// nuove e quante sostituite.
pub fn install(doc: &mut Document, views: Vec<View>) -> (usize, usize) {
    let (mut created, mut replaced) = (0, 0);
    for mut view in views {
        view.handle = match doc.views.get(&view.name) {
            Some(old) => {
                replaced += 1;
                old.handle
            }
            None => {
                created += 1;
                doc.allocate_handle()
            }
        };
        doc.views.add_or_replace(view);
    }
    (created, replaced)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed() -> Vec<View> {
        build([0.0, 0.0, 0.0], [10.0, 6.0, 3.0])
    }

    #[test]
    fn the_five_views_have_the_exact_names_in_order() {
        let got: Vec<String> = boxed().into_iter().map(|v| v.name).collect();
        assert_eq!(
            got,
            [
                "Assonometria SE",
                "Assonometria SO",
                "Assonometria NE",
                "Assonometria NO",
                "Prospettiva"
            ]
        );
        assert_eq!(names().len(), 5);
    }

    #[test]
    fn only_the_perspective_is_a_perspective() {
        for view in boxed() {
            assert_eq!(view.perspective, view.name == "Prospettiva", "{}", view.name);
        }
        assert_eq!(boxed()[4].lens_length, 50.0);
    }

    #[test]
    fn directions_are_unit_vectors_looking_from_the_right_corner() {
        let views = boxed();
        let sign = |v: &View| (v.direction.x.signum(), v.direction.y.signum());
        assert_eq!(sign(&views[0]), (1.0, -1.0)); // SE
        assert_eq!(sign(&views[1]), (-1.0, -1.0)); // SO
        assert_eq!(sign(&views[2]), (1.0, 1.0)); // NE
        assert_eq!(sign(&views[3]), (-1.0, 1.0)); // NO
        assert_eq!(sign(&views[4]), (1.0, -1.0)); // prospettiva da SE
        for v in &views {
            let d = &v.direction;
            let len = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
            assert!((len - 1.0).abs() < 1e-12, "{}: {len}", v.name);
            assert!(d.z > 0.0, "{}: l'occhio sta sopra", v.name);
        }
        // Isometrica vera: le tre componenti hanno lo stesso modulo.
        let d = &views[0].direction;
        assert!((d.x.abs() - d.z).abs() < 1e-12 && (d.y.abs() - d.z).abs() < 1e-12);
        // Prospettiva: 30° sull'orizzonte.
        assert!((views[4].direction.z - 0.5).abs() < 1e-12);
    }

    #[test]
    fn views_aim_at_the_centre_and_contain_the_model() {
        let diagonal = (10f64 * 10.0 + 6.0 * 6.0 + 3.0 * 3.0).sqrt();
        for v in boxed() {
            assert_eq!((v.target.x, v.target.y, v.target.z), (5.0, 3.0, 1.5), "{}", v.name);
            assert!(v.height >= diagonal, "{}: altezza {}", v.name, v.height);
            assert!((v.height - diagonal * MARGIN).abs() < 1e-12);
        }
    }

    #[test]
    fn bounds_of_points() {
        let pts = [[1.0, 2.0, 0.0], [-3.0, 5.0, 2.7], [0.0, 0.0, 1.0]];
        assert_eq!(bounds(pts.iter()), Some(([-3.0, 0.0, 0.0], [1.0, 5.0, 2.7])));
        assert_eq!(bounds(std::iter::empty::<&[f64; 3]>()), None);
    }

    #[test]
    fn install_creates_then_replaces_keeping_handles() {
        let mut doc = Document::new();
        let (created, replaced) = install(&mut doc, boxed());
        assert_eq!((created, replaced), (5, 0));
        let handles: Vec<_> = names().iter().map(|n| doc.views.get(n).unwrap().handle).collect();
        assert!(handles.iter().all(|h| h.is_valid()));
        let (created, replaced) = install(&mut doc, build([0.0, 0.0, 0.0], [20.0, 12.0, 3.0]));
        assert_eq!((created, replaced), (0, 5));
        for (name, handle) in names().iter().zip(handles) {
            let view = doc.views.get(name).unwrap();
            assert_eq!(view.handle, handle, "{name}");
            assert!(view.height > 20.0, "{name}: ora inquadra il modello più grande");
        }
        assert_eq!(doc.views.iter().count(), 5);
    }
}
