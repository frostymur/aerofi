# Bundled Fonts

Drop font files here and aerofi registers them with its text system at
startup, so a theme's `[font] family` can reference fonts that are **not**
installed on your machine. This folder is the template for
`~/.config/aerofi/fonts/` — copy your fonts there once set up.

## How to use

1. Copy your font file(s) into `~/.config/aerofi/fonts/`.
   aerofi reads `.ttf`, `.otf`, and `.ttc` files.
2. Reference the font by its **real family name** in your theme:
   ```toml
   [font]
   family = "Inter"
   ```
3. Restart aerofi (you'll see `aerofi: registered N bundled font(s)` on
   startup).

> **This folder is intentionally empty in the repo.** We don't ship font
> binaries to keep the repo light and avoid licensing problems. Use any
> system font (the default `SF Pro Text` ships with macOS) or add your own
> here. If you bundle a font, prefer the SIL Open Font License (OFL) — e.g.
> Inter, JetBrains Mono, Fira Code, Source Sans — and keep its `LICENSE` file
> alongside it.
