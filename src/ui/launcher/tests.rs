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
    l.filtered.iter().map(|t| t.name().to_string()).collect()
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
    let target = l.filtered[1].clone();
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
    l.handle_keystroke(&key("z"));
    assert_eq!(l.effective_columns(), 3);

    // Executing a script without metatags replaces the override with
    // the theme defaults (the last executed script decides the layout).
    l.handle_keystroke(&key("backspace")); // clear "z", full list back
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
    assert_eq!(l.filtered[1].inline_output(), Some("subtitle-updated"));
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
    l.handle_keystroke(&key("g"));
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
    l.handle_keystroke(&key("g"));
    assert_eq!(names(&l).len(), 2);
    l.handle_keystroke(&key("g")); // "gg" matches neither
    assert!(names(&l).is_empty());
    l.handle_keystroke(&key("backspace")); // back to "g"
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
    l.handle_keystroke(&key("down"));
    assert_eq!(l.selected, 1);
    l.handle_keystroke(&key("down"));
    assert_eq!(l.selected, 2);
    l.handle_keystroke(&key("down")); // clamps at the last row
    assert_eq!(l.selected, 2);
    l.handle_keystroke(&key("up"));
    assert_eq!(l.selected, 1);
}

#[test]
fn list_shows_all_items() {
    let mut l = Launcher::new(
        vec![item("A One"), item("A Two"), item("A Three")],
        ThemeConfig::default(),
        cap_config(2),
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
    l.handle_keystroke(&key("a"));
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
    l.handle_keystroke(&key("g"));
    assert_eq!(l.query, "g");
    assert_eq!(l.handle_keystroke(&key("escape")), LauncherAction::Hide);
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

#[test]
fn arrow_key_defers_scroll_to_selected_item() {
    let mut l = Launcher::new(
        vec![item("Git Status"), item("Grep"), item("Copy")],
        ThemeConfig::default(),
        AppConfig::default(),
        History::test_new(PathBuf::new(), Vec::new()),
    );
    l.handle_keystroke(&key("down"));
    assert_eq!(deferred(&l), Some((1, ScrollStrategy::Nearest)));
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
        .global_shortcuts
        .insert("opt+g".to_string(), "Marker".to_string());
    app_config
        .shortcuts
        .insert("cmd+r".to_string(), "Reload Configuration".to_string());
    app_config
        .global_shortcuts
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
    l.handle_keystroke(&key("g"));
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
    let action = l.handle_keystroke(&key("enter"));
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
    l.handle_keystroke(&key("enter")); // enter the prompt
    l.handle_keystroke(&key("h"));
    l.handle_keystroke(&key("i"));
    assert_eq!(
        arg_state(&l),
        Some((vec!["hi".to_string(), String::new()], 0))
    );
    l.handle_keystroke(&key("enter")); // advance to the next argument
    assert_eq!(
        arg_state(&l),
        Some((vec!["hi".to_string(), String::new()], 1))
    );
    l.handle_keystroke(&key("tab")); // wraps back to the first
    assert_eq!(
        arg_state(&l),
        Some((vec!["hi".to_string(), String::new()], 0))
    );
    l.handle_keystroke(&key("tab")); // back to the second
    l.handle_keystroke(&key("t"));
    l.handle_keystroke(&key("a"));
    l.handle_keystroke(&key("b"));
    l.handle_keystroke(&key("backspace")); // "ta"
    let action = l.handle_keystroke(&key("enter")); // last arg -> run
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
    l.handle_keystroke(&key("enter"));
    l.handle_keystroke(&key("h"));
    let action = l.handle_keystroke(&key("escape"));
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
    l.handle_keystroke(&key("enter"));
    l.handle_keystroke(&key("x"));
    l.handle_keystroke(&key("enter")); // last arg -> confirmation
    assert!(matches!(l.state, LauncherState::Confirming { .. }));
    l.handle_keystroke(&key("escape")); // decline
    assert_eq!(l.state, LauncherState::Search);
    l.handle_keystroke(&key("enter")); // prompt again
    l.handle_keystroke(&key("y"));
    l.handle_keystroke(&key("enter"));
    let action = l.handle_keystroke(&key("enter")); // confirm
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
    l.handle_keystroke(&key("a"));
    assert_eq!(l.query, "");
    l.handle_keystroke(&key("down"));
    assert_eq!(l.selected, 0);
    let action = l.handle_keystroke(&key("escape"));
    assert_eq!(action, LauncherAction::None);
    assert_eq!(l.state, LauncherState::Search);
}
