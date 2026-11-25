use gpui::{App, Hsla, px, rgb, rgba};
use gpui_kit::component::{Theme, ThemeMode};

/// Kanagawa Dragon base with oniViolet (#957fb8) Obsidian-inspired accent.
pub mod kd {
    pub const BG_DEEP: u32 = 0x0e0c0f;
    pub const BG_MAIN: u32 = 0x181619;
    pub const BG_SIDEBAR: u32 = 0x1e1c20;
    pub const BG_PANEL: u32 = 0x28262b;
    pub const BG_BTN: u32 = 0x39363d;
    pub const BG_BTN_HOVER: u32 = 0x46424c;
    pub const BG_BTN_ACTIVE: u32 = 0x312e35;

    pub const BORDER: u32 = 0x3a3640;
    pub const BORDER_SUBTLE: u32 = 0x2b2830;

    pub const FG: u32 = 0xdcd7ba;
    pub const FG_MUTED: u32 = 0x727169;
    pub const FG_SUBTLE: u32 = 0x9e9b93;

    pub const ACCENT: u32 = 0x957fb8;
    pub const ACCENT_HOVER: u32 = 0xa994cc;
    pub const ACCENT_ACTIVE: u32 = 0x816ca3;

    pub const BLUE: u32 = 0x8ba4b0;
    pub const LINK: u32 = 0xa490c7;
    pub const GREEN: u32 = 0x87a987;
    pub const YELLOW: u32 = 0xc4b28a;
    pub const RED: u32 = 0xc4746e;
    pub const VIOLET: u32 = 0xa38ec7;
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

#[inline]
pub fn accent_alpha(alpha: u8) -> Hsla {
    rgba((kd::ACCENT << 8) | (alpha as u32)).into()
}

/// Applies VKarma's Kanagawa Dragon palette to gpui-kit's global Theme.
pub fn apply_mobile_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);

    Theme::update(cx, |theme| {
        theme.radius = px(8.);
        theme.radius_lg = px(12.);
        theme.font_size = px(15.);
        theme.mono_font_size = px(13.5);

        let c = &mut theme.colors;
        c.background = hex_color(kd::BG_MAIN);
        c.foreground = hex_color(kd::FG);
        c.border = hex_color(kd::BORDER);
        c.window_border = hex_color(kd::BORDER);
        c.muted = hex_color(kd::BG_PANEL);
        c.muted_foreground = hex_color(kd::FG_MUTED);
        c.accent = hex_color(kd::BG_PANEL);
        c.accent_foreground = hex_color(kd::FG);
        c.overlay = hex_alpha(0x0e0c0fcc);

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
        c.primary_foreground = hex_color(kd::BG_MAIN);

        c.button_primary = hex_color(kd::ACCENT);
        c.button_primary_hover = hex_color(kd::ACCENT_HOVER);
        c.button_primary_active = hex_color(kd::ACCENT_ACTIVE);
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

        c.input = hex_color(kd::BORDER);
        c.ring = hex_color(kd::ACCENT);
        c.caret = hex_color(kd::ACCENT);
        c.selection = accent_alpha(0x45);

        c.link = hex_color(kd::LINK);
        c.link_hover = hex_color(0xb8a6d9);
        c.link_active = hex_color(kd::ACCENT);

        c.list = hex_color(kd::BG_SIDEBAR);
        c.list_even = hex_color(kd::BG_SIDEBAR);
        c.list_head = hex_color(kd::BG_PANEL);
        c.list_hover = hex_color(kd::BG_PANEL);
        c.list_active = accent_alpha(0x2e);
        c.list_active_border = hex_color(kd::ACCENT);

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
    fn palette_helpers_produce_opaque_and_translucent_colors() {
        let fg = hex_color(kd::FG);
        let acc = accent_alpha(0x45);
        assert!(fg.a > 0.99);
        assert!(acc.a > 0.2 && acc.a < 0.35);
    }
}
