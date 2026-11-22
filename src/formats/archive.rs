use crate::formats::office::image_format_from_name;
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use gpui_kit::Image;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use zip::ZipArchive;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
}

impl ArchiveFormat {
    pub fn from_path(path: &Path) -> Self {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            Self::TarGz
        } else if name.ends_with(".tar") {
            Self::Tar
        } else {
            Self::Zip
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Zip => "ZIP",
            Self::Tar => "TAR",
            Self::TarGz => "TAR.GZ",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub compressed_size: Option<u64>,
}

#[derive(Clone)]
pub enum ArchivePreview {
    Text(String),
    Image(Arc<Image>),
    Binary { hex_dump: String, total_bytes: usize },
    Directory,
}

#[derive(Debug, Clone)]
pub struct ArchiveIndex {
    pub format: ArchiveFormat,
    pub entries: Vec<ArchiveEntry>,
    pub total_uncompressed: u64,
}

fn cstr_trim(slice: &[u8]) -> String {
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    String::from_utf8_lossy(&slice[..end]).trim().to_string()
}

fn parse_tar_size(field: &[u8]) -> u64 {
    if field.first().is_some_and(|&b| b & 0x80 != 0) {
        let mut val = 0u64;
        for &b in &field[1..] {
            val = (val << 8) | (b as u64);
        }
        return val;
    }
    let s = cstr_trim(field);
    u64::from_str_radix(s.trim(), 8).unwrap_or(0)
}

fn read_exact_or_eof<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<bool> {
    let mut read_total = 0usize;
    while read_total < buf.len() {
        match r.read(&mut buf[read_total..])? {
            0 => {
                if read_total == 0 {
                    return Ok(false);
                }
                anyhow::bail!("Unexpected EOF inside tar archive");
            }
            n => read_total += n,
        }
    }
    Ok(true)
}

fn skip_exact<R: Read>(r: &mut R, mut len: u64) -> Result<()> {
    let mut scratch = [0u8; 4096];
    while len > 0 {
        let want = (len as usize).min(scratch.len());
        let got = r.read(&mut scratch[..want])?;
        if got == 0 {
            break;
        }
        len -= got as u64;
    }
    Ok(())
}

fn read_limited_bytes<R: Read>(r: &mut R, total_size: u64, cap: usize) -> Result<(Vec<u8>, u64)> {
    let want = (total_size as usize).min(cap);
    let mut out = vec![0u8; want];
    let mut pos = 0usize;
    while pos < want {
        let n = r.read(&mut out[pos..])?;
        if n == 0 {
            break;
        }
        pos += n;
    }
    out.truncate(pos);
    let remaining = total_size.saturating_sub(pos as u64);
    if remaining > 0 {
        skip_exact(r, remaining)?;
    }
    Ok((out, remaining))
}

fn walk_tar_stream<R: Read, F>(mut reader: R, mut on_entry: F) -> Result<()>
where
    F: FnMut(ArchiveEntry, &mut R, u64) -> Result<bool>,
{
    let mut header = [0u8; 512];
    let mut long_name: Option<String> = None;

    while read_exact_or_eof(&mut reader, &mut header)? {
        if header.iter().all(|&b| b == 0) {
            break;
        }

        let size = parse_tar_size(&header[124..136]);
        let typeflag = header[156];
        let pad = (512 - (size % 512)) % 512;

        if typeflag == b'L' {
            // GNU LongLink path record for next entry
            let (bytes, _) = read_limited_bytes(&mut reader, size, 32 * 1024)?;
            long_name = Some(cstr_trim(&bytes));
            skip_exact(&mut reader, pad)?;
            continue;
        }

        let name = if let Some(ln) = long_name.take() {
            ln
        } else {
            let base = cstr_trim(&header[0..100]);
            let prefix = cstr_trim(&header[345..500]);
            if prefix.is_empty() {
                base
            } else {
                format!("{prefix}/{base}")
            }
        };

        // Ignore Pax extended headers (`x` / `g`) but advance past their payload
        if matches!(typeflag, b'x' | b'g' | b'X') {
            skip_exact(&mut reader, size + pad)?;
            continue;
        }

        let is_dir = typeflag == b'5' || name.ends_with('/');
        let entry = ArchiveEntry {
            path: name,
            is_dir,
            size,
            compressed_size: None,
        };

        let before_size = size;
        let keep_going = on_entry(entry, &mut reader, before_size)?;
        if !keep_going {
            break;
        }
        skip_exact(&mut reader, pad)?;
    }
    Ok(())
}

impl ArchiveIndex {
    pub fn load(path: &Path) -> Result<Self> {
        let format = ArchiveFormat::from_path(path);
        let mut entries = Vec::new();
        let mut total_uncompressed = 0u64;

        match format {
            ArchiveFormat::Zip => {
                let file =
                    File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
                let mut zip = ZipArchive::new(file).context("Invalid ZIP archive")?;
                for i in 0..zip.len() {
                    let f = zip.by_index(i)?;
                    let epath = f.name().ok().map(|s| s.into_owned()).unwrap_or_default();
                    if epath.is_empty() {
                        continue;
                    }
                    let size = f.size();
                    let comp = f.compressed_size();
                    let is_dir = f.is_dir() || epath.ends_with('/');
                    if !is_dir {
                        total_uncompressed += size;
                    }
                    entries.push(ArchiveEntry {
                        path: epath,
                        is_dir,
                        size,
                        compressed_size: Some(comp),
                    });
                }
            }
            ArchiveFormat::Tar => {
                let file =
                    File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
                walk_tar_stream(file, |entry, r, sz| {
                    if !entry.is_dir {
                        total_uncompressed += entry.size;
                    }
                    entries.push(entry);
                    skip_exact(r, sz)?;
                    Ok(true)
                })?;
            }
            ArchiveFormat::TarGz => {
                let file =
                    File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
                let dec = GzDecoder::new(file);
                walk_tar_stream(dec, |entry, r, sz| {
                    if !entry.is_dir {
                        total_uncompressed += entry.size;
                    }
                    entries.push(entry);
                    skip_exact(r, sz)?;
                    Ok(true)
                })?;
            }
        }

        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self {
            format,
            entries,
            total_uncompressed,
        })
    }
}

fn format_hex_dump(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (row_ix, chunk) in bytes.chunks(16).enumerate() {
        let off = row_ix * 16;
        out.push_str(&format!("{off:08x}  "));
        for i in 0..16 {
            if let Some(b) = chunk.get(i) {
                out.push_str(&format!("{b:02x} "));
            } else {
                out.push_str("   ");
            }
            if i == 7 {
                out.push(' ');
            }
        }
        out.push_str(" |");
        for &b in chunk {
            let ch = if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '.'
            };
            out.push(ch);
        }
        out.push_str("|\n");
    }
    out
}

fn bytes_to_preview(entry_path: &str, bytes: Vec<u8>) -> ArchivePreview {
    if let Some(fmt) = image_format_from_name(entry_path)
        && !bytes.is_empty()
    {
        return ArchivePreview::Image(Arc::new(Image::from_bytes(fmt, bytes)));
    }
    if !bytes.contains(&0)
        && let Ok(text) = String::from_utf8(bytes.clone())
    {
        return ArchivePreview::Text(text);
    }
    let hex_dump = format_hex_dump(&bytes[..bytes.len().min(512)]);
    ArchivePreview::Binary {
        hex_dump,
        total_bytes: bytes.len(),
    }
}

pub fn read_entry_preview(
    archive_path: &Path,
    entry_path: &str,
    max_bytes: usize,
) -> Result<ArchivePreview> {
    if entry_path.ends_with('/') {
        return Ok(ArchivePreview::Directory);
    }
    let format = ArchiveFormat::from_path(archive_path);
    match format {
        ArchiveFormat::Zip => {
            let file = File::open(archive_path)?;
            let mut zip = ZipArchive::new(file)?;
            let f = zip.by_name(entry_path)?;
            if f.is_dir() {
                return Ok(ArchivePreview::Directory);
            }
            let total = f.size();
            let mut limited = f.take(max_bytes as u64);
            let mut buf = Vec::new();
            limited.read_to_end(&mut buf)?;
            if total > max_bytes as u64 && !buf.contains(&0) && let Ok(mut txt) = String::from_utf8(buf.clone()) {
                txt.push_str(&format!("\n\n… truncated (showing {max_bytes} of {total} bytes)"));
                return Ok(ArchivePreview::Text(txt));
            }
            Ok(bytes_to_preview(entry_path, buf))
        }
        ArchiveFormat::Tar => {
            let file = File::open(archive_path)?;
            read_tar_entry_preview(file, entry_path, max_bytes)
        }
        ArchiveFormat::TarGz => {
            let file = File::open(archive_path)?;
            let dec = GzDecoder::new(file);
            read_tar_entry_preview(dec, entry_path, max_bytes)
        }
    }
}

fn read_tar_entry_preview<R: Read>(
    reader: R,
    target_path: &str,
    max_bytes: usize,
) -> Result<ArchivePreview> {
    let mut found: Option<ArchivePreview> = None;
    walk_tar_stream(reader, |entry, r, sz| {
        if entry.path == target_path {
            if entry.is_dir {
                found = Some(ArchivePreview::Directory);
            } else {
                let (buf, remaining) = read_limited_bytes(r, sz, max_bytes)?;
                if remaining > 0
                    && !buf.contains(&0)
                    && let Ok(mut txt) = String::from_utf8(buf.clone())
                {
                    txt.push_str(&format!("\n\n… truncated (showing {max_bytes} of {sz} bytes)"));
                    found = Some(ArchivePreview::Text(txt));
                    return Ok(false);
                }
                found = Some(bytes_to_preview(target_path, buf));
            }
            return Ok(false);
        }
        skip_exact(r, sz)?;
        Ok(true)
    })?;
    found.context("Entry not found inside archive")
}

pub fn sanitize_relative_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim().trim_start_matches("./");
    if trimmed.is_empty() {
        return None;
    }
    let p = Path::new(trimmed);
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

pub fn extract_all_safe(archive_path: &Path, dest_dir: &Path) -> Result<usize> {
    fs::create_dir_all(dest_dir)?;
    let format = ArchiveFormat::from_path(archive_path);
    let mut count = 0usize;

    match format {
        ArchiveFormat::Zip => {
            let file = File::open(archive_path)?;
            let mut zip = ZipArchive::new(file)?;
            for i in 0..zip.len() {
                let mut f = zip.by_index(i)?;
                let Some(name) = f.name().ok().map(|s| s.into_owned()) else {
                    continue;
                };
                let Some(rel) = sanitize_relative_path(&name) else {
                    continue;
                };
                let target = dest_dir.join(rel);
                if f.is_dir() {
                    fs::create_dir_all(&target)?;
                } else {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let mut out_f = File::create(&target)?;
                    std::io::copy(&mut f, &mut out_f)?;
                    count += 1;
                }
            }
        }
        ArchiveFormat::Tar => {
            let file = File::open(archive_path)?;
            count = extract_tar_safe(file, dest_dir)?;
        }
        ArchiveFormat::TarGz => {
            let file = File::open(archive_path)?;
            let dec = GzDecoder::new(file);
            count = extract_tar_safe(dec, dest_dir)?;
        }
    }

    Ok(count)
}

fn extract_tar_safe<R: Read>(reader: R, dest_dir: &Path) -> Result<usize> {
    let mut count = 0usize;
    walk_tar_stream(reader, |entry, r, sz| {
        if let Some(rel) = sanitize_relative_path(&entry.path) {
            let target = dest_dir.join(rel);
            if entry.is_dir {
                fs::create_dir_all(&target)?;
                skip_exact(r, sz)?;
            } else {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut limited = r.take(sz);
                let mut out_f = File::create(&target)?;
                let copied = std::io::copy(&mut limited, &mut out_f)?;
                if copied < sz {
                    skip_exact(r, sz - copied)?;
                }
                count += 1;
            }
        } else {
            skip_exact(r, sz)?;
        }
        Ok(true)
    })?;
    Ok(count)
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn append_tar_entry(tar_buf: &mut Vec<u8>, name: &str, data: &[u8]) {
        let mut hdr = [0u8; 512];
        let nb = name.as_bytes();
        hdr[..nb.len().min(100)].copy_from_slice(&nb[..nb.len().min(100)]);
        hdr[100..108].copy_from_slice(b"0000644\0");
        hdr[108..116].copy_from_slice(b"0000000\0");
        hdr[116..124].copy_from_slice(b"0000000\0");
        let sz_str = format!("{:011o}\0", data.len());
        hdr[124..136].copy_from_slice(sz_str.as_bytes());
        hdr[136..148].copy_from_slice(b"00000000000\0");
        hdr[148..156].copy_from_slice(b"        ");
        hdr[156] = b'0';
        hdr[257..263].copy_from_slice(b"ustar\0");
        hdr[263..265].copy_from_slice(b"00");
        let csum: u32 = hdr.iter().map(|&b| b as u32).sum();
        let cstr = format!("{csum:06o}\0 ");
        hdr[148..156].copy_from_slice(cstr.as_bytes());
        tar_buf.extend_from_slice(&hdr);
        tar_buf.extend_from_slice(data);
        let pad = (512 - (data.len() % 512)) % 512;
        tar_buf.resize(tar_buf.len() + pad, 0);
    }

    #[test]
    fn loads_previews_and_safely_extracts_zip_and_targz() {
        let tmp = tempfile::tempdir().unwrap();

        // 1. Zip test (including an unsafe ../escape entry)
        let zip_path = tmp.path().join("bundle.zip");
        {
            let file = File::create(&zip_path).unwrap();
            let mut zw = zip::ZipWriter::new(file);
            zw.start_file("docs/readme.txt", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(b"Hello VKarma").unwrap();
            zw.start_file("raw.bin", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(&[0u8, 1, 2, 0xff]).unwrap();
            zw.start_file("../escape.txt", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(b"blocked").unwrap();
            zw.finish().unwrap();
        }

        let zidx = ArchiveIndex::load(&zip_path).unwrap();
        assert_eq!(zidx.format, ArchiveFormat::Zip);
        assert_eq!(zidx.entries.len(), 3);
        assert!(matches!(
            read_entry_preview(&zip_path, "docs/readme.txt", 4096).unwrap(),
            ArchivePreview::Text(t) if t == "Hello VKarma"
        ));
        assert!(matches!(
            read_entry_preview(&zip_path, "raw.bin", 4096).unwrap(),
            ArchivePreview::Binary { total_bytes: 4, .. }
        ));

        let zip_out = tmp.path().join("zip_out");
        let extracted = extract_all_safe(&zip_path, &zip_out).unwrap();
        assert_eq!(extracted, 2);
        assert!(zip_out.join("docs/readme.txt").exists());
        assert!(!tmp.path().join("escape.txt").exists());

        // 2. Tar.Gz test
        let tgz_path = tmp.path().join("bundle.tar.gz");
        {
            let mut tar_bytes = Vec::new();
            append_tar_entry(&mut tar_bytes, "notes/todo.md", b"- [x] Tar works");
            tar_bytes.resize(tar_bytes.len() + 1024, 0);
            let f = File::create(&tgz_path).unwrap();
            let mut gz = GzEncoder::new(f, Compression::default());
            gz.write_all(&tar_bytes).unwrap();
            gz.finish().unwrap();
        }

        let tidx = ArchiveIndex::load(&tgz_path).unwrap();
        assert_eq!(tidx.format, ArchiveFormat::TarGz);
        assert_eq!(tidx.entries.len(), 1);
        assert!(matches!(
            read_entry_preview(&tgz_path, "notes/todo.md", 4096).unwrap(),
            ArchivePreview::Text(t) if t == "- [x] Tar works"
        ));
    }
}
