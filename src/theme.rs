use gpui_kit::component::highlighter::{GrammarConfig, HighlightTheme, LanguageRegistry};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, px, rgb, rgba};
use std::sync::Arc;

/// Exact KDE Plasma Kanagawa Dragon palette (`KanagawaDragon.colors`).
pub mod kd {
    pub const BG_DEEP: u32 = 0x0d0c0c;
    pub const BG_MAIN: u32 = 0x181616;
    pub const BG_SIDEBAR: u32 = 0x1d1c19;
    pub const BG_PANEL: u32 = 0x282727;
    pub const BG_BTN: u32 = 0x393836;
    pub const BG_BTN_HOVER: u32 = 0x464441;
    pub const BG_BTN_ACTIVE: u32 = 0x31302e;

    pub const BORDER: u32 = 0x393836;
    pub const BORDER_SUBTLE: u32 = 0x2a2826;

    pub const FG: u32 = 0xdcd7ba;
    pub const FG_MUTED: u32 = 0x727169;
    pub const FG_SUBTLE: u32 = 0x9e9b93;

    pub const BLUE: u32 = 0x8ba4b0;
    pub const LINK: u32 = 0x8fa1b2;
    pub const GREEN: u32 = 0x87a987;
    pub const YELLOW: u32 = 0xc4b28a;
    pub const RED: u32 = 0xc4746e;
    pub const VIOLET: u32 = 0x8992a7;
    pub const ORANGE: u32 = 0xb6927b;
    pub const AQUA: u32 = 0x8ea4a2;
    pub const PINK: u32 = 0xa292a3;
}

#[inline]
pub fn hex_color(hex: u32) -> Hsla {
    rgb(hex).into()
}

#[inline]
pub fn hex_alpha(hex_rgba: u32) -> Hsla {
    rgba(hex_rgba).into()
}

const KANAGAWA_DRAGON_HIGHLIGHT_JSON: &str = r##"{
  "name": "Kanagawa Dragon",
  "appearance": "dark",
  "style": {
    "editor.foreground": "#dcd7ba",
    "editor.background": "#181616",
    "editor.active_line.background": "#22201e",
    "editor.line_number": "#727169",
    "editor.active_line_number": "#c4b28a",
    "editor.invisible": "#72716966",
    "editor.gutter.background": "#181616",
    "error": "#c4746e",
    "error.background": "#3b2322",
    "error.border": "#c4746e",
    "warning": "#c4b28a",
    "warning.background": "#363022",
    "warning.border": "#c4b28a",
    "info": "#8ba4b0",
    "info.background": "#242c30",
    "info.border": "#8ba4b0",
    "success": "#87a987",
    "success.background": "#222d24",
    "success.border": "#87a987",
    "hint": "#8992a7",
    "hint.background": "#24262e",
    "hint.border": "#8992a7",
    "created": "#87a987",
    "modified": "#c4b28a",
    "deleted": "#c4746e",
    "syntax": {
      "attribute": { "color": "#b6927b" },
      "boolean": { "color": "#b6927b", "font_weight": 600 },
      "comment": { "color": "#727169", "font_style": "italic" },
      "comment.doc": { "color": "#8992a7", "font_style": "italic" },
      "constant": { "color": "#b6927b" },
      "constructor": { "color": "#8ea4a2" },
      "embedded": { "color": "#dcd7ba" },
      "emphasis": { "color": "#c4b28a", "font_style": "italic" },
      "emphasis.strong": { "color": "#dcd7ba", "font_weight": 700 },
      "enum": { "color": "#c4b28a" },
      "function": { "color": "#8ba4b0" },
      "hint": { "color": "#8992a7" },
      "keyword": { "color": "#8992a7" },
      "label": { "color": "#a292a3" },
      "link_text": { "color": "#8fa1b2" },
      "link_uri": { "color": "#8ea4a2", "font_style": "italic" },
      "number": { "color": "#a292a3" },
      "operator": { "color": "#c4746e" },
      "predictive": { "color": "#727169" },
      "preproc": { "color": "#c4746e" },
      "primary": { "color": "#dcd7ba" },
      "property": { "color": "#c4b28a" },
      "punctuation": { "color": "#9e9b93" },
      "punctuation.bracket": { "color": "#9e9b93" },
      "punctuation.delimiter": { "color": "#9e9b93" },
      "punctuation.list_marker": { "color": "#c4746e", "font_weight": 600 },
      "punctuation.special": { "color": "#c4746e" },
      "string": { "color": "#87a987" },
      "string.escape": { "color": "#8ea4a2" },
      "string.regex": { "color": "#c4b28a" },
      "string.special": { "color": "#b6927b" },
      "string.special.symbol": { "color": "#b6927b" },
      "tag": { "color": "#8ea4a2" },
      "tag.doctype": { "color": "#8992a7" },
      "text.code.span": { "color": "#87a987" },
      "text.literal": { "color": "#87a987" },
      "title": { "color": "#8ba4b0", "font_weight": 700 },
      "type": { "color": "#c4b28a" },
      "variable": { "color": "#dcd7ba" },
      "variable.special": { "color": "#b6927b" },
      "variant": { "color": "#a292a3" }
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

/// Applies the user's KDE Plasma Kanagawa Dragon theme globally to GPUI Component.
pub fn apply_kanagawa_dragon_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);

    let highlight_theme: Arc<HighlightTheme> = Arc::new(
        serde_json::from_str(KANAGAWA_DRAGON_HIGHLIGHT_JSON)
            .expect("Kanagawa Dragon highlight JSON must parse"),
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
        c.overlay = hex_alpha(0x0d0c0ccc);

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

        c.primary = hex_color(kd::BLUE);
        c.primary_hover = hex_color(0x9fb8c4);
        c.primary_active = hex_color(0x7a929e);
        c.primary_foreground = hex_color(kd::BG_MAIN);

        c.button_primary = hex_color(kd::BLUE);
        c.button_primary_hover = hex_color(0x9fb8c4);
        c.button_primary_active = hex_color(0x7a929e);
        c.button_primary_foreground = hex_color(kd::BG_MAIN);

        c.secondary = hex_color(kd::BG_BTN);
        c.secondary_hover = hex_color(kd::BG_BTN_HOVER);
        c.secondary_active = hex_color(kd::BG_BTN_ACTIVE);
        c.secondary_foreground = hex_color(kd::FG);

        c.danger = hex_color(kd::RED);
        c.danger_hover = hex_color(0xd4847e);
        c.danger_active = hex_color(0xb0635e);
        c.danger_foreground = hex_color(kd::BG_MAIN);

        c.button_danger = hex_color(kd::RED);
        c.button_danger_hover = hex_color(0xd4847e);
        c.button_danger_active = hex_color(0xb0635e);
        c.button_danger_foreground = hex_color(kd::BG_MAIN);

        c.success = hex_color(kd::GREEN);
        c.success_hover = hex_color(0x97ba97);
        c.success_active = hex_color(0x759675);
        c.success_foreground = hex_color(kd::BG_MAIN);

        c.button_success = hex_color(kd::GREEN);
        c.button_success_hover = hex_color(0x97ba97);
        c.button_success_active = hex_color(0x759675);
        c.button_success_foreground = hex_color(kd::BG_MAIN);

        c.info = hex_color(kd::BLUE);
        c.info_hover = hex_color(0x9fb8c4);
        c.info_active = hex_color(0x7a929e);
        c.info_foreground = hex_color(kd::BG_MAIN);

        c.warning = hex_color(kd::YELLOW);
        c.warning_hover = hex_color(0xd2c19a);
        c.warning_active = hex_color(0xb2a07a);
        c.warning_foreground = hex_color(kd::BG_MAIN);

        c.sidebar = hex_color(kd::BG_SIDEBAR);
        c.sidebar_border = hex_color(kd::BORDER_SUBTLE);
        c.sidebar_foreground = hex_color(kd::FG);
        c.sidebar_accent = hex_color(kd::BG_PANEL);
        c.sidebar_accent_foreground = hex_color(kd::FG);
        c.sidebar_primary = hex_color(kd::BLUE);
        c.sidebar_primary_foreground = hex_color(kd::BG_MAIN);

        c.title_bar = hex_color(kd::BG_DEEP);
        c.title_bar_border = hex_color(kd::BORDER_SUBTLE);
        c.status_bar = hex_color(kd::BG_DEEP);
        c.status_bar_border = hex_color(kd::BORDER_SUBTLE);

        c.input = hex_color(kd::BORDER);
        c.ring = hex_color(kd::BLUE);
        c.caret = hex_color(kd::BLUE);
        c.selection = hex_alpha(0x8ba4b040);

        c.link = hex_color(kd::LINK);
        c.link_hover = hex_color(0xa9bac9);
        c.link_active = hex_color(kd::BLUE);

        c.list = hex_color(kd::BG_SIDEBAR);
        c.list_even = hex_color(kd::BG_SIDEBAR);
        c.list_head = hex_color(kd::BG_PANEL);
        c.list_hover = hex_color(kd::BG_PANEL);
        c.list_active = hex_alpha(0x8ba4b02e);
        c.list_active_border = hex_color(kd::BLUE);
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
        c.table_hover = hex_color(0x232120);
        c.table_active = hex_alpha(0x8ba4b026);
        c.table_active_border = hex_color(kd::BLUE);
        c.table_row_border = hex_color(kd::BORDER_SUBTLE);

        c.scrollbar = hex_alpha(0x00000000);
        c.scrollbar_thumb = hex_alpha(0x72716966);
        c.scrollbar_thumb_hover = hex_alpha(0x9e9b93aa);

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
        assert_eq!(parsed.name, "Kanagawa Dragon");
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
