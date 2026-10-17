//! Sheet-by-sheet PDF delivery: one file per layout instead of one PDF with
//! every page. The file for a layout is `<base>-<Layout>.pdf`, the layout name
//! reduced to letters, digits, `-` and `_`.

use std::path::{Path, PathBuf};

use crate::io::pdf_export::PdfPageInput;
use crate::io::plot_style::PlotStyleTable;

/// File names (without the folder) for `layouts` under `base`, which is the
/// path the user chose with or without its `.pdf`. Two layouts that reduce to
/// the same name, ignoring case, get `-2`, `-3`… so no sheet overwrites another.
pub fn sheet_file_names(base: &Path, layouts: &[String]) -> Vec<String> {
    let file = base.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = match file.len().checked_sub(4).filter(|&at| file.is_char_boundary(at)) {
        Some(at) if file[at..].eq_ignore_ascii_case(".pdf") => file[..at].to_string(),
        _ => file,
    };
    let mut taken: Vec<String> = Vec::new();
    layouts
        .iter()
        .map(|layout| {
            let safe: String = layout
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
                .collect();
            let safe = if safe.is_empty() { "Sheet".to_string() } else { safe };
            let mut name = format!("{stem}-{safe}");
            let mut copy = 2;
            while taken.contains(&name.to_lowercase()) {
                name = format!("{stem}-{safe}-{copy}");
                copy += 1;
            }
            taken.push(name.to_lowercase());
            format!("{name}.pdf")
        })
        .collect()
}

/// Writes each page to its own PDF beside `base`, returning the layout and the
/// file written for each. Stops at the first failure.
pub fn export_sheets(
    sheets: &[(String, PdfPageInput)],
    base: &Path,
    plot_style: Option<&PlotStyleTable>,
) -> Result<Vec<(String, PathBuf)>, String> {
    if sheets.is_empty() {
        return Err("No pages were selected.".into());
    }
    let layouts: Vec<String> = sheets.iter().map(|(layout, _)| layout.clone()).collect();
    let folder = base.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut written = Vec::with_capacity(sheets.len());
    for ((layout, page), name) in sheets.iter().zip(sheet_file_names(base, &layouts)) {
        let file = folder.join(name);
        crate::io::pdf_export::export_pdf_pages(std::slice::from_ref(page), &file, plot_style)
            .map_err(|error| format!("{}: {error}", file.display()))?;
        written.push((layout.clone(), file));
    }
    Ok(written)
}

/// The PDF step of "Print all": every page in the one file `path`, or one file
/// per sheet beside it. The message says what was written.
pub fn publish_pdf(
    sheets: Vec<(String, PdfPageInput)>,
    path: &Path,
    per_sheet: bool,
    plot_style: Option<&PlotStyleTable>,
) -> Result<String, String> {
    if per_sheet {
        let written = export_sheets(&sheets, path, plot_style)?;
        let folder = path.parent().map(|dir| dir.display().to_string()).unwrap_or_default();
        return Ok(crate::tf!("Exported {} sheets, one PDF each, to {}", written.len(), folder).into_owned());
    }
    let pages: Vec<PdfPageInput> = sheets.into_iter().map(|(_, page)| page).collect();
    crate::io::pdf_export::export_pdf_pages(&pages, path, plot_style)?;
    Ok(crate::tf!("Exported {} layouts to {}", pages.len(), path.display()).into_owned())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::io::pdf_export::{PdfPlotOptions, PlotContent};

    fn names(base: &str, layouts: &[&str]) -> Vec<String> {
        let layouts: Vec<String> = layouts.iter().map(|name| name.to_string()).collect();
        sheet_file_names(Path::new(base), &layouts)
    }

    fn page() -> PdfPageInput {
        PdfPageInput {
            content: PlotContent::default(),
            paper_w: 210.0,
            paper_h: 297.0,
            offset_x: 0.0,
            offset_y: 0.0,
            rotation_deg: 0,
            scale: 1.0,
            clip: None,
            options: PdfPlotOptions::default(),
            plot_style: None,
        }
    }

    fn temp_base(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("archline-publish-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("progetto.pdf")
    }

    #[test]
    fn each_layout_gets_the_chosen_name_and_its_own() {
        assert_eq!(names("C:/p/progetto.pdf", &["A1", "Pianta"]), ["progetto-A1.pdf", "progetto-Pianta.pdf"]);
        assert_eq!(names("C:/p/progetto", &["A1"]), ["progetto-A1.pdf"]);
    }

    #[test]
    fn characters_a_file_name_cannot_hold_become_underscores() {
        assert_eq!(names("progetto.pdf", &["Pianta P.T. 1:100"]), ["progetto-Pianta_P_T__1_100.pdf"]);
        assert_eq!(names("progetto.pdf", &["Prospetto nord"]), ["progetto-Prospetto_nord.pdf"]);
    }

    #[test]
    fn accented_letters_are_kept() {
        assert_eq!(names("progetto.pdf", &["Sezione città"]), ["progetto-Sezione_città.pdf"]);
    }

    #[test]
    fn layouts_that_reduce_to_the_same_name_do_not_overwrite_each_other() {
        assert_eq!(
            names("p.pdf", &["A 1", "A_1", "a_1"]),
            ["p-A_1.pdf", "p-A_1-2.pdf", "p-a_1-3.pdf"]
        );
    }

    #[test]
    fn a_layout_without_a_name_is_called_sheet() {
        assert_eq!(names("p.pdf", &[""]), ["p-Sheet.pdf"]);
    }

    #[test]
    fn one_pdf_is_written_per_sheet() {
        let base = temp_base("write");
        let sheets = vec![("A1".to_string(), page()), ("A2".to_string(), page())];
        let written = export_sheets(&sheets, &base, None).unwrap();
        let dir = base.parent().unwrap().to_path_buf();
        let listed: Vec<String> = written.iter().map(|(layout, _)| layout.clone()).collect();
        assert_eq!(listed, ["A1", "A2"]);
        for (_, file) in &written {
            assert!(std::fs::read(file).unwrap().starts_with(b"%PDF"), "{}", file.display());
        }
        assert_eq!(written[0].1.file_name().unwrap(), "progetto-A1.pdf");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn without_the_option_all_pages_go_into_one_pdf() {
        let base = temp_base("one");
        let sheets = vec![("A1".to_string(), page()), ("A2".to_string(), page())];
        let message = publish_pdf(sheets, &base, false, None).unwrap();
        assert!(base.exists());
        assert!(!base.with_file_name("progetto-A1.pdf").exists());
        assert!(message.contains('2') && message.contains("progetto.pdf"), "{message}");
        let _ = std::fs::remove_dir_all(base.parent().unwrap());
    }

    #[test]
    fn with_the_option_each_sheet_gets_its_own_pdf() {
        let base = temp_base("each");
        let sheets = vec![("A1".to_string(), page()), ("A2".to_string(), page())];
        let message = publish_pdf(sheets, &base, true, None).unwrap();
        assert!(!base.exists());
        assert!(base.with_file_name("progetto-A1.pdf").exists());
        assert!(base.with_file_name("progetto-A2.pdf").exists());
        assert!(message.contains('2'), "{message}");
        let _ = std::fs::remove_dir_all(base.parent().unwrap());
    }

    #[test]
    fn nothing_selected_is_an_error_not_an_empty_success() {
        let base = temp_base("empty");
        let error = export_sheets(&[], &base, None).unwrap_err();
        assert!(!error.is_empty());
        let _ = std::fs::remove_dir_all(base.parent().unwrap());
    }
}
