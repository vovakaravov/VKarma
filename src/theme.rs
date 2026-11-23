use gpui_kit::component::highlighter::{GrammarConfig, HighlightTheme, LanguageRegistry};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, px, rgb, rgba};
use std::sync::Arc;

/// Amethyst Obsidian dark palette tailored to VKarma's amethyst & gold nib icon.
pub mod kd {
    pub const BG_DEEP: u32 = 0x0e0b14;
    pub const BG_MAIN: u32 = 0x15111e;
    pub const BG_SIDEBAR: u32 = 0x1b1626;
    pub const BG_PANEL: u32 = 0x241d33;
    pub const BG_BTN: u32 = 0x312845;
    pub const BG_BTN_HOVER: u32 = 0x3e3357;
    pub const BG_BTN_ACTIVE: u32 = 0x282039;

    pub const BORDER: u32 = 0x3a3052;
    pub const BORDER_SUBTLE: u32 = 0x272038;

    pub const FG: u32 = 0xe4dff0;
    pub const FG_MUTED: u32 = 0x776e8c;
    pub const FG_SUBTLE: u32 = 0xa49bb8;

    pub const ACCENT: u32 = 0x9d7ce0;
    pub const ACCENT_HOVER: u32 = 0xb093eb;
    pub const ACCENT_ACTIVE: u32 = 0x8563cc;

    pub const BLUE: u32 = 0x7ca6d8;
    pub const LINK: u32 = 0xb194ed;
    pub const GREEN: u32 = 0x83b88c;
    pub const YELLOW: u32 = 0xe0be6f;
    pub const RED: u32 = 0xd6747e;
    pub const VIOLET: u32 = 0xb698f0;
    pub const ORANGE: u32 = 0xd49677;
    pub const AQUA: u32 = 0x7fb5b0;
    pub const PINK: u32 = 0xc98ab8;
}

#[inline]
pub fn hex_color(hex: u32) -> Hsla {
    rgb(hex).into()
}

#[inline]
pub fn hex_alpha(hex_rgba: u32) -> Hsla {
    rgba(hex_rgba).into()
}

#[inline]
pub fn accent_alpha(alpha: u8) -> Hsla {
    rgba((kd::ACCENT << 8) | (alpha as u32)).into()
}

const KANAGAWA_DRAGON_HIGHLIGHT_JSON: &str = r##"{
  "name": "VKarma Amethyst",
  "appearance": "dark",
  "style": {
    "editor.foreground": "#e4dff0",
    "editor.background": "#15111e",
    "editor.active_line.background": "#201a2e",
    "editor.line_number": "#776e8c",
    "editor.active_line_number": "#e0be6f",
    "editor.invisible": "#776e8c66",
    "editor.gutter.background": "#15111e",
    "error": "#d6747e",
    "error.background": "#3b222c",
    "error.border": "#d6747e",
    "warning": "#e0be6f",
    "warning.background": "#352e24",
    "warning.border": "#e0be6f",
    "info": "#7ca6d8",
    "info.background": "#22293b",
    "info.border": "#7ca6d8",
    "success": "#83b88c",
    "success.background": "#212e28",
    "success.border": "#83b88c",
    "hint": "#b698f0",
    "hint.background": "#28213b",
    "hint.border": "#b698f0",
    "created": "#83b88c",
    "modified": "#e0be6f",
    "deleted": "#d6747e",
    "syntax": {
      "attribute": { "color": "#d49677" },
      "boolean": { "color": "#d49677", "font_weight": 600 },
      "comment": { "color": "#776e8c", "font_style": "italic" },
      "comment.doc": { "color": "#a49bb8", "font_style": "italic" },
      "constant": { "color": "#d49677" },
      "constructor": { "color": "#7fb5b0" },
      "embedded": { "color": "#e4dff0" },
      "emphasis": { "color": "#e0be6f", "font_style": "italic" },
      "emphasis.strong": { "color": "#e4dff0", "font_weight": 700 },
      "enum": { "color": "#e0be6f" },
      "function": { "color": "#7ca6d8" },
      "hint": { "color": "#b698f0" },
      "keyword": { "color": "#b698f0" },
      "label": { "color": "#c98ab8" },
      "link_text": { "color": "#b194ed" },
      "link_uri": { "color": "#7fb5b0", "font_style": "italic" },
      "number": { "color": "#c98ab8" },
      "operator": { "color": "#d6747e" },
      "predictive": { "color": "#776e8c" },
      "preproc": { "color": "#d6747e" },
      "primary": { "color": "#e4dff0" },
      "property": { "color": "#e0be6f" },
      "punctuation": { "color": "#a49bb8" },
      "punctuation.bracket": { "color": "#a49bb8" },
      "punctuation.delimiter": { "color": "#a49bb8" },
      "punctuation.list_marker": { "color": "#d6747e", "font_weight": 600 },
      "punctuation.special": { "color": "#d6747e" },
      "string": { "color": "#83b88c" },
      "string.escape": { "color": "#7fb5b0" },
      "string.regex": { "color": "#e0be6f" },
      "string.special": { "color": "#d49677" },
      "string.special.symbol": { "color": "#d49677" },
      "tag": { "color": "#7fb5b0" },
      "tag.doctype": { "color": "#b698f0" },
      "text.code.span": { "color": "#83b88c" },
      "text.literal": { "color": "#83b88c" },
      "title": { "color": "#b093eb", "font_weight": 700 },
      "type": { "color": "#e0be6f" },
      "variable": { "color": "#e4dff0" },
      "variable.special": { "color": "#d49677" },
      "variant": { "color": "#c98ab8" }
    }
  }
}"##;

/// Tree-sitter highlight query for Typst mapped to gpui-component HighlightTheme capture names.
pub const TYPST_HIGHLIGHTS_QUERY: &str = r##"
(call
  item: (ident) @function)
(call
  item: (field field: (ident) @function))
(tagged field: (ident) @tag)
(field field: (ident) @property)
(comment) @comment

(let "let" @keyword)
(branch ["if" "else"] @keyword)
(while "while" @keyword)
(for ["for" "in"] @keyword)
(import "import" @keyword)
(as "as" @keyword)
(include "include" @keyword)
(show "show" @keyword)
(set "set" @keyword)
(return "return" @keyword)
(flow ["break" "continue"] @keyword)

(in ["in" "not"] @keyword)
(context "context" @keyword)
(and "and" @keyword)
(or "or" @keyword)
(not "not" @keyword)

(sign ["+" "-"] @operator)
(add "+" @operator)
(sub "-" @operator)
(mul "*" @operator)
(div "/" @operator)
(cmp ["==" "<=" ">=" "!=" "<" ">"] @operator)
(fraction "/" @operator)
(fac "!" @operator)
(attach ["^" "_"] @operator)
(wildcard) @operator
(math "$" @operator)
"#" @punctuation.special
"end" @keyword

(raw_blck "```" @punctuation.delimiter) @text.literal
(raw_span "`" @punctuation.delimiter) @text.code.span
(raw_blck lang: (ident) @tag)
(label) @tag
(ref) @link_text
(number) @number
(string) @string
(content ["[" "]"] @punctuation.bracket)
(bool) @boolean
(none) @constant
(auto) @constant
(ident) @variable

(item "-" @punctuation.list_marker)
(term ["/" ":"] @punctuation.list_marker)
(heading "=" @punctuation.special) @title
(heading "==" @punctuation.special) @title
(heading "===" @punctuation.special) @title
(heading "====" @punctuation.special) @title
(heading "=====" @punctuation.special) @title
(heading "======" @punctuation.special) @title
(url) @link_uri
(emph) @emphasis
(strong) @emphasis.strong
(symbol) @constant
(shorthand) @constant
(quote) @string.special
(align) @operator
(letter) @constant
(linebreak) @string.escape

(escape) @string.escape
["(" ")" "{" "}"] @punctuation.bracket
["," ";" ".." ":" "sep"] @punctuation.delimiter
"assign" @operator
(field "." @punctuation.delimiter)
"##;

/// Registers extra Tree-sitter languages not bundled directly in gpui-component.
pub fn register_custom_languages() {
    let typst_lang = tree_sitter::Language::new(codebook_tree_sitter_typst::LANGUAGE);
    let config = GrammarConfig::new("typst", typst_lang, vec![], TYPST_HIGHLIGHTS_QUERY, "", "");
    LanguageRegistry::singleton().register("typst", &config);
}

/// Applies the Amethyst Obsidian dark theme globally to GPUI Component.
pub fn apply_kanagawa_dragon_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);

    let highlight_theme: Arc<HighlightTheme> = Arc::new(
        serde_json::from_str(KANAGAWA_DRAGON_HIGHLIGHT_JSON)
            .expect("VKarma highlight JSON must parse"),
    );

    Theme::update(cx, |theme| {
        theme.radius = px(6.);
        theme.radius_lg = px(8.);
        theme.font_size = px(13.5);
        theme.mono_font_size = px(13.);
        theme.highlight_theme = highlight_theme.clone();

        let c = &mut theme.colors;
        c.background = hex_color(kd::BG_MAIN);
        c.foreground = hex_color(kd::FG);
        c.border = hex_color(kd::BORDER);
        c.window_border = hex_color(kd::BORDER);
        c.muted = hex_color(kd::BG_PANEL);
        c.muted_foreground = hex_color(kd::FG_MUTED);
        c.accent = hex_color(kd::BG_PANEL);
        c.accent_foreground = hex_color(kd::FG);
        c.overlay = hex_alpha(0x0e0b14cc);

        c.popover = hex_color(kd::BG_PANEL);
        c.popover_foreground = hex_color(kd::FG);
        c.group_box = hex_color(kd::BG_SIDEBAR);
        c.group_box_foreground = hex_color(kd::FG);

        c.button = hex_color(kd::BG_BTN);
        c.button_hover = hex_color(kd::BG_BTN_HOVER);
        c.button_active = hex_color(kd::BG_BTN_ACTIVE);
        c.button_foreground = hex_color(kd::FG);

        c.button_secondary = hex_color(kd::BG_PANEL);
        c.button_secondary_hover = hex_color(kd::BG_BTN);
        c.button_secondary_active = hex_color(kd::BG_BTN_ACTIVE);
        c.button_secondary_foreground = hex_color(kd::FG);

        c.primary = hex_color(kd::ACCENT);
        c.primary_hover = hex_color(kd::ACCENT_HOVER);
        c.primary_active = hex_color(kd::ACCENT_ACTIVE);
        c.primary_foreground = hex_color(kd::BG_DEEP);

        c.button_primary = hex_color(kd::ACCENT);
        c.button_primary_hover = hex_color(kd::ACCENT_HOVER);
        c.button_primary_active = hex_color(kd::ACCENT_ACTIVE);
        c.button_primary_foreground = hex_color(kd::BG_DEEP);

        c.secondary = hex_color(kd::BG_BTN);
        c.secondary_hover = hex_color(kd::BG_BTN_HOVER);
        c.secondary_active = hex_color(kd::BG_BTN_ACTIVE);
        c.secondary_foreground = hex_color(kd::FG);

        c.danger = hex_color(kd::RED);
        c.danger_hover = hex_color(0xe28a93);
        c.danger_active = hex_color(0xbe5e68);
        c.danger_foreground = hex_color(kd::BG_DEEP);

        c.button_danger = hex_color(kd::RED);
        c.button_danger_hover = hex_color(0xe28a93);
        c.button_danger_active = hex_color(0xbe5e68);
        c.button_danger_foreground = hex_color(kd::BG_DEEP);

        c.success = hex_color(kd::GREEN);
        c.success_hover = hex_color(0x98c9a0);
        c.success_active = hex_color(0x6da176);
        c.success_foreground = hex_color(kd::BG_DEEP);

        c.button_success = hex_color(kd::GREEN);
        c.button_success_hover = hex_color(0x98c9a0);
        c.button_success_active = hex_color(0x6da176);
        c.button_success_foreground = hex_color(kd::BG_DEEP);

        c.info = hex_color(kd::BLUE);
        c.info_hover = hex_color(0x94b9e3);
        c.info_active = hex_color(0x6791c2);
        c.info_foreground = hex_color(kd::BG_DEEP);

        c.warning = hex_color(kd::YELLOW);
        c.warning_hover = hex_color(0xeacc85);
        c.warning_active = hex_color(0xc7a558);
        c.warning_foreground = hex_color(kd::BG_DEEP);

        c.sidebar = hex_color(kd::BG_SIDEBAR);
        c.sidebar_border = hex_color(kd::BORDER_SUBTLE);
        c.sidebar_foreground = hex_color(kd::FG);
        c.sidebar_accent = hex_color(kd::BG_PANEL);
        c.sidebar_accent_foreground = hex_color(kd::FG);
        c.sidebar_primary = hex_color(kd::ACCENT);
        c.sidebar_primary_foreground = hex_color(kd::BG_DEEP);

        c.title_bar = hex_color(kd::BG_DEEP);
        c.title_bar_border = hex_color(kd::BORDER_SUBTLE);
        c.status_bar = hex_color(kd::BG_DEEP);
        c.status_bar_border = hex_color(kd::BORDER_SUBTLE);

        c.input = hex_color(kd::BORDER);
        c.ring = hex_color(kd::ACCENT);
        c.caret = hex_color(kd::ACCENT);
        c.selection = accent_alpha(0x45);

        c.link = hex_color(kd::LINK);
        c.link_hover = hex_color(0xc7b0f5);
        c.link_active = hex_color(kd::ACCENT);

        c.list = hex_color(kd::BG_SIDEBAR);
        c.list_even = hex_color(kd::BG_SIDEBAR);
        c.list_head = hex_color(kd::BG_PANEL);
        c.list_hover = hex_color(kd::BG_PANEL);
        c.list_active = accent_alpha(0x2e);
        c.list_active_border = hex_color(kd::ACCENT);
        c.list_highlight_foreground = hex_color(kd::YELLOW);

        c.tab_bar = hex_color(kd::BG_DEEP);
        c.tab_bar_segmented = hex_color(kd::BG_PANEL);
        c.tab = gpui_kit::transparent_black();
        c.tab_foreground = hex_color(kd::FG_MUTED);
        c.tab_active = hex_color(kd::BG_MAIN);
        c.tab_active_foreground = hex_color(kd::FG);

        c.table = hex_color(kd::BG_MAIN);
        c.table_even = hex_color(kd::BG_SIDEBAR);
        c.table_head = hex_color(kd::BG_PANEL);
        c.table_head_foreground = hex_color(kd::YELLOW);
        c.table_foot = hex_color(kd::BG_PANEL);
        c.table_foot_foreground = hex_color(kd::FG_MUTED);
        c.table_hover = hex_color(0x201a2d);
        c.table_active = accent_alpha(0x26);
        c.table_active_border = hex_color(kd::ACCENT);
        c.table_row_border = hex_color(kd::BORDER_SUBTLE);

        c.scrollbar = hex_alpha(0x00000000);
        c.scrollbar_thumb = hex_alpha(0x776e8c66);
        c.scrollbar_thumb_hover = hex_alpha(0xa49bb8aa);

        c.red = hex_color(kd::RED);
        c.green = hex_color(kd::GREEN);
        c.blue = hex_color(kd::BLUE);
        c.yellow = hex_color(kd::YELLOW);
        c.magenta = hex_color(kd::PINK);
        c.cyan = hex_color(kd::AQUA);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kanagawa_highlight_theme_parses_cleanly() {
        let parsed: HighlightTheme = serde_json::from_str(KANAGAWA_DRAGON_HIGHLIGHT_JSON)
            .expect("HighlightTheme JSON should deserialize cleanly");
        assert_eq!(parsed.name, "VKarma Amethyst");
        assert!(parsed.style.syntax.keyword.is_some());
        assert!(parsed.style.syntax.title.is_some());
    }

    #[test]
    fn typst_tree_sitter_query_compiles() {
        let lang = tree_sitter::Language::new(codebook_tree_sitter_typst::LANGUAGE);
        let query = tree_sitter::Query::new(&lang, TYPST_HIGHLIGHTS_QUERY);
        assert!(
            query.is_ok(),
            "Typst highlight query failed: {:?}",
            query.err()
        );
    }
}
