use gpui_kit::{Image, ImageFormat};
use std::sync::{Arc, OnceLock};

static SVG_FONTDB: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();

fn shared_fontdb() -> Arc<resvg::usvg::fontdb::Database> {
    SVG_FONTDB
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

pub struct SvgRenderResult {
    pub image: Arc<Image>,
    pub png_bytes: Vec<u8>,
    pub width: f32,
    pub height: f32,
    pub element_count: usize,
}

pub fn render_svg_to_image(svg_text: &str, max_dim: u32) -> Result<SvgRenderResult, String> {
    if svg_text.trim().is_empty() {
        return Err("Empty SVG document".to_string());
    }

    let opt = resvg::usvg::Options {
        fontdb: shared_fontdb(),
        ..Default::default()
    };

    let tree =
        resvg::usvg::Tree::from_str(svg_text, &opt).map_err(|e| format!("SVG error: {e}"))?;

    let element_count = roxmltree::Document::parse(svg_text)
        .map(|d| d.descendants().filter(|n| n.is_element()).count())
        .unwrap_or(0);

    let sz = tree.size();
    let w = sz.width().max(1.0);
    let h = sz.height().max(1.0);
    let max_side = w.max(h);

    let mut scale = 2.0f32;
    if max_side < 128.0 {
        scale = (256.0 / max_side).clamp(2.0, 8.0);
    }
    let cap = max_dim.max(256) as f32;
    if max_side * scale > cap {
        scale = (cap / max_side).max(0.1);
    }

    let px_w = ((w * scale).round() as u32).max(1);
    let px_h = ((h * scale).round() as u32).max(1);

    let mut pixmap = resvg::tiny_skia::Pixmap::new(px_w, px_h)
        .ok_or_else(|| "Failed to allocate SVG pixmap".to_string())?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let png_bytes = pixmap
        .encode_png()
        .map_err(|e| format!("PNG encode error: {e}"))?;
    let image = Arc::new(Image::from_bytes(ImageFormat::Png, png_bytes.clone()));

    Ok(SvgRenderResult {
        image,
        png_bytes,
        width: w,
        height: h,
        element_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_valid_svg_and_rejects_invalid() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80" viewBox="0 0 120 80">
            <rect x="8" y="8" width="104" height="64" rx="8" fill="#8ba4b0"/>
            <text x="60" y="45" text-anchor="middle" font-size="14" fill="#181616">VKarma</text>
        </svg>"##;

        let res = render_svg_to_image(svg, 2048).unwrap();
        assert_eq!(res.width, 120.0);
        assert_eq!(res.height, 80.0);
        assert_eq!(res.element_count, 3);
        assert!(res.png_bytes.starts_with(b"\x89PNG"));

        assert!(render_svg_to_image("<svg><unclosed>", 2048).is_err());
        assert!(render_svg_to_image("   ", 2048).is_err());
    }
}
