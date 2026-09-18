use super::*;
use crate::core::config::AppConfig;
use crate::core::history::History;
use crate::core::item::{ScriptMetatags, ScriptMode, Target};
use crate::core::theme::ThemeConfig;
use gpui::{Keystroke, Modifiers, ScrollStrategy};
use std::path::PathBuf;

fn item(name: &str) -> Target {
    Target::Script {
        name: name.into(),
        mode: ScriptMode::FullOutput,
        icon: None,
        icon_image_path: None,
        path: std::sync::Arc::from(PathBuf::from(name)),
        metadata: std::sync::Arc::default(),
        metatags: ScriptMetatags::default(),
        inline_output: None,
    }
}

fn key(k: &str) -> Keystroke {
    Keystroke {
        modifiers: Modifiers::default(),
        key: k.to_string(),
        key_char: (k.len() == 1).then(|| k.to_string()),
    }
}

fn names(l: &Launcher) -> Vec<String> {
    l.filtered
        .iter()
        .map(|&i| l.all[i].name().to_string())
        .collect()
}

fn cap_config(max_results: usize) -> AppConfig {
    let mut config = AppConfig::default();
    config.general.max_results = max_results;
    config
}

fn grid_item(name: &str) -> Target {
    let mut t = item(name);
    if let Target::Script { mode, metatags, .. } = &mut t {
        *mode = ScriptMode::Compact;
        *metatags = ScriptMetatags {
            show_search: Some(false),
            columns: Some(3),
            layout: None,
            width: None,
        };
    }
    t
}

#[test]
fn metatags_commit_on_execute_not_on_hover() {
    let mut l = Launcher::new(
        vec![item("Alpha"), grid_item("Grid Script"), item("Zeta")],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );

    // Hovering/selecting the grid script alone must not change the layout.
    l.selected = 1;
    assert_eq!(l.effective_columns(), 1);

    // Executing it commits the override...
    let target = l.all[l.filtered[1]].clone();
    l.execute_target_with_args(&target, Vec::new());
    assert_eq!(l.effective_columns(), 3);
    assert_eq!(
        l.sticky_metatags.as_ref().and_then(|m| m.show_search),
        Some(false)
    );

    // ...and it survives moving the selection elsewhere.
    l.selected = 2;
    assert_eq!(l.effective_columns(), 3);

    // Typing a new query keeps the override for the session.
    l.handle_keystroke(&key("z"), None);
    assert_eq!(l.effective_columns(), 3);

    // Executing a script without metatags replaces the override with
    // the theme defaults (the last executed script decides the layout).
    l.handle_keystroke(&key("backspace"), None); // clear "z", full list back
    let plain = l.all.iter().find(|t| t.name() == "Alpha").unwrap().clone();
    l.execute_target_with_args(&plain, Vec::new());
    assert_eq!(l.effective_columns(), 1);

    // Hiding the launcher resets it.
    l.on_hide();
    assert_eq!(l.effective_columns(), 1);
}

#[test]
fn inline_output_updates_rows_without_reordering() {
    let mut inline = item("Inline Script");
    if let Target::Script { mode, .. } = &mut inline {
        *mode = ScriptMode::Inline;
    }
    let mut l = Launcher::new(
        vec![item("Alpha"), inline, item("Zeta")],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    let path = match &l.all[1] {
        Target::Script { path, .. } => path.clone(),
        _ => unreachable!(),
    };

    // A periodic refresh updates the subtitle on both the master list
    // and the rendered rows, without re-filtering (which would reset
    // the scroll position).
    l.apply_inline_output(&path, Some("subtitle-updated".into()));
    assert_eq!(l.all[1].inline_output(), Some("subtitle-updated"));
    assert_eq!(
        l.all[l.filtered[1]].inline_output(),
        Some("subtitle-updated")
    );
    assert_eq!(names(&l), vec!["Alpha", "Inline Script", "Zeta"]);
}

#[test]
fn alias_resolves_target_by_exact_query() {
    let mut app_config = AppConfig::default();
    app_config
        .aliases
        .insert("rc".to_string(), "Reload Configuration".to_string());
    let mut l = Launcher::new(
        vec![item("Grep"), Target::reload_config()],
        ThemeConfig::default(),
        app_config,
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.query = "rc".to_string();
    assert_eq!(
        l.alias_target().map(|t| t.name()),
        Some("Reload Configuration")
    );
    l.query = "r".to_string();
    assert_eq!(l.alias_target(), None);
}

#[test]
fn alias_lists_every_alias_pointing_at_target() {
    let mut app_config = AppConfig::default();
    app_config
        .aliases
        .insert("gh".to_string(), "GitHub".to_string());
    app_config
        .aliases
        .insert("github".to_string(), "GitHub".to_string());
    app_config
        .aliases
        .insert("rc".to_string(), "Reload Configuration".to_string());
    let l = Launcher::new(
        vec![item("GitHub"), Target::reload_config(), item("Grep")],
        ThemeConfig::default(),
        app_config,
        History::test_new(PathBuf::new(), Vec::new()),
    );
    let gh = l.alias_labels("GitHub");
    assert!(gh.contains(&"gh".to_string()));
    assert!(gh.contains(&"github".to_string()));
    assert_eq!(gh.len(), 2);
    // A target with no configured aliases yields none.
    assert!(l.alias_labels("Grep").is_empty());
}

fn keystroke(key: &str, modifiers: Modifiers) -> Keystroke {
    Keystroke {
        modifiers,
        key: key.to_string(),
        key_char: (key.len() == 1).then(|| key.to_string()),
    }
}

#[test]
fn combo_matches_key_combinations() {
    let cmd_r = keystroke(
        "r",
        Modifiers {
            platform: true,
            ..Default::default()
        },
    );
    assert!(combo_matches("cmd+r", &cmd_r));
    assert!(combo_matches("command+r", &cmd_r));
    assert!(!combo_matches("ctrl+r", &cmd_r));
    assert!(!combo_matches("cmd+x", &cmd_r));
    assert!(!combo_matches("cmd+shift+r", &cmd_r));

    let plain_r = keystroke("r", Modifiers::default());
    assert!(combo_matches("r", &plain_r));
    assert!(!combo_matches("cmd+r", &plain_r));

    let ctrl_shift_x = keystroke(
        "x",
        Modifiers {
            control: true,
            shift: true,
            ..Default::default()
        },
    );
    assert!(combo_matches("ctrl+shift+x", &ctrl_shift_x));
    assert!(combo_matches("shift+ctrl+x", &ctrl_shift_x));
}

#[test]
fn empty_query_shows_all_in_order() {
    let l = Launcher::new(
        vec![item("Git Status"), item("Clipboard History")],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    assert_eq!(
        names(&l),
        vec!["Git Status".to_string(), "Clipboard History".to_string()]
    );
}

#[test]
fn typing_filters_fuzzy_and_excludes_non_matches() {
    let mut l = Launcher::new(
        vec![item("Git Status"), item("Clipboard History"), item("Grep")],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&key("g"), None);
    let n = names(&l);
    assert!(n.contains(&"Git Status".to_string()));
    assert!(n.contains(&"Grep".to_string()));
    assert!(!n.contains(&"Clipboard History".to_string()));
}

#[test]
fn backspace_restores_previous_results() {
    let mut l = Launcher::new(
        vec![item("Git Status"), item("Grep")],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&key("g"), None);
    assert_eq!(names(&l).len(), 2);
    l.handle_keystroke(&key("g"), None); // "gg" matches neither
    assert!(names(&l).is_empty());
    l.handle_keystroke(&key("backspace"), None); // back to "g"
    assert!(names(&l).contains(&"Git Status".to_string()));
}

#[test]
fn arrows_move_and_clamp_selection() {
    let mut l = Launcher::new(
        vec![item("Git Status"), item("Grep"), item("Copy")],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    assert_eq!(l.selected, 0);
    l.handle_keystroke(&key("down"), None);
    assert_eq!(l.selected, 1);
    l.handle_keystroke(&key("down"), None);
    assert_eq!(l.selected, 2);
    l.handle_keystroke(&key("down"), None); // clamps at the last row
    assert_eq!(l.selected, 2);
    l.handle_keystroke(&key("up"), None);
    assert_eq!(l.selected, 1);
}

#[test]
fn list_shows_all_items() {
    let mut l = Launcher::new(
        vec![item("A One"), item("A Two"), item("A Three")],
        ThemeConfig::default(),
        cap_config(10),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    assert_eq!(
        names(&l),
        vec![
            "A One".to_string(),
            "A Two".to_string(),
            "A Three".to_string()
        ]
    );
    l.handle_keystroke(&key("a"), None);
    assert_eq!(names(&l).len(), 3);
}

#[test]
fn escape_signals_hide_and_resets_query() {
    let mut l = Launcher::new(
        vec![item("Git Status"), item("Grep")],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&key("g"), None);
    assert_eq!(l.query, "g");
    assert_eq!(
        l.handle_keystroke(&key("escape"), None),
        LauncherAction::Hide
    );
    assert_eq!(l.query, "");
    assert_eq!(l.selected, 0);
}

fn deferred(l: &Launcher) -> Option<(usize, ScrollStrategy)> {
    l.list
        .0
        .borrow()
        .deferred_scroll_to_item
        .map(|d| (d.item_index, d.strategy))
}

fn gui_deferred(l: &Launcher) -> Option<(usize, ScrollStrategy)> {
    l.gui_rows_scroll
        .0
        .borrow()
        .deferred_scroll_to_item
        .map(|d| (d.item_index, d.strategy))
}

#[test]
fn arrow_key_defers_scroll_to_selected_item() {
    let mut l = Launcher::new(
        vec![item("Git Status"), item("Grep"), item("Copy")],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&key("down"), None);
    assert_eq!(deferred(&l), Some((1, ScrollStrategy::Nearest)));
}

#[test]
fn gui_arrow_key_defers_scroll_to_selected_row() {
    let mut l = Launcher::new(
        Vec::new(),
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.state = LauncherState::GuiMode {
        title: "Clipboard".to_string(),
        rows: std::sync::Arc::new(
            (0..5)
                .map(|i| crate::core::gui_protocol::GuiRow::new(format!("item {i}")))
                .collect(),
        ),
        filtered_rows: (0..5).collect(),
        prompt: None,
        message: None,
        no_custom: false,
        selected: 0,
        columns: None,
        query: String::new(),
        loading: false,
        live_search: false,
        active_indices: Vec::new(),
        data: None,
        preview_blocks: None,
        multi_select: false,
        toggled_indices: std::collections::HashSet::new(),
        markup_rows: false,
        layout: None,
    };

    for _ in 0..4 {
        l.handle_keystroke(&key("down"), None);
    }

    assert_eq!(
        gui_deferred(&l),
        Some((4, ScrollStrategy::Nearest)),
        "GUI arrow key must scroll the rows list to the selected row"
    );
}

#[test]
fn format_combo_uses_macos_glyphs_in_canonical_order() {
    assert_eq!(format_combo("cmd+r"), "⌘R");
    assert_eq!(format_combo("opt+d"), "⌥D");
    assert_eq!(format_combo("ctrl+alt+f12"), "⌃⌥F12");
    assert_eq!(format_combo("cmd+shift+space"), "⇧⌘␣");
    assert_eq!(format_combo("alt+return"), "⌥⏎");
    assert_eq!(format_combo("cmd+f1"), "⌘F1");
    assert_eq!(format_combo("ctrl+left"), "⌃←");
}

#[test]
fn shortcut_label_shows_global_then_local_combos() {
    let mut app_config = AppConfig::default();
    app_config
        .bindings
        .global
        .insert("opt+g".to_string(), "Marker".to_string());
    app_config
        .bindings
        .launcher
        .insert("cmd+r".to_string(), "Reload Configuration".to_string());
    app_config
        .bindings
        .global
        .insert("opt+m".to_string(), "Reload Configuration".to_string());
    let l = Launcher::new(
        vec![item("Marker"), item("Grep")],
        ThemeConfig::default(),
        app_config,
        History::test_new(PathBuf::new(), Vec::new()),
    );
    assert_eq!(l.shortcut_label("Marker").as_deref(), Some("⌥G"));
    assert_eq!(l.shortcut_label("Grep").as_deref(), None);
    assert_eq!(
        l.shortcut_label("Reload Configuration").as_deref(),
        Some("⌥M  ⌘R")
    );
}

#[test]
fn typing_defers_scroll_back_to_top() {
    let mut l = Launcher::new(
        vec![item("Git Status"), item("Grep")],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&key("g"), None);
    assert_eq!(deferred(&l), Some((0, ScrollStrategy::Top)));
}

fn item_with_args(name: &str, n_args: usize, confirm: bool) -> Target {
    use crate::core::item::{RaycastMetadata, ScriptArgument};
    let mut metadata = RaycastMetadata::default();
    for (i, slot) in [
        &mut metadata.argument1,
        &mut metadata.argument2,
        &mut metadata.argument3,
    ]
    .iter_mut()
    .enumerate()
    .take(n_args)
    {
        **slot = Some(ScriptArgument {
            arg_type: Some("text".to_string()),
            placeholder: Some(format!("Arg {i}")),
            optional: None,
            percent_encoded: None,
            data: None,
        });
    }
    metadata.needs_confirmation = confirm.then_some(true);
    Target::Script {
        name: name.into(),
        mode: ScriptMode::Compact,
        icon: None,
        icon_image_path: None,
        path: std::sync::Arc::from(PathBuf::from(name)),
        metadata: std::sync::Arc::new(metadata),
        metatags: ScriptMetatags::default(),
        inline_output: None,
    }
}

fn arg_state(l: &Launcher) -> Option<(Vec<String>, usize)> {
    match &l.state {
        LauncherState::ArgumentInput {
            values,
            focused_index,
            ..
        } => Some((values.clone(), *focused_index)),
        _ => None,
    }
}

#[test]
fn enter_on_script_with_args_starts_argument_prompt() {
    let mut l = Launcher::new(
        vec![item_with_args("Args Test", 2, false)],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    let action = l.handle_keystroke(&key("enter"), None);
    assert_eq!(action, LauncherAction::None);
    assert_eq!(arg_state(&l), Some((vec![String::new(), String::new()], 0)));
}

#[test]
fn argument_prompt_accepts_typing_focus_and_confirm() {
    let mut l = Launcher::new(
        vec![item_with_args("Args Test", 2, false)],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&key("enter"), None); // enter the prompt
    l.handle_keystroke(&key("h"), None);
    l.handle_keystroke(&key("i"), None);
    assert_eq!(
        arg_state(&l),
        Some((vec!["hi".to_string(), String::new()], 0))
    );
    l.handle_keystroke(&key("enter"), None); // advance to the next argument
    assert_eq!(
        arg_state(&l),
        Some((vec!["hi".to_string(), String::new()], 1))
    );
    l.handle_keystroke(&key("tab"), None); // wraps back to the first
    assert_eq!(
        arg_state(&l),
        Some((vec!["hi".to_string(), String::new()], 0))
    );
    l.handle_keystroke(&key("tab"), None); // back to the second
    l.handle_keystroke(&key("t"), None);
    l.handle_keystroke(&key("a"), None);
    l.handle_keystroke(&key("b"), None);
    l.handle_keystroke(&key("backspace"), None); // "ta"
    let action = l.handle_keystroke(&key("enter"), None); // last arg -> run
    assert!(matches!(
        action,
        LauncherAction::ExecuteScript(_, ref args)
            if *args == vec!["hi".to_string(), "ta".to_string()]
    ));
    assert_eq!(l.state, LauncherState::Search);
}

#[test]
fn escape_cancels_argument_prompt() {
    let mut l = Launcher::new(
        vec![item_with_args("Args Test", 2, false)],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&key("enter"), None);
    l.handle_keystroke(&key("h"), None);
    let action = l.handle_keystroke(&key("escape"), None);
    assert_eq!(action, LauncherAction::None);
    assert_eq!(l.state, LauncherState::Search);
}

#[test]
fn argument_prompt_confirms_before_executing() {
    let mut l = Launcher::new(
        vec![item_with_args("Confirm Args", 1, true)],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&key("enter"), None);
    l.handle_keystroke(&key("x"), None);
    l.handle_keystroke(&key("enter"), None); // last arg -> confirmation
    assert!(matches!(l.state, LauncherState::Confirming { .. }));
    l.handle_keystroke(&key("escape"), None); // decline
    assert_eq!(l.state, LauncherState::Search);
    l.handle_keystroke(&key("enter"), None); // prompt again
    l.handle_keystroke(&key("y"), None);
    l.handle_keystroke(&key("enter"), None);
    let action = l.handle_keystroke(&key("enter"), None); // confirm
    assert!(matches!(
        action,
        LauncherAction::ExecuteScript(_, ref args) if *args == vec!["y".to_string()]
    ));
    assert_eq!(l.state, LauncherState::Search);
}

#[test]
fn full_output_still_swallows_keystrokes() {
    let mut l = Launcher::new(
        vec![item("Git Status")],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.state = LauncherState::FullOutput { title: "t".into() };
    l.full_output_blocks = crate::core::markdown::parse("out");
    l.handle_keystroke(&key("a"), None);
    assert_eq!(l.query, "");
    l.handle_keystroke(&key("down"), None);
    assert_eq!(l.selected, 0);
    let action = l.handle_keystroke(&key("escape"), None);
    assert_eq!(action, LauncherAction::None);
    assert_eq!(l.state, LauncherState::Search);
}

#[test]
fn launcher_initializes_widget_registry_from_theme() {
    use crate::core::theme::WidgetDef;
    let mut theme = ThemeConfig::default();
    theme.widgets.push(WidgetDef::Text {
        id: "greeting".to_string(),
        text: Some("Hello".to_string()),
        color: None,
        font_size: None,
        font_weight: None,
        align: None,
    });
    let l = Launcher::new(
        vec![item("Git Status")],
        theme,
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    assert!(l.widget_registry.get("greeting").is_some());
}

#[test]
fn button_widget_in_registry() {
    use crate::core::theme::WidgetDef;
    let mut theme = ThemeConfig::default();
    theme.widgets.push(WidgetDef::Button {
        id: "btn_test".to_string(),
        text: Some("Action".to_string()),
        icon: Some("⚡".to_string()),
        action: Some("reload".to_string()),
        hotkey: None,
        color: None,
        background: None,
        hover_background: None,
        hover_color: None,
        border_color: None,
        border_width: None,
        radius: None,
        padding: None,
        font_size: None,
        font_weight: None,
        gap: None,
    });
    let l = Launcher::new(
        vec![item("Git Status")],
        theme,
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    let btn = l.widget_registry.get("btn_test");
    assert!(btn.is_some());
    if let Some(WidgetDef::Button { action, icon, .. }) = btn {
        assert_eq!(action.as_deref(), Some("reload"));
        assert_eq!(icon.as_deref(), Some("⚡"));
    } else {
        panic!("expected Button widget");
    }
}

#[test]
fn button_hotkey_triggers_action() {
    use crate::core::theme::WidgetDef;
    let mut theme = ThemeConfig::default();
    theme.widgets.push(WidgetDef::Button {
        id: "btn_reload".to_string(),
        text: Some("Reload".to_string()),
        icon: None,
        action: Some("reload".to_string()),
        hotkey: Some("cmd+r".to_string()),
        color: None,
        background: None,
        hover_background: None,
        hover_color: None,
        border_color: None,
        border_width: None,
        radius: None,
        padding: None,
        font_size: None,
        font_weight: None,
        gap: None,
    });
    let l = Launcher::new(
        vec![Target::reload_config()],
        theme,
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    assert_eq!(l.button_hotkeys.len(), 1);
    assert_eq!(
        l.button_hotkeys.get("cmd+r").map(|s| s.as_str()),
        Some("reload")
    );
}

#[test]
fn element_layout_accepts_custom_slot_ordering() {
    let mut theme = ThemeConfig::default();
    theme.element.layout = Some(vec![
        "name".to_string(),
        "spacer".to_string(),
        "icon".to_string(),
        "shortcuts".to_string(),
    ]);
    let l = Launcher::new(
        vec![item("Git Status")],
        theme,
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    assert_eq!(
        l.theme.element.layout.as_ref().unwrap(),
        &["name", "spacer", "icon", "shortcuts"]
    );
}

#[test]
fn element_layout_supports_custom_widgets_in_row() {
    use crate::core::theme::WidgetDef;
    let mut theme = ThemeConfig::default();
    theme.widgets.push(WidgetDef::Button {
        id: "btn_run".to_string(),
        text: Some("Run".to_string()),
        icon: Some("▶".to_string()),
        action: Some("run".to_string()),
        hotkey: None,
        color: None,
        background: None,
        hover_background: None,
        hover_color: None,
        border_color: None,
        border_width: None,
        radius: None,
        padding: None,
        font_size: None,
        font_weight: None,
        gap: None,
    });
    theme.element.layout = Some(vec![
        "icon".to_string(),
        "name".to_string(),
        "spacer".to_string(),
        "btn_run".to_string(),
    ]);
    let l = Launcher::new(
        vec![item("Test Script")],
        theme,
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    assert!(l.widget_registry.get("btn_run").is_some());
    assert_eq!(
        l.theme.element.layout.as_ref().unwrap(),
        &["icon", "name", "spacer", "btn_run"]
    );
}

/// Drive the GUI-mode state machine the way a script-driven list does —
/// repeated bursts re-placing `rows: Arc<…>` and local selection churn (what
/// arrow-key "scrolling" does without a script round-trip) — and assert the
/// process RSS does not grow without bound. The row count varies each
/// iteration so any per-burst retention would accumulate and be caught.
#[test]
fn gui_mode_burst_and_selection_loop_does_not_leak() {
    use crate::core::gui_protocol::GuiBurst;
    use std::process::Command;

    fn rss_kb() -> usize {
        let pid = std::process::id();
        let Ok(out) = Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
        else {
            return 0;
        };
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0)
    }

    let mut l = Launcher::new(
        Vec::new(),
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );

    // Warm up once so the first-burst one-time allocations don't skew the
    // initial reading.
    l.gui_apply_burst(
        GuiBurst::from_lines(&["\0prompt\x1fPick".to_string(), "seed".to_string()]),
        "Theme Switcher",
    );
    std::hint::black_box(&l);
    let initial = rss_kb();

    for i in 0..8000usize {
        let row_count = 20 + (i % 40) as u32;
        let mut lines: Vec<String> =
            vec!["\0prompt\x1fPick a theme".to_string(), "\0message\x1fApply".to_string()];
        for j in 0..row_count {
            lines.push(format!(
                "theme {i} row {j}\x00id\x1fid_{i}_{j}\x00info\x1fswatches\x00meta\x1fmeta-{i}"
            ));
        }
        lines.push("\0flush".to_string());
        l.gui_apply_burst(GuiBurst::from_lines(&lines), "Theme Switcher");
        // Local selection churn — the state-side of "scrolling" through the
        // list (arrow keys route into `gui_move_selection`).
        l.handle_keystroke(&key("down"), None);
        l.handle_keystroke(&key("up"), None);
    }

    let final_rss = rss_kb();
    let delta = final_rss.saturating_sub(initial);
    println!("gui-mode loop: initial={initial} KB, final={final_rss} KB, delta={delta} KB");
    assert!(delta < 8192, "GUI-mode loop leaked too much: {delta} KB");
}
