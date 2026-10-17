//! AutoCAD command names that ArchLine has under another one. They are
//! rewritten to the ArchLine command before dispatch, so they behave the same
//! and keep working when the user's own alias file predates them.
//!
//! Deliberately left out: `BCLOSE` (AutoCAD asks before dropping edits, and
//! `BEDIT_DISCARD` does not), `XBIND` (binds chosen dependent symbols, `XREF
//! Bind` binds a whole reference), `LAYOUT` and `MODEL` (no command with their
//! options here).

/// AutoCAD name, the ArchLine command line it stands for.
const NAMES: &[(&str, &str)] = &[
    ("GCPARALLEL", "PCONSTRAINT"),
    ("GCTANGENT", "TCONSTRAINT"),
    ("GCSYMMETRIC", "SYCONSTRAINT"),
    ("GCCOLLINEAR", "LCONSTRAINT"),
    ("MTEDIT", "TEXTEDIT"),
    ("LAYERSTATESAVE", "LAYERSTATE SAVE"),
    ("NEWVIEW", "VIEW"),
];

/// `cmd` with its verb renamed, when the verb is one of [`NAMES`].
pub(super) fn rename(cmd: &str) -> Option<String> {
    let cmd = cmd.trim_start();
    let (verb, rest) = match cmd.split_once(char::is_whitespace) {
        Some((verb, rest)) => (verb, Some(rest)),
        None => (cmd, None),
    };
    let (_, target) = NAMES.iter().find(|(name, _)| name.eq_ignore_ascii_case(verb))?;
    Some(match rest {
        Some(rest) => format!("{target} {rest}"),
        None => (*target).to_string(),
    })
}

// Commands that are dispatched but were not offered by autocomplete.
// (BSAVE is an upstream command now; ULAYERS and PDFADJUST are upstream commands
// that autocomplete still did not list.)
inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "GCPARALLEL",
        "GCTANGENT",
        "GCSYMMETRIC",
        "GCCOLLINEAR",
        "MTEDIT",
        "LAYERSTATESAVE",
        "NEWVIEW",
        "ULAYERS",
        "PDFADJUST",
    ]
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;

    fn run(app: &mut OpenCADStudio, cmd: &str) -> String {
        let start = app.command_line.history.len();
        let _ = app.run_command_line(cmd);
        app.command_line.history[start..]
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn fresh() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn rename_replaces_only_the_verb_and_keeps_the_arguments() {
        assert_eq!(rename("GCPARALLEL").as_deref(), Some("PCONSTRAINT"));
        assert_eq!(rename("gcparallel").as_deref(), Some("PCONSTRAINT"));
        assert_eq!(rename("LAYERSTATESAVE Plan").as_deref(), Some("LAYERSTATE SAVE Plan"));
        assert_eq!(rename("NEWVIEW"), Some("VIEW".to_string()));
        assert_eq!(rename("LINE"), None);
        assert_eq!(rename("GCPARALLELS"), None, "whole word only");
        assert_eq!(rename(""), None);
    }

    #[test]
    fn every_name_behaves_like_the_command_it_stands_for() {
        for (autocad, archline) in NAMES {
            let mut a = fresh();
            let mut b = fresh();
            let by_alias = run(&mut a, autocad);
            let direct = run(&mut b, archline);
            assert_eq!(by_alias, direct, "{autocad} should do what {archline} does");
            assert_eq!(
                a.tabs[0].active_cmd.as_ref().map(|c| c.name()),
                b.tabs[0].active_cmd.as_ref().map(|c| c.name()),
                "{autocad}"
            );
            assert!(!by_alias.to_lowercase().contains("unknown"), "{autocad}: {by_alias:?}");
        }
    }

    #[test]
    fn the_names_are_offered_by_autocomplete() {
        let registered = crate::command::all_registered_command_names();
        for (autocad, _) in NAMES {
            assert!(registered.contains(autocad), "{autocad}");
        }
        for name in ["ULAYERS", "PDFADJUST"] {
            assert!(registered.contains(&name), "{name}");
        }
    }
}
