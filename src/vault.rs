use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Markdown,
    Mdx,
    Fountain,
    Typst,
    Spreadsheet,
    Image,
    Code(&'static str),
    PlainText,
}

impl FileKind {
    pub fn short_badge(&self) -> &'static str {
        match self {
            Self::Markdown => "MD",
            Self::Mdx => "MDX",
            Self::Fountain => "FTN",
            Self::Typst => "TYP",
            Self::Spreadsheet => "XLS",
            Self::Image => "IMG",
            Self::Code(lang) => match *lang {
                "rust" => "RS",
                "toml" => "TOML",
                "yaml" => "YAML",
                "json" => "JSON",
                "csharp" => "C#",
                "cpp" => "C++",
                "c" => "C",
                "python" => "PY",
                "bash" => "SH",
                "lua" => "LUA",
                "html" => "HTML",
                "css" => "CSS",
                "javascript" => "JS",
                "typescript" => "TS",
                "tsx" => "TSX",
                "sql" => "SQL",
                "diff" => "DIFF",
                "go" => "GO",
                "cmake" => "CMK",
                "make" => "MK",
                "zig" => "ZIG",
                _ => "CODE",
            },
            Self::PlainText => "TXT",
        }
    }
}

pub fn detect_file_kind(path: &Path) -> FileKind {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        file_name.as_str(),
        "makefile" | "gnumakefile" | "justfile"
    ) {
        return FileKind::Code("make");
    }
    if file_name == "cmakelists.txt" {
        return FileKind::Code("cmake");
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match ext.as_str() {
        "md" | "markdown" => FileKind::Markdown,
        "mdx" => FileKind::Mdx,
        "fountain" => FileKind::Fountain,
        "typ" => FileKind::Typst,
        "xlsx" | "csv" | "tsv" => FileKind::Spreadsheet,
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "bmp" => FileKind::Image,
        "rs" => FileKind::Code("rust"),
        "toml" => FileKind::Code("toml"),
        "yaml" | "yml" => FileKind::Code("yaml"),
        "json" | "jsonc" => FileKind::Code("json"),
        "cs" => FileKind::Code("csharp"),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => FileKind::Code("cpp"),
        "c" | "h" => FileKind::Code("c"),
        "py" | "pyi" => FileKind::Code("python"),
        "sh" | "bash" | "zsh" => FileKind::Code("bash"),
        "lua" => FileKind::Code("lua"),
        "html" | "htm" => FileKind::Code("html"),
        "css" => FileKind::Code("css"),
        "js" | "mjs" | "cjs" => FileKind::Code("javascript"),
        "ts" | "mts" | "cts" => FileKind::Code("typescript"),
        "tsx" | "jsx" => FileKind::Code("tsx"),
        "sql" => FileKind::Code("sql"),
        "diff" | "patch" => FileKind::Code("diff"),
        "go" => FileKind::Code("go"),
        "cmake" => FileKind::Code("cmake"),
        "zig" => FileKind::Code("zig"),
        _ => FileKind::PlainText,
    }
}

pub fn is_image_extension(target: &str) -> bool {
    let lower = target.trim().to_ascii_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif")
        || lower.ends_with(".svg")
        || lower.ends_with(".bmp")
}

pub fn is_ignored_dir(name: &str, show_hidden: bool) -> bool {
    if matches!(
        name,
        ".git" | ".stversions" | ".stfolder" | "target" | "node_modules" | "__pycache__" | ".next"
            | ".astro" | "obj"
    ) {
        return true;
    }
    if !show_hidden && name.starts_with('.') {
        return true;
    }
    false
}

pub fn is_unity_project(root: &Path) -> bool {
    root.join("Assets").is_dir() && root.join("ProjectSettings").is_dir()
}

pub fn is_ignored_dir_for_root(
    name: &str,
    depth: usize,
    is_unity: bool,
    show_hidden: bool,
) -> bool {
    if is_ignored_dir(name, show_hidden) {
        return true;
    }
    is_unity
        && depth == 1
        && matches!(
            name,
            "Library" | "Temp" | "Logs" | "UserSettings" | "MemoryCaptures" | "Builds"
        )
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub path: PathBuf,
    pub name: String,
    pub rel_path: String,
    pub is_dir: bool,
    pub kind: Option<FileKind>,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone)]
pub struct VaultFile {
    pub path: PathBuf,
    pub name: String,
    pub stem: String,
    pub rel_path: String,
    pub kind: FileKind,
}

#[derive(Debug, Clone)]
pub struct BacklinkEntry {
    pub source_path: PathBuf,
    pub source_name: String,
    pub line_num: usize,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineHeading {
    pub level: usize,
    pub title: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub path: PathBuf,
    pub rel_path: String,
    pub line_num: usize,
    pub line_text: String,
}

#[derive(Debug, Clone)]
pub struct VaultIndex {
    pub root: PathBuf,
    pub tree: Vec<TreeNode>,
    pub all_files: Vec<VaultFile>,
    pub notes_by_stem: HashMap<String, Vec<PathBuf>>,
    pub assets_by_name: HashMap<String, Vec<PathBuf>>,
    pub backlinks: HashMap<PathBuf, Vec<BacklinkEntry>>,
    pub md_count: usize,
}

impl VaultIndex {
    pub fn build(root: &Path, show_hidden: bool) -> Self {
        let mut all_files = Vec::new();
        let mut notes_by_stem: HashMap<String, Vec<PathBuf>> = HashMap::new();
        let mut assets_by_name: HashMap<String, Vec<PathBuf>> = HashMap::new();
        let mut md_count = 0usize;
        let is_unity = is_unity_project(root);

        let tree = read_dir_tree(
            root,
            root,
            1,
            is_unity,
            show_hidden,
            &mut all_files,
            &mut notes_by_stem,
            &mut assets_by_name,
            &mut md_count,
        );

        let mut index = Self {
            root: root.to_path_buf(),
            tree,
            all_files,
            notes_by_stem,
            assets_by_name,
            backlinks: HashMap::new(),
            md_count,
        };

        index.build_backlinks();
        index
    }

    fn build_backlinks(&mut self) {
        let md_files: Vec<VaultFile> = self
            .all_files
            .iter()
            .filter(|f| matches!(f.kind, FileKind::Markdown | FileKind::Mdx))
            .cloned()
            .collect();

        let mut map: HashMap<PathBuf, Vec<BacklinkEntry>> = HashMap::new();

        for file in md_files {
            let Ok(content) = fs::read_to_string(&file.path) else {
                continue;
            };
            let mut in_fence = false;
            let mut seen_for_file: HashSet<(PathBuf, usize)> = HashSet::new();

            for (idx, line) in content.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                    in_fence = !in_fence;
                    continue;
                }
                if in_fence {
                    continue;
                }
                for link_target in extract_wikilink_targets(line) {
                    if let Some(dest) = self.resolve_wikilink(&link_target, &file.path)
                        && dest != file.path
                        && seen_for_file.insert((dest.clone(), idx + 1))
                    {
                        map.entry(dest).or_default().push(BacklinkEntry {
                            source_path: file.path.clone(),
                            source_name: file.stem.clone(),
                            line_num: idx + 1,
                            snippet: line.trim().chars().take(120).collect(),
                        });
                    }
                }
            }
        }

        self.backlinks = map;
    }

    pub fn resolve_wikilink(&self, raw_target: &str, from_file: &Path) -> Option<PathBuf> {
        let clean = raw_target
            .split('|')
            .next()
            .unwrap_or(raw_target)
            .split('#')
            .next()
            .unwrap_or("")
            .trim();
        if clean.is_empty() {
            return None;
        }

        let candidates = if clean.ends_with(".md") || clean.ends_with(".markdown") {
            vec![clean.to_string()]
        } else {
            vec![format!("{clean}.md"), clean.to_string()]
        };

        if let Some(parent) = from_file.parent() {
            for cand in &candidates {
                let p = parent.join(cand);
                if p.is_file() {
                    return Some(p);
                }
            }
        }

        for cand in &candidates {
            let p = self.root.join(cand);
            if p.is_file() {
                return Some(p);
            }
        }

        // Case-insensitive match by relative path suffix or stem
        let lower = clean.to_ascii_lowercase();
        let lower_no_ext = lower.strip_suffix(".md").unwrap_or(&lower).to_string();

        if lower.contains('/') || lower.contains('\\') {
            let norm = lower.replace('\\', "/");
            let norm_md = if norm.ends_with(".md") {
                norm.clone()
            } else {
                format!("{norm}.md")
            };
            if let Some(f) = self.all_files.iter().find(|f| {
                let r = f.rel_path.to_ascii_lowercase().replace('\\', "/");
                r == norm_md || r == norm || r.ends_with(&format!("/{norm_md}"))
            }) {
                return Some(f.path.clone());
            }
        }

        if let Some(paths) = self.notes_by_stem.get(&lower_no_ext)
            && !paths.is_empty()
        {
            if paths.len() == 1 {
                return Some(paths[0].clone());
            }
            // Prefer closest sibling to `from_file`
            let from_dir = from_file.parent().unwrap_or(&self.root);
            let best = paths
                .iter()
                .min_by_key(|p| {
                    let common = common_prefix_len(from_dir, p);
                    (usize::MAX - common, p.as_os_str().len())
                })
                .cloned();
            return best;
        }

        None
    }

    pub fn resolve_asset(&self, raw_target: &str, from_file: &Path) -> Option<PathBuf> {
        let clean = raw_target
            .split('|')
            .next()
            .unwrap_or(raw_target)
            .split('#')
            .next()
            .unwrap_or("")
            .trim();
        if clean.is_empty() {
            return None;
        }

        if clean.starts_with('/') {
            let trimmed = clean.trim_start_matches('/');
            let pub_candidate = self.root.join("public").join(trimmed);
            if pub_candidate.is_file() {
                return Some(pub_candidate);
            }
            let root_candidate = self.root.join(trimmed);
            if root_candidate.is_file() {
                return Some(root_candidate);
            }
        }

        if let Some(parent) = from_file.parent() {
            let rel = parent.join(clean);
            if rel.is_file() {
                return Some(rel);
            }
        }

        let ws = self.root.join(clean);
        if ws.is_file() {
            return Some(ws);
        }

        let file_part = Path::new(clean)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(clean)
            .to_ascii_lowercase();

        if let Some(paths) = self.assets_by_name.get(&file_part)
            && !paths.is_empty()
        {
            if paths.len() == 1 {
                return Some(paths[0].clone());
            }
            let from_dir = from_file.parent().unwrap_or(&self.root);
            return paths
                .iter()
                .min_by_key(|p| {
                    let common = common_prefix_len(from_dir, p);
                    (usize::MAX - common, p.as_os_str().len())
                })
                .cloned();
        }

        None
    }

    pub fn fuzzy_search_files(&self, query: &str, limit: usize) -> Vec<&VaultFile> {
        let q = query.trim().to_ascii_lowercase();
        if q.is_empty() {
            return self.all_files.iter().take(limit).collect();
        }

        let mut scored: Vec<(i32, &VaultFile)> = self
            .all_files
            .iter()
            .filter_map(|f| {
                let name_l = f.name.to_ascii_lowercase();
                let rel_l = f.rel_path.to_ascii_lowercase();
                let stem_l = f.stem.to_ascii_lowercase();

                let score = if stem_l == q || name_l == q {
                    10_000
                } else if name_l.starts_with(&q) {
                    5_000 - (name_l.len() as i32)
                } else if let Some(pos) = name_l.find(&q) {
                    3_000 - (pos as i32 * 4) - (name_l.len() as i32)
                } else if let Some(pos) = rel_l.find(&q) {
                    1_500 - (pos as i32 * 2) - (rel_l.len() as i32)
                } else {
                    subsequence_score(&rel_l, &q)?
                };
                Some((score, f))
            })
            .collect();

        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.rel_path.to_ascii_lowercase().cmp(&b.1.rel_path.to_ascii_lowercase()))
        });
        scored.into_iter().take(limit).map(|(_, f)| f).collect()
    }
}

fn common_prefix_len(a: &Path, b: &Path) -> usize {
    a.components()
        .zip(b.components())
        .take_while(|(ca, cb)| ca == cb)
        .count()
}

fn subsequence_score(haystack: &str, needle: &str) -> Option<i32> {
    let mut n_chars = needle.chars();
    let mut cur = n_chars.next()?;
    let mut gaps = 0i32;
    let mut matched_span = 0i32;
    let mut started = false;

    for ch in haystack.chars() {
        if started {
            matched_span += 1;
        }
        if ch == cur {
            started = true;
            if let Some(next) = n_chars.next() {
                cur = next;
            } else {
                return Some(500 - gaps - matched_span);
            }
        } else if started {
            gaps += 1;
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn read_dir_tree(
    dir: &Path,
    root: &Path,
    depth: usize,
    is_unity: bool,
    show_hidden: bool,
    all_files: &mut Vec<VaultFile>,
    notes_by_stem: &mut HashMap<String, Vec<PathBuf>>,
    assets_by_name: &mut HashMap<String, Vec<PathBuf>>,
    md_count: &mut usize,
) -> Vec<TreeNode> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = path.is_dir();

        if is_dir {
            if is_ignored_dir_for_root(&name, depth, is_unity, show_hidden) {
                continue;
            }
            let rel_path = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let children = read_dir_tree(
                &path,
                root,
                depth + 1,
                is_unity,
                show_hidden,
                all_files,
                notes_by_stem,
                assets_by_name,
                md_count,
            );
            dirs.push(TreeNode {
                path,
                name,
                rel_path,
                is_dir: true,
                kind: None,
                children,
            });
        } else {
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            if name.ends_with(".vkarma-tmp") {
                continue;
            }
            let rel_path = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let kind = detect_file_kind(&path);
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&name)
                .to_string();

            if matches!(kind, FileKind::Markdown | FileKind::Mdx) {
                *md_count += 1;
                notes_by_stem
                    .entry(stem.to_ascii_lowercase())
                    .or_default()
                    .push(path.clone());
            } else {
                assets_by_name
                    .entry(name.to_ascii_lowercase())
                    .or_default()
                    .push(path.clone());
            }

            all_files.push(VaultFile {
                path: path.clone(),
                name: name.clone(),
                stem,
                rel_path: rel_path.clone(),
                kind,
            });

            files.push(TreeNode {
                path,
                name,
                rel_path,
                is_dir: false,
                kind: Some(kind),
                children: Vec::new(),
            });
        }
    }

    dirs.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    files.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    dirs.extend(files);
    dirs
}

fn extract_wikilink_targets(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let is_embed = i > 0 && bytes[i - 1] == b'!';
            let start = i + 2;
            if let Some(rel_end) = line[start..].find("]]") {
                let inner = &line[start..start + rel_end];
                let clean = inner
                    .split('|')
                    .next()
                    .unwrap_or(inner)
                    .split('#')
                    .next()
                    .unwrap_or("")
                    .trim();
                if !clean.is_empty() && (!is_embed || !is_image_extension(clean)) {
                    out.push(clean.to_string());
                }
                i = start + rel_end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

pub fn search_content(
    root: &Path,
    query: &str,
    show_hidden: bool,
    max_hits: usize,
) -> Vec<SearchHit> {
    let q = query.trim().to_ascii_lowercase();
    if q.len() < 2 {
        return Vec::new();
    }

    let mut hits = Vec::new();
    let is_unity = is_unity_project(root);
    let walker = WalkDir::new(root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        if e.file_type().is_dir() {
            return e.depth() == 0
                || !is_ignored_dir_for_root(&name, e.depth(), is_unity, show_hidden);
        }
        show_hidden || !name.starts_with('.')
    });

    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let kind = detect_file_kind(path);
        if matches!(kind, FileKind::Image)
            || path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
        {
            continue;
        }
        if let Ok(meta) = entry.metadata()
            && meta.len() > 2 * 1024 * 1024
        {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let rel_path = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        for (idx, line) in content.lines().enumerate() {
            if line.to_ascii_lowercase().contains(&q) {
                hits.push(SearchHit {
                    path: path.to_path_buf(),
                    rel_path: rel_path.clone(),
                    line_num: idx + 1,
                    line_text: line.trim().chars().take(140).collect(),
                });
                if hits.len() >= max_hits {
                    return hits;
                }
            }
        }
    }

    hits
}

pub fn extract_headings(content: &str, is_typst: bool) -> Vec<OutlineHeading> {
    let mut out = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut start_idx = 0;

    // Skip Markdown YAML frontmatter block
    if !is_typst && lines.first().is_some_and(|l| l.trim() == "---") {
        for (i, line) in lines.iter().enumerate().skip(1) {
            if line.trim() == "---" || line.trim() == "..." {
                start_idx = i + 1;
                break;
            }
        }
    }

    let mut in_fence = false;
    for (idx, line) in lines.iter().enumerate().skip(start_idx) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || (!is_typst && trimmed.starts_with("~~~")) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        if is_typst {
            let eq_count = trimmed.chars().take_while(|c| *c == '=').count();
            if (1..=6).contains(&eq_count) && trimmed[eq_count..].starts_with(char::is_whitespace) {
                let title = trimmed[eq_count..].trim().to_string();
                if !title.is_empty() {
                    out.push(OutlineHeading {
                        level: eq_count,
                        title,
                        line: idx + 1,
                    });
                }
            }
        } else {
            let hash_count = trimmed.chars().take_while(|c| *c == '#').count();
            if (1..=6).contains(&hash_count)
                && trimmed[hash_count..].starts_with(char::is_whitespace)
            {
                let title = trimmed[hash_count..]
                    .trim()
                    .trim_end_matches('#')
                    .trim()
                    .to_string();
                if !title.is_empty() {
                    out.push(OutlineHeading {
                        level: hash_count,
                        title,
                        line: idx + 1,
                    });
                }
            }
        }
    }

    out
}

pub fn atomic_write_string(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent dir {}", parent.display()))?;
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let tmp = path.with_file_name(format!(".{file_name}.vkarma-tmp"));
    fs::write(&tmp, content).with_context(|| format!("write temp file {}", tmp.display()))?;
    if fs::rename(&tmp, path).is_err() {
        let _ = fs::remove_file(&tmp);
        fs::write(path, content).with_context(|| format!("write file {}", path.display()))?;
    }
    Ok(())
}

pub fn create_new_file(path: &Path, initial_content: &str) -> Result<()> {
    if path.exists() {
        anyhow::bail!("File already exists: {}", path.display());
    }
    atomic_write_string(path, initial_content)
}

pub fn create_new_folder(path: &Path) -> Result<()> {
    if path.exists() {
        anyhow::bail!("Folder already exists: {}", path.display());
    }
    fs::create_dir_all(path).with_context(|| format!("create folder {}", path.display()))
}

pub fn rename_entry(old_path: &Path, new_name: &str) -> Result<PathBuf> {
    let trimmed = new_name.trim();
    if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains('\\') {
        anyhow::bail!("Invalid file name: {new_name:?}");
    }
    let new_path = old_path.with_file_name(trimmed);
    if new_path.exists() {
        anyhow::bail!("Target already exists: {}", new_path.display());
    }
    fs::rename(old_path, &new_path).with_context(|| {
        format!(
            "rename {} to {}",
            old_path.display(),
            new_path.display()
        )
    })?;
    Ok(new_path)
}

pub fn duplicate_file(path: &Path) -> Result<PathBuf> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = path.extension().and_then(|e| e.to_str());
    for i in 1..1000 {
        let candidate_name = match (i, ext) {
            (1, Some(e)) => format!("{stem} copy.{e}"),
            (1, None) => format!("{stem} copy"),
            (n, Some(e)) => format!("{stem} copy {n}.{e}"),
            (n, None) => format!("{stem} copy {n}"),
        };
        let candidate = path.with_file_name(candidate_name);
        if !candidate.exists() {
            fs::copy(path, &candidate)
                .with_context(|| format!("duplicate {}", path.display()))?;
            return Ok(candidate);
        }
    }
    anyhow::bail!("Could not find unique duplicate filename")
}

pub fn delete_entry(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
            .with_context(|| format!("delete folder {}", path.display()))
    } else {
        fs::remove_file(path).with_context(|| format!("delete file {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn vault_indexes_resolves_wikilinks_assets_and_backlinks() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("folder/sub")).unwrap();
        fs::create_dir_all(root.join("attachments")).unwrap();
        fs::create_dir_all(root.join(".obsidian")).unwrap();

        fs::write(root.join(".obsidian/app.json"), "{}").unwrap();
        fs::write(root.join("Index.md"), "# Home\nLink to [[Sub Note#Part|Alias]]\n![[pic.png|400]]\n").unwrap();
        fs::write(root.join("folder/sub/Sub Note.md"), "# Sub Note\nBack [[Index]]").unwrap();
        fs::write(root.join("attachments/pic.png"), [137u8, 80, 78, 71]).unwrap();

        let vault = VaultIndex::build(root, false);
        assert_eq!(vault.md_count, 2);
        assert!(!vault.all_files.iter().any(|f| f.name == "app.json"));

        let resolved = vault
            .resolve_wikilink("Sub Note#Part|Alias", &root.join("Index.md"))
            .unwrap();
        assert_eq!(resolved, root.join("folder/sub/Sub Note.md"));

        let asset = vault
            .resolve_asset("pic.png", &root.join("Index.md"))
            .unwrap();
        assert_eq!(asset, root.join("attachments/pic.png"));

        let bl = vault
            .backlinks
            .get(&root.join("folder/sub/Sub Note.md"))
            .unwrap();
        assert_eq!(bl.len(), 1);
        assert_eq!(bl[0].source_name, "Index");
        assert_eq!(bl[0].line_num, 2);

        let hits = search_content(root, "Sub Note", false, 20);
        assert!(hits.len() >= 2);

        let fuzzy = vault.fuzzy_search_files("sub", 5);
        assert_eq!(fuzzy[0].name, "Sub Note.md");
    }

    #[test]
    fn extracts_headings_skipping_frontmatter_and_fences() {
        let md = "---\ntitle: # Not Heading\n---\n\n# Top\n```bash\n# Comment\n```\n## Section\n";
        let md_h = extract_headings(md, false);
        assert_eq!(
            md_h,
            vec![
                OutlineHeading {
                    level: 1,
                    title: "Top".into(),
                    line: 5
                },
                OutlineHeading {
                    level: 2,
                    title: "Section".into(),
                    line: 9
                }
            ]
        );

        let typ = "= Title\n#set text(size: 11pt)\n== Sub\n```typ\n= Fake\n```\n";
        let typ_h = extract_headings(typ, true);
        assert_eq!(
            typ_h,
            vec![
                OutlineHeading {
                    level: 1,
                    title: "Title".into(),
                    line: 1
                },
                OutlineHeading {
                    level: 2,
                    title: "Sub".into(),
                    line: 3
                }
            ]
        );
    }

    #[test]
    fn ignored_dir_for_root_skips_unity_caches_at_depth_one_only() {
        assert!(is_ignored_dir_for_root("Library", 1, true, false));
        assert!(is_ignored_dir_for_root("Temp", 1, true, true));
        assert!(!is_ignored_dir_for_root("Library", 2, true, false));
        assert!(!is_ignored_dir_for_root("Library", 1, false, false));
        assert!(is_ignored_dir_for_root("obj", 2, false, false));
    }
}
