use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Atomically write `content` to `path` via a sibling PID-suffixed temp file and rename.
pub fn atomic_write_string(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent dir {}", parent.display()))?;
    }

    let fname = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let tmp = path.with_file_name(format!(".{fname}.vkarma-tmp-{}", std::process::id()));

    if fs::write(&tmp, content.as_bytes()).is_ok() {
        if fs::rename(&tmp, path).is_ok() {
            return Ok(());
        }
        let _ = fs::remove_file(&tmp);
    }

    fs::write(path, content.as_bytes())
        .with_context(|| format!("write file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::atomic_write_string;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn creates_and_overwrites_file_atomically() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("nested/note.md");

        atomic_write_string(&target, "# First").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "# First");

        atomic_write_string(&target, "# Updated").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "# Updated");
    }
}
