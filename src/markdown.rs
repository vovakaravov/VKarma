use crate::theme::{hex_alpha, hex_color, kd};
use crate::vault::{VaultIndex, is_image_extension};
use gpui_kit::component::h_flex;
use gpui_kit::component::text::{
    MarkdownNode, MarkdownParseContext, MarkdownPlugin, TextView, markdown_ast,
};
use gpui_kit::{
    App, ImageSource, IntoElement, ObjectFit, ParentElement as _, SharedString, SharedUri, Styled,
    StyledImage as _, Window, div, img, prelude::FluentBuilder as _, px, relative,
};
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const VKARMA_NOTE_SCHEME: &str = "vkarma-note://";
pub const VKARMA_IMG_SCHEME: &str = "vkarma-img://";

/// URL-safe percent encoding that leaves `/` `-` `_` `.` `~` readable while escaping spaces, `?`, `#`, `(`, `)`, `%`, etc.
const PATH_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'(')
    .add(b')')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

pub fn encode_uri_payload(s: &str) -> String {
    utf8_percent_encode(s, PATH_ENCODE_SET).to_string()
}

pub fn decode_uri_payload(uri: &str, scheme: &str) -> Option<String> {
    let rest = uri.strip_prefix(scheme)?;
    percent_decode_str(rest)
        .decode_utf8()
        .ok()
        .map(|s| s.into_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StyledImageSpec {
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub align: ImageAlign,
    pub wrap: bool,
    pub rounded: bool,
    pub border: bool,
    pub caption: Option<String>,
}

impl Default for StyledImageSpec {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            align: ImageAlign::Left,
            wrap: false,
            rounded: true,
            border: false,
            caption: None,
        }
    }
}

impl StyledImageSpec {
    /// True when the image floats left or right so adjacent text wraps beside it.
    pub fn is_floated(&self) -> bool {
        self.wrap && matches!(self.align, ImageAlign::Left | ImageAlign::Right)
    }
}

pub fn parse_image_spec(alt: &str) -> StyledImageSpec {
    let mut spec = StyledImageSpec::default();
    let mut captions = Vec::new();

    for raw_part in alt.split('|') {
        let part = raw_part.trim();
        if part.is_empty() {
            continue;
        }
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "left" => spec.align = ImageAlign::Left,
            "center" | "centre" => spec.align = ImageAlign::Center,
            "right" => spec.align = ImageAlign::Right,
            "wrap" | "float" => spec.wrap = true,
            "float-left" | "left-wrap" => {
                spec.align = ImageAlign::Left;
                spec.wrap = true;
            }
            "float-right" | "right-wrap" => {
                spec.align = ImageAlign::Right;
                spec.wrap = true;
            }
            "nowrap" | "block" => spec.wrap = false,
            "rounded" | "round" => spec.rounded = true,
            "square" | "sharp" => spec.rounded = false,
            "border" | "framed" => spec.border = true,
            "noborder" => spec.border = false,
            _ => {
                if let Ok(w) = lower.trim_end_matches("px").parse::<f32>()
                    && w > 0.0
                {
                    spec.width = Some(w);
                    continue;
                }
                if let Some((w_str, h_str)) = lower.split_once('x')
                    && let (Ok(w), Ok(h)) = (
                        w_str.trim_end_matches("px").parse::<f32>(),
                        h_str.trim_end_matches("px").parse::<f32>(),
                    )
                    && w > 0.0
                    && h > 0.0
                {
                    spec.width = Some(w);
                    spec.height = Some(h);
                    continue;
                }
                captions.push(part.to_string());
            }
        }
    }

    if !captions.is_empty() {
        spec.caption = Some(captions.join(" | "));
    }
    spec
}

/// Serialize a `StyledImageSpec` into compact, Obsidian-compatible pipe-separated options.
pub fn format_image_spec(spec: &StyledImageSpec) -> String {
    let mut parts: Vec<String> = Vec::new();

    match (spec.width, spec.height) {
        (Some(w), Some(h)) => {
            parts.push(format!("{}x{}", w.round().max(1.0) as u32, h.round().max(1.0) as u32));
        }
        (Some(w), None) => {
            parts.push(format!("{}", w.round().max(1.0) as u32));
        }
        _ => {}
    }

    match spec.align {
        ImageAlign::Center => parts.push("center".to_string()),
        ImageAlign::Right => parts.push("right".to_string()),
        ImageAlign::Left => {
            if spec.wrap {
                parts.push("left".to_string());
            }
        }
    }

    if spec.wrap {
        parts.push("wrap".to_string());
    }
    if !spec.rounded {
        parts.push("square".to_string());
    }
    if spec.border {
        parts.push("border".to_string());
    }
    if let Some(caption) = spec.caption.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        parts.push(caption.to_string());
    }

    parts.join("|")
}

/// Probe intrinsic pixel `(width, height)` from image header bytes (PNG, GIF, BMP, WebP, JPEG).
pub fn read_image_dimensions_from_bytes(data: &[u8]) -> Option<(f32, f32)> {
    // PNG: 8-byte signature + IHDR chunk (width at 16..20, height at 20..24)
    if data.len() >= 24 && &data[..8] == b"\x89PNG\r\n\x1a\n" && &data[12..16] == b"IHDR" {
        let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        if w > 0 && h > 0 {
            return Some((w as f32, h as f32));
        }
        return None;
    }

    // GIF: GIF87a / GIF89a
    if data.len() >= 10 && (&data[..6] == b"GIF87a" || &data[..6] == b"GIF89a") {
        let w = u16::from_le_bytes([data[6], data[7]]) as u32;
        let h = u16::from_le_bytes([data[8], data[9]]) as u32;
        if w > 0 && h > 0 {
            return Some((w as f32, h as f32));
        }
        return None;
    }

    // BMP: "BM" + DIB header
    if data.len() >= 26 && &data[..2] == b"BM" {
        let dib_size = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);
        let (w, h) = if dib_size >= 40 {
            let w = i32::from_le_bytes([data[18], data[19], data[20], data[21]]).unsigned_abs();
            let h = i32::from_le_bytes([data[22], data[23], data[24], data[25]]).unsigned_abs();
            (w, h)
        } else if dib_size == 12 {
            let w = u16::from_le_bytes([data[18], data[19]]) as u32;
            let h = u16::from_le_bytes([data[20], data[21]]) as u32;
            (w, h)
        } else {
            (0, 0)
        };
        if w > 0 && h > 0 {
            return Some((w as f32, h as f32));
        }
        return None;
    }

    // WebP: RIFF....WEBP + VP8 / VP8L / VP8X
    if data.len() >= 25 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        match &data[12..16] {
            b"VP8 " if data.len() >= 30 && data[23..26] == [0x9d, 0x01, 0x2a] => {
                let w = (u16::from_le_bytes([data[26], data[27]]) & 0x3fff) as u32;
                let h = (u16::from_le_bytes([data[28], data[29]]) & 0x3fff) as u32;
                if w > 0 && h > 0 {
                    return Some((w as f32, h as f32));
                }
            }
            b"VP8L" if data[20] == 0x2f => {
                let bits = u32::from_le_bytes([data[21], data[22], data[23], data[24]]);
                let w = (bits & 0x3fff) + 1;
                let h = ((bits >> 14) & 0x3fff) + 1;
                return Some((w as f32, h as f32));
            }
            b"VP8X" if data.len() >= 30 => {
                let w =
                    1 + (data[24] as u32 | ((data[25] as u32) << 8) | ((data[26] as u32) << 16));
                let h =
                    1 + (data[27] as u32 | ((data[28] as u32) << 8) | ((data[29] as u32) << 16));
                return Some((w as f32, h as f32));
            }
            _ => {}
        }
        return None;
    }

    // JPEG: SOI (FF D8) + SOF0..SOF15 markers (excluding DHT C4, JPG C8, DAC CC)
    if data.len() >= 4 && data[0] == 0xFF && data[1] == 0xD8 {
        let mut i = 2usize;
        while i + 1 < data.len() {
            while i < data.len() && data[i] != 0xFF {
                i += 1;
            }
            while i < data.len() && data[i] == 0xFF {
                i += 1;
            }
            if i >= data.len() {
                break;
            }
            let marker = data[i];
            i += 1;

            // Standalone markers with no payload length
            if matches!(marker, 0x00 | 0x01 | 0xD0..=0xD9) {
                if marker == 0xD9 {
                    break;
                }
                continue;
            }

            if i + 1 >= data.len() {
                break;
            }
            let seg_len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
            if seg_len < 2 || i + seg_len > data.len() {
                break;
            }

            if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                if seg_len >= 7 && i + 6 < data.len() {
                    let h = u16::from_be_bytes([data[i + 3], data[i + 4]]) as u32;
                    let w = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
                    if w > 0 && h > 0 {
                        return Some((w as f32, h as f32));
                    }
                }
                return None;
            }

            i += seg_len;
        }
    }

    None
}

/// Read intrinsic pixel dimensions of an image file from up to 64 KB of header bytes.
pub fn read_image_dimensions(path: &Path) -> Option<(f32, f32)> {
    let file = File::open(path).ok()?;
    let mut buf = Vec::with_capacity(4096);
    file.take(65536).read_to_end(&mut buf).ok()?;
    read_image_dimensions_from_bytes(&buf)
}

/// Compute rendered `(width, height)` in logical pixels for an image given its spec,
/// optional intrinsic natural dimensions, and available column width.
pub fn compute_image_display_size(
    spec: &StyledImageSpec,
    natural_dims: Option<(f32, f32)>,
    col_width: f32,
) -> (f32, f32) {
    let max_w = col_width.max(120.0);
    let ratio = natural_dims
        .and_then(|(nw, nh)| (nw > 1.0 && nh > 1.0).then_some(nw / nh))
        .unwrap_or(16.0 / 10.0);

    match (spec.width, spec.height) {
        (Some(raw_w), Some(raw_h)) => {
            let w = raw_w.clamp(48.0, max_w);
            let scale = (w / raw_w.max(1.0)).min(1.0);
            let h = (raw_h * scale).clamp(36.0, 900.0);
            (w, h)
        }
        (Some(raw_w), None) => {
            let w = raw_w.clamp(48.0, max_w);
            let h = (w / ratio).clamp(36.0, 900.0);
            (w, h)
        }
        (None, Some(raw_h)) => {
            let mut h = raw_h.clamp(36.0, 900.0);
            let w = (h * ratio).clamp(48.0, max_w);
            if w / ratio < h {
                h = (w / ratio).clamp(36.0, 900.0);
            }
            (w, h)
        }
        (None, None) => {
            if let Some((nat_w, _)) = natural_dims {
                let auto_max_w = if spec.is_floated() {
                    (max_w * 0.44).clamp(150.0, max_w)
                } else {
                    max_w
                };
                let mut w = nat_w.min(auto_max_w).clamp(48.0, max_w);
                let mut h = w / ratio;
                let max_auto_h = 460.0;
                if h > max_auto_h {
                    h = max_auto_h;
                    w = (h * ratio).clamp(48.0, max_w);
                }
                if h < 36.0 {
                    h = 36.0;
                }
                (w, h)
            } else if spec.is_floated() {
                let w = (max_w * 0.42).clamp(160.0, 320.0);
                (w, (w / 1.5).clamp(96.0, 260.0))
            } else {
                let w = max_w.clamp(220.0, 480.0);
                (w, (w / 1.6).clamp(120.0, 280.0))
            }
        }
    }
}

type WrapLinkCallback = Arc<dyn Fn(&SharedString, &mut Window, &mut App) + Send + Sync>;

#[derive(Debug, Clone)]
struct StyledImageData {
    path: PathBuf,
    spec: StyledImageSpec,
    natural_dims: Option<(f32, f32)>,
    wrap_markdown: Option<String>,
}

#[derive(Clone, Default)]
pub struct StyledImagePlugin {
    on_link_click: Option<WrapLinkCallback>,
}

impl StyledImagePlugin {
    pub fn with_link_handler(
        handler: impl Fn(&SharedString, &mut Window, &mut App) + Send + Sync + 'static,
    ) -> Self {
        Self {
            on_link_click: Some(Arc::new(handler)),
        }
    }
}

impl MarkdownPlugin for StyledImagePlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "vkarma_styled_image"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Paragraph(paragraph) = node else {
            return None;
        };
        let Some(markdown_ast::Node::Image(image)) = paragraph.children.first() else {
            return None;
        };
        let decoded = decode_uri_payload(&image.url, VKARMA_IMG_SCHEME)?;
        let path = PathBuf::from(decoded);
        let spec = parse_image_spec(&image.alt);

        let wrap_markdown = if paragraph.children.len() > 1 {
            if !spec.is_floated() {
                return None;
            }
            let start = paragraph.children[1].position()?.start.offset;
            let end = paragraph.position.as_ref()?.end.offset;
            let rest = cx.source().get(start..end)?.trim();
            if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            }
        } else {
            None
        };

        let natural_dims = path.is_file().then(|| read_image_dimensions(&path)).flatten();

        let caption_or_name = spec.caption.clone().unwrap_or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image")
                .to_string()
        });
        let text = match &wrap_markdown {
            Some(rest) if !rest.is_empty() => format!("{caption_or_name} {rest}"),
            _ => caption_or_name,
        };

        Some(
            MarkdownNode::new(
                self.name(),
                StyledImageData {
                    path,
                    spec,
                    natural_dims,
                    wrap_markdown,
                },
            )
            .text(text),
        )
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let Some(data) = node.data::<StyledImageData>() else {
            return div().into_any_element();
        };

        let justify = match data.spec.align {
            ImageAlign::Left => div().flex().justify_start(),
            ImageAlign::Center => div().flex().justify_center(),
            ImageAlign::Right => div().flex().justify_end(),
        };

        let radius = if data.spec.rounded { px(8.) } else { px(0.) };
        let is_floated_row = data.spec.is_floated() && data.wrap_markdown.is_some();

        let frame: gpui_kit::AnyElement = if !data.path.is_file() {
            div()
                .px_3()
                .py_2()
                .rounded(px(6.))
                .bg(hex_color(kd::BG_PANEL))
                .border_1()
                .border_color(hex_color(kd::RED))
                .text_xs()
                .text_color(hex_color(kd::RED))
                .when(is_floated_row, |this| this.max_w(px(320.)).flex_shrink_0())
                .child(format!("Missing image: {}", data.path.display()))
                .into_any_element()
        } else {
            let (disp_w, disp_h) = if is_floated_row {
                let (w, h) = compute_image_display_size(&data.spec, data.natural_dims, 460.0);
                (Some(w), Some(h))
            } else {
                match (data.spec.width, data.spec.height) {
                    (Some(_), None) if data.natural_dims.is_some() => {
                        let (w, h) =
                            compute_image_display_size(&data.spec, data.natural_dims, 1600.0);
                        (Some(w), Some(h))
                    }
                    _ => (
                        data.spec.width.map(|w| w.clamp(24., 1600.)),
                        data.spec.height.map(|h| h.clamp(24., 1200.)),
                    ),
                }
            };

            let img_el = img(data.path.clone())
                .object_fit(ObjectFit::Contain)
                .max_w(relative(1.))
                .when_some(disp_w, |this, w| this.w(px(w.clamp(24., 1600.))))
                .when_some(disp_h, |this, h| this.h(px(h.clamp(24., 1200.))))
                .rounded(radius);

            div()
                .flex()
                .flex_col()
                .gap_1()
                .max_w(relative(1.))
                .when(is_floated_row, |this| this.flex_shrink_0())
                .child(
                    div()
                        .overflow_hidden()
                        .rounded(radius)
                        .when(data.spec.border, |this| {
                            this.border_1()
                                .border_color(hex_color(kd::BORDER))
                                .bg(hex_alpha(0x0e0c0f80))
                                .p_1()
                        })
                        .child(img_el),
                )
                .when_some(data.spec.caption.clone(), |this, caption| {
                    this.child(
                        div()
                            .text_xs()
                            .italic()
                            .text_color(hex_color(kd::FG_MUTED))
                            .when_some(disp_w, |d, w| d.max_w(px(w.clamp(24., 1600.))))
                            .when(data.spec.align == ImageAlign::Center, |d| d.text_center())
                            .when(data.spec.align == ImageAlign::Right, |d| d.text_right())
                            .child(caption),
                    )
                })
                .into_any_element()
        };

        if let Some(wrap_md) = data.wrap_markdown.as_ref().filter(|_| data.spec.is_floated()) {
            let mut hasher = DefaultHasher::new();
            data.path.hash(&mut hasher);
            wrap_md.hash(&mut hasher);
            let tv_id: SharedString = format!("md-wrap-{:x}", hasher.finish()).into();

            let link_cb = self.on_link_click.clone();
            let mut wrap_tv = TextView::markdown(tv_id, wrap_md.clone())
                .image_source(resolve_inline_image_source)
                .selectable(true)
                .w_full()
                .text_size(px(15.))
                .line_height(relative(1.65))
                .text_color(hex_color(kd::FG));
            if let Some(cb) = link_cb {
                wrap_tv = wrap_tv.on_link_click(move |url, _, window, cx| {
                    cb(url, window, cx);
                });
            }

            let text_col = div().flex_1().min_w_0().child(wrap_tv);
            let row = h_flex().w_full().items_start().gap_4().my_2();
            return match data.spec.align {
                ImageAlign::Right => row.child(text_col).child(frame).into_any_element(),
                _ => row.child(frame).child(text_col).into_any_element(),
            };
        }

        justify.w_full().my_2().child(frame).into_any_element()
    }
}

#[derive(Default)]
pub struct SceneBreakPlugin;

impl MarkdownPlugin for SceneBreakPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "vkarma_scene_break"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::ThematicBreak(_) = node else {
            return None;
        };
        let src = cx.node_source(node)?.trim();
        if src.starts_with("***") {
            Some(MarkdownNode::new(self.name(), ()).text("***"))
        } else {
            None
        }
    }

    fn render(&self, _node: &MarkdownNode, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .w_full()
            .my_4()
            .flex()
            .justify_center()
            .items_center()
            .text_sm()
            .text_color(hex_color(kd::YELLOW))
            .child("✦   ✦   ✦")
    }
}

pub fn resolve_inline_image_source(uri: &SharedUri) -> ImageSource {
    if let Some(decoded) = decode_uri_payload(uri.as_ref(), VKARMA_IMG_SCHEME) {
        return ImageSource::from(PathBuf::from(decoded));
    }
    ImageSource::from(uri.clone())
}

pub fn preprocess_obsidian_markdown(
    raw: &str,
    current_file: &Path,
    vault: &VaultIndex,
    depth: usize,
) -> String {
    preprocess_markdown_with_options(raw, current_file, vault, depth, false)
}

pub fn preprocess_markdown_with_options(
    raw: &str,
    current_file: &Path,
    vault: &VaultIndex,
    depth: usize,
    is_mdx: bool,
) -> String {
    let mdx_normalized: Cow<'_, str> = if is_mdx {
        Cow::Owned(normalize_mdx_constructs(raw))
    } else {
        Cow::Borrowed(raw)
    };

    let mut out = String::with_capacity(mdx_normalized.len() + 256);
    let lines: Vec<&str> = mdx_normalized.split('\n').collect();
    let mut idx = 0;

    // Preserve YAML frontmatter verbatim so FrontmatterPlugin can parse it
    if lines.first().is_some_and(|l| l.trim_end_matches('\r') == "---") {
        let mut end_fm = None;
        for (i, line) in lines.iter().enumerate().skip(1) {
            let clean = line.trim_end_matches('\r');
            if clean == "---" || clean == "..." {
                end_fm = Some(i);
                break;
            }
        }
        if let Some(end) = end_fm {
            for line in &lines[..=end] {
                out.push_str(line.trim_end_matches('\r'));
                out.push('\n');
            }
            idx = end + 1;
        }
    }

    let mut fence: Option<(char, usize)> = None;
    while idx < lines.len() {
        let line = lines[idx].trim_end_matches('\r');
        let trimmed = line.trim_start();

        if let Some((f_ch, f_len)) = fence {
            out.push_str(line);
            if idx + 1 < lines.len() {
                out.push('\n');
            }
            let close_len = trimmed.chars().take_while(|c| *c == f_ch).count();
            if close_len >= f_len && trimmed[close_len..].trim().is_empty() {
                fence = None;
            }
            idx += 1;
            continue;
        }

        if trimmed.starts_with("```") {
            let count = trimmed.chars().take_while(|c| *c == '`').count();
            fence = Some(('`', count));
            out.push_str(line);
            if idx + 1 < lines.len() {
                out.push('\n');
            }
            idx += 1;
            continue;
        }
        if trimmed.starts_with("~~~") {
            let count = trimmed.chars().take_while(|c| *c == '~').count();
            fence = Some(('~', count));
            out.push_str(line);
            if idx + 1 < lines.len() {
                out.push('\n');
            }
            idx += 1;
            continue;
        }

        // Preserve thematic breaks and callout headers cleanly before smart typography
        let converted = if let Some(rewritten_callout) = rewrite_callout_line(line) {
            preprocess_inline_obsidian(&rewritten_callout, current_file, vault, depth)
        } else {
            preprocess_inline_obsidian(line, current_file, vault, depth)
        };

        out.push_str(&converted);

        if idx + 1 < lines.len() {
            let cur_trim = line.trim();
            let next_raw = lines[idx + 1].trim_end_matches('\r');
            let next_trim = next_raw.trim();

            if is_standalone_floated_image_line(cur_trim)
                && next_trim.is_empty()
                && lines.get(idx + 2).is_some_and(|l| {
                    let t = l.trim();
                    !t.is_empty() && !is_block_starter_line(t) && !is_standalone_image_line(t)
                })
            {
                // Join a floated wrap image with the immediately following prose paragraph
                // (omitting one blank line so CommonMark groups them into one paragraph
                // that StyledImagePlugin intercepts as a side-by-side float row).
                out.push('\n');
                idx += 2;
                continue;
            }

            if should_insert_hard_break(line, &converted, next_raw) {
                out.push_str("  ");
            }
            out.push('\n');
        }
        idx += 1;
    }

    out
}

fn is_standalone_image_line(trimmed: &str) -> bool {
    trimmed
        .strip_prefix("![[")
        .and_then(|s| s.strip_suffix("]]"))
        .is_some_and(|inner| !inner.is_empty() && !inner.contains("[["))
        || (trimmed.starts_with("![")
            && parse_std_md_image(trimmed).is_some_and(|(consumed, _, _)| consumed == trimmed.len()))
}

fn is_standalone_floated_image_line(trimmed: &str) -> bool {
    if let Some(inner) = trimmed.strip_prefix("![[").and_then(|s| s.strip_suffix("]]"))
        && !inner.is_empty()
        && !inner.contains("[[")
        && !inner.contains("]]")
    {
        let (target, spec_str) = inner.split_once('|').unwrap_or((inner, ""));
        return is_image_extension(target.trim()) && parse_image_spec(spec_str).is_floated();
    }
    if let Some((consumed, alt, _)) = parse_std_md_image(trimmed)
        && consumed == trimmed.len()
    {
        return parse_image_spec(alt).is_floated();
    }
    false
}

fn should_insert_hard_break(raw_line: &str, converted_line: &str, next_raw: &str) -> bool {
    let cur_trim = raw_line.trim();
    let nxt_trim = next_raw.trim();

    if cur_trim.is_empty() || nxt_trim.is_empty() {
        return false;
    }
    if converted_line.ends_with('\n')
        || converted_line.ends_with("  ")
        || converted_line.ends_with('\\')
    {
        return false;
    }

    if is_block_boundary_line(cur_trim) || is_block_starter_line(nxt_trim) {
        return false;
    }

    // Standalone embeds / images on their own line don't need trailing spaces
    if is_standalone_image_line(cur_trim) || is_standalone_image_line(nxt_trim) {
        return false;
    }

    // Match blockquote continuation cleanly without leaking trailing spaces out of a quote
    let cur_quote = cur_trim.starts_with('>');
    let nxt_quote = nxt_trim.starts_with('>');
    if cur_quote != nxt_quote {
        return false;
    }
    if cur_quote && cur_trim.trim_start_matches('>').trim().is_empty() {
        return false;
    }

    true
}

fn is_heading_line(trimmed: &str) -> bool {
    let hash_cnt = trimmed.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hash_cnt) && trimmed[hash_cnt..].starts_with(char::is_whitespace)
}

fn is_thematic_break_line(trimmed: &str) -> bool {
    if trimmed.len() < 3 {
        return false;
    }
    let non_space: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    non_space.len() >= 3
        && (non_space.chars().all(|c| c == '-')
            || non_space.chars().all(|c| c == '*')
            || non_space.chars().all(|c| c == '_'))
}

fn is_list_item_line(trimmed: &str) -> bool {
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'-' | b'*' | b'+') && bytes[1] == b' ' {
        return true;
    }
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    digits > 0
        && digits + 2 <= bytes.len()
        && matches!(bytes[digits], b'.' | b')')
        && bytes[digits + 1] == b' '
}

fn is_html_or_jsx_block_line(trimmed: &str) -> bool {
    if !trimmed.starts_with('<') {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("<div")
        || lower.starts_with("</div")
        || lower.starts_with("<p")
        || lower.starts_with("</p")
        || lower.starts_with("<h1")
        || lower.starts_with("<h2")
        || lower.starts_with("<h3")
        || lower.starts_with("<h4")
        || lower.starts_with("<table")
        || lower.starts_with("</table")
        || lower.starts_with("<blockquote")
        || lower.starts_with("<!--")
}

fn is_block_boundary_line(trimmed: &str) -> bool {
    is_heading_line(trimmed)
        || is_thematic_break_line(trimmed)
        || trimmed.starts_with('|')
        || is_html_or_jsx_block_line(trimmed)
}

fn is_block_starter_line(trimmed: &str) -> bool {
    is_block_boundary_line(trimmed)
        || trimmed.starts_with("```")
        || trimmed.starts_with("~~~")
        || is_list_item_line(trimmed)
}

fn normalize_mdx_constructs(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 128);
    let lines: Vec<&str> = raw.split('\n').collect();
    let mut i = 0usize;

    // Copy frontmatter intact
    if lines.first().is_some_and(|l| l.trim_end_matches('\r') == "---") {
        let mut end_fm = None;
        for (idx, line) in lines.iter().enumerate().skip(1) {
            let clean = line.trim_end_matches('\r');
            if clean == "---" || clean == "..." {
                end_fm = Some(idx);
                break;
            }
        }
        if let Some(end) = end_fm {
            for line in &lines[..=end] {
                out.push_str(line.trim_end_matches('\r'));
                out.push('\n');
            }
            i = end + 1;
        }
    }

    let mut in_fence = false;
    while i < lines.len() {
        let line = lines[i].trim_end_matches('\r');
        let trimmed = line.trim();

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push_str(line);
            if i + 1 < lines.len() {
                out.push('\n');
            }
            i += 1;
            continue;
        }
        if in_fence {
            out.push_str(line);
            if i + 1 < lines.len() {
                out.push('\n');
            }
            i += 1;
            continue;
        }

        // Top-level MDX import statement
        if line.trim_start().starts_with("import ") && line.contains(" from ") {
            let imported = line
                .trim_start()
                .strip_prefix("import ")
                .and_then(|rest| rest.split(" from ").next())
                .unwrap_or("module")
                .trim();
            out.push_str(&format!("*MDX import:* `{imported}`"));
            if i + 1 < lines.len() {
                out.push('\n');
            }
            i += 1;
            continue;
        }

        // Multi-line or self-closing PascalCase JSX component: <ComponentName ... />
        if let Some(tag_name) = parse_pascal_jsx_start(trimmed) {
            let mut block = String::from(line);
            while !block_ends_jsx(&block, &tag_name) && i + 1 < lines.len() {
                i += 1;
                block.push('\n');
                block.push_str(lines[i].trim_end_matches('\r'));
            }
            let props = summarize_jsx_props(&block);
            if props.is_empty() {
                out.push_str(&format!("\n> **Component:** `{tag_name}`\n\n"));
            } else {
                out.push_str(&format!("\n> **Component:** `{tag_name}` — {props}\n\n"));
            }
            i += 1;
            continue;
        }

        // Multi-line or inline <video ...>...</video>
        if line.contains("<video") {
            let mut block = String::from(line);
            while !block.contains("</video>") && i + 1 < lines.len() {
                i += 1;
                block.push('\n');
                block.push_str(lines[i].trim_end_matches('\r'));
            }
            let normalized = rewrite_inline_media_tags(&block);
            out.push_str(&normalized);
            if i + 1 < lines.len() {
                out.push('\n');
            }
            i += 1;
            continue;
        }

        // <iframe ...></iframe>
        if line.contains("<iframe") {
            let mut block = String::from(line);
            while !block.contains("</iframe>") && i + 1 < lines.len() {
                i += 1;
                block.push('\n');
                block.push_str(lines[i].trim_end_matches('\r'));
            }
            let normalized = rewrite_inline_media_tags(&block);
            out.push_str(&normalized);
            if i + 1 < lines.len() {
                out.push('\n');
            }
            i += 1;
            continue;
        }

        out.push_str(line);
        if i + 1 < lines.len() {
            out.push('\n');
        }
        i += 1;
    }

    out
}

fn parse_pascal_jsx_start(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix('<')?;
    let first = rest.chars().next()?;
    if !first.is_ascii_uppercase() {
        return None;
    }
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_')
        .collect();
    if name.is_empty() { None } else { Some(name) }
}

fn block_ends_jsx(block: &str, tag_name: &str) -> bool {
    let trimmed = block.trim_end();
    trimmed.ends_with("/>") || trimmed.contains(&format!("</{tag_name}>"))
}

fn summarize_jsx_props(block: &str) -> String {
    let mut parts = Vec::new();
    for key in ["title", "storyTitle", "modelPath", "src", "alt"] {
        if let Some(val) = extract_attr(block, key) {
            parts.push(format!("{key}: `{val}`"));
        }
    }
    parts.join(" · ")
}

fn extract_attr(snippet: &str, attr_name: &str) -> Option<String> {
    let double = format!("{attr_name}=\"");
    if let Some(idx) = snippet.find(&double) {
        let rest = &snippet[idx + double.len()..];
        if let Some(end) = rest.find('"') {
            return Some(rest[..end].to_string());
        }
    }
    let single = format!("{attr_name}='");
    if let Some(idx) = snippet.find(&single) {
        let rest = &snippet[idx + single.len()..];
        if let Some(end) = rest.find('\'') {
            return Some(rest[..end].to_string());
        }
    }
    None
}

fn rewrite_inline_media_tags(text: &str) -> String {
    let mut cur = text.to_string();

    while let Some(start) = cur.find("<video") {
        let end = cur[start..]
            .find("</video>")
            .map(|e| start + e + "</video>".len())
            .or_else(|| cur[start..].find("/>").map(|e| start + e + 2))
            .unwrap_or(cur.len());
        let snippet = &cur[start..end];
        let src = extract_attr(snippet, "src").unwrap_or_else(|| "video".to_string());
        let file_label = src.rsplit('/').next().unwrap_or(&src);
        let replacement = format!("\n> **Video:** `{file_label}` (`{src}`)\n\n");
        cur.replace_range(start..end, &replacement);
    }

    while let Some(start) = cur.find("<iframe") {
        let end = cur[start..]
            .find("</iframe>")
            .map(|e| start + e + "</iframe>".len())
            .or_else(|| cur[start..].find("/>").map(|e| start + e + 2))
            .unwrap_or(cur.len());
        let snippet = &cur[start..end];
        let src = extract_attr(snippet, "src").unwrap_or_else(|| "embed".to_string());
        let replacement = format!("\n> **Embed:** [{src}]({src})\n\n");
        cur.replace_range(start..end, &replacement);
    }

    cur
}

fn rewrite_callout_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let after_quote = trimmed.strip_prefix('>')?.trim_start();
    let rest = after_quote.strip_prefix("[!")?;
    let end_bracket = rest.find(']')?;
    let kind = rest[..end_bracket].trim();
    if kind.is_empty()
        || !kind
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let after_bracket = rest[end_bracket + 1..]
        .trim_start_matches(['+', '-', ' '])
        .trim();
    let label = kind.to_ascii_uppercase();
    if after_bracket.is_empty() {
        Some(format!("> **{label}**"))
    } else {
        Some(format!("> **{label}** — {after_bracket}"))
    }
}

fn preprocess_inline_obsidian(
    line: &str,
    current_file: &Path,
    vault: &VaultIndex,
    depth: usize,
) -> String {
    let line_trim = line.trim();
    let skip_smart_typography = is_thematic_break_line(line_trim) || line_trim.starts_with('|');

    let mut out = String::with_capacity(line.len());
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut pos = 0;

    while pos < chars.len() {
        let byte_idx = chars[pos].0;
        let ch = chars[pos].1;

        // Skip inline code spans untouched
        if ch == '`' {
            let tick_start = pos;
            while pos < chars.len() && chars[pos].1 == '`' {
                pos += 1;
            }
            let tick_count = pos - tick_start;
            let ticks: String = "`".repeat(tick_count);
            let rest = &line[chars[tick_start].0 + tick_count..];
            if let Some(close_rel) = find_closing_ticks(rest, tick_count) {
                let end_byte = chars[tick_start].0 + tick_count + close_rel + tick_count;
                out.push_str(&line[byte_idx..end_byte]);
                while pos < chars.len() && chars[pos].0 < end_byte {
                    pos += 1;
                }
            } else {
                out.push_str(&ticks);
            }
            continue;
        }

        // Skip inline HTML / JSX tags untouched (so `<!--` or style attrs aren't altered)
        if ch == '<'
            && let Some(tag_len) = parse_html_tag_len(&line[byte_idx..])
        {
            out.push_str(&line[byte_idx..byte_idx + tag_len]);
            let target_byte = byte_idx + tag_len;
            while pos < chars.len() && chars[pos].0 < target_byte {
                pos += 1;
            }
            continue;
        }

        // Check `![[...]]` (embed)
        if line[byte_idx..].starts_with("![[")
            && let Some(end_rel) = line[byte_idx + 3..].find("]]")
        {
            let inner = &line[byte_idx + 3..byte_idx + 3 + end_rel];
            let replacement = render_obsidian_embed(inner, current_file, vault, depth);
            out.push_str(&replacement);
            let target_byte = byte_idx + 3 + end_rel + 2;
            while pos < chars.len() && chars[pos].0 < target_byte {
                pos += 1;
            }
            continue;
        }

        // Check `[[...]]` (wikilink)
        if line[byte_idx..].starts_with("[[")
            && let Some(end_rel) = line[byte_idx + 2..].find("]]")
        {
            let inner = &line[byte_idx + 2..byte_idx + 2 + end_rel];
            let (raw_target, alias) = match inner.split_once('|') {
                Some((t, a)) => (t.trim(), Some(a.trim())),
                None => (inner.trim(), None),
            };
            let display = alias
                .filter(|a| !a.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    raw_target
                        .split('#')
                        .next()
                        .unwrap_or(raw_target)
                        .rsplit('/')
                        .next()
                        .unwrap_or(raw_target)
                        .to_string()
                });
            let encoded = encode_uri_payload(raw_target);
            out.push_str(&format!("[{display}]({VKARMA_NOTE_SCHEME}{encoded})"));
            let target_byte = byte_idx + 2 + end_rel + 2;
            while pos < chars.len() && chars[pos].0 < target_byte {
                pos += 1;
            }
            continue;
        }

        // Check standard Markdown image `![alt](url)` with relative or `/public` asset path
        if line[byte_idx..].starts_with("![")
            && !line[byte_idx..].starts_with("![[")
            && let Some((consumed, alt, url)) = parse_std_md_image(&line[byte_idx..])
        {
            let trimmed_url = url.trim();
            let resolved_img = if trimmed_url.starts_with("http://")
                || trimmed_url.starts_with("https://")
                || trimmed_url.starts_with("data:")
                || trimmed_url.starts_with(VKARMA_IMG_SCHEME)
            {
                None
            } else {
                vault.resolve_asset(trimmed_url, current_file).or_else(|| {
                    current_file
                        .parent()
                        .map(|p| p.join(trimmed_url))
                        .filter(|p| p.is_file())
                })
            };

            if let Some(abs_path) = resolved_img {
                let encoded = encode_uri_payload(&abs_path.to_string_lossy());
                out.push_str(&format!("![{alt}]({VKARMA_IMG_SCHEME}{encoded})"));
            } else {
                out.push_str(&line[byte_idx..byte_idx + consumed]);
            }

            let target_byte = byte_idx + consumed;
            while pos < chars.len() && chars[pos].0 < target_byte {
                pos += 1;
            }
            continue;
        }

        // Preserve Markdown `[text](url)` URLs intact so URLs aren't altered by smart typography
        if ch == '['
            && !line[byte_idx..].starts_with("[[")
            && let Some((consumed, label, url)) = parse_std_md_link(&line[byte_idx..])
        {
            let processed_label = preprocess_inline_obsidian(label, current_file, vault, depth);
            out.push_str(&format!("[{processed_label}]({url})"));
            let target_byte = byte_idx + consumed;
            while pos < chars.len() && chars[pos].0 < target_byte {
                pos += 1;
            }
            continue;
        }

        // Highlight syntax: `==highlighted==`
        if line[byte_idx..].starts_with("==")
            && !line[byte_idx..].starts_with("===")
            && let Some(end_rel) = line[byte_idx + 2..].find("==")
            && end_rel > 0
        {
            let inner = &line[byte_idx + 2..byte_idx + 2 + end_rel];
            if !inner.starts_with('=') {
                out.push_str(&format!(
                    "<mark style=\"background-color: #c4b28a44\">{inner}</mark>"
                ));
                let target_byte = byte_idx + 2 + end_rel + 2;
                while pos < chars.len() && chars[pos].0 < target_byte {
                    pos += 1;
                }
                continue;
            }
        }

        // Smart typography outside code / URLs / HTML tags
        if !skip_smart_typography {
            let slice = &line[byte_idx..];
            if slice.starts_with("...") {
                out.push('…');
                pos += 3;
                continue;
            }
            if slice.starts_with("->") {
                out.push('→');
                pos += 2;
                continue;
            }
            if slice.starts_with("<-") {
                out.push('←');
                pos += 2;
                continue;
            }
            if slice.starts_with("--") && !slice.starts_with("---") {
                out.push('—');
                pos += 2;
                continue;
            }
        }

        out.push(ch);
        pos += 1;
    }

    out
}

fn parse_html_tag_len(slice: &str) -> Option<usize> {
    if slice.starts_with("<!--") {
        return slice.find("-->").map(|i| i + 3);
    }
    let after = slice.strip_prefix('<')?;
    let first = after.chars().next()?;
    if !(first.is_ascii_alphabetic() || first == '/' || first == '!') {
        return None;
    }
    let close = after.find('>')?;
    Some(1 + close + 1)
}

fn find_closing_ticks(rest: &str, count: usize) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            if i - start == count {
                return Some(start);
            }
        } else {
            i += 1;
        }
    }
    None
}

fn parse_std_md_image(slice: &str) -> Option<(usize, &str, &str)> {
    let after_bang = slice.strip_prefix("![")?;
    let close_bracket = after_bang.find(']')?;
    let alt = &after_bang[..close_bracket];
    let after_bracket = &after_bang[close_bracket + 1..];
    if !after_bracket.starts_with('(') {
        return None;
    }
    let url_start = 1;
    let mut depth = 0usize;
    for (i, c) in after_bracket[url_start..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    let url = &after_bracket[url_start..url_start + i];
                    let consumed = 2 + close_bracket + 1 + url_start + i + 1;
                    return Some((consumed, alt, url));
                }
                depth -= 1;
            }
            '\n' => return None,
            _ => {}
        }
    }
    None
}

fn parse_std_md_link(slice: &str) -> Option<(usize, &str, &str)> {
    let after_open = slice.strip_prefix('[')?;
    let close_bracket = after_open.find(']')?;
    let label = &after_open[..close_bracket];
    let after_bracket = &after_open[close_bracket + 1..];
    if !after_bracket.starts_with('(') {
        return None;
    }
    let url_start = 1;
    let mut depth = 0usize;
    for (i, c) in after_bracket[url_start..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    let url = &after_bracket[url_start..url_start + i];
                    let consumed = 1 + close_bracket + 1 + url_start + i + 1;
                    return Some((consumed, label, url));
                }
                depth -= 1;
            }
            '\n' => return None,
            _ => {}
        }
    }
    None
}

fn render_obsidian_embed(
    inner: &str,
    current_file: &Path,
    vault: &VaultIndex,
    depth: usize,
) -> String {
    let (target, spec) = match inner.split_once('|') {
        Some((t, s)) => (t.trim(), s.trim()),
        None => (inner.trim(), ""),
    };

    if is_image_extension(target) {
        let resolved = vault
            .resolve_asset(target, current_file)
            .unwrap_or_else(|| current_file.parent().unwrap_or(&vault.root).join(target));
        let encoded = encode_uri_payload(&resolved.to_string_lossy());
        return format!("![{spec}]({VKARMA_IMG_SCHEME}{encoded})");
    }

    if let Some(note_path) = vault.resolve_wikilink(target, current_file) {
        let note_name = note_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(target);
        let note_uri = encode_uri_payload(target);

        if depth >= 2 {
            return format!("[Embedded: {note_name}]({VKARMA_NOTE_SCHEME}{note_uri})");
        }

        if let Ok(raw_embedded) = fs::read_to_string(&note_path) {
            let body = strip_frontmatter(&raw_embedded);
            let rendered_body = preprocess_obsidian_markdown(body, &note_path, vault, depth + 1);
            let mut quoted =
                format!("> **[Embedded: {note_name}]({VKARMA_NOTE_SCHEME}{note_uri})**\n>\n");
            for l in rendered_body.lines().take(60) {
                if l.is_empty() {
                    quoted.push_str(">\n");
                } else {
                    quoted.push_str("> ");
                    quoted.push_str(l);
                    quoted.push('\n');
                }
            }
            return quoted;
        }
    }

    format!("*[Missing embed: {target}]*")
}

fn strip_frontmatter(raw: &str) -> &str {
    if let Some(rest) = raw.strip_prefix("---\n").or_else(|| raw.strip_prefix("---\r\n")) {
        if let Some(pos) = rest.find("\n---") {
            let after = &rest[pos + 4..];
            return after
                .strip_prefix("\r\n")
                .or_else(|| after.strip_prefix('\n'))
                .unwrap_or(after);
        }
    }
    raw
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryChapter {
    pub title: String,
    pub content: String,
}

/// Splits long-form fiction (`.md` / `.mdx`) into chapter pages on `## ` and `### ` headings,
/// merging an empty `## Part` header with its following `### Chapter`.
pub fn split_markdown_chapters(raw: &str) -> Vec<StoryChapter> {
    let body = strip_frontmatter(raw);
    let mut raw_sections: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_fence = false;
    let mut skipped_h1 = false;

    for raw_line in body.lines() {
        let trimmed = raw_line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        }
        if !in_fence && !skipped_h1 && trimmed.starts_with("# ") && current.trim().is_empty() {
            skipped_h1 = true;
            continue;
        }
        if !in_fence && (trimmed.starts_with("## ") || trimmed.starts_with("### ")) {
            if !current.trim().is_empty() {
                raw_sections.push(std::mem::take(&mut current));
            }
        }
        current.push_str(raw_line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        raw_sections.push(current);
    }

    let mut sections = Vec::new();
    let mut idx = 0usize;
    while idx < raw_sections.len() {
        let sec = &raw_sections[idx];
        let trimmed = sec.trim();
        let has_h3 = trimmed.lines().any(|l| l.trim_start().starts_with("### "));
        let starts_h2 = trimmed.starts_with("## ");
        if !has_h3 && starts_h2 && idx + 1 < raw_sections.len() {
            let after_first = trimmed
                .lines()
                .skip(1)
                .collect::<Vec<_>>()
                .join("\n");
            if after_first.trim().is_empty() {
                let merged = format!("{sec}\n{}", raw_sections[idx + 1]);
                let title = merged
                    .lines()
                    .find(|l| l.trim_start().starts_with("### "))
                    .or_else(|| merged.lines().find(|l| l.trim_start().starts_with("## ")))
                    .map(|l| l.trim_start().trim_start_matches('#').trim().to_string())
                    .unwrap_or_else(|| format!("Chapter {}", sections.len() + 1));
                sections.push(StoryChapter {
                    title,
                    content: merged,
                });
                idx += 2;
                continue;
            }
        }

        let title = trimmed
            .lines()
            .find(|l| l.trim_start().starts_with("## ") || l.trim_start().starts_with("### "))
            .map(|l| l.trim_start().trim_start_matches('#').trim().to_string())
            .unwrap_or_else(|| format!("Section {}", sections.len() + 1));
        sections.push(StoryChapter {
            title,
            content: sec.clone(),
        });
        idx += 1;
    }

    sections
}

/// Counts task checkbox occurrences outside fenced code blocks along with their byte offsets.
fn find_task_checkbox_offsets(text: &str) -> Vec<(usize, usize)> {
    let mut offsets = Vec::new();
    let mut cursor = 0usize;
    let mut in_fence = false;

    for raw_line in text.split('\n') {
        let line_len = raw_line.len();
        let line = raw_line.trim_end_matches('\r');
        let trimmed_start_len = line.len() - line.trim_start().len();
        let trimmed = &line[trimmed_start_len..];

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence {
            if let Some(rel_bracket) = detect_task_marker(trimmed) {
                let abs_bracket = cursor + trimmed_start_len + rel_bracket;
                offsets.push((abs_bracket, abs_bracket + 3));
            }
        }

        cursor += line_len + 1;
    }

    offsets
}

fn detect_task_marker(trimmed: &str) -> Option<usize> {
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 5
        && matches!(bytes[0], b'-' | b'*' | b'+')
        && bytes[1] == b' '
        && is_bracket_marker(&trimmed[2..5])
    {
        return Some(2);
    }

    let mut digits = 0usize;
    while digits < bytes.len() && bytes[digits].is_ascii_digit() {
        digits += 1;
    }
    if digits > 0
        && digits + 5 <= bytes.len()
        && matches!(bytes[digits], b'.' | b')')
        && bytes[digits + 1] == b' '
        && is_bracket_marker(&trimmed[digits + 2..digits + 5])
    {
        return Some(digits + 2);
    }

    None
}

fn is_bracket_marker(s: &str) -> bool {
    matches!(s, "[ ]" | "[x]" | "[X]")
}

/// Toggles the checkbox corresponding to `marker_offset` inside `preprocessed` cleanly back in `raw`.
pub fn toggle_nth_task_checkbox(
    raw: &str,
    preprocessed: &str,
    marker_offset: usize,
    checked: bool,
) -> Option<String> {
    let pre_boxes = find_task_checkbox_offsets(preprocessed);
    let idx = pre_boxes
        .iter()
        .position(|(start, end)| marker_offset >= *start && marker_offset <= *end)
        .or_else(|| {
            pre_boxes
                .iter()
                .enumerate()
                .min_by_key(|(_, (s, _))| s.abs_diff(marker_offset))
                .map(|(i, _)| i)
        })?;

    let raw_boxes = find_task_checkbox_offsets(raw);
    let (start, end) = *raw_boxes.get(idx)?;
    let mut out = String::with_capacity(raw.len());
    out.push_str(&raw[..start]);
    out.push_str(if checked { "[x]" } else { "[ ]" });
    out.push_str(&raw[end..]);
    Some(out)
}

pub fn word_and_char_count(text: &str) -> (usize, usize) {
    let words = text.split_whitespace().count();
    let chars = text.chars().count();
    (words, chars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parses_obsidian_image_styling_specs() {
        let spec = parse_image_spec("center | 480 | rounded | border | System Architecture");
        assert_eq!(spec.align, ImageAlign::Center);
        assert_eq!(spec.width, Some(480.0));
        assert_eq!(spec.height, None);
        assert!(spec.rounded);
        assert!(spec.border);
        assert_eq!(spec.caption.as_deref(), Some("System Architecture"));

        let dims = parse_image_spec("320x200|right|square");
        assert_eq!(dims.width, Some(320.0));
        assert_eq!(dims.height, Some(200.0));
        assert_eq!(dims.align, ImageAlign::Right);
        assert!(!dims.rounded);
        assert_eq!(dims.caption, None);
    }

    #[test]
    fn single_enter_adds_hard_breaks_without_breaking_blocks() {
        let tmp = TempDir::new().unwrap();
        let vault = VaultIndex::build(tmp.path(), false);
        let raw = "# Heading\nLine one\nLine two\n\n> Quote one\n> Quote two\n- Item A\n- Item B\n***\nAfter break\n";
        let pre = preprocess_obsidian_markdown(raw, &tmp.path().join("test.md"), &vault, 0);

        assert!(pre.contains("# Heading\nLine one  \nLine two\n"));
        assert!(pre.contains("> Quote one  \n> Quote two\n"));
        assert!(!pre.contains("- Item A  \n"));
        assert!(!pre.contains("***  \n"));
    }

    #[test]
    fn applies_smart_typography_and_highlight_outside_code_and_urls() {
        let tmp = TempDir::new().unwrap();
        let vault = VaultIndex::build(tmp.path(), false);
        let raw = "Wait -- what... a ==key point== -> look `[keep--dots...]` [url](https://example.com/a--b)\n";
        let pre = preprocess_obsidian_markdown(raw, &tmp.path().join("smart.md"), &vault, 0);

        assert!(pre.contains("Wait — what… a <mark style=\"background-color: #c4b28a44\">key point</mark> → look `[keep--dots...]` [url](https://example.com/a--b)"));
    }

    #[test]
    fn preprocesses_mdx_imports_videos_and_components() {
        let tmp = TempDir::new().unwrap();
        let vault = VaultIndex::build(tmp.path(), false);
        let raw = "\
---
title: \"Demo\"
---
import ModelViewer from '../../components/ModelViewer.astro';

<video controls src=\"/resources/blog/clip.mp4\"></video>Paragraph right after video.

<ModelViewer
  modelPath=\"/models/greg.glb\"
  title=\"Greg\"
/>
";
        let pre = preprocess_markdown_with_options(raw, &tmp.path().join("post.mdx"), &vault, 0, true);
        assert!(pre.contains("*MDX import:* `ModelViewer`"));
        assert!(pre.contains("> **Video:** `clip.mp4` (`/resources/blog/clip.mp4`)"));
        assert!(pre.contains("Paragraph right after video."));
        assert!(pre.contains("> **Component:** `ModelViewer` — title: `Greg` · modelPath: `/models/greg.glb`"));
    }

    #[test]
    fn splits_long_story_into_chapters() {
        let raw = "\
---
title: Test Story
---

# Test Story

## Part I

### Chapter 1: First

Hello world.

### Chapter 2: Second

Second body.
";
        let chapters = split_markdown_chapters(raw);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "Chapter 1: First");
        assert!(chapters[0].content.contains("## Part I"));
        assert_eq!(chapters[1].title, "Chapter 2: Second");
    }

    #[test]
    fn rewrites_wikilinks_embeds_callouts_and_respects_code_fences() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("attachments")).unwrap();
        fs::write(root.join("Other Note.md"), "Body line").unwrap();
        fs::write(root.join("attachments/my pic.png"), [137u8, 80, 78, 71]).unwrap();

        let vault = VaultIndex::build(root, false);
        let raw = "\
---
tags:
  - test
---
> [!tip] Fast workflow
See [[Other Note#Sec|My Link]] and `[[Ignored Code]]`.
```md
[[Inside Fence]]
```
![[my pic.png|center|400]]
![rel](attachments/my pic.png)
";
        let pre = preprocess_obsidian_markdown(raw, &root.join("Current.md"), &vault, 0);
        assert!(pre.contains("> **TIP** — Fast workflow"));
        assert!(pre.contains("[My Link](vkarma-note://Other%20Note%23Sec)"));
        assert!(pre.contains("`[[Ignored Code]]`"));
        assert!(pre.contains("[[Inside Fence]]"));
        assert!(pre.contains("![center|400](vkarma-img://"));
        assert!(!pre.contains("](attachments/my pic.png)"));
    }

    #[test]
    fn toggles_nth_task_checkbox_accurately_despite_offset_shift() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("Alpha.md"), "a").unwrap();
        let vault = VaultIndex::build(root, false);

        let raw = "Link [[Alpha]] here.\n- [ ] First task\n- [x] Second task with [[Alpha|A]]\n";
        let pre = preprocess_obsidian_markdown(raw, &root.join("Doc.md"), &vault, 0);

        let pre_boxes = find_task_checkbox_offsets(&pre);
        assert_eq!(pre_boxes.len(), 2);

        let toggled = toggle_nth_task_checkbox(raw, &pre, pre_boxes[1].0, false).unwrap();
        assert_eq!(
            toggled,
            "Link [[Alpha]] here.\n- [ ] First task\n- [ ] Second task with [[Alpha|A]]\n"
        );
    }

    #[test]
    fn image_spec_parses_and_formats_float_wrap_roundtrip() {
        let s = parse_image_spec("320 | left | wrap | border | Hero Shot");
        assert_eq!(s.width, Some(320.0));
        assert_eq!(s.height, None);
        assert_eq!(s.align, ImageAlign::Left);
        assert!(s.wrap);
        assert!(s.is_floated());
        assert!(s.border);
        assert_eq!(s.caption.as_deref(), Some("Hero Shot"));
        assert_eq!(format_image_spec(&s), "320|left|wrap|border|Hero Shot");
        assert_eq!(parse_image_spec(&format_image_spec(&s)), s);

        let rw = parse_image_spec("260|right-wrap|square");
        assert_eq!(rw.width, Some(260.0));
        assert_eq!(rw.align, ImageAlign::Right);
        assert!(rw.wrap);
        assert!(!rw.rounded);
        assert_eq!(format_image_spec(&rw), "260|right|wrap|square");

        let center = parse_image_spec("480|center|nowrap");
        assert_eq!(center.align, ImageAlign::Center);
        assert!(!center.wrap);
        assert!(!center.is_floated());
        assert_eq!(format_image_spec(&center), "480|center");
    }

    #[test]
    fn probes_image_header_dimensions_across_formats() {
        // Minimal PNG header (640x360)
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&360u32.to_be_bytes());
        assert_eq!(read_image_dimensions_from_bytes(&png), Some((640.0, 360.0)));

        // Minimal GIF89a header (320x240)
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&320u16.to_le_bytes());
        gif.extend_from_slice(&240u16.to_le_bytes());
        assert_eq!(read_image_dimensions_from_bytes(&gif), Some((320.0, 240.0)));

        // Minimal BMP header (800x600)
        let mut bmp = vec![0u8; 26];
        bmp[0..2].copy_from_slice(b"BM");
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&800i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-600i32).to_le_bytes());
        assert_eq!(read_image_dimensions_from_bytes(&bmp), Some((800.0, 600.0)));

        // Minimal WebP VP8L header (500x300)
        let mut webp = vec![0u8; 25];
        webp[0..4].copy_from_slice(b"RIFF");
        webp[8..12].copy_from_slice(b"WEBP");
        webp[12..16].copy_from_slice(b"VP8L");
        webp[20] = 0x2f;
        let packed = (500u32 - 1) | ((300u32 - 1) << 14);
        webp[21..25].copy_from_slice(&packed.to_le_bytes());
        assert_eq!(read_image_dimensions_from_bytes(&webp), Some((500.0, 300.0)));

        // Minimal JPEG SOF0 header (1280x720)
        let jpeg = vec![
            0xFF, 0xD8, // SOI
            0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, // APP0 dummy
            0xFF, 0xC0, 0x00, 0x0B, 0x08, // SOF0 + len + precision
            0x02, 0xD0, // height = 720
            0x05, 0x00, // width = 1280
            0x03, 0x01, 0x11, 0x00,
        ];
        assert_eq!(read_image_dimensions_from_bytes(&jpeg), Some((1280.0, 720.0)));

        assert_eq!(read_image_dimensions_from_bytes(b"not an image"), None);
    }

    #[test]
    fn preprocess_joins_floated_image_with_following_paragraph_for_read_mode() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("pic.png"), [137u8, 80, 78, 71]).unwrap();
        let vault = VaultIndex::build(root, false);

        let raw = "\
![[pic.png|240|left|wrap]]

First prose line
Second prose line

## Next Heading
![[pic.png|300|center]]

After center image.
";
        let pre = preprocess_obsidian_markdown(raw, &root.join("note.md"), &vault, 0);
        // Floated image joins across the blank line into the same paragraph
        assert!(pre.contains("![240|left|wrap](vkarma-img://"));
        assert!(!pre.contains("pic.png)\n\nFirst prose line"));
        assert!(pre.contains("pic.png)\nFirst prose line  \nSecond prose line\n"));
        // Center non-wrapped image keeps blank line separator intact
        assert!(pre.contains("![300|center](vkarma-img://"));
        assert!(pre.contains("pic.png)\n\nAfter center image."));
    }
}
