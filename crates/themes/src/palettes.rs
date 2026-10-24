//! Upstream palettes for the built-in themes. Colors come from each
//! project's published palette; roles are worked out in `derive`.

use crate::Palette;
use crate::color::h;

pub(crate) struct Entry {
    pub id: &'static str,
    pub label: &'static str,
    pub palette: Palette,
}

/// Order of `c`: bg, fg, muted, border, accent, red, green, yellow,
/// viz low, viz mid, viz high.
const fn e(id: &'static str, label: &'static str, dark: bool, c: [&str; 11]) -> Entry {
    Entry {
        id,
        label,
        palette: Palette {
            dark,
            bg: h(c[0]),
            fg: h(c[1]),
            muted: h(c[2]),
            border: h(c[3]),
            accent: h(c[4]),
            red: h(c[5]),
            green: h(c[6]),
            yellow: h(c[7]),
            viz: [h(c[8]), h(c[9]), h(c[10])],
        },
    }
}

const DARK: bool = true;
const LIGHT: bool = false;

#[rustfmt::skip]
pub(crate) const ALL: [Entry; 22] = [
    // Catppuccin: base, text, overlay1, surface1, blue, red, green, yellow, blue, mauve, pink.
    e("catppuccin-mocha", "Catppuccin Mocha", DARK,
      ["#1e1e2e", "#cdd6f4", "#7f849c", "#45475a", "#89b4fa", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#cba6f7", "#f5c2e7"]),
    e("catppuccin-macchiato", "Catppuccin Macchiato", DARK,
      ["#24273a", "#cad3f5", "#8087a2", "#494d64", "#8aadf4", "#ed8796", "#a6da95", "#eed49f", "#8aadf4", "#c6a0f6", "#f5bde6"]),
    e("catppuccin-frappe", "Catppuccin Frappé", DARK,
      ["#303446", "#c6d0f5", "#838ba7", "#51576d", "#8caaee", "#e78284", "#a6d189", "#e5c890", "#8caaee", "#ca9ee6", "#f4b8e4"]),
    e("catppuccin-latte", "Catppuccin Latte", LIGHT,
      ["#eff1f5", "#4c4f69", "#8c8fa1", "#bcc0cc", "#1e66f5", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#8839ef", "#ea76cb"]),
    // Tokyo Night: bg, fg, comment, terminal_black/fg_gutter, blue, red, green, yellow, blue, magenta, red.
    e("tokyo-night", "Tokyo Night", DARK,
      ["#1a1b26", "#c0caf5", "#565f89", "#414868", "#7aa2f7", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#f7768e"]),
    e("tokyo-night-storm", "Tokyo Night Storm", DARK,
      ["#24283b", "#c0caf5", "#565f89", "#414868", "#7aa2f7", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#f7768e"]),
    e("tokyo-night-day", "Tokyo Night Day", LIGHT,
      ["#e1e2e7", "#3760bf", "#848cb5", "#a8aecb", "#2e7de9", "#f52a65", "#587539", "#8c6c3e", "#2e7de9", "#9854f1", "#f52a65"]),
    // Rosé Pine: base, text, muted, highlight med, iris, love, foam, gold, foam, iris, rose.
    e("rose-pine", "Rosé Pine", DARK,
      ["#191724", "#e0def4", "#6e6a86", "#403d52", "#c4a7e7", "#eb6f92", "#9ccfd8", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba"]),
    e("rose-pine-moon", "Rosé Pine Moon", DARK,
      ["#232136", "#e0def4", "#6e6a86", "#44415a", "#c4a7e7", "#eb6f92", "#9ccfd8", "#f6c177", "#9ccfd8", "#c4a7e7", "#ea9a97"]),
    e("rose-pine-dawn", "Rosé Pine Dawn", LIGHT,
      ["#faf4ed", "#575279", "#9893a5", "#dfdad9", "#907aa9", "#b4637a", "#56949f", "#ea9d34", "#56949f", "#907aa9", "#d7827e"]),
    // Gruvbox: bg, fg, gray, bg2, orange, red, green, yellow, blue, purple, red.
    e("gruvbox-dark", "Gruvbox Dark", DARK,
      ["#282828", "#ebdbb2", "#928374", "#504945", "#fe8019", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#fb4934"]),
    e("gruvbox-light", "Gruvbox Light", LIGHT,
      ["#fbf1c7", "#3c3836", "#928374", "#d5c4a1", "#af3a03", "#9d0006", "#79740e", "#b57614", "#076678", "#8f3f71", "#9d0006"]),
    // Nord: nord0, nord4, comment, nord3, nord8, nord11, nord14, nord13, nord9, nord15, nord11.
    e("nord", "Nord", DARK,
      ["#2e3440", "#d8dee9", "#616e88", "#4c566a", "#88c0d0", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#bf616a"]),
    // Dracula: background, foreground, comment, current line, purple, red, green, yellow, cyan, purple, pink.
    e("dracula", "Dracula", DARK,
      ["#282a36", "#f8f8f2", "#6272a4", "#44475a", "#bd93f9", "#ff5555", "#50fa7b", "#f1fa8c", "#8be9fd", "#bd93f9", "#ff79c6"]),
    // One Dark (Atom): bg, fg, comment, gutter, blue, red, green, yellow, blue, purple, red.
    e("one-dark", "One Dark", DARK,
      ["#282c34", "#abb2bf", "#5c6370", "#3e4451", "#61afef", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#e06c75"]),
    // Solarized: base03/3, base0/00, base01/1, base02/2, blue, red, green, yellow, blue, violet, magenta.
    e("solarized-dark", "Solarized Dark", DARK,
      ["#002b36", "#839496", "#586e75", "#073642", "#268bd2", "#dc322f", "#859900", "#b58900", "#268bd2", "#6c71c4", "#d33682"]),
    e("solarized-light", "Solarized Light", LIGHT,
      ["#fdf6e3", "#657b83", "#93a1a1", "#eee8d5", "#268bd2", "#dc322f", "#859900", "#b58900", "#268bd2", "#6c71c4", "#d33682"]),
    // Kanagawa Wave: sumiInk3, fujiWhite, fujiGray, sumiInk6, crystalBlue, samuraiRed, springGreen,
    // carpYellow, crystalBlue, oniViolet, sakuraPink.
    e("kanagawa-wave", "Kanagawa Wave", DARK,
      ["#1f1f28", "#dcd7ba", "#727169", "#54546d", "#7e9cd8", "#e82424", "#98bb6c", "#e6c384", "#7e9cd8", "#957fb8", "#d27e99"]),
    // Everforest Dark (medium): bg0, fg, grey1, bg4, green, red, green, yellow, blue, purple, red.
    e("everforest-dark", "Everforest Dark", DARK,
      ["#2d353b", "#d3c6aa", "#859289", "#4f585e", "#a7c080", "#e67e80", "#a7c080", "#dbbc7f", "#7fbbb3", "#d699b6", "#e67e80"]),
    // Ayu Mirage: surface base, editor fg, ui fg, ui fg, accent, error, vcs added, keyword orange,
    // tag indigo, purple, markup red.
    e("ayu-mirage", "Ayu Mirage", DARK,
      ["#1f2430", "#cccac2", "#707a8c", "#707a8c", "#ffcc66", "#ff6666", "#87d96c", "#ffa659", "#5ccfe6", "#dfbfff", "#f28779"]),
    // GitHub (Primer): canvas, fg default, fg muted, border, accent, danger, success, attention,
    // accent, done, sponsors.
    e("github-dark", "GitHub Dark", DARK,
      ["#0d1117", "#c9d1d9", "#8b949e", "#30363d", "#58a6ff", "#f85149", "#3fb950", "#d29922", "#58a6ff", "#a371f7", "#db61a2"]),
    e("github-light", "GitHub Light", LIGHT,
      ["#ffffff", "#24292f", "#57606a", "#d0d7de", "#0969da", "#cf222e", "#1a7f37", "#9a6700", "#0969da", "#8250df", "#bf3989"]),
];
