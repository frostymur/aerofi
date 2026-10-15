//! Widget registry: resolves widget IDs to their definitions and validates
//! the widget tree (no circular box references, no unknown IDs).

use std::collections::{HashMap, HashSet};

use crate::core::theme::WidgetDef;

/// A registry of custom widget definitions, indexed by their unique `id`.
///
/// Built from the `[[widgets]]` array in a theme file. Provides O(1)
/// lookup by id and validates the tree on construction so the renderer
/// never encounters cycles or dangling references.
#[derive(Debug, Clone, Default)]
pub struct WidgetRegistry {
    defs: HashMap<String, WidgetDef>,
}

impl WidgetRegistry {
    /// Build a registry from the theme's widget definitions.
    ///
    /// Duplicate ids are logged and the last definition wins. After
    /// indexing, the tree is validated: unknown child references and
    /// circular box nesting are reported to stderr (but don't panic —
    /// the renderer will simply skip the bad widget).
    pub fn from_theme(widgets: &[WidgetDef]) -> Self {
        let mut defs = HashMap::with_capacity(widgets.len());
        for w in widgets {
            let id = w.id().to_string();
            if defs.contains_key(&id) {
                eprintln!("aerofi: warning: duplicate widget id '{id}', last definition wins");
            }
            defs.insert(id, w.clone());
        }
        let registry = Self { defs };
        if let Err(errors) = registry.validate() {
            for err in errors {
                eprintln!("aerofi: warning: {err}");
            }
        }
        registry
    }

    /// Look up a widget definition by `id`. Returns `None` for unknown ids.
    pub fn get(&self, id: &str) -> Option<&WidgetDef> {
        self.defs.get(id)
    }

    /// Collect all button hotkey bindings: maps hotkey combo string to
    /// the button's action string. Only buttons with both `hotkey` and
    /// `action` set are included.
    pub fn button_hotkeys(&self) -> HashMap<String, (String, bool)> {
        self.defs
            .values()
            .filter_map(|def| {
                if let WidgetDef::Button {
                    hotkey: Some(hotkey),
                    action: Some(action),
                    close,
                    ..
                } = def
                {
                    Some((
                        hotkey.clone(),
                        (action.clone(), matches!(close, Some(false))),
                    ))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Validate the registry: check for unknown child references in Box
    /// widgets and detect circular nesting. Built-in widgets (InputBar, ListView, etc.)
    /// are recognized and allowed inside Box containers.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        for def in self.defs.values() {
            // Warn at startup about state predicates written outside the
            // grammar — otherwise a typo silently never highlights or
            // hides.
            let mut check = |raw: &str, field: &str| {
                if crate::core::widget_state::parse_condition(raw).is_none() {
                    errors.push(format!(
                        "widget '{}': invalid {field} predicate {raw:?} \
(expected `mode == <name>`, `query == empty|nonempty`, \
`list == empty|nonempty`)",
                        def.id()
                    ));
                }
            };
            let visible = match def {
                WidgetDef::Text { visible_when, .. }
                | WidgetDef::Icon { visible_when, .. }
                | WidgetDef::Image { visible_when, .. }
                | WidgetDef::Divider { visible_when, .. }
                | WidgetDef::Box { visible_when, .. } => visible_when.as_deref(),
                _ => None,
            };
            if let Some(raw) = visible {
                check(raw, "visible_when");
            }
            if let WidgetDef::Button {
                selected_when: Some(raw),
                ..
            } = def
            {
                check(raw, "selected_when");
            }

            if let WidgetDef::Box { id, children, .. } = def {
                for child_id in children {
                    if !is_builtin(child_id) && !self.defs.contains_key(child_id) {
                        errors.push(format!("widget '{id}': child '{child_id}' is not defined"));
                    }
                }
            }
        }

        // Cycle detection: DFS from every Box node.
        for def in self.defs.values() {
            if let WidgetDef::Box { id, .. } = def {
                let mut visited = HashSet::new();
                if self.has_cycle(id, &mut visited) {
                    errors.push(format!(
                        "widget '{id}': circular reference detected in Box children"
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// DFS cycle detection: returns `true` if following Box children from
    /// `id` leads back to an already-visited node.
    fn has_cycle(&self, id: &str, visited: &mut HashSet<String>) -> bool {
        if is_builtin(id) {
            return false;
        }
        if !visited.insert(id.to_string()) {
            return true;
        }
        if let Some(WidgetDef::Box { children, .. }) = self.defs.get(id) {
            for child_id in children {
                if self.has_cycle(child_id, visited) {
                    return true;
                }
            }
        }
        visited.remove(id);
        false
    }
}

/// Check if an id refers to a built-in launcher widget (e.g. InputBar, ListView).
pub fn is_builtin(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "inputbar" | "listview" | "banner" | "prompt" | "entry" | "sidebarimage" | "contentbox"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::theme::WidgetDef;

    fn text_widget(id: &str) -> WidgetDef {
        WidgetDef::Text {
            id: id.to_string(),
            text: Some("hello".to_string()),
            color: None,
            font_size: None,
            font_weight: None,
            align: None,
            margin: None,
            visible_when: None,
        }
    }

    fn spacer_widget(id: &str) -> WidgetDef {
        WidgetDef::Spacer { id: id.to_string() }
    }

    fn box_widget(id: &str, children: Vec<&str>) -> WidgetDef {
        WidgetDef::Box {
            id: id.to_string(),
            orientation: None,
            gap: None,
            padding: None,
            margin: None,
            align: None,
            background: None,
            radius: None,
            border_color: None,
            border_width: None,
            border_top: None,
            border_right: None,
            border_bottom: None,
            border_left: None,
            shadow: None,
            width: None,
            height: None,
            flex: None,
            children: children.into_iter().map(|s| s.to_string()).collect(),
            visible_when: None,
        }
    }

    #[test]
    fn lookup_by_id() {
        let reg = WidgetRegistry::from_theme(&[text_widget("greeting"), spacer_widget("flex")]);
        assert!(reg.get("greeting").is_some());
        assert!(reg.get("flex").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn validates_unknown_child() {
        let reg = WidgetRegistry {
            defs: HashMap::from([(
                "header".to_string(),
                box_widget("header", vec!["missing_child"]),
            )]),
        };
        let errs = reg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("missing_child")));
    }

    #[test]
    fn detects_direct_cycle() {
        let reg = WidgetRegistry {
            defs: HashMap::from([
                ("a".to_string(), box_widget("a", vec!["b"])),
                ("b".to_string(), box_widget("b", vec!["a"])),
            ]),
        };
        let errs = reg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("circular")));
    }

    #[test]
    fn detects_self_reference() {
        let reg = WidgetRegistry {
            defs: HashMap::from([("loop".to_string(), box_widget("loop", vec!["loop"]))]),
        };
        let errs = reg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("circular")));
    }

    fn button_widget(id: &str, action: &str) -> WidgetDef {
        WidgetDef::Button {
            id: id.to_string(),
            close: None,
            text: Some("btn".to_string()),
            icon: None,
            action: Some(action.to_string()),
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
        }
    }

    #[test]
    fn valid_tree_with_buttons_passes() {
        let reg = WidgetRegistry::from_theme(&[
            text_widget("greeting"),
            spacer_widget("flex"),
            button_widget("btn_reload", "reload"),
            box_widget("header", vec!["greeting", "flex", "btn_reload"]),
        ]);
        assert!(reg.validate().is_ok());
        assert!(reg.get("btn_reload").is_some());
    }

    #[test]
    fn valid_tree_passes() {
        let reg = WidgetRegistry::from_theme(&[
            text_widget("greeting"),
            spacer_widget("flex"),
            box_widget("header", vec!["greeting", "flex"]),
        ]);
        assert!(reg.validate().is_ok());
    }

    #[test]
    fn box_with_builtin_children_passes_validation() {
        let reg =
            WidgetRegistry::from_theme(&[box_widget("main_pane", vec!["InputBar", "ListView"])]);
        assert!(reg.validate().is_ok());
    }

    #[test]
    fn invalid_state_predicates_fail_validation() {
        // A visible_when typo (no ==) and an out-of-grammar selected_when
        // value must surface at theme load, not hide silently.
        let mut bad = text_widget("hint");
        if let WidgetDef::Text { visible_when, .. } = &mut bad {
            *visible_when = Some("query empty".to_string());
        }
        let mut btn = button_widget("m1", "mode:apps");
        if let WidgetDef::Button { selected_when, .. } = &mut btn {
            *selected_when = Some("mode = apps".to_string());
        }
        let reg = WidgetRegistry::from_theme(&[bad, btn]);
        let errs = reg.validate().err().unwrap();
        // Order is map-iteration order; check the set of findings.
        assert_eq!(errs.len(), 2, "got: {errs:?}");
        let all = errs.join("\n");
        assert!(all.contains("visible_when") && all.contains("hint"));
        assert!(all.contains("selected_when") && all.contains("m1"));

        // Valid predicates pass.
        let mut ok1 = text_widget("hint");
        if let WidgetDef::Text { visible_when, .. } = &mut ok1 {
            *visible_when = Some("query == empty".to_string());
        }
        let mut ok2 = button_widget("m1", "mode:apps");
        if let WidgetDef::Button { selected_when, .. } = &mut ok2 {
            *selected_when = Some("mode == apps".to_string());
        }
        let reg = WidgetRegistry::from_theme(&[ok1, ok2]);
        assert!(reg.validate().is_ok());
    }
}
