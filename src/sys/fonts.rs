//! Register user-bundled fonts with the text system so a theme's `[font]
//! family` can reference fonts that aren't installed system-wide.
//!
//! Any `.ttf`, `.otf`, or `.ttc` file placed in `~/.config/aerofi/fonts/`
//! is loaded at startup via `TextSystem::add_fonts`; the family name is read
//! from each file, so the theme just references it by its real name.

use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

/// File extensions accepted as bundleable fonts.
const FONT_EXTENSIONS: &[&str] = &["ttf", "otf", "ttc"];

/// Directory scanned for bundled fonts: `~/.config/aerofi/fonts/`.
fn bundled_fonts_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".config").join("aerofi").join("fonts"))
}

/// True when `path` has a recognised font extension (case-insensitive).
pub fn is_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| FONT_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Load every font file in the bundle directory into the text system.
///
/// Returns the number of fonts successfully registered. A missing directory
/// is a silent no-op; unreadable or unparseable files are skipped with a
/// warning so a single bad file never blocks startup.
pub fn load_custom_fonts(cx: &mut gpui::App) -> usize {
    let Some(dir) = bundled_fonts_dir() else {
        return 0;
    };
    if !dir.is_dir() {
        return 0;
    }
    let Ok(entries) = fs::read_dir(&dir) else {
        return 0;
    };

    let mut loaded = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_font_file(&path) {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            eprintln!("aerofi: could not read font {path:?}");
            continue;
        };
        match cx.text_system().add_fonts(vec![Cow::Owned(bytes)]) {
            Ok(()) => loaded += 1,
            Err(e) => eprintln!("aerofi: could not register font {path:?}: {e}"),
        }
    }
    loaded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_extensions_match_case_insensitively() {
        for ext in ["ttf", "TTF", "otf", "OTF", "ttc"] {
            let name = format!("font.{ext}");
            assert!(is_font_file(Path::new(&name)), "should accept {name}");
        }
    }

    #[test]
    fn non_font_extensions_are_rejected() {
        for ext in ["txt", "md", "so", "dylib"] {
            let name = format!("font.{ext}");
            assert!(!is_font_file(Path::new(&name)), "should reject {name}");
        }
        assert!(!is_font_file(Path::new("no-extension")));
    }

    #[test]
    fn bundled_dir_points_at_config_fonts() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        assert_eq!(
            bundled_fonts_dir().as_deref(),
            Some(home.join(".config").join("aerofi").join("fonts").as_path())
        );
    }
}
