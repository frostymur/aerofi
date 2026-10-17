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

fn rofi_deferred(l: &Launcher) -> Option<(usize, ScrollStrategy)> {
    l.rofi_rows_scroll
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
fn rofi_arrow_key_defers_scroll_to_selected_row() {
    let mut l = Launcher::new(
        Vec::new(),
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.state = LauncherState::RofiMode {
        title: "Clipboard".to_string(),
        rows: std::sync::Arc::new(
            (0..5)
                .map(|i| crate::core::rofi_protocol::RofiRow::new(format!("item {i}")))
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
        preview_styled: None,
        preview_image: None,
        multi_select: false,
        toggled_indices: std::collections::HashSet::new(),
        markup_rows: false,
        layout: None,
    };

    for _ in 0..4 {
        l.handle_keystroke(&key("down"), None);
    }

    assert_eq!(
        rofi_deferred(&l),
        Some((4, ScrollStrategy::Nearest)),
        "Rofi arrow key must scroll the rows list to the selected row"
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
        margin: None,
        visible_when: None,
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
        close: None,
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
        border_top: None,
        border_right: None,
        border_bottom: None,
        border_left: None,
        shadow: None,
        radius: None,
        padding: None,
        margin: None,
        font_size: None,
        font_weight: None,
        gap: None,
        selected_when: None,
        selected: None,
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
        close: None,
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
        border_top: None,
        border_right: None,
        border_bottom: None,
        border_left: None,
        shadow: None,
        radius: None,
        padding: None,
        margin: None,
        font_size: None,
        font_weight: None,
        gap: None,
        selected_when: None,
        selected: None,
    });
    theme.widgets.push(WidgetDef::Button {
        id: "btn_stay".to_string(),
        close: Some(false),
        text: Some("Stay".to_string()),
        icon: None,
        action: Some("reload".to_string()),
        hotkey: Some("cmd+shift+s".to_string()),
        color: None,
        background: None,
        hover_background: None,
        hover_color: None,
        border_color: None,
        border_width: None,
        border_top: None,
        border_right: None,
        border_bottom: None,
        border_left: None,
        shadow: None,
        radius: None,
        padding: None,
        margin: None,
        font_size: None,
        font_weight: None,
        gap: None,
        selected_when: None,
        selected: None,
    });
    let l = Launcher::new(
        vec![Target::reload_config()],
        theme,
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    assert_eq!(l.button_hotkeys.len(), 2);
    let (action, stay_open) = l.button_hotkeys.get("cmd+r").cloned().unwrap();
    assert_eq!(action, "reload");
    assert!(!stay_open, "default close=true means stay_open is false");
    let (_, stay_open) = l.button_hotkeys.get("cmd+shift+s").cloned().unwrap();
    assert!(stay_open, "close=false keeps the launcher open");
}

#[test]
fn mode_all_action_resets_active_mode() {
    let mut config = AppConfig::default();
    config
        .modes
        .insert("scripts".to_string(), "scripts".to_string());
    let mut l = Launcher::new(
        vec![
            Target::App {
                name: "Safari".into(),
                path: std::sync::Arc::from(PathBuf::from("/Applications/Safari.app")),
                icon_path: None,
            },
            item("Git Status"),
            Target::reload_config(),
        ],
        ThemeConfig::default(),
        config,
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.activate_mode("scripts", None);
    assert_eq!(l.mode_name.as_deref(), Some("scripts"));
    assert_eq!(names(&l).len(), 1, "mode filters to scripts only");

    l.activate_mode("all", None);
    assert!(l.mode_name.is_none(), "mode:all clears the filter");
    assert_eq!(names(&l).len(), 3, "all sources are back");

    // With no active mode it is a no-op, not an error.
    l.activate_mode("all", None);
    assert!(l.mode_name.is_none());
}

#[test]
fn user_defined_all_mode_wins_over_reset() {
    let mut config = AppConfig::default();
    // A mode named "All" (capital A) takes precedence over the reset and
    // is activated by its exact key.
    config
        .modes
        .insert("All".to_string(), "scripts".to_string());
    let mut l = Launcher::new(
        vec![Target::App {
            name: "Safari".into(),
            path: std::sync::Arc::from(PathBuf::from("/Applications/Safari.app")),
            icon_path: None,
        }],
        ThemeConfig::default(),
        config,
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.activate_mode("all", None);
    assert_eq!(l.mode_name.as_deref(), Some("All"));
    assert!(
        names(&l).is_empty(),
        "the All mode filters to scripts, of which there are none"
    );
    // mode:all now switches TO the user's mode, not past it.
    l.activate_mode("all", None);
    assert_eq!(l.mode_name.as_deref(), Some("All"));
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
        close: None,
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
        border_top: None,
        border_right: None,
        border_bottom: None,
        border_left: None,
        shadow: None,
        radius: None,
        padding: None,
        margin: None,
        font_size: None,
        font_weight: None,
        gap: None,
        selected_when: None,
        selected: None,
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
fn app_target(name: &str, path: &str) -> Target {
    Target::App {
        name: name.into(),
        path: std::sync::Arc::from(PathBuf::from(path)),
        icon_path: None,
    }
}

fn cmd_key(k: &str) -> Keystroke {
    keystroke(
        k,
        Modifiers {
            platform: true,
            ..Default::default()
        },
    )
}

#[test]
fn cmd_k_opens_action_menu_for_app_row() {
    let mut l = Launcher::new(
        vec![
            app_target("Finder", "/System/Library/CoreServices/Finder.app"),
            item("Script"),
        ],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    assert!(matches!(l.state, LauncherState::Search));
    let action = l.handle_keystroke(&cmd_key("k"), None);
    assert_eq!(action, LauncherAction::None);
    let LauncherState::ActionMenu {
        target,
        actions,
        selected,
    } = &l.state
    else {
        panic!(
            "expected ActionMenu, got {:?}",
            std::mem::discriminant(&l.state)
        );
    };
    assert_eq!(target.name(), "Finder");
    assert_eq!(*selected, 0);
    assert_eq!(actions.len(), 3);
    assert_eq!(actions[0].label, "Reveal in Finder");
}

#[test]
fn cmd_k_on_script_row_includes_new_window() {
    let mut l = Launcher::new(
        vec![item("deploy.sh")],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&cmd_key("k"), None);
    let LauncherState::ActionMenu { actions, .. } = &l.state else {
        panic!("expected ActionMenu");
    };
    // Scripts get the full built-in set: New Window opens them in
    // Terminal (via execute_item's script path), so it's safe to offer.
    assert_eq!(actions.len(), 3);
    assert_eq!(actions[2].label, "New Window");
}

#[test]
fn cmd_k_on_builtin_row_does_nothing() {
    let mut l = Launcher::new(
        vec![Target::reload_config(), item("Script")],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&cmd_key("k"), None);
    assert!(matches!(l.state, LauncherState::Search));
}

#[test]
fn action_menu_navigates_and_escape_restores() {
    let mut l = Launcher::new(
        vec![
            app_target("Finder", "/Applications/Finder.app"),
            item("Script"),
        ],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&cmd_key("k"), None);
    // j/k and arrows navigate the menu rows.
    l.handle_keystroke(&key("j"), None);
    l.handle_keystroke(&key("down"), None);
    if let LauncherState::ActionMenu { selected, .. } = &l.state {
        assert_eq!(*selected, 2);
    } else {
        panic!("expected ActionMenu");
    }
    l.handle_keystroke(&key("k"), None);
    l.handle_keystroke(&key("up"), None);
    if let LauncherState::ActionMenu { selected, .. } = &l.state {
        assert_eq!(*selected, 0);
    } else {
        panic!("expected ActionMenu");
    }
    // Escape closes the menu and the previous state (query + selection)
    // is untouched.
    l.handle_keystroke(&key("escape"), None);
    assert!(matches!(l.state, LauncherState::Search));
    assert_eq!(l.query, "");
    assert_eq!(l.selected, 0);
}

#[test]
fn action_menu_enter_executes_and_hides() {
    let mut l = Launcher::new(
        vec![
            app_target("Finder", "/Applications/Finder.app"),
            item("Script"),
        ],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&cmd_key("k"), None);
    // Copy Path is row 1.
    l.handle_keystroke(&key("j"), None);
    let action = l.handle_keystroke(&key("enter"), None);
    match action {
        LauncherAction::CopyWithToast { text, toast } => {
            assert_eq!(text, "/Applications/Finder.app");
            assert!(toast.contains("Finder"));
        }
        other => panic!("expected CopyWithToast, got {other:?}"),
    }
    assert!(matches!(l.state, LauncherState::Search));
}

#[test]
fn action_menu_custom_action_templates() {
    let mut cfg = AppConfig::default();
    cfg.actions = Some(crate::core::config::ActionsConfig {
        apps: Some(crate::core::config::ActionSetConfig {
            reveal: false,
            copy_path: false,
            new_window: false,
            custom: vec![crate::core::config::CustomActionConfig {
                label: "Ping".into(),
                command: "ping -c 1 {name}".into(),
            }],
        }),
        scripts: None,
    });
    let mut l = Launcher::new(
        vec![app_target("Finder", "/Applications/Finder.app")],
        ThemeConfig::default(),
        cfg,
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&cmd_key("k"), None);
    let action = l.handle_keystroke(&key("enter"), None);
    match action {
        LauncherAction::RunShell(cmd) => assert_eq!(cmd, "ping -c 1 Finder"),
        other => panic!("expected RunShell, got {other:?}"),
    }
}

#[test]
fn rofi_keep_rows_burst_preserves_list_and_selection() {
    // Preview-only frame (\0keep-rows, no rows): the list, the selection
    // and the query must be untouched; the preview command applies.
    let mut l = Launcher::new(
        Vec::new(),
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.state = LauncherState::RofiMode {
        title: "Clipboard".to_string(),
        rows: std::sync::Arc::new(
            (0..5)
                .map(|i| crate::core::rofi_protocol::RofiRow::new(format!("item {i}")))
                .collect(),
        ),
        filtered_rows: (0..5).collect(),
        prompt: None,
        message: None,
        no_custom: false,
        selected: 3,
        columns: None,
        query: "abc".to_string(),
        loading: false,
        live_search: false,
        active_indices: Vec::new(),
        data: None,
        preview_blocks: None,
        preview_styled: None,
        preview_image: None,
        multi_select: false,
        toggled_indices: std::collections::HashSet::new(),
        markup_rows: false,
        layout: None,
    };

    let burst = crate::core::rofi_protocol::RofiBurst {
        commands: vec![
            crate::core::rofi_protocol::RofiCommand::PreviewImage("/tmp/clip.png".to_string()),
            // The preview-only marker: without it a zero-row burst still
            // replaces the list (legacy "clear" behavior).
            crate::core::rofi_protocol::RofiCommand::KeepRows,
        ],
        rows: Vec::new(),
    };
    l.rofi_apply_burst(burst, "Clipboard", None);

    let LauncherState::RofiMode {
        rows,
        filtered_rows,
        selected,
        query,
        preview_image,
        ..
    } = &l.state
    else {
        panic!("expected RofiMode");
    };
    assert_eq!(rows.len(), 5, "rows must be preserved");
    assert_eq!(filtered_rows.len(), 5);
    assert_eq!(*selected, 3, "selection must be preserved");
    assert_eq!(query, "abc", "query must be preserved");
    assert_eq!(preview_image.as_deref(), Some("/tmp/clip.png"));
}

#[test]
fn rofi_zero_row_burst_without_keep_rows_clears() {
    // Legacy contract: a zero-row burst *without* \0keep-rows replaces
    // (clears) the list — scripts that clear via an empty flush keep
    // working.
    let mut l = Launcher::new(
        Vec::new(),
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.state = LauncherState::RofiMode {
        title: "Clipboard".to_string(),
        rows: std::sync::Arc::new(
            (0..5)
                .map(|i| crate::core::rofi_protocol::RofiRow::new(format!("item {i}")))
                .collect(),
        ),
        filtered_rows: (0..5).collect(),
        prompt: None,
        message: None,
        no_custom: false,
        selected: 3,
        columns: None,
        query: String::new(),
        loading: false,
        live_search: false,
        active_indices: Vec::new(),
        data: None,
        preview_blocks: None,
        preview_styled: None,
        preview_image: None,
        multi_select: false,
        toggled_indices: std::collections::HashSet::new(),
        markup_rows: false,
        layout: None,
    };
    let burst = crate::core::rofi_protocol::RofiBurst {
        commands: vec![crate::core::rofi_protocol::RofiCommand::SetPrompt(
            "Empty".to_string(),
        )],
        rows: Vec::new(),
    };
    l.rofi_apply_burst(burst, "Clipboard", None);
    if let LauncherState::RofiMode { rows, selected, .. } = &l.state {
        assert!(rows.is_empty());
        assert_eq!(*selected, 0);
    } else {
        panic!("expected RofiMode");
    }
}

#[test]
fn rofi_selection_event_indexes_emitted_rows_not_filtered_position() {
    // With an active search filter, `selected` is a position in
    // `filtered_rows`. The selection event must carry the row's index in
    // the *emitted* row list (the coordinate select/action events use),
    // not the visible position — scripts index their last emitted frame.
    let mut l = Launcher::new(
        Vec::new(),
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    let rows: Vec<_> = (0..5)
        .map(|i| {
            let mut row = crate::core::rofi_protocol::RofiRow::new(format!("item {i}"));
            row.id = Some(format!("id-{i}"));
            row
        })
        .collect();
    l.state = LauncherState::RofiMode {
        title: "Clipboard".to_string(),
        rows: std::sync::Arc::new(rows),
        // A query filtered the list down to emitted rows 2 and 3.
        filtered_rows: vec![2, 3],
        prompt: None,
        message: None,
        no_custom: false,
        selected: 1,
        columns: None,
        query: "it".to_string(),
        loading: false,
        live_search: false,
        active_indices: Vec::new(),
        data: None,
        preview_blocks: None,
        preview_styled: None,
        preview_image: None,
        multi_select: false,
        toggled_indices: std::collections::HashSet::new(),
        markup_rows: false,
        layout: None,
    };
    match l.rofi_selection_event() {
        Some(crate::core::rofi_protocol::RofiEvent::Selection { index, id }) => {
            assert_eq!(
                index, 3,
                "index must be the row's position in the emitted frame"
            );
            assert_eq!(id, "id-3");
        }
        other => panic!("expected Selection event, got {other:?}"),
    }

    // Outside Rofi mode there is nothing to notify about.
    l.state = LauncherState::Search;
    assert!(l.rofi_selection_event().is_none());
}

#[test]
fn rofi_preview_escaped_codeblock_roundtrip() {
    // The clipboard script's preview shape: a fenced code block whose
    // newlines ride escaped on the single protocol line. The launcher must
    // unescape it into a real CodeBlock — a raw multi-line emission would
    // be split into stray rows and leave the pane empty.
    let mut l = Launcher::new(
        Vec::new(),
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.state = LauncherState::RofiMode {
        title: "Clipboard".to_string(),
        rows: std::sync::Arc::new(vec![crate::core::rofi_protocol::RofiRow::new("item 0")]),
        filtered_rows: vec![0],
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
        preview_styled: None,
        preview_image: None,
        multi_select: false,
        toggled_indices: std::collections::HashSet::new(),
        markup_rows: false,
        layout: None,
    };
    let lines = vec![
        "\0preview\x1f```text\\nhello\\nworld\\n```".to_string(),
        "\0keep-rows".to_string(),
        "\0flush".to_string(),
    ];
    let burst = crate::core::rofi_protocol::RofiBurst::from_lines(&lines);
    l.rofi_apply_burst(burst, "Clipboard", None);

    let LauncherState::RofiMode {
        preview_blocks,
        preview_image,
        ..
    } = &l.state
    else {
        panic!("expected RofiMode");
    };
    assert!(preview_image.is_none());
    let blocks = preview_blocks.as_ref().expect("preview must be set");
    match blocks.as_slice() {
        [crate::core::markdown::MdBlock::CodeBlock { lang, text }] => {
            assert_eq!(lang.as_deref(), Some("text"));
            assert_eq!(text, "hello\nworld");
        }
        other => panic!("expected one code block, got {other:?}"),
    }
}
