use std::fs::File;
use std::io::Read;
use std::path::Path;

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

#[cfg(test)]
mod tests {
    use super::{
        ImageAlign, StyledImageSpec, format_image_spec, parse_image_spec,
        read_image_dimensions_from_bytes,
    };

    #[test]
    fn parses_and_formats_image_specs_round_trip() {
        let s = parse_image_spec("320|left|wrap|border|Hero Shot");
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
        assert_eq!(format_image_spec(&StyledImageSpec::default()), "");
    }

    #[test]
    fn probes_image_header_dimensions_across_formats() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&360u32.to_be_bytes());
        assert_eq!(read_image_dimensions_from_bytes(&png), Some((640.0, 360.0)));

        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&320u16.to_le_bytes());
        gif.extend_from_slice(&240u16.to_le_bytes());
        assert_eq!(read_image_dimensions_from_bytes(&gif), Some((320.0, 240.0)));

        let mut bmp = vec![0u8; 26];
        bmp[0..2].copy_from_slice(b"BM");
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&800i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-600i32).to_le_bytes());
        assert_eq!(read_image_dimensions_from_bytes(&bmp), Some((800.0, 600.0)));

        let mut webp = vec![0u8; 25];
        webp[0..4].copy_from_slice(b"RIFF");
        webp[8..12].copy_from_slice(b"WEBP");
        webp[12..16].copy_from_slice(b"VP8L");
        webp[20] = 0x2f;
        let packed = (500u32 - 1) | ((300u32 - 1) << 14);
        webp[21..25].copy_from_slice(&packed.to_le_bytes());
        assert_eq!(read_image_dimensions_from_bytes(&webp), Some((500.0, 300.0)));

        let jpeg = vec![
            0xFF, 0xD8,
            0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00,
            0xFF, 0xC0, 0x00, 0x0B, 0x08,
            0x02, 0xD0,
            0x05, 0x00,
            0x03, 0x01, 0x11, 0x00,
        ];
        assert_eq!(read_image_dimensions_from_bytes(&jpeg), Some((1280.0, 720.0)));
        assert_eq!(read_image_dimensions_from_bytes(b"not an image"), None);
    }
}
