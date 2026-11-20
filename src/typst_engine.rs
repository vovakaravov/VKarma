use anyhow::{Context, Result};
use gpui_kit::{Image, ImageFormat};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static COMPILE_SEQ: AtomicU64 = AtomicU64::new(1);

pub struct TypstCompileOutput {
    pub pages: Vec<Arc<Image>>,
    pub duration_ms: u128,
    pub error: Option<String>,
}

pub fn effective_typst_root(doc_path: &Path, workspace_root: &Path) -> PathBuf {
    if doc_path.starts_with(workspace_root) {
        workspace_root.to_path_buf()
    } else {
        doc_path
            .parent()
            .unwrap_or(workspace_root)
            .to_path_buf()
    }
}

pub fn compile_typst_png(
    source: &str,
    doc_path: &Path,
    workspace_root: &Path,
    ppi: u32,
) -> TypstCompileOutput {
    let start = Instant::now();
    let seq = COMPILE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir().join(format!(
        "vkarma-typst-{}-{seq}",
        std::process::id()
    ));

    if let Err(e) = fs::create_dir_all(&tmp_dir) {
        return TypstCompileOutput {
            pages: Vec::new(),
            duration_ms: start.elapsed().as_millis(),
            error: Some(format!("Failed to create Typst temp dir: {e}")),
        };
    }

    let doc_parent = doc_path.parent().unwrap_or(workspace_root);
    let root = effective_typst_root(doc_path, workspace_root);
    let out_pattern = tmp_dir.join("page-{0p}.png");

    let mut cmd = Command::new("typst");
    cmd.args(["compile", "--format", "png", "--ppi"])
        .arg(ppi.to_string())
        .args(["--diagnostic-format", "human", "--root"])
        .arg(&root)
        .arg("-")
        .arg(&out_pattern)
        .current_dir(doc_parent)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }

    let result = match cmd.spawn() {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(source.as_bytes());
            }
            child.wait_with_output()
        }
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp_dir);
            return TypstCompileOutput {
                pages: Vec::new(),
                duration_ms: start.elapsed().as_millis(),
                error: Some(format!(
                    "Failed to run `typst` binary (is typst installed and on PATH?): {e}"
                )),
            };
        }
    };

    let duration_ms = start.elapsed().as_millis();

    let output = match result {
        Ok(o) => o,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp_dir);
            return TypstCompileOutput {
                pages: Vec::new(),
                duration_ms,
                error: Some(format!("Failed waiting for typst process: {e}")),
            };
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let _ = fs::remove_dir_all(&tmp_dir);
        return TypstCompileOutput {
            pages: Vec::new(),
            duration_ms,
            error: Some(if stderr.is_empty() {
                format!("typst exited with status {}", output.status)
            } else {
                stderr
            }),
        };
    }

    let mut png_paths: Vec<PathBuf> = fs::read_dir(&tmp_dir)
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
    png_paths.sort();

    let mut pages = Vec::with_capacity(png_paths.len());
    for png_path in png_paths {
        if let Ok(bytes) = fs::read(&png_path) {
            pages.push(Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
        }
    }

    let _ = fs::remove_dir_all(&tmp_dir);

    TypstCompileOutput {
        pages,
        duration_ms,
        error: None,
    }
}

pub fn export_typst_pdf(
    source: &str,
    doc_path: &Path,
    workspace_root: &Path,
    output_pdf: &Path,
) -> Result<PathBuf> {
    let doc_parent = doc_path.parent().unwrap_or(workspace_root);
    let root = effective_typst_root(doc_path, workspace_root);

    let mut cmd = Command::new("typst");
    cmd.args([
        "compile",
        "--format",
        "pdf",
        "--diagnostic-format",
        "human",
        "--root",
    ])
    .arg(&root)
    .arg("-")
    .arg(output_pdf)
    .current_dir(doc_parent)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }

    let mut child = cmd.spawn().context("spawn typst for PDF export")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(source.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{}", stderr.trim());
    }
    Ok(output_pdf.to_path_buf())
}

pub fn empty_typst_debounce() -> Duration {
    Duration::from_millis(200)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn compiles_typst_to_png_and_exports_pdf() {
        let tmp = TempDir::new().unwrap();
        let doc = tmp.path().join("doc.typ");
        let pdf = tmp.path().join("doc.pdf");
        let source = "= Hello VKarma\n\n$ a^2 + b^2 = c^2 $\n";

        let compiled = compile_typst_png(source, &doc, tmp.path(), 144);
        assert!(compiled.error.is_none(), "error: {:?}", compiled.error);
        assert_eq!(compiled.pages.len(), 1);

        let exported = export_typst_pdf(source, &doc, tmp.path(), &pdf).unwrap();
        assert_eq!(exported, pdf);
        let meta = fs::metadata(&pdf).unwrap();
        assert!(meta.len() > 100);
    }
}
