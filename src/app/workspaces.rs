//! ArchLine: workspace actions (switch, save, delete, ...) and, for now, the
//! startup behaviour. The model is `crate::workspaces`.

#[cfg(test)]
mod tests {
    use crate::app::config::AppConfig;
    use crate::app::OpenCADStudio;
    use crate::ui::toolbar_layout::{Target, ToolbarId};
    use crate::workspaces::{Interface, WorkspaceSet, AUTOCAD_CLASSIC, MY_WORKSPACE, OPENCAD_STUDIO};

    #[test]
    fn apply_config_runs_first_run_and_sets_the_interface() {
        let mut app = OpenCADStudio::new_for_test();
        // A custom layout and no workspaces yet: kept as "My Workspace".
        let mut cfg = AppConfig::default();
        cfg.toolbars
            .move_to(ToolbarId::Draw, Target::Float { x: 40.0, y: 90.0 });
        cfg.workspaces = WorkspaceSet::default();
        app.apply_config(cfg);
        assert_eq!(app.workspaces.current, MY_WORKSPACE);
        assert!(crate::workspace::is_classic());
        // A saved OpenCAD Studio turns the ribbon on.
        let mut cfg = AppConfig::default();
        cfg.workspaces.current = OPENCAD_STUDIO.to_string();
        app.apply_config(cfg);
        assert!(!crate::workspace::is_classic());
        // And back.
        let mut cfg = AppConfig::default();
        cfg.workspaces.current = AUTOCAD_CLASSIC.to_string();
        app.apply_config(cfg);
        assert!(crate::workspace::is_classic());
    }

    #[test]
    fn apply_config_keeps_the_working_toolbars() {
        let mut app = OpenCADStudio::new_for_test();
        let mut cfg = AppConfig::default();
        cfg.workspaces.current = AUTOCAD_CLASSIC.to_string();
        // The working layout differs from the saved AutoCAD Classic one.
        cfg.toolbars
            .move_to(ToolbarId::Draw, Target::Float { x: 40.0, y: 90.0 });
        app.apply_config(cfg.clone());
        assert_eq!(app.toolbars, cfg.toolbars.sanitized());
    }

    #[test]
    fn current_config_carries_the_workspaces() {
        let mut app = OpenCADStudio::new_for_test();
        app.apply_config(AppConfig::default());
        let cfg = app.current_config();
        assert_eq!(cfg.workspaces.current, AUTOCAD_CLASSIC);
        assert_eq!(cfg.workspaces, app.workspaces);
        assert_eq!(app.workspaces.resolved().interface, Interface::Classic);
    }
}
