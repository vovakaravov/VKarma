use gpui_kit::{Image, ImageFormat};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub const DEFAULT_PDF_PAGE_LIMIT: usize = 40;
pub const DEFAULT_PDF_DPI: u32 = 150;

static PDF_RENDER_SEQ: AtomicU64 = AtomicU64::new(1);

pub struct PdfRenderOutput {
    pub pages: Vec<Arc<Image>>,
    pub total_pages: usize,
    pub duration_ms: u128,
    pub error: Option<String>,
}

pub fn probe_pdf_page_count(path: &Path) -> Option<usize> {
    let out = Command::new("pdfinfo").arg(path).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Pages:")
            && let Ok(n) = rest.trim().parse::<usize>()
        {
            return Some(n);
        }
    }
    None
}

fn trailing_page_number(path: &Path) -> usize {
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.rsplit('-').next())
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(usize::MAX)
}

pub fn render_pdf_pages(path: &Path, dpi: u32, max_pages: Option<usize>) -> PdfRenderOutput {
    let start = Instant::now();
    let seq = PDF_RENDER_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir().join(format!(
        "vkarma-pdf-{}-{seq}",
        std::process::id()
    ));

    if let Err(e) = fs::create_dir_all(&tmp_dir) {
        return PdfRenderOutput {
            pages: Vec::new(),
            total_pages: 0,
            duration_ms: start.elapsed().as_millis(),
            error: Some(format!("Failed to create PDF temp dir: {e}")),
        };
    }

    let probed = probe_pdf_page_count(path);
    let prefix = tmp_dir.join("page");
    let mut cmd = Command::new("pdftoppm");
    cmd.args(["-png", "-r"]).arg(dpi.to_string());
    if let Some(limit) = max_pages.filter(|&l| l > 0) {
        let effective_limit = probed.map_or(limit, |tot| limit.min(tot.max(1)));
        cmd.args(["-f", "1", "-l"]).arg(effective_limit.to_string());
    }
    cmd.arg(path).arg(&prefix);

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp_dir);
            return PdfRenderOutput {
                pages: Vec::new(),
                total_pages: probed.unwrap_or(0),
                duration_ms: start.elapsed().as_millis(),
                error: Some(format!(
                    "Failed to run `pdftoppm` (install `poppler` package?): {e}"
                )),
            };
        }
    };

    let duration_ms = start.elapsed().as_millis();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let _ = fs::remove_dir_all(&tmp_dir);
        return PdfRenderOutput {
            pages: Vec::new(),
            total_pages: probed.unwrap_or(0),
            duration_ms,
            error: Some(if stderr.is_empty() {
                format!("pdftoppm exited with status {}", output.status)
            } else {
                stderr
            }),
        };
    }

    let mut png_paths: Vec<PathBuf> = fs::read_dir(&tmp_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("png"))
        })
        .collect();
    png_paths.sort_by(|a, b| {
        trailing_page_number(a)
            .cmp(&trailing_page_number(b))
            .then_with(|| a.cmp(b))
    });

    let mut pages = Vec::with_capacity(png_paths.len());
    for png in png_paths {
        if let Ok(bytes) = fs::read(&png) {
            pages.push(Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
        }
    }

    let _ = fs::remove_dir_all(&tmp_dir);
    let total_pages = probed.unwrap_or(pages.len()).max(pages.len());

    PdfRenderOutput {
        pages,
        total_pages,
        duration_ms,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typst_engine::export_typst_pdf;

    #[test]
    fn renders_multipage_pdf_and_respects_page_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let typ_path = tmp.path().join("sample.typ");
        let pdf_path = tmp.path().join("sample.pdf");
        fs::write(&typ_path, "= Page 1\n#pagebreak()\n= Page 2\n").unwrap();

        export_typst_pdf("= Page 1\n#pagebreak()\n= Page 2\n", &typ_path, tmp.path(), &pdf_path)
            .unwrap();

        assert_eq!(probe_pdf_page_count(&pdf_path), Some(2));

        let capped = render_pdf_pages(&pdf_path, 96, Some(1));
        assert!(capped.error.is_none(), "error: {:?}", capped.error);
        assert_eq!(capped.total_pages, 2);
        assert_eq!(capped.pages.len(), 1);
        assert!(!capped.pages[0].bytes.is_empty());

        let full = render_pdf_pages(&pdf_path, 96, None);
        assert!(full.error.is_none(), "error: {:?}", full.error);
        assert_eq!(full.total_pages, 2);
        assert_eq!(full.pages.len(), 2);
        assert!(!full.pages[1].bytes.is_empty());
    }
}
