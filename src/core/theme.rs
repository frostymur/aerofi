//! Theme configuration and loader.
//!
//! Themes define the visual appearance of every launcher widget. A theme
//! is a TOML file at `~/.config/aerofi/themes/{name}.toml`; the special
//! name `"default"` returns the built-in Tokyo Night palette without
//! reading any file.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Widget enum
// ---------------------------------------------------------------------------

/// Identifies a launcher widget that can appear in a container's children
/// list. Built-in names are case-sensitive; any other string is treated as
/// a reference to a custom widget defined in `[[widgets]]`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Widget {
    #[serde(deserialize_with = "deserialize_builtin_widget")]
    Builtin(BuiltinWidget),
    Custom(String),
}

/// The fixed set of built-in widgets that the launcher knows how to render
/// natively.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub enum BuiltinWidget {
    InputBar,
    ListView,
    Prompt,
    Entry,
    Banner,
    SidebarImage,
    ContentBox,
}

/// Deserialise a `BuiltinWidget` from its case-sensitive name.
fn deserialize_builtin_widget<'de, D>(deserializer: D) -> Result<BuiltinWidget, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.as_str() {
        "InputBar" => Ok(BuiltinWidget::InputBar),
        "ListView" => Ok(BuiltinWidget::ListView),
        "Prompt" => Ok(BuiltinWidget::Prompt),
        "Entry" => Ok(BuiltinWidget::Entry),
        "Banner" => Ok(BuiltinWidget::Banner),
        "SidebarImage" => Ok(BuiltinWidget::SidebarImage),
        "ContentBox" => Ok(BuiltinWidget::ContentBox),
        _ => Err(serde::de::Error::custom(format!(
            "unknown builtin widget: {s}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Custom widget definitions
// ---------------------------------------------------------------------------

/// A single custom widget definition from the `[[widgets]]` array in a
/// theme file. Each variant maps to a `type` value in the TOML.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WidgetDef {
    Text {
        #[serde(default)]
        id: String,
        text: Option<String>,
        color: Option<String>,
        font_size: Option<f32>,
        font_weight: Option<String>,
        align: Option<String>,
    },
    Icon {
        #[serde(default)]
        id: String,
        icon: String,
        size: Option<f32>,
        color: Option<String>,
    },
    Image {
        #[serde(default)]
        id: String,
        path: String,
        width: Option<f32>,
        height: Option<f32>,
        radius: Option<f32>,
    },
    Spacer {
        #[serde(default)]
        id: String,
    },
    Divider {
        #[serde(default)]
        id: String,
        color: Option<String>,
        thickness: Option<f32>,
        margin: Option<f32>,
    },
    Box {
        #[serde(default)]
        id: String,
        orientation: Option<String>,
        gap: Option<f32>,
        padding: Option<Vec<f32>>,
        align: Option<String>,
        background: Option<String>,
        radius: Option<f32>,
        width: Option<f32>,
        height: Option<f32>,
        flex: Option<bool>,
        #[serde(default)]
        children: Vec<String>,
    },
    Button {
        #[serde(default)]
        id: String,
        text: Option<String>,
        icon: Option<String>,
        action: Option<String>,
        hotkey: Option<String>,
        color: Option<String>,
        background: Option<String>,
        hover_background: Option<String>,
        hover_color: Option<String>,
        border_color: Option<String>,
        border_width: Option<f32>,
        radius: Option<f32>,
        padding: Option<Vec<f32>>,
        font_size: Option<f32>,
        font_weight: Option<String>,
        gap: Option<f32>,
    },
}

impl WidgetDef {
    /// The unique id of this widget definition.
    pub fn id(&self) -> &str {
        match self {
            Self::Text { id, .. }
            | Self::Icon { id, .. }
            | Self::Image { id, .. }
            | Self::Spacer { id }
            | Self::Divider { id, .. }
            | Self::Box { id, .. }
            | Self::Button { id, .. } => id,
        }
    }

    /// Override the id of this widget (used when parsing `[widgets.<id>]` syntax).
    pub fn set_id(&mut self, new_id: String) {
        match self {
            Self::Text { id, .. }
            | Self::Icon { id, .. }
            | Self::Image { id, .. }
            | Self::Spacer { id }
            | Self::Divider { id, .. }
            | Self::Box { id, .. }
            | Self::Button { id, .. } => *id = new_id,
        }
    }

    /// Resolve `$alias` colour references within this widget's fields.
    pub fn resolve_colors(&mut self, colors: &HashMap<String, String>) {
        match self {
            Self::Text { color, .. } => resolve_opt(color, colors),
            Self::Icon { color, .. } => resolve_opt(color, colors),
            Self::Divider { color, .. } => resolve_opt(color, colors),
            Self::Box { background, .. } => resolve_opt(background, colors),
            Self::Button {
                color,
                background,
                hover_background,
                hover_color,
                border_color,
                ..
            } => {
                resolve_opt(color, colors);
                resolve_opt(background, colors);
                resolve_opt(hover_background, colors);
                resolve_opt(hover_color, colors);
                resolve_opt(border_color, colors);
            }
            Self::Image { .. } | Self::Spacer { .. } => {}
        }
    }
}

/// Helper enum to deserialize custom widgets from either:
/// 1. A table of widget definitions: `[widgets.<id>]`
/// 2. An array of widget definitions: `[[widgets]]`
#[derive(Deserialize)]
#[serde(untagged)]
enum WidgetsRepr {
    Map(std::collections::BTreeMap<String, WidgetDef>),
    List(Vec<WidgetDef>),
}

/// Deserialize widgets supporting both `[widgets.<id>]` and `[[widgets]]` formats.
pub fn deserialize_widgets<'de, D>(deserializer: D) -> Result<Vec<WidgetDef>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let repr = WidgetsRepr::deserialize(deserializer)?;
    match repr {
        WidgetsRepr::Map(map) => {
            let mut list = Vec::with_capacity(map.len());
            for (key, mut widget) in map {
                if widget.id().is_empty() {
                    widget.set_id(key);
                }
                list.push(widget);
            }
            Ok(list)
        }
        WidgetsRepr::List(list) => Ok(list),
    }
}

// ---------------------------------------------------------------------------
// Font
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub weight: Option<String>,
    pub fallback: Option<Vec<String>>,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "SF Pro Text".to_string(),
            size: 17.0,
            weight: None,
            fallback: Some(vec!["SF Mono".to_string()]),
        }
    }
}

// ---------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub width: f32,
    pub height: f32,
    pub padding: f32,
    pub background: String,
    pub background_image: Option<String>,
    /// Position of the background image: `"cover"` (full), `"left"`,
    /// `"right"`, or `"center"`.
    pub background_position: Option<String>,
    pub image_scale: Option<f32>,
    pub blur: bool,
    /// Window background opacity: `1.0` = fully opaque (default),
    /// `0.0` = fully transparent. Makes the NSWindow non-opaque so
    /// the desktop shows through.
    pub background_opacity: Option<f32>,
    pub corner_radius: f32,
    pub border_width: f32,
    pub border_color: String,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 760.0,
            height: 480.0,
            padding: 16.0,
            background: "#0d0d0d".to_string(),
            background_image: None,
            background_position: None,
            image_scale: None,
            blur: true,
            background_opacity: Some(0.50),
            corner_radius: 16.0,
            border_width: 1.0,
            border_color: "#2a2a2a".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Container
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ContainerConfig {
    /// `"vertical"` or `"horizontal"`.
    pub orientation: String,
    pub children: Vec<Widget>,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            orientation: "vertical".to_string(),
            children: vec![
                Widget::Builtin(BuiltinWidget::InputBar),
                Widget::Builtin(BuiltinWidget::ListView),
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BannerConfig {
    pub image_path: Option<String>,
    pub height: f32,
    pub align: Option<String>,
}

impl Default for BannerConfig {
    fn default() -> Self {
        Self {
            image_path: None,
            height: 120.0,
            align: Some("center".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// InputBar
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct InputBarConfig {
    pub height: f32,
    pub padding: Vec<f32>,
    pub margin: Vec<f32>,
    pub background: String,
    pub text_color: String,
    pub placeholder: String,
    pub placeholder_color: String,
    pub corner_radius: f32,
    pub icon: Option<String>,
    pub icon_color: Option<String>,
}

impl Default for InputBarConfig {
    fn default() -> Self {
        Self {
            height: 44.0,
            padding: vec![12.0, 16.0],
            margin: vec![0.0, 0.0, 8.0, 0.0],
            background: "transparent".to_string(),
            text_color: "#ffffff".to_string(),
            placeholder: "Search...".to_string(),
            placeholder_color: "#888888".to_string(),
            corner_radius: 8.0,
            icon: Some("❯".to_string()),
            icon_color: Some("#888888".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Badge config (shared by category and alias badges)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BadgeConfig {
    /// Whether to show this badge.
    pub show: bool,
    /// Text color.
    pub color: String,
    /// Font size offset (subtracted from the theme base font size).
    pub font_size_offset: f32,
    /// Corner radius.
    pub radius: f32,
    /// Horizontal padding.
    pub padding_x: f32,
    /// Whether to draw a border around the badge.
    pub border: bool,
    /// Border color (falls back to `color` if empty).
    pub border_color: Option<String>,
}

impl Default for BadgeConfig {
    fn default() -> Self {
        Self {
            show: true,
            color: "#7a88b5".to_string(),
            font_size_offset: 3.0,
            radius: 4.0,
            padding_x: 4.0,
            border: false,
            border_color: None,
        }
    }
}

// ---------------------------------------------------------------------------
// ListView
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ListViewConfig {
    pub columns: usize,
    pub spacing: f32,
    pub scrollbar: bool,
    pub empty_text: String,
    pub empty_text_color: String,
    /// Styling for category badges ("Script", "Application", "Aerofi").
    pub category_badge: BadgeConfig,
    /// Styling for alias pill badges.
    pub alias_badge: BadgeConfig,
    /// When `true`, the list is hidden and the window shrinks to the
    /// inputbar until the user types a query.
    pub require_input: Option<bool>,
}

impl Default for ListViewConfig {
    fn default() -> Self {
        let alias_badge = BadgeConfig {
            border: true,
            ..Default::default()
        };
        Self {
            columns: 1,
            spacing: 4.0,
            scrollbar: false,
            empty_text: "No matches".to_string(),
            empty_text_color: "#888888".to_string(),
            category_badge: BadgeConfig {
                show: false,
                ..BadgeConfig::default()
            },
            alias_badge,
            require_input: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Element states
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SelectedState {
    pub background: String,
    pub text_color: String,
    pub description_color: Option<String>,
}

impl Default for SelectedState {
    fn default() -> Self {
        Self {
            background: "#ffffff20".to_string(),
            text_color: "#ffffff".to_string(),
            description_color: Some("#bbbbbb".to_string()),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HoverState {
    pub background: String,
    pub text_color: String,
    pub description_color: Option<String>,
}

impl Default for HoverState {
    fn default() -> Self {
        Self {
            background: "#ffffff10".to_string(),
            text_color: "#ffffff".to_string(),
            description_color: Some("#bbbbbb".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Element
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ElementConfig {
    pub padding: Vec<f32>,
    pub corner_radius: f32,
    pub background: String,
    pub text_color: String,
    pub description_color: Option<String>,
    pub show_icons: bool,
    pub icon_size: f32,
    pub layout: Option<Vec<String>>,
    pub selected: SelectedState,
    pub hover: Option<HoverState>,
}

impl Default for ElementConfig {
    fn default() -> Self {
        Self {
            padding: vec![6.0, 10.0],
            corner_radius: 8.0,
            background: "transparent".to_string(),
            text_color: "#ffffff".to_string(),
            description_color: Some("#888888".to_string()),
            show_icons: true,
            icon_size: 24.0,
            layout: Some(vec![
                "icon".to_string(),
                "name".to_string(),
                "spacer".to_string(),
            ]),
            selected: SelectedState::default(),
            hover: Some(HoverState::default()),
        }
    }
}

// ---------------------------------------------------------------------------
// Status Colors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct StatusColorsConfig {
    pub urgent_background: String,
    pub urgent_text: String,
    pub urgent_row_background: String,
    pub active_background: String,
    pub active_text: String,
    pub active_row_background: String,
    pub accent: String,
    pub muted: String,
}

impl Default for StatusColorsConfig {
    fn default() -> Self {
        Self {
            urgent_background: "#f7768e".to_string(),
            urgent_text: "#1a1b26".to_string(),
            urgent_row_background: "#ff555518".to_string(),
            active_background: "#73daca".to_string(),
            active_text: "#1a1b26".to_string(),
            active_row_background: "#50fa7b18".to_string(),
            accent: "#7aa2f7".to_string(),
            muted: "#565f89".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Toast
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ToastConfig {
    pub running_dot: String,
    pub success_dot: String,
    pub error_dot: String,
}

impl Default for ToastConfig {
    fn default() -> Self {
        Self {
            running_dot: "#aaaaaa".to_string(),
            success_dot: "#9ece6a".to_string(),
            error_dot: "#f7768e".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Root theme config
// ---------------------------------------------------------------------------

/// Top-level theme configuration loaded from a TOML file or constructed
/// from built-in defaults.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub name: String,
    pub author: Option<String>,
    pub font: FontConfig,
    pub window: WindowConfig,
    pub mainbox: ContainerConfig,
    pub banner: Option<BannerConfig>,
    pub inputbar: InputBarConfig,
    pub listview: ListViewConfig,
    pub element: ElementConfig,
    #[serde(default)]
    pub status_colors: StatusColorsConfig,
    #[serde(default)]
    pub toast: ToastConfig,
    /// Custom widget definitions. Supports both table syntax (`[widgets.<id>]`)
    /// and array-of-tables syntax (`[[widgets]]`).
    #[serde(default, deserialize_with = "deserialize_widgets")]
    pub widgets: Vec<WidgetDef>,
    /// Named colour aliases: `$key` in any colour field is replaced with
    /// the corresponding hex value from this map.
    #[serde(default)]
    pub colors: HashMap<String, String>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            name: "Dark Transparent".to_string(),
            author: Some("AeroFi".to_string()),
            font: FontConfig::default(),
            window: WindowConfig::default(),
            mainbox: ContainerConfig::default(),
            banner: None,
            inputbar: InputBarConfig::default(),
            listview: ListViewConfig::default(),
            element: ElementConfig::default(),
            status_colors: StatusColorsConfig::default(),
            toast: ToastConfig::default(),
            widgets: Vec::new(),
            colors: HashMap::new(),
        }
    }
}

impl ThemeConfig {
    /// Resolve colour aliases: every string field that starts with `$`
    /// (e.g. `"$bg"`) is replaced with the hex value from the `[colors]`
    /// map. Unknown aliases fall back to `"#000000"` with a warning.
    pub fn resolve_colors(&mut self) {
        let colors = &self.colors;

        // Window
        resolve(&mut self.window.background, colors);
        resolve_opt(&mut self.window.background_image, colors);
        resolve(&mut self.window.border_color, colors);

        // InputBar
        resolve(&mut self.inputbar.background, colors);
        resolve(&mut self.inputbar.text_color, colors);
        resolve(&mut self.inputbar.placeholder_color, colors);
        resolve_opt(&mut self.inputbar.icon_color, colors);

        // ListView
        resolve(&mut self.listview.empty_text_color, colors);
        resolve(&mut self.listview.category_badge.color, colors);
        resolve(&mut self.listview.alias_badge.color, colors);
        resolve_opt(&mut self.listview.alias_badge.border_color, colors);

        // Element
        resolve(&mut self.element.background, colors);
        resolve(&mut self.element.text_color, colors);
        resolve_opt(&mut self.element.description_color, colors);

        // Element.selected
        resolve(&mut self.element.selected.background, colors);
        resolve(&mut self.element.selected.text_color, colors);
        resolve_opt(&mut self.element.selected.description_color, colors);

        // Element.hover
        if let Some(hover) = &mut self.element.hover {
            resolve(&mut hover.background, colors);
            resolve(&mut hover.text_color, colors);
            resolve_opt(&mut hover.description_color, colors);
        }

        // Status colors
        resolve(&mut self.status_colors.urgent_background, colors);
        resolve(&mut self.status_colors.urgent_text, colors);
        resolve(&mut self.status_colors.urgent_row_background, colors);
        resolve(&mut self.status_colors.active_background, colors);
        resolve(&mut self.status_colors.active_text, colors);
        resolve(&mut self.status_colors.active_row_background, colors);
        resolve(&mut self.status_colors.accent, colors);
        resolve(&mut self.status_colors.muted, colors);

        // Toast
        resolve(&mut self.toast.running_dot, colors);
        resolve(&mut self.toast.success_dot, colors);
        resolve(&mut self.toast.error_dot, colors);

        // Custom widgets
        for w in &mut self.widgets {
            w.resolve_colors(colors);
        }
    }
}

/// If `value` starts with `$`, look up the alias in `colors` and replace
/// it. Unknown aliases are replaced with `"#000000"` + a warning.
fn resolve(value: &mut String, colors: &HashMap<String, String>) {
    if let Some(key) = value.strip_prefix('$') {
        *value = match colors.get(key) {
            Some(hex) => hex.clone(),
            None => {
                eprintln!("aerofi: warning: unknown colour alias ${key}, falling back to #000000");
                "#000000".to_string()
            }
        };
    }
}

/// Same as [`resolve`] but for `Option<String>` fields.
fn resolve_opt(value: &mut Option<String>, colors: &HashMap<String, String>) {
    if let Some(s) = value {
        resolve(s, colors);
    }
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Load a theme by name. `"default"` returns the built-in theme without
/// touching the filesystem. Any other name is resolved to
/// `~/.config/aerofi/themes/{name}.toml`; if the file is missing or
/// unparseable, a warning is printed and the default theme is returned.
pub fn load_theme(theme_name: &str) -> ThemeConfig {
    if theme_name == "default" {
        return ThemeConfig::default();
    }

    let file_name = format!("{theme_name}.toml");

    // Check ~/.config/aerofi/themes/{name}.toml first (standard per config.rs).
    let dot_config_path = dirs::home_dir().map(|h| {
        h.join(".config")
            .join("aerofi")
            .join("themes")
            .join(&file_name)
    });

    // Fallback to dirs::config_dir() (~/Library/Application Support/aerofi/themes/ on macOS).
    let app_support_path =
        dirs::config_dir().map(|c| c.join("aerofi").join("themes").join(&file_name));

    let path = match (&dot_config_path, &app_support_path) {
        (Some(p), _) if p.is_file() => p.clone(),
        (_, Some(p)) if p.is_file() => p.clone(),
        (Some(p), _) => p.clone(),
        (_, Some(p)) => p.clone(),
        _ => {
            eprintln!("aerofi: warning: cannot determine config dir, using default theme");
            return ThemeConfig::default();
        }
    };

    let contents = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(err) => {
            eprintln!(
                "aerofi: warning: failed to read theme {}: {err}; using default",
                path.display()
            );
            return ThemeConfig::default();
        }
    };

    match toml::from_str::<ThemeConfig>(&contents) {
        Ok(mut theme) => {
            theme.resolve_colors();
            theme
        }
        Err(err) => {
            eprintln!(
                "aerofi: warning: failed to parse theme {}: {err}; using default",
                path.display()
            );
            ThemeConfig::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Hex colour helpers
// ---------------------------------------------------------------------------

/// Parse a CSS-style hex colour (`"#1a1b26"`, `"7aa2f7"`, `"#fff"`) into
/// a 24-bit RGB value suitable for GPUI's `rgb()`.  Returns `None` on
/// malformed input.
pub fn parse_hex_color_alpha(hex: &str) -> Option<u32> {
    if hex == "transparent" {
        return Some(0x00000000);
    }
    let hex = hex.trim().trim_start_matches('#');
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 0x11;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 0x11;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 0x11;
            Some(((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | 0xFF)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | 0xFF)
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | (a as u32))
        }
        _ => None,
    }
}

pub fn parse_hex_color(hex: &str) -> Option<u32> {
    let hex = hex.trim().trim_start_matches('#');
    let (r, g, b) = match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 0x11;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 0x11;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 0x11;
            (r, g, b)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            (r, g, b)
        }
        _ => return None,
    };
    Some(((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_has_dark_transparent_palette() {
        let t = ThemeConfig::default();
        assert_eq!(t.name, "Dark Transparent");
        assert_eq!(t.window.background, "#0d0d0d");
        assert_eq!(t.inputbar.background, "transparent");
        assert_eq!(t.inputbar.text_color, "#ffffff");
        assert_eq!(t.element.selected.background, "#ffffff20");
    }

    #[test]
    fn example_tokyo_night_theme_is_valid() {
        let content = r##"
name    = "Tokyo Night"
author  = "AeroFi"
[font]
family = "SF Pro Text"
size = 17.0
[window]
width = 800.0
height = 500.0
padding = 16.0
background = "$bg"
blur = false
corner_radius = 12.0
border_width = 1.0
border_color = "$border"
[mainbox]
orientation = "vertical"
children = ["InputBar", "ListView"]
[inputbar]
height = 48.0
padding = [12.0, 16.0]
margin = [0.0, 0.0, 8.0, 0.0]
background = "$surface"
text_color = "$text"
placeholder = "Type to filter…"
placeholder_color = "$subtle"
corner_radius = 8.0
icon = "❯"
icon_color = "$accent"
[listview]
columns = 1
spacing = 6.0
scrollbar = false
empty_text = "No matches"
empty_text_color = "$subtle"
[element]
padding = [8.0, 12.0]
corner_radius = 8.0
background = "transparent"
text_color = "$text"
description_color = "$subtle"
show_icons = true
icon_size = 24.0
[element.selected]
background = "$surface2"
text_color = "$text"
description_color = "$subtle"
[colors]
bg = "#1a1b26"
surface = "#24283b"
surface1 = "#33374a"
surface2 = "#414868"
border = "#414868"
text = "#c0caf5"
subtle = "#565f89"
accent = "#7aa2f7"
"##;
        let t: ThemeConfig =
            toml::from_str(content).expect("tokyo-night.toml should parse cleanly");
        assert_eq!(t.name, "Tokyo Night");
    }

    #[test]
    fn example_horizon_theme_is_valid() {
        let content = r##"
name   = "Horizon"
author = "AeroFi"
[font]
family = "SF Pro Text"
size = 14.0
fallback = ["SF Mono", "Menlo"]
[window]
width = 880.0
height = 540.0
padding = 16.0
background = "$bg"
blur = true
background_opacity = 0.96
corner_radius = 16.0
border_width = 1.0
border_color = "$border"
[mainbox]
orientation = "horizontal"
children = ["sidebar", "main_pane"]
[inputbar]
height = 44.0
padding = [12.0, 14.0]
margin = [0.0, 0.0, 4.0, 0.0]
background = "$surface"
text_color = "$text"
placeholder = "Type to search apps, scripts, or aliases..."
placeholder_color = "$subtle"
corner_radius = 10.0
icon = "🔍"
icon_color = "$accent"
[listview]
columns = 1
spacing = 4.0
scrollbar = false
empty_text = "No matching targets"
empty_text_color = "$subtle"
[element]
padding = [8.0, 10.0]
corner_radius = 8.0
background = "transparent"
text_color = "$text"
description_color = "$subtle"
show_icons = true
icon_size = 22.0
layout = ["icon", "name", "spacer", "btn_copy", "btn_edit", "btn_run", "shortcuts", "badge"]
[element.selected]
background = "$surface"
text_color = "$text"
description_color = "$subtle"
[colors]
bg = "#0d1117"
sidebar_bg = "#161b22"
surface = "#21262d"
card_bg = "#1c2128"
hover_bg = "#30363d"
border = "#30363d"
accent = "#58a6ff"
accent_dim = "#1f3b5c"
badge_text = "#7d8590"
text = "#f0f6fc"
text_subtle = "#c9d1d9"
subtle = "#8b949e"
[widgets.sidebar]
type = "box"
orientation = "vertical"
width = 230.0
padding = [14.0, 14.0]
gap = 10.0
background = "$sidebar_bg"
radius = 12.0
children = ["btn_run", "btn_edit", "btn_copy"]
[widgets.main_pane]
type = "box"
orientation = "vertical"
flex = true
gap = 10.0
children = ["InputBar", "ListView"]
[widgets.btn_run]
type = "button"
icon = "▶"
action = "run"
[widgets.btn_edit]
type = "button"
icon = "✎"
action = "edit"
[widgets.btn_copy]
type = "button"
icon = "⎘"
action = "copy"
"##;
        let mut t: ThemeConfig =
            toml::from_str(content).expect("horizon.toml should parse cleanly");
        assert_eq!(t.name, "Horizon");
        assert_eq!(t.mainbox.orientation, "horizontal");
        t.resolve_colors();
        assert_eq!(t.window.background, "#0d1117");

        let reg = crate::core::widget::WidgetRegistry::from_theme(&t.widgets);
        assert!(reg.validate().is_ok());
        assert!(reg.get("sidebar").is_some());
        assert!(reg.get("main_pane").is_some());
        assert!(reg.get("btn_run").is_some());
        assert!(reg.get("btn_edit").is_some());
        assert!(reg.get("btn_copy").is_some());
    }

    #[test]
    fn default_mainbox_children_are_inputbar_and_listview() {
        let t = ThemeConfig::default();
        assert_eq!(t.mainbox.children.len(), 2);
        assert_eq!(
            t.mainbox.children[0],
            Widget::Builtin(BuiltinWidget::InputBar)
        );
        assert_eq!(
            t.mainbox.children[1],
            Widget::Builtin(BuiltinWidget::ListView)
        );
    }

    #[test]
    fn load_theme_default_returns_builtin() {
        let t = load_theme("default");
        assert_eq!(t.name, "Dark Transparent");
    }

    #[test]
    fn load_theme_missing_file_returns_builtin() {
        let t = load_theme("nonexistent_theme_12345");
        assert_eq!(t.name, "Dark Transparent");
    }

    #[test]
    fn load_theme_loads_from_dot_config_or_app_support() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let theme_dir = home.join(".config").join("aerofi").join("themes");
        let _ = std::fs::create_dir_all(&theme_dir);
        let test_theme_path = theme_dir.join("test_custom.toml");
        let content = r#"
name = "Test Custom"
[mainbox]
orientation = "horizontal"
"#;
        let _ = std::fs::write(&test_theme_path, content);
        let t = load_theme("test_custom");
        let _ = std::fs::remove_file(&test_theme_path);
        assert_eq!(t.name, "Test Custom");
        assert_eq!(t.mainbox.orientation, "horizontal");
    }

    #[test]
    fn parse_hex_color_six_digit() {
        assert_eq!(parse_hex_color("#1a1b26"), Some(0x1a1b26));
        assert_eq!(parse_hex_color("7aa2f7"), Some(0x7aa2f7));
    }

    #[test]
    fn parse_hex_color_three_digit() {
        assert_eq!(parse_hex_color("#fff"), Some(0xffffff));
        assert_eq!(parse_hex_color("#000"), Some(0x000000));
        assert_eq!(parse_hex_color("#abc"), Some(0xaabbcc));
    }

    #[test]
    fn parse_hex_color_invalid() {
        assert_eq!(parse_hex_color(""), None);
        assert_eq!(parse_hex_color("#gggggg"), None);
        assert_eq!(parse_hex_color("#12"), None);
        assert_eq!(parse_hex_color("#1234"), None);
    }

    #[test]
    fn resolve_colors_replaces_aliases() {
        let mut t = ThemeConfig::default();
        t.colors.insert("bg".to_string(), "#112233".to_string());
        t.colors.insert("fg".to_string(), "#aabbcc".to_string());
        t.window.background = "$bg".to_string();
        t.inputbar.text_color = "$fg".to_string();
        t.element.selected.background = "$bg".to_string();
        t.resolve_colors();
        assert_eq!(t.window.background, "#112233");
        assert_eq!(t.inputbar.text_color, "#aabbcc");
        assert_eq!(t.element.selected.background, "#112233");
    }

    #[test]
    fn resolve_colors_unknown_alias_falls_back() {
        let mut t = ThemeConfig::default();
        t.window.background = "$nonexistent".to_string();
        t.resolve_colors();
        assert_eq!(t.window.background, "#000000");
    }

    #[test]
    fn resolve_colors_skips_non_aliases() {
        let mut t = ThemeConfig::default();
        t.window.background = "#1a1b26".to_string();
        t.resolve_colors();
        assert_eq!(t.window.background, "#1a1b26");
    }

    #[test]
    fn widget_def_deserializes_from_toml() {
        let toml = r##"
            [[widgets]]
            id = "greeting"
            type = "text"
            text = "Hello"
            color = "#ff0000"
            font_size = 14.0

            [[widgets]]
            id = "logo"
            type = "icon"
            icon = "🚀"
            size = 32.0

            [[widgets]]
            id = "flex"
            type = "spacer"

            [[widgets]]
            id = "sep"
            type = "divider"
            color = "#414868"
            thickness = 2.0
            margin = 8.0

            [[widgets]]
            id = "avatar"
            type = "image"
            path = "~/.config/aerofi/avatar.png"
            width = 48.0
            height = 48.0
            radius = 24.0

            [[widgets]]
            id = "btn"
            type = "button"
            icon = "⚡"
            text = "Reload"
            action = "reload"
            color = "#7aa2f7"
            background = "#24283b"
            hover_background = "#414868"
            hover_color = "#bb9af7"
            border_color = "#3b4261"
            border_width = 1.0
            radius = 6.0
            padding = [8.0, 4.0]
            gap = 4.0

            [[widgets]]
            id = "header"
            type = "box"
            orientation = "horizontal"
            gap = 8.0
            padding = [8.0, 12.0]
            children = ["logo", "greeting", "flex", "btn"]
        "##;

        #[derive(Deserialize)]
        struct Partial {
            widgets: Vec<WidgetDef>,
        }
        let parsed: Partial = toml::from_str(toml).expect("should parse");
        assert_eq!(parsed.widgets.len(), 7);
        assert_eq!(parsed.widgets[0].id(), "greeting");
        assert_eq!(parsed.widgets[1].id(), "logo");
        assert_eq!(parsed.widgets[2].id(), "flex");
        assert_eq!(parsed.widgets[3].id(), "sep");
        assert_eq!(parsed.widgets[4].id(), "avatar");
        assert_eq!(parsed.widgets[5].id(), "btn");
        assert_eq!(parsed.widgets[6].id(), "header");

        // Verify Button widget
        if let WidgetDef::Button {
            action,
            icon,
            text,
            radius,
            ..
        } = &parsed.widgets[5]
        {
            assert_eq!(action.as_deref(), Some("reload"));
            assert_eq!(icon.as_deref(), Some("⚡"));
            assert_eq!(text.as_deref(), Some("Reload"));
            assert_eq!(*radius, Some(6.0));
        } else {
            panic!("expected Button widget");
        }

        // Verify Box children
        if let WidgetDef::Box { children, gap, .. } = &parsed.widgets[6] {
            assert_eq!(children, &["logo", "greeting", "flex", "btn"]);
            assert_eq!(*gap, Some(8.0));
        } else {
            panic!("expected Box widget");
        }
    }

    #[test]
    fn widget_custom_in_mainbox_children() {
        let toml = r#"
            name = "Test"
            [mainbox]
            children = ["InputBar", "header", "ListView"]
        "#;
        let t: ThemeConfig = toml::from_str(toml).expect("should parse");
        assert_eq!(t.mainbox.children.len(), 3);
        assert_eq!(
            t.mainbox.children[0],
            Widget::Builtin(BuiltinWidget::InputBar)
        );
        assert_eq!(t.mainbox.children[1], Widget::Custom("header".to_string()));
        assert_eq!(
            t.mainbox.children[2],
            Widget::Builtin(BuiltinWidget::ListView)
        );
    }

    #[test]
    fn resolve_colors_resolves_widget_aliases() {
        let mut t = ThemeConfig::default();
        t.colors.insert("accent".to_string(), "#7aa2f7".to_string());
        t.colors.insert("bg_btn".to_string(), "#24283b".to_string());
        t.widgets.push(WidgetDef::Text {
            id: "test".to_string(),
            text: Some("hi".to_string()),
            color: Some("$accent".to_string()),
            font_size: None,
            font_weight: None,
            align: None,
        });
        t.widgets.push(WidgetDef::Button {
            id: "btn".to_string(),
            text: Some("Click".to_string()),
            icon: None,
            action: Some("hide".to_string()),
            hotkey: None,
            color: Some("$accent".to_string()),
            background: Some("$bg_btn".to_string()),
            hover_background: Some("$accent".to_string()),
            hover_color: None,
            border_color: None,
            border_width: None,
            radius: None,
            padding: None,
            font_size: None,
            font_weight: None,
            gap: None,
        });
        t.resolve_colors();
        if let WidgetDef::Text { color, .. } = &t.widgets[0] {
            assert_eq!(color.as_deref(), Some("#7aa2f7"));
        } else {
            panic!("expected Text widget");
        }
        if let WidgetDef::Button {
            color,
            background,
            hover_background,
            ..
        } = &t.widgets[1]
        {
            assert_eq!(color.as_deref(), Some("#7aa2f7"));
            assert_eq!(background.as_deref(), Some("#24283b"));
            assert_eq!(hover_background.as_deref(), Some("#7aa2f7"));
        } else {
            panic!("expected Button widget");
        }
    }

    #[test]
    fn widgets_table_map_syntax_deserializes_into_theme_config() {
        let toml = r#"
            name = "TableTest"

            [widgets.logo]
            type = "icon"
            icon = "🚀"
            size = 20.0

            [widgets.flex]
            type = "spacer"

            [widgets.quick_stats]
            type = "box"
            orientation = "horizontal"
            gap = 6.0
            children = ["logo", "flex"]

            [widgets.btn_reload]
            type = "button"
            icon = "🔄"
            text = "Reload"
            action = "reload"
        "#;
        let t: ThemeConfig = toml::from_str(toml).expect("should parse table widgets");
        assert_eq!(t.widgets.len(), 4);

        let by_id: HashMap<&str, &WidgetDef> = t.widgets.iter().map(|w| (w.id(), w)).collect();
        assert!(by_id.contains_key("logo"));
        assert!(by_id.contains_key("flex"));
        assert!(by_id.contains_key("quick_stats"));
        assert!(by_id.contains_key("btn_reload"));

        if let WidgetDef::Icon { icon, size, .. } = by_id["logo"] {
            assert_eq!(icon, "🚀");
            assert_eq!(*size, Some(20.0));
        } else {
            panic!("expected Icon widget");
        }

        if let WidgetDef::Spacer { id } = by_id["flex"] {
            assert_eq!(id, "flex");
        } else {
            panic!("expected Spacer widget");
        }

        if let WidgetDef::Box { children, gap, .. } = by_id["quick_stats"] {
            assert_eq!(children, &["logo", "flex"]);
            assert_eq!(*gap, Some(6.0));
        } else {
            panic!("expected Box widget");
        }

        if let WidgetDef::Button {
            action, icon, text, ..
        } = by_id["btn_reload"]
        {
            assert_eq!(action.as_deref(), Some("reload"));
            assert_eq!(icon.as_deref(), Some("🔄"));
            assert_eq!(text.as_deref(), Some("Reload"));
        } else {
            panic!("expected Button widget");
        }
    }
}
