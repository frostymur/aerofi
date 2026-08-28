//! Typed configuration defaults for the launcher.
//!
//! v0.1 has no config file: everything is a hard-coded default. The types
//! exist so the window setup and the UI pull their values from one place.
//! TODO(v0.2): load a `Config` from `config.toml` (the `serde` and `toml`
//! dependencies are already in `Cargo.toml`).

/// Catppuccin-Mocha palette for the dark launcher surface.
#[derive(Debug, Clone, Copy)]
pub struct ThemeColors {
    pub bg: u32,
    pub input_bg: u32,
    pub selection: u32,
    pub text: u32,
    pub text_muted: u32,
    pub dim: u32,
    pub accent: u32,
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self {
            bg: 0x1e1e2e,
            input_bg: 0x313244,
            selection: 0x45475a,
            text: 0xcdd6f4,
            text_muted: 0xa6adc8,
            dim: 0x6c7086,
            accent: 0x89b4fa,
        }
    }
}

/// Launcher window geometry (points).
#[derive(Debug, Clone, Copy)]
pub struct WindowConfig {
    pub width: f32,
    pub height: f32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 680.0,
            height: 420.0,
        }
    }
}

/// Top-level configuration.
#[derive(Debug, Clone, Copy, Default)]
pub struct Config {
    pub window: WindowConfig,
    pub theme: ThemeColors,
}
