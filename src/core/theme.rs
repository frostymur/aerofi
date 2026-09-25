//! Theme configuration and loader.
//!
//! Themes define the visual appearance of every launcher widget. A theme
//! is a TOML file at `~/.config/aerofi/themes/{name}.toml`; the special
//! name `"default"` returns the built-in Tokyo Night palette without
//! reading any file.

use std::collections::HashMap;

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
        _ => Err(serde::de::Error::custom(format!(
            "unknown builtin widget: {s} (valid: InputBar, ListView)"
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
        font_weight: Option<FontWeightSpec>,
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
        /// Stretch the image across the container's full width (overrides
        /// `width`). Pair with a fixed `height` for banner-style images.
        #[serde(default)]
        w_full: Option<bool>,
        /// Stretch the image across the container's full height (overrides
        /// `height`). With `w_full` the image cover-fills the container at
        /// any window size — the artwork-pane use case.
        #[serde(default)]
        h_full: Option<bool>,
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
        font_weight: Option<FontWeightSpec>,
        gap: Option<f32>,
        close: Option<bool>,
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

/// A font weight: either a CSS name (`"bold"`, `"medium"`) or a numeric
/// value (`700`, `550`). Accepts both TOML forms — quoted or bare.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum FontWeightSpec {
    Named(String),
    Numeric(f64),
}

impl FontWeightSpec {
    /// The spec as a plain string so rendering helpers can parse it
    /// uniformly (numeric values are stringified without a trailing `.0`).
    pub fn as_str(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Self::Named(s) => std::borrow::Cow::Borrowed(s.as_str()),
            Self::Numeric(n) => {
                let mut s = n.to_string();
                if let Some(idx) = s.find('.')
                    && s[idx + 1..].chars().all(|c| c == '0')
                {
                    s.truncate(idx);
                }
                std::borrow::Cow::Owned(s)
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub weight: Option<FontWeightSpec>,
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

/// Per-element font override. Any field left unset inherits the global
/// `[font]` value.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FontOverride {
    pub family: Option<String>,
    pub size: Option<f32>,
    pub weight: Option<FontWeightSpec>,
    pub fallback: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dimension {
    Points(f32),
    Percent(f32),
}

impl Dimension {
    pub fn resolve(&self, screen_size: f32) -> f32 {
        let value = match self {
            Dimension::Points(p) => *p,
            Dimension::Percent(pct) => screen_size * (pct / 100.0),
        };
        // Defensive clamp: a malformed config value (negative, NaN, "0%")
        // must not collapse the window to zero or below. `f32::max` is
        // NaN-safe (returns the other operand).
        value.max(1.0)
    }
}

impl<'de> serde::Deserialize<'de> for Dimension {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Dimension;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a float or a string ending in '%'")
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(Dimension::Points(v as f32))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(Dimension::Points(v as f32))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(Dimension::Points(v as f32))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                if let Some(s) = v.strip_suffix('%') {
                    let val = s.parse::<f32>().map_err(serde::de::Error::custom)?;
                    Ok(Dimension::Percent(val))
                } else {
                    let val = v.parse::<f32>().map_err(serde::de::Error::custom)?;
                    Ok(Dimension::Points(val))
                }
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub width: Dimension,
    pub height: Dimension,
    pub padding: f32,
    /// Horizontal offset from screen centre (points). `0.0` = centred,
    /// negative = towards the left edge.
    pub x_offset: f32,
    /// Vertical offset from screen centre (points). `0.0` = centred,
    /// negative = towards the top edge.
    pub y_offset: f32,
    pub background: String,
    pub background_image: Option<String>,
    /// Position of the background image: `"cover"` (full background),
    /// `"left"`, or `"right"` (image as a side panel). Unknown values fall
    /// back to `"cover"`.
    pub background_position: Option<String>,
    pub blur: bool,
    /// Window background opacity: `1.0` = fully opaque, `0.0` = fully
    /// transparent. The default (0.80) keeps enough of the dark background
    /// for light text to stay readable even over a bright desktop.
    pub background_opacity: Option<f32>,
    pub corner_radius: f32,
    pub border_width: f32,
    pub border_color: String,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: Dimension::Percent(51.7),
            height: Dimension::Percent(50.2),
            padding: 16.0,
            x_offset: 0.0,
            y_offset: 0.0,
            background: "#1a1a1a".to_string(),
            background_image: None,
            background_position: None,
            blur: true,
            background_opacity: Some(0.80),
            corner_radius: 16.0,
            border_width: 1.0,
            border_color: "#2a2a2a".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Script view
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ScriptViewConfig {
    /// Extra padding around the full-window script view (script output, theme switcher).
    /// Adds to the existing `[window].padding`. Default: 0.
    pub padding: Option<f32>,
}

// ---------------------------------------------------------------------------
// Per-mode element overrides
// ---------------------------------------------------------------------------

/// Optional element overrides for a specific Rofi-mode (identified by
/// `@aerofi.preset` in the script). Unset fields inherit from `[element]`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PresetElementOverride {
    /// Override `[element].padding` — `[vertical, horizontal]`.
    pub padding: Option<Vec<f32>>,
    /// Override `[element].icon_size`.
    pub icon_size: Option<f32>,
    /// Override `[element].corner_radius`.
    pub corner_radius: Option<f32>,
    /// Override listview columns for this mode.
    pub columns: Option<usize>,
}

/// Config for a single Rofi-mode (e.g. `[presets.emoji]`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PresetConfig {
    pub window_width: Option<Dimension>,
    pub element: PresetElementOverride,
}

/// Map of preset name → config. Populated from `[presets.<name>]` tables.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PresetsConfig(HashMap<String, PresetConfig>);

impl PresetsConfig {
    pub fn get(&self, name: &str) -> Option<&PresetConfig> {
        self.0.get(name)
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
    /// Gap between mainbox children in points. Defaults to
    /// `[listview].spacing` when unset.
    pub gap: Option<f32>,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            orientation: "vertical".to_string(),
            children: vec![
                Widget::Builtin(BuiltinWidget::InputBar),
                Widget::Builtin(BuiltinWidget::ListView),
            ],
            gap: None,
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
    pub border_width: f32,
    pub border_color: String,
    /// Optional font override for the search bar text (see FontOverride).
    pub font: Option<FontOverride>,
}

impl Default for InputBarConfig {
    fn default() -> Self {
        Self {
            height: 44.0,
            padding: vec![12.0, 16.0],
            margin: vec![0.0, 0.0, 8.0, 0.0],
            background: "transparent".to_string(),
            text_color: "#ffffff".to_string(),
            font: None,
            placeholder: "Search...".to_string(),
            placeholder_color: "#888888".to_string(),
            corner_radius: 8.0,
            icon: Some("❯".to_string()),
            icon_color: Some("#888888".to_string()),
            border_width: 0.0,
            border_color: "transparent".to_string(),
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
    pub empty_text: String,
    pub empty_text_color: String,
    /// Highlight the query's matched characters inside item names.
    pub highlight_matches: bool,
    /// Colour of the matched characters. Defaults to
    /// `status_colors.accent` when unset.
    pub match_color: Option<String>,
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
            empty_text: "No matches".to_string(),
            highlight_matches: true,
            match_color: None,
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
    /// Gap between the icon and the name inside a grid cell (points).
    pub icon_gap: f32,
    /// Border drawn around an item: `0.0` for a borderless tile.
    pub border_width: f32,
    /// Corner radius applied to the item's icon image.
    pub icon_radius: f32,
    pub layout: Option<Vec<String>>,
    pub selected: SelectedState,
    /// Optional font override for row names (see FontOverride).
    pub font: Option<FontOverride>,
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
            icon_gap: 6.0,
            border_width: 0.0,
            icon_radius: 4.0,
            layout: Some(vec![
                "icon".to_string(),
                "name".to_string(),
                "spacer".to_string(),
            ]),
            selected: SelectedState::default(),
            font: None,
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
    pub script_view: ScriptViewConfig,
    pub presets: PresetsConfig,
    pub mainbox: ContainerConfig,
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
            author: Some("aerofi".to_string()),
            font: FontConfig::default(),
            window: WindowConfig::default(),
            script_view: ScriptViewConfig::default(),
            presets: PresetsConfig::default(),
            mainbox: ContainerConfig::default(),
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
        resolve(&mut self.inputbar.border_color, colors);

        // ListView
        resolve(&mut self.listview.empty_text_color, colors);
        resolve_opt(&mut self.listview.match_color, colors);
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
fn resolve_theme_file(rel_path: &str) -> Option<std::path::PathBuf> {
    let dot_config_path = dirs::home_dir().map(|h| {
        h.join(".config")
            .join("aerofi")
            .join("themes")
            .join(rel_path)
    });

    let app_support_path =
        dirs::config_dir().map(|c| c.join("aerofi").join("themes").join(rel_path));

    match (&dot_config_path, &app_support_path) {
        (Some(p), _) if p.is_file() => Some(p.clone()),
        (_, Some(p)) if p.is_file() => Some(p.clone()),
        (Some(p), _) => Some(p.clone()),
        (_, Some(p)) => Some(p.clone()),
        _ => None,
    }
}

fn merge_toml(base: &mut toml::Table, override_table: toml::Table) {
    for (k, v) in override_table {
        match (base.get_mut(&k), v) {
            (Some(toml::Value::Table(base_table)), toml::Value::Table(over_table)) => {
                merge_toml(base_table, over_table);
            }
            (Some(_), new_val) => {
                base.insert(k, new_val);
            }
            (None, new_val) => {
                base.insert(k, new_val);
            }
        }
    }
}

fn load_theme_table(
    rel_path: &str,
    visited: &mut std::collections::HashSet<String>,
) -> Option<toml::Table> {
    let normalized = rel_path.to_string();
    if visited.contains(&normalized) {
        eprintln!(
            "aerofi: warning: cyclic theme import detected: {}",
            normalized
        );
        return Some(toml::Table::new());
    }
    visited.insert(normalized.clone());

    let path = resolve_theme_file(rel_path)?;

    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(err) => {
            eprintln!(
                "aerofi: warning: failed to read theme {}: {err}",
                path.display()
            );
            return Some(toml::Table::new());
        }
    };

    let mut current_table = match toml::from_str::<toml::Table>(&contents) {
        Ok(t) => t,
        Err(err) => {
            eprintln!(
                "aerofi: warning: failed to parse theme {}: {err}",
                path.display()
            );
            return Some(toml::Table::new());
        }
    };

    let mut merged = toml::Table::new();
    if let Some(toml::Value::Array(imports)) = current_table.remove("imports") {
        for import_val in imports {
            if let toml::Value::String(import_path) = import_val {
                if let Some(imported_table) = load_theme_table(&import_path, visited) {
                    merge_toml(&mut merged, imported_table);
                }
            } else {
                eprintln!(
                    "aerofi: warning: non-string value in imports array in {}",
                    path.display()
                );
            }
        }
    }

    merge_toml(&mut merged, current_table);

    Some(merged)
}

pub fn load_theme(theme_name: &str) -> ThemeConfig {
    if theme_name == "default" {
        return ThemeConfig::default();
    }

    let file_name = format!("{theme_name}.toml");
    let mut visited = std::collections::HashSet::new();

    let merged_table = match load_theme_table(&file_name, &mut visited) {
        Some(t) if !t.is_empty() => t,
        _ => {
            eprintln!(
                "aerofi: warning: cannot determine config dir or load theme, using default theme"
            );
            return ThemeConfig::default();
        }
    };

    let merged_val = toml::Value::Table(merged_table);
    match merged_val.try_into::<ThemeConfig>() {
        Ok(mut theme) => {
            theme.resolve_colors();
            theme
        }
        Err(err) => {
            eprintln!(
                "aerofi: warning: failed to deserialize theme {}: {err}; using default",
                file_name
            );
            ThemeConfig::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Hex colour helpers
// ---------------------------------------------------------------------------

/// Cap for the parse caches: theme colours number in the tens; the only
/// dynamic input is pango-markup colour values from script output, so a
/// bounded cache (cleared wholesale when full) keeps memory flat.
const MAX_HEX_CACHE: usize = 4096;

thread_local! {
    static HEX_ALPHA_CACHE: std::cell::RefCell<std::collections::HashMap<String, Option<u32>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    static HEX_CACHE: std::cell::RefCell<std::collections::HashMap<String, Option<u32>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Parse a CSS-style hex colour (`"#1a1b26"`, `"7aa2f7"`, `"#fff"`) into
/// a 24-bit RGB value suitable for GPUI's `rgb()`.  Returns `None` on
/// malformed input.
pub fn parse_hex_color_alpha(hex: &str) -> Option<u32> {
    HEX_ALPHA_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(value) = cache.get(hex) {
            return *value;
        }
        let value = parse_hex_color_alpha_uncached(hex);
        if cache.len() >= MAX_HEX_CACHE {
            cache.clear();
        }
        cache.insert(hex.to_string(), value);
        value
    })
}

fn parse_hex_color_alpha_uncached(hex: &str) -> Option<u32> {
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
    HEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(value) = cache.get(hex) {
            return *value;
        }
        let value = parse_hex_color_uncached(hex);
        if cache.len() >= MAX_HEX_CACHE {
            cache.clear();
        }
        cache.insert(hex.to_string(), value);
        value
    })
}

fn parse_hex_color_uncached(hex: &str) -> Option<u32> {
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

    /// Base directory of the example themes bundled in the repo.
    const EXAMPLE_THEMES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/themes");

    /// Recursively load an example theme, resolving its `imports` relative to
    /// the examples/themes dir (mirrors the runtime loader).
    fn load_example_table(
        rel_path: &str,
        visited: &mut std::collections::HashSet<String>,
    ) -> Option<toml::Table> {
        if visited.contains(rel_path) {
            return Some(toml::Table::new());
        }
        visited.insert(rel_path.to_string());
        let path = std::path::Path::new(EXAMPLE_THEMES_DIR).join(rel_path);
        let contents = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read example theme {}: {e}", path.display()));
        let mut table = toml::from_str::<toml::Table>(&contents)
            .unwrap_or_else(|e| panic!("failed to parse example theme {}: {e}", path.display()));
        let mut merged = toml::Table::new();
        if let Some(toml::Value::Array(imports)) = table.remove("imports") {
            for import in imports {
                if let toml::Value::String(import_path) = import
                    && let Some(imported) = load_example_table(&import_path, visited)
                {
                    super::merge_toml(&mut merged, imported);
                }
            }
        }
        super::merge_toml(&mut merged, table);
        Some(merged)
    }

    /// Load a bundled example theme end-to-end (imports resolved + colors).
    fn load_example_theme(name: &str) -> ThemeConfig {
        let table =
            load_example_table(name, &mut std::collections::HashSet::new()).expect("should load");
        let mut theme: ThemeConfig = toml::Value::Table(table)
            .try_into()
            .expect("should deserialize");
        theme.resolve_colors();
        theme
    }

    #[test]
    fn default_theme_has_dark_transparent_palette() {
        let t = ThemeConfig::default();
        assert_eq!(t.name, "Dark Transparent");
        assert_eq!(t.window.background, "#1a1a1a");
        assert_eq!(t.inputbar.background, "transparent");
        assert_eq!(t.inputbar.text_color, "#ffffff");
        assert_eq!(t.element.selected.background, "#ffffff20");
        // Selection is highlight-only in the builtin theme: no border.
        assert_eq!(t.element.border_width, 0.0);
    }

    #[test]
    fn example_tokyo_night_theme_is_valid() {
        let content = r##"
name    = "Tokyo Night"
author  = "aerofi"
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
    fn example_tokyo_night_file_is_valid() {
        let t = load_example_theme("tokyo-night.toml");
        assert_eq!(t.name, "Tokyo Night");
        assert_eq!(t.window.background, "#1a1b26");
        assert_eq!(t.status_colors.accent, "#7aa2f7");
    }

    #[test]
    fn example_gruvbox_file_is_valid() {
        let t = load_example_theme("gruvbox.toml");
        assert_eq!(t.name, "Gruvbox Dark");
        assert_eq!(t.window.background, "#282828");
        assert_eq!(t.status_colors.accent, "#8ec07c");
    }

    #[test]
    fn example_tokyo_night_grid_file_is_valid() {
        // Modular composition + a partial palette override (bg, surface2).
        let t = load_example_theme("tokyo-night-grid.toml");
        assert_eq!(t.name, "Tokyo Night Grid");
        assert_eq!(t.window.width, Dimension::Percent(51.0));
        assert_eq!(t.window.height, Dimension::Percent(46.0));
        assert_eq!(t.listview.columns, 4);
        // Rofi-ported grid: borderless tiles, 12px radius, 72px icons, 15px gap.
        assert_eq!(t.font.size, 15.0);
        assert_eq!(t.element.background, "transparent");
        assert_eq!(t.element.corner_radius, 12.0);
        assert_eq!(t.element.icon_size, 72.0);
        assert_eq!(t.element.icon_gap, 15.0);
        assert_eq!(t.element.border_width, 0.0);
        assert_eq!(t.colors.get("bg").map(String::as_str), Some("#1a1b26"));
        assert_eq!(
            t.colors.get("surface2").map(String::as_str),
            Some("#343b58")
        );
        assert_eq!(t.status_colors.accent, "#7aa2f7");
    }

    #[test]
    fn example_catppuccin_mocha_theme_is_valid() {
        let t = load_example_theme("catppuccin-mocha.toml");
        assert_eq!(t.name, "Catppuccin Mocha");
        // Window is 20% wider than the default; normal (1:1) icon/text sizes.
        assert_eq!(t.window.width, Dimension::Percent(62.0));
        assert_eq!(t.font.size, 15.0);
        assert_eq!(t.element.icon_size, 24.0);
        // No blur; $bg at 80% opacity (unblurred desktop shows through).
        assert_eq!(t.window.background, "#1e1e2e");
        assert_eq!(t.window.background_opacity, Some(0.8));
        assert!(!t.window.blur);
        // Two EQUAL panes: horizontal mainbox with two flex custom widgets.
        assert_eq!(t.mainbox.orientation, "horizontal");
        assert_eq!(t.mainbox.children.len(), 2);
        // Catppuccin palette + opaque right-pane colour.
        assert_eq!(t.colors.get("bg").map(String::as_str), Some("#1e1e2e"));
        assert_eq!(t.colors.get("panel").map(String::as_str), Some("#1e1e2e"));

        // Both panes resolve, are equal (flex), and the tree validates.
        let registry = crate::core::widget::WidgetRegistry::from_theme(&t.widgets);
        assert!(registry.validate().is_ok());
        for id in ["left_panel", "right_panel"] {
            match registry.get(id) {
                Some(WidgetDef::Box { flex, width, .. }) => {
                    assert!(
                        matches!(flex, &Some(true)),
                        "{id} should be flex (equal panes)"
                    );
                    assert!(width.is_none(), "{id} should not have a fixed width");
                }
                other => panic!("{id} should be a Box, got {other:?}"),
            }
        }
        // Right pane keeps its opaque background after resolution.
        match registry.get("right_panel") {
            Some(WidgetDef::Box { background, .. }) => {
                assert_eq!(background.as_deref(), Some("#1e1e2e"));
            }
            other => panic!("right_panel should be a Box, got {other:?}"),
        }
    }

    #[test]
    fn element_config_item_fields_default_when_absent() {
        // icon_gap / border_width / icon_radius are optional and fall back to
        // defaults when a theme omits them (backward compatible).
        let el: ElementConfig = toml::from_str("icon_size = 40.0").unwrap();
        assert_eq!(el.icon_size, 40.0);
        assert_eq!(el.icon_gap, 6.0);
        assert_eq!(el.border_width, 0.0);
        assert_eq!(el.icon_radius, 4.0);
    }

    #[test]
    fn reference_theme_file_is_valid() {
        let content = include_str!("../../examples/theme.toml");
        let mut t: ThemeConfig =
            toml::from_str(content).expect("examples/theme.toml should parse cleanly");
        assert_eq!(t.name, "Complete Reference Theme");
        t.resolve_colors();
        assert_eq!(t.window.background, "#1a1b26f0");
        assert_eq!(t.widgets.len(), 10);
        let registry = crate::core::widget::WidgetRegistry::from_theme(&t.widgets);
        assert!(registry.validate().is_ok());
    }

    #[test]
    fn example_horizon_theme_is_valid() {
        let content = r##"
name   = "Horizon"
author = "aerofi"
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
            id = "banner_img"
            type = "image"
            path = "~/.config/aerofi/themes/banner.jpg"
            radius = 8.0
            w_full = true
            h_full = true

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
        assert_eq!(parsed.widgets.len(), 8);
        assert_eq!(parsed.widgets[0].id(), "greeting");
        assert_eq!(parsed.widgets[1].id(), "logo");
        assert_eq!(parsed.widgets[2].id(), "flex");
        assert_eq!(parsed.widgets[3].id(), "sep");
        assert_eq!(parsed.widgets[4].id(), "avatar");
        assert_eq!(parsed.widgets[5].id(), "banner_img");
        assert_eq!(parsed.widgets[6].id(), "btn");
        assert_eq!(parsed.widgets[7].id(), "header");

        // Image widget: fixed size (w_full absent) and full-width variant.
        if let WidgetDef::Image {
            width,
            w_full,
            h_full,
            ..
        } = &parsed.widgets[4]
        {
            assert_eq!(*width, Some(48.0));
            assert_eq!(*w_full, None);
            assert_eq!(*h_full, None);
        } else {
            panic!("expected Image widget");
        }
        if let WidgetDef::Image {
            width,
            height,
            w_full,
            h_full,
            ..
        } = &parsed.widgets[5]
        {
            assert_eq!(*width, None);
            assert_eq!(*height, None);
            assert_eq!(*w_full, Some(true));
            assert_eq!(*h_full, Some(true));
        } else {
            panic!("expected Image widget");
        }

        // Verify Button widget
        if let WidgetDef::Button {
            action,
            icon,
            text,
            radius,
            ..
        } = &parsed.widgets[6]
        {
            assert_eq!(action.as_deref(), Some("reload"));
            assert_eq!(icon.as_deref(), Some("⚡"));
            assert_eq!(text.as_deref(), Some("Reload"));
            assert_eq!(*radius, Some(6.0));
        } else {
            panic!("expected Button widget");
        }

        // Verify Box children
        if let WidgetDef::Box { children, gap, .. } = &parsed.widgets[7] {
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
            close: None,
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

    #[test]
    fn test_merge_toml_deep_merge() {
        let mut base: toml::Table = toml::from_str(
            r##"
            [window]
            width = 500
            
            [colors]
            background = "#000000"
        "##,
        )
        .unwrap();

        let override_table: toml::Table = toml::from_str(
            r##"
            [window]
            height = 300
            
            [colors]
            background = "#ffffff"
            foreground = "#aaaaaa"
        "##,
        )
        .unwrap();

        super::merge_toml(&mut base, override_table);

        let expected: toml::Table = toml::from_str(
            r##"
            [window]
            width = 500
            height = 300
            
            [colors]
            background = "#ffffff"
            foreground = "#aaaaaa"
        "##,
        )
        .unwrap();

        assert_eq!(base, expected);
    }

    #[test]
    fn test_merge_toml_array_replacement() {
        let mut base: toml::Table = toml::from_str(
            r##"
            children = ["inputbar", "listview"]
        "##,
        )
        .unwrap();

        let override_table: toml::Table = toml::from_str(
            r##"
            children = ["inputbar"]
        "##,
        )
        .unwrap();

        super::merge_toml(&mut base, override_table);

        let expected: toml::Table = toml::from_str(
            r##"
            children = ["inputbar"]
        "##,
        )
        .unwrap();

        assert_eq!(base, expected);
    }

    #[test]
    fn modular_mixins_parse_cleanly() {
        let colors_tn = include_str!("../../examples/themes/colors/tokyo-night.toml");
        let _: toml::Table =
            toml::from_str(colors_tn).expect("colors/tokyo-night.toml should parse as TOML table");

        let colors_gruv = include_str!("../../examples/themes/colors/gruvbox.toml");
        let _: toml::Table =
            toml::from_str(colors_gruv).expect("colors/gruvbox.toml should parse as TOML table");

        let layout_compact = include_str!("../../examples/themes/layouts/compact.toml");
        let _: toml::Table = toml::from_str(layout_compact)
            .expect("layouts/compact.toml should parse as TOML table");

        let layout_grid = include_str!("../../examples/themes/layouts/grid.toml");
        let _: toml::Table =
            toml::from_str(layout_grid).expect("layouts/grid.toml should parse as TOML table");
    }

    #[test]
    fn test_merge_modular_with_widget() {
        // Compose a theme from palette + layout + a widget mixin, the way a
        // user theme would. Proves imports merge and the widget tree validates.
        let mut visited = std::collections::HashSet::new();
        let mut merged = toml::Table::new();
        for file in [
            "colors/tokyo-night.toml",
            "layouts/compact.toml",
            "widgets/header-bar.toml",
        ] {
            let table = load_example_table(file, &mut visited).expect("should load");
            super::merge_toml(&mut merged, table);
        }

        let mut theme: ThemeConfig = toml::Value::Table(merged)
            .try_into()
            .expect("merged theme should deserialize");
        theme.resolve_colors();
        assert_eq!(theme.window.width, Dimension::Percent(46.3));
        assert_eq!(theme.window.height, Dimension::Percent(47.1));
        // header-bar.toml defines 4 widgets (header_bar + 3 children).
        assert_eq!(theme.widgets.len(), 4);
        let registry = crate::core::widget::WidgetRegistry::from_theme(&theme.widgets);
        assert!(registry.validate().is_ok());
    }
    #[test]
    fn font_weight_accepts_names_and_numbers() {
        // Quoted names.
        let named: FontConfig = toml::from_str(
            r#"family = "A"
weight = "bold""#,
        )
        .unwrap();
        assert_eq!(
            named.weight,
            Some(crate::core::theme::FontWeightSpec::Named("bold".into()))
        );

        // Bare numbers.
        let numeric: FontConfig = toml::from_str("family = \"A\"\nweight = 700").unwrap();
        assert_eq!(
            numeric.weight,
            Some(crate::core::theme::FontWeightSpec::Numeric(700.0))
        );

        // Fractional numbers stringified without a trailing .0.
        assert_eq!(
            crate::core::theme::FontWeightSpec::Numeric(550.0).as_str(),
            "550"
        );
        assert_eq!(
            crate::core::theme::FontWeightSpec::Named("medium".into()).as_str(),
            "medium"
        );
    }

    #[test]
    fn window_position_and_match_highlight_fields_deserialize() {
        let mut theme: ThemeConfig = toml::from_str(
            r##"
[window]
x_offset = 10.0
y_offset = -40.0

[listview]
highlight_matches = false
match_color = "#9ece6a"

[inputbar]
[inputbar.font]
family = "JetBrains Mono"
size = 14.0
weight = "medium"

[element]
[element.font]
family = "Fira Code"
weight = 700
"##,
        )
        .unwrap();
        theme.resolve_colors();
        assert_eq!(theme.window.x_offset, 10.0);
        assert_eq!(theme.window.y_offset, -40.0);
        assert!(!theme.listview.highlight_matches);
        assert_eq!(theme.listview.match_color.as_deref(), Some("#9ece6a"));
        let ib = theme.inputbar.font.as_ref().expect("inputbar font");
        assert_eq!(ib.family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(ib.size, Some(14.0));
        let el = theme.element.font.as_ref().expect("element font");
        assert_eq!(el.family.as_deref(), Some("Fira Code"));
    }

    #[test]
    fn mainbox_gap_defaults_to_none_and_deserializes() {
        let theme: ThemeConfig = toml::from_str("[mainbox]\ngap = 0.0").unwrap();
        assert_eq!(theme.mainbox.gap, Some(0.0));
        let theme: ThemeConfig = toml::from_str("").unwrap();
        assert_eq!(theme.mainbox.gap, None);
    }

    #[test]
    fn script_view_padding_deserializes() {
        let theme: ThemeConfig = toml::from_str("[script_view]\npadding = 12.0").unwrap();
        assert_eq!(theme.script_view.padding, Some(12.0));
        let theme: ThemeConfig = toml::from_str("").unwrap();
        assert_eq!(theme.script_view.padding, None);
    }

    #[test]
    fn presets_element_overrides_deserialize() {
        let theme: ThemeConfig = toml::from_str(
            "[presets.emoji.element]\npadding = [4.0, 4.0]\nicon_size = 48.0\ncorner_radius = 8.0\ncolumns = 8",
        )
        .unwrap();
        let m = theme.presets.get("emoji").unwrap();
        assert_eq!(m.element.padding, Some(vec![4.0, 4.0]));
        assert_eq!(m.element.icon_size, Some(48.0));
        assert_eq!(m.element.corner_radius, Some(8.0));
        assert_eq!(m.element.columns, Some(8));
        assert!(theme.presets.get("clipboard").is_none());
    }

    #[test]
    fn window_position_and_match_highlight_fields_default() {
        let theme: ThemeConfig = toml::from_str("").unwrap();
        assert_eq!(theme.window.x_offset, 0.0);
        assert_eq!(theme.window.y_offset, 0.0);
        assert!(theme.listview.highlight_matches);
        assert!(theme.listview.match_color.is_none());
        assert!(theme.inputbar.font.is_none());
        assert!(theme.element.font.is_none());
    }
}
