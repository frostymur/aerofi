use gpui::Context;

use super::helpers::combo_matches;
use super::state::Launcher;
use super::types::{LauncherAction, LauncherState};

impl Launcher {
    /// Handle a keystroke. Returns the action that the host (main.rs) should perform.
    pub fn handle_keystroke(
        &mut self,
        ks: &gpui::Keystroke,
        cx: Option<&mut Context<Self>>,
    ) -> LauncherAction {
        // Full-output pages (spinner and result view) swallow every
        // keystroke; only Escape returns to the search list. Argument
        // prompts and confirmations fall through so their own handlers run.
        if matches!(
            &self.state,
            LauncherState::RunningFull { .. } | LauncherState::FullOutput { .. }
        ) {
            if ks.key == "escape" {
                self.back_from_full_output();
            }
            return LauncherAction::None;
        }

        // Rofi-mode interactive script: handle keystrokes within the
        // script-driven list.
        if matches!(&self.state, LauncherState::RofiMode { .. }) {
            return self.handle_rofi_mode_keystroke(ks, cx);
        }

        // Action menu (Cmd+K): owns the keyboard. Navigation, Enter,
        // Escape — everything else is ignored (the list underneath is
        // untouched, so Escape restores it exactly).
        if matches!(&self.state, LauncherState::ActionMenu { .. }) {
            let plain = !ks.modifiers.platform
                && !ks.modifiers.control
                && !ks.modifiers.alt
                && !ks.modifiers.shift;
            match (ks.key.as_str(), plain) {
                ("up" | "k", true) => {
                    if let LauncherState::ActionMenu {
                        actions, selected, ..
                    } = &mut self.state
                    {
                        let last = actions.len().saturating_sub(1);
                        *selected = (*selected).wrapping_sub(1).min(last);
                    }
                }
                ("down" | "j", true) => {
                    if let LauncherState::ActionMenu {
                        actions, selected, ..
                    } = &mut self.state
                    {
                        let last = actions.len().saturating_sub(1);
                        *selected = (*selected + 1).min(last);
                    }
                }
                ("enter" | "return", _) => {
                    let (target, spec) = match &self.state {
                        LauncherState::ActionMenu {
                            target,
                            actions,
                            selected,
                        } => (target.clone(), actions.get(*selected).cloned()),
                        _ => unreachable!(),
                    };
                    self.state = LauncherState::Search;
                    return self.execute_target_action(&target, spec);
                }
                ("escape", _) => {
                    self.state = LauncherState::Search;
                }
                _ => {}
            }
            return LauncherAction::None;
        }

        // A configured key-combo shortcut (e.g. "cmd+r") runs its target
        // immediately; explicit config overrides the built-in bindings.
        // Only honoured in plain search mode — while an argument prompt or
        // confirmation is up, every keystroke belongs to that prompt.
        if matches!(self.state, LauncherState::Search)
            && let Some((_, name)) = self
                .app_config
                .bindings
                .launcher
                .iter()
                .find(|(combo, _)| combo_matches(combo, ks))
            && let Some(item) = self.all.iter().find(|t| t.name() == name)
        {
            let item = item.clone();
            let action = self.execute_item(&item, false);
            if matches!(
                action,
                LauncherAction::Hide | LauncherAction::ExecuteScript(..)
            ) {
                self.reset();
            }
            return action;
        }

        // Button widget hotkeys (e.g. "cmd+r" mapped to a sidebar button's
        // action). Only honoured in plain search mode.
        if matches!(self.state, LauncherState::Search)
            && let Some((_, (action, stay_open))) = self
                .button_hotkeys
                .iter()
                .find(|(combo, _)| combo_matches(combo, ks))
        {
            let (action, stay_open) = (action.clone(), *stay_open);
            if let Some(cx) = cx {
                self.handle_widget_button_action(&action, None, stay_open, cx);
            }
            return LauncherAction::None;
        }

        // Reload configuration (customizable via [general] reload_hotkey or built-in Cmd+R / Ctrl+R)
        if self.is_reload_keystroke(ks) {
            self.reload();
            if let Some(cx) = cx {
                cx.notify();
            }
            return LauncherAction::None;
        }

        let cmd = ks.modifiers.platform;
        let ctrl = ks.modifiers.control;
        let alt = ks.modifiers.alt;

        // Common text editing and navigation hotkeys
        if matches!(self.state, LauncherState::Search) {
            match (ks.key.as_str(), cmd, ctrl, alt) {
                ("u", false, true, false) | ("backspace", true, false, false) => {
                    self.reset();
                    return LauncherAction::None;
                }
                ("w", false, true, false) | ("backspace", false, false, true) => {
                    let mut len = self.query.trim_end().rfind(' ').map(|i| i + 1).unwrap_or(0);
                    // Never cut into the plugin session anchor prefix.
                    if let Some(p) = &self.active_plugin {
                        len = len.max(p.prefix.len());
                    }
                    self.query.truncate(len);
                    self.refilter(cx);
                    self.selected = 0;
                    return LauncherAction::None;
                }
                ("n", false, true, false) | ("j", false, true, false) => {
                    let cols = self.effective_columns();
                    let step = if cols > 1 { cols as isize } else { 1 };
                    self.move_selection(step);
                    return LauncherAction::None;
                }
                ("p", false, true, false) | ("k", false, true, false) => {
                    let cols = self.effective_columns();
                    let step = if cols > 1 { cols as isize } else { 1 };
                    self.move_selection(-step);
                    return LauncherAction::None;
                }
                // Cmd+. cycles the active source filter ([modes]):
                // All -> each [modes] entry -> each plugin -> All.
                // Cmd+K opens the action menu for the highlighted row
                // (apps/scripts get built-in actions; see core::item).
                ("k", true, false, false) => {
                    let Some(item) = self.selected_item() else {
                        return LauncherAction::None;
                    };
                    let actions = item.actions(&self.app_config);
                    if !actions.is_empty() {
                        self.state = LauncherState::ActionMenu {
                            target: item.clone(),
                            actions,
                            selected: 0,
                        };
                    }
                    return LauncherAction::None;
                }
                (".", true, false, false) => {
                    if let Some(cx) = cx {
                        // Reborrow so `cx` stays usable for notify.
                        self.cycle_mode(Some(&mut *cx));
                        cx.notify();
                    } else {
                        self.cycle_mode(None);
                    }
                    return LauncherAction::None;
                }
                _ => {}
            }
        }

        match (ks.key.as_str(), cmd) {
            ("escape", _) => {
                if !matches!(self.state, LauncherState::Search) {
                    self.state = LauncherState::Search;
                    return LauncherAction::None;
                }
                self.reset();
                LauncherAction::Hide
            }
            ("enter" | "return", false)
                if matches!(&self.state, LauncherState::Confirming { .. }) =>
            {
                self.confirm_and_run()
            }
            ("enter" | "return", false)
                if matches!(&self.state, LauncherState::ArgumentInput { .. }) =>
            {
                if let LauncherState::ArgumentInput {
                    target,
                    args,
                    values,
                    focused_index,
                } = &mut self.state
                {
                    // Validation: check if all required fields are filled.
                    let mut first_invalid = None;
                    let mut all_valid = true;
                    for (i, (arg, val)) in args.iter().zip(values.iter()).enumerate() {
                        let is_optional = arg.optional.unwrap_or(false);
                        if !is_optional && val.trim().is_empty() {
                            all_valid = false;
                            if first_invalid.is_none() {
                                first_invalid = Some(i);
                            }
                        }
                    }

                    if *focused_index < args.len() - 1 {
                        *focused_index += 1;
                        Self::prefill_dropdown(values, args, *focused_index);
                        return LauncherAction::None;
                    }

                    if !all_valid {
                        // Jump back to the first unfilled required field.
                        *focused_index = first_invalid.unwrap_or(*focused_index);
                        return LauncherAction::None;
                    }

                    let t = target.clone();
                    let vals = values.clone();
                    if t.needs_confirmation() {
                        self.state = LauncherState::Confirming {
                            target: t,
                            args_values: vals,
                        };
                        return LauncherAction::None;
                    } else {
                        self.state = LauncherState::Search;
                        let action = self.execute_target_with_args(&t, vals);
                        // Full output keeps the window open, so preserve the
                        // query and selection for when the user comes back.
                        if matches!(
                            action,
                            LauncherAction::Hide | LauncherAction::ExecuteScript(..)
                        ) {
                            self.reset();
                        }
                        return action;
                    }
                }
                LauncherAction::None
            }
            ("tab", false) if matches!(&self.state, LauncherState::ArgumentInput { .. }) => {
                let shift = ks.modifiers.shift;
                if let LauncherState::ArgumentInput {
                    args,
                    values,
                    focused_index,
                    ..
                } = &mut self.state
                {
                    if shift {
                        *focused_index = (*focused_index + args.len() - 1) % args.len();
                    } else {
                        *focused_index = (*focused_index + 1) % args.len();
                    }
                    Self::prefill_dropdown(values, args, *focused_index);
                }
                LauncherAction::None
            }
            ("up" | "down", false)
                if matches!(&self.state, LauncherState::ArgumentInput { .. }) =>
            {
                if let LauncherState::ArgumentInput {
                    args,
                    values,
                    focused_index,
                    ..
                } = &mut self.state
                {
                    let arg = &args[*focused_index];
                    if arg.arg_type.as_deref() == Some("dropdown")
                        && let Some(data) = arg.data.as_deref().filter(|d| !d.is_empty())
                    {
                        let current_val = &values[*focused_index];
                        let current_idx = data
                            .iter()
                            .position(|o| o.value == *current_val)
                            .unwrap_or(0);
                        let new_idx = if ks.key == "up" {
                            if current_idx == 0 {
                                data.len() - 1
                            } else {
                                current_idx - 1
                            }
                        } else {
                            if current_idx + 1 >= data.len() {
                                0
                            } else {
                                current_idx + 1
                            }
                        };
                        values[*focused_index] = data[new_idx].value.clone();
                    }
                }
                LauncherAction::None
            }
            ("backspace", false) if matches!(&self.state, LauncherState::ArgumentInput { .. }) => {
                if let LauncherState::ArgumentInput {
                    args,
                    values,
                    focused_index,
                    ..
                } = &mut self.state
                {
                    let is_dropdown = args[*focused_index].arg_type.as_deref() == Some("dropdown");
                    if is_dropdown {
                        values[*focused_index].clear();
                    } else if values[*focused_index].pop().is_none() && *focused_index > 0 {
                        *focused_index -= 1;
                    }
                }
                LauncherAction::None
            }
            _ if matches!(&self.state, LauncherState::ArgumentInput { .. }) => {
                if !cmd
                    && !ks.modifiers.control
                    && !ks.modifiers.alt
                    && ks.key != "tab"
                    && let Some(c) = ks.key_char.as_deref()
                    && !c.is_empty()
                    && !c.chars().any(char::is_control)
                    && let LauncherState::ArgumentInput {
                        args,
                        values,
                        focused_index,
                        ..
                    } = &mut self.state
                {
                    let is_dropdown = args[*focused_index].arg_type.as_deref() == Some("dropdown");
                    if !is_dropdown {
                        values[*focused_index].push_str(c);
                    }
                }
                LauncherAction::None
            }
            _ if !matches!(self.state, LauncherState::Search) => LauncherAction::None,
            ("up", false) => {
                let cols = self.effective_columns();
                let step = if cols > 1 { cols as isize } else { 1 };
                self.move_selection(-step);
                LauncherAction::None
            }
            ("down", false) => {
                let cols = self.effective_columns();
                let step = if cols > 1 { cols as isize } else { 1 };
                self.move_selection(step);
                LauncherAction::None
            }
            ("up", true) => {
                if self.filtered.is_empty() {
                    return LauncherAction::None;
                }
                self.selected = 0;
                LauncherAction::None
            }
            ("down", true) => {
                if self.filtered.is_empty() {
                    return LauncherAction::None;
                }
                self.selected = self.filtered.len() - 1;
                LauncherAction::None
            }
            ("left", false) if self.effective_columns() > 1 => {
                self.move_selection(-1);
                LauncherAction::None
            }
            ("right", false) if self.effective_columns() > 1 => {
                self.move_selection(1);
                LauncherAction::None
            }
            ("enter" | "return", false) => {
                // Plain Enter activates a running app; Shift+Enter smart-launches
                // a new window (AppleScript, `open -n` fallback).
                let action = self.execute_selected(ks.modifiers.shift);
                // Full output keeps the window open, so preserve the query
                // and selection for when the user comes back.
                if matches!(
                    action,
                    LauncherAction::Hide | LauncherAction::ExecuteScript(..)
                ) {
                    self.reset();
                }
                action
            }
            ("backspace", false) => {
                self.backspace(cx);
                LauncherAction::None
            }
            _ => {
                // Treat plain printable characters (no cmd/ctrl/alt) as filter input.
                if !cmd
                    && !ks.modifiers.control
                    && !ks.modifiers.alt
                    && ks.key != "tab"
                    && let Some(c) = ks.key_char.as_deref()
                    && !c.is_empty()
                    && !c.chars().any(char::is_control)
                {
                    self.query.push_str(c);
                    self.refilter(cx);
                    self.selected = 0;
                    // A configured alias: typing it exactly runs its target
                    // immediately (no Enter needed).
                    if let Some(item) = self.alias_target() {
                        let item = item.clone();
                        let action = self.execute_item(&item, false);
                        if matches!(
                            action,
                            LauncherAction::Hide | LauncherAction::ExecuteScript(..)
                        ) {
                            self.reset();
                        }
                        return action;
                    }
                }
                LauncherAction::None
            }
        }
    }

    /// Check if the keystroke triggers configuration reload.
    pub(super) fn is_reload_keystroke(&self, ks: &gpui::Keystroke) -> bool {
        let is_bound = self
            .app_config
            .bindings
            .launcher
            .iter()
            .any(|(combo, name)| name == "Reload Configuration" && combo_matches(combo, ks));

        if is_bound {
            return true;
        }

        let cmd = ks.modifiers.platform;
        let ctrl = ks.modifiers.control;
        (cmd || ctrl) && (ks.key.eq_ignore_ascii_case("r") || ks.key == "к" || ks.key == "К")
    }
}
