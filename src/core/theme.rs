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
/// list.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Widget {
    InputBar,
    ListView,
    Prompt,
    Entry,
    Banner,
    SidebarImage,
    ContentBox,
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
    pub image_scale: Option<f32>,
    pub blur: bool,
    pub corner_radius: f32,
    pub border_width: f32,
    pub border_color: String,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 800.0,
            height: 500.0,
            padding: 16.0,
            background: "#1a1b26".to_string(),
            background_image: None,
            image_scale: None,
            blur: false,
            corner_radius: 12.0,
            border_width: 1.0,
            border_color: "#414868".to_string(),
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
            children: vec![Widget::InputBar, Widget::ListView],
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
            height: 48.0,
            padding: vec![12.0, 16.0],
            margin: vec![0.0, 0.0, 8.0, 0.0],
            background: "#24283b".to_string(),
            text_color: "#c0caf5".to_string(),
            placeholder: "Type to filter…".to_string(),
            placeholder_color: "#565f89".to_string(),
            corner_radius: 8.0,
            icon: Some("❯".to_string()),
            icon_color: Some("#7aa2f7".to_string()),
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
    /// When `true`, the list is hidden and the window shrinks to the
    /// inputbar until the user types a query.
    pub require_input: Option<bool>,
}

impl Default for ListViewConfig {
    fn default() -> Self {
        Self {
            columns: 1,
            spacing: 6.0,
            scrollbar: false,
            empty_text: "No matches".to_string(),
            empty_text_color: "#565f89".to_string(),
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
            background: "#414868".to_string(),
            text_color: "#c0caf5".to_string(),
            description_color: Some("#a9b1d6".to_string()),
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
            background: "#33374a".to_string(),
            text_color: "#c0caf5".to_string(),
            description_color: Some("#a9b1d6".to_string()),
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
            padding: vec![8.0, 12.0],
            corner_radius: 8.0,
            background: "transparent".to_string(),
            text_color: "#c0caf5".to_string(),
            description_color: Some("#565f89".to_string()),
            show_icons: true,
            icon_size: 24.0,
            layout: None,
            selected: SelectedState::default(),
            hover: Some(HoverState::default()),
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
    /// Named colour aliases: `$key` in any colour field is replaced with
    /// the corresponding hex value from this map.
    #[serde(default)]
    pub colors: HashMap<String, String>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            name: "Tokyo Night".to_string(),
            author: Some("AeroFi".to_string()),
            font: FontConfig::default(),
            window: WindowConfig::default(),
            mainbox: ContainerConfig::default(),
            banner: None,
            inputbar: InputBarConfig::default(),
            listview: ListViewConfig::default(),
            element: ElementConfig::default(),
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

    let Some(config_dir) = dirs::config_dir() else {
        eprintln!("aerofi: warning: cannot determine config dir, using default theme");
        return ThemeConfig::default();
    };

    let path = config_dir
        .join("aerofi")
        .join("themes")
        .join(format!("{theme_name}.toml"));

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
    fn default_theme_has_tokyo_night_palette() {
        let t = ThemeConfig::default();
        assert_eq!(t.name, "Tokyo Night");
        assert_eq!(t.window.background, "#1a1b26");
        assert_eq!(t.inputbar.background, "#24283b");
        assert_eq!(t.inputbar.text_color, "#c0caf5");
        assert_eq!(t.element.selected.background, "#414868");
    }

    #[test]
    fn default_mainbox_children_are_inputbar_and_listview() {
        let t = ThemeConfig::default();
        assert_eq!(t.mainbox.children.len(), 2);
        assert_eq!(t.mainbox.children[0], Widget::InputBar);
        assert_eq!(t.mainbox.children[1], Widget::ListView);
    }

    #[test]
    fn load_theme_default_returns_builtin() {
        let t = load_theme("default");
        assert_eq!(t.name, "Tokyo Night");
    }

    #[test]
    fn load_theme_missing_file_returns_builtin() {
        let t = load_theme("nonexistent_theme_12345");
        assert_eq!(t.name, "Tokyo Night");
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
}
