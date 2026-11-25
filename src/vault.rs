use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Markdown,
    Mdx,
    Canvas,
    Kanban,
    Subtitle,
    Fountain,
    Typst,
    Spreadsheet,
    Image,
    Svg,
    Mermaid,
    Office,
    Epub,
    Pdf,
    Sqlite,
    Archive,
    Audio,
    Font,
    Code(&'static str),
    PlainText,
}

impl FileKind {
    pub fn is_binary(&self) -> bool {
        matches!(
            self,
            Self::Image
                | Self::Pdf
                | Self::Office
                | Self::Epub
                | Self::Sqlite
                | Self::Archive
                | Self::Audio
                | Self::Font
        )
    }

    pub fn short_badge(&self) -> &'static str {
        match self {
            Self::Markdown => "MD",
            Self::Mdx => "MDX",
            Self::Canvas => "CNV",
            Self::Kanban => "KBN",
            Self::Subtitle => "SUB",
            Self::Fountain => "FTN",
            Self::Typst => "TYP",
            Self::Spreadsheet => "XLS",
            Self::Image => "IMG",
            Self::Svg => "SVG",
            Self::Mermaid => "MMD",
            Self::Office => "DOC",
            Self::Epub => "EPUB",
            Self::Pdf => "PDF",
            Self::Sqlite => "DB",
            Self::Archive => "ZIP",
            Self::Audio => "AUD",
            Self::Font => "FNT",
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
    if file_name.ends_with(".kanban.md") {
        return FileKind::Kanban;
    }
    if file_name.ends_with(".tar.gz") {
        return FileKind::Archive;
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match ext.as_str() {
        "md" | "markdown" => FileKind::Markdown,
        "mdx" => FileKind::Mdx,
        "canvas" => FileKind::Canvas,
        "kanban" => FileKind::Kanban,
        "srt" | "vtt" => FileKind::Subtitle,
        "fountain" => FileKind::Fountain,
        "typ" => FileKind::Typst,
        "xlsx" | "csv" | "tsv" => FileKind::Spreadsheet,
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => FileKind::Image,
        "svg" => FileKind::Svg,
        "mmd" | "mermaid" => FileKind::Mermaid,
        "docx" | "odt" => FileKind::Office,
        "epub" => FileKind::Epub,
        "pdf" => FileKind::Pdf,
        "sqlite" | "sqlite3" | "db" => FileKind::Sqlite,
        "zip" | "tar" | "tgz" => FileKind::Archive,
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "opus" => FileKind::Audio,
        "ttf" | "otf" => FileKind::Font,
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
    pub is_title_match: bool,
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
        let mut clean = raw_target
            .split('|')
            .next()
            .unwrap_or(raw_target)
            .split('#')
            .next()
            .unwrap_or("")
            .trim();
        while let Some(rest) = clean.strip_prefix("./").or_else(|| clean.strip_prefix(".\\")) {
            clean = rest;
        }
        if clean.is_empty() {
            return None;
        }

        let final_seg = clean.rsplit(['/', '\\']).next().unwrap_or(clean);
        let has_explicit_ext = final_seg
            .rfind('.')
            .is_some_and(|dot| dot > 0 && dot + 1 < final_seg.len());
        let lower_clean = clean.to_ascii_lowercase();
        let ends_with_md = lower_clean.ends_with(".md") || lower_clean.ends_with(".markdown");

        // Pass 0: direct sibling or vault-root lookup; check explicit extensions verbatim first.
        let candidates = if ends_with_md {
            vec![clean.to_string()]
        } else if has_explicit_ext {
            vec![clean.to_string(), format!("{clean}.md")]
        } else {
            vec![format!("{clean}.md"), clean.to_string()]
        };

        if let Some(parent) = from_file.parent() {
            for cand in &candidates {
                let p = normalize_lexical_path(&parent.join(cand));
                if p.is_file() {
                    return Some(p);
                }
            }
        }

        for cand in &candidates {
            let p = normalize_lexical_path(&self.root.join(cand));
            if p.is_file() {
                return Some(p);
            }
        }

        // Pass 1: case-insensitive vault-wide search across all files (markdown + non-markdown).
        let norm = clean
            .trim_start_matches(['/', '\\'])
            .replace('\\', "/")
            .to_ascii_lowercase();
        if norm.is_empty() {
            return None;
        }
        let norm_md = (!ends_with_md).then(|| format!("{norm}.md"));
        let from_dir = from_file.parent().unwrap_or(&self.root);

        let pick_best = |mut matches: Vec<&VaultFile>, prefer_md: bool| -> Option<PathBuf> {
            if matches.is_empty() {
                return None;
            }
            matches.sort_by_key(|f| {
                let md_rank = if prefer_md && !matches!(f.kind, FileKind::Markdown | FileKind::Mdx) {
                    1u8
                } else {
                    0u8
                };
                let common = common_prefix_len(from_dir, &f.path);
                (
                    md_rank,
                    usize::MAX - common,
                    f.path.as_os_str().len(),
                    f.rel_path.to_ascii_lowercase(),
                )
            });
            Some(matches[0].path.clone())
        };

        // 1a. Exact relative path or "/filename.ext" suffix match (all extensions, all folders).
        let suffix = format!("/{norm}");
        let exact_matches: Vec<&VaultFile> = self
            .all_files
            .iter()
            .filter(|f| {
                let rel = f.rel_path.to_ascii_lowercase().replace('\\', "/");
                rel == norm || rel.ends_with(&suffix)
            })
            .collect();
        if let Some(best) = pick_best(exact_matches, false) {
            return Some(best);
        }

        // 1b. Exact "<target>.md" relative path or "/<target>.md" suffix match.
        if let Some(ref nmd) = norm_md {
            let suffix_md = format!("/{nmd}");
            let md_matches: Vec<&VaultFile> = self
                .all_files
                .iter()
                .filter(|f| {
                    let rel = f.rel_path.to_ascii_lowercase().replace('\\', "/");
                    rel == *nmd || rel.ends_with(&suffix_md)
                })
                .collect();
            if let Some(best) = pick_best(md_matches, false) {
                return Some(best);
            }
        }

        // 1c. Extensionless path/stem suffix match across any file kind, preferring Markdown when
        // no explicit extension was requested.
        let stem_matches: Vec<&VaultFile> = self
            .all_files
            .iter()
            .filter(|f| {
                let rel = f.rel_path.to_ascii_lowercase().replace('\\', "/");
                let rel_no_ext = strip_final_ext(&rel);
                rel_no_ext == norm || rel_no_ext.ends_with(&suffix)
            })
            .collect();
        if let Some(best) = pick_best(stem_matches, !has_explicit_ext) {
            return Some(best);
        }

        // Backup stem lookup for markdown notes.
        let lower_no_ext = norm.strip_suffix(".md").unwrap_or(&norm).to_string();
        if !norm.contains('/')
            && !has_explicit_ext
            && let Some(paths) = self.notes_by_stem.get(&lower_no_ext)
            && !paths.is_empty()
        {
            if paths.len() == 1 {
                return Some(paths[0].clone());
            }
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

fn normalize_lexical_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !out.has_root() {
                    out.push(comp);
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn strip_final_ext(rel_lower: &str) -> &str {
    let seg_start = rel_lower.rfind('/').map_or(0, |i| i + 1);
    let last_seg = &rel_lower[seg_start..];
    if let Some(dot) = last_seg.rfind('.')
        && dot > 0
    {
        &rel_lower[..seg_start + dot]
    } else {
        rel_lower
    }
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

fn first_preview_line(content: &str, is_md: bool) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut start_idx = 0;

    if is_md && lines.first().is_some_and(|l| l.trim() == "---") {
        for (i, line) in lines.iter().enumerate().skip(1) {
            if line.trim() == "---" || line.trim() == "..." {
                start_idx = i + 1;
                break;
            }
        }
    }

    lines
        .iter()
        .skip(start_idx)
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(140).collect())
}

fn score_title_match(
    rel_path: &str,
    name: &str,
    stem: &str,
    q: &str,
    q_tokens: &[&str],
) -> Option<i32> {
    let name_l = name.to_lowercase();
    let stem_l = stem.to_lowercase();
    let rel_l = rel_path.to_lowercase();

    if stem_l == q || name_l == q {
        Some(10_000)
    } else if stem_l.starts_with(q) || name_l.starts_with(q) {
        Some(5_000 - (name_l.len() as i32))
    } else if let Some(pos) = name_l.find(q) {
        Some(3_000 - (pos as i32 * 4) - (name_l.len() as i32))
    } else if q_tokens.len() > 1 && q_tokens.iter().all(|t| name_l.contains(t)) {
        Some(2_000 - (name_l.len() as i32))
    } else if let Some(pos) = rel_l.find(q) {
        Some(1_500 - (pos as i32 * 2) - (rel_l.len() as i32))
    } else if q_tokens.len() > 1 && q_tokens.iter().all(|t| rel_l.contains(t)) {
        Some(1_000 - (rel_l.len() as i32))
    } else {
        None
    }
}

pub fn search_content(
    root: &Path,
    query: &str,
    show_hidden: bool,
    max_hits: usize,
) -> Vec<SearchHit> {
    let q = query.trim().to_lowercase();
    if q.chars().count() < 2 || max_hits == 0 {
        return Vec::new();
    }
    let q_tokens: Vec<&str> = q.split_whitespace().collect();

    let mut title_hits: Vec<(i32, SearchHit)> = Vec::new();
    let mut content_hits: Vec<SearchHit> = Vec::new();
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
        let rel_path = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(name);
        let title_score = score_title_match(&rel_path, name, stem, &q, &q_tokens);

        let kind = detect_file_kind(path);
        let is_binary = kind.is_binary();
        let is_xlsx = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"));
        let within_size = entry
            .metadata()
            .map(|m| m.len() <= 2 * 1024 * 1024)
            .unwrap_or(false);

        let content_opt = if !is_binary
            && !is_xlsx
            && within_size
            && (title_score.is_some() || content_hits.len() < max_hits)
        {
            fs::read_to_string(path).ok()
        } else {
            None
        };

        if let Some(score) = title_score {
            let is_md = matches!(kind, FileKind::Markdown | FileKind::Mdx);
            let line_text = content_opt
                .as_deref()
                .and_then(|c| first_preview_line(c, is_md))
                .unwrap_or_else(|| {
                    if matches!(kind, FileKind::Image) {
                        "Image".to_string()
                    } else if matches!(kind, FileKind::Pdf) {
                        "PDF document".to_string()
                    } else if matches!(kind, FileKind::Office) {
                        "Office document".to_string()
                    } else if matches!(kind, FileKind::Epub) {
                        "EPUB eBook".to_string()
                    } else if matches!(kind, FileKind::Sqlite) {
                        "SQLite database".to_string()
                    } else if matches!(kind, FileKind::Archive) {
                        "Archive".to_string()
                    } else if matches!(kind, FileKind::Audio) {
                        "Audio file".to_string()
                    } else if matches!(kind, FileKind::Font) {
                        "Font file".to_string()
                    } else if is_xlsx {
                        "Spreadsheet".to_string()
                    } else {
                        "Title match".to_string()
                    }
                });
            title_hits.push((
                score,
                SearchHit {
                    path: path.to_path_buf(),
                    rel_path: rel_path.clone(),
                    line_num: 0,
                    line_text,
                    is_title_match: true,
                },
            ));
        }

        if content_hits.len() < max_hits
            && let Some(content) = content_opt.as_deref()
        {
            for (idx, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&q) {
                    content_hits.push(SearchHit {
                        path: path.to_path_buf(),
                        rel_path: rel_path.clone(),
                        line_num: idx + 1,
                        line_text: line.trim().chars().take(140).collect(),
                        is_title_match: false,
                    });
                    if content_hits.len() >= max_hits {
                        break;
                    }
                }
            }
        }
    }

    title_hits.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.rel_path.to_lowercase().cmp(&b.1.rel_path.to_lowercase()))
    });

    let mut out: Vec<SearchHit> = title_hits.into_iter().map(|(_, hit)| hit).collect();
    out.extend(content_hits);
    out.truncate(max_hits);
    out
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

pub use vkarma_core::fs_util::atomic_write_string;

pub fn normalize_new_file_name(raw_name: &str) -> String {
    let trimmed = raw_name.trim().trim_start_matches(['/', '\\']);
    if trimmed.is_empty() {
        return String::new();
    }
    let last_seg = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    if last_seg.is_empty() || last_seg == "." || last_seg == ".." {
        return trimmed.to_string();
    }
    if last_seg.starts_with('.') {
        return trimmed.to_string();
    }
    let lower_seg = last_seg.to_ascii_lowercase();
    if matches!(
        lower_seg.as_str(),
        "makefile"
            | "gnumakefile"
            | "justfile"
            | "dockerfile"
            | "license"
            | "readme"
            | "procfile"
            | "gemfile"
            | "rakefile"
            | "vagrantfile"
    ) {
        return trimmed.to_string();
    }
    if last_seg.ends_with('.') {
        let base = trimmed.trim_end_matches('.');
        if base.is_empty() {
            return trimmed.to_string();
        }
        return format!("{base}.md");
    }
    if last_seg.contains('.') {
        return trimmed.to_string();
    }
    format!("{trimmed}.md")
}

pub fn create_new_file(path: &Path, initial_content: &str) -> Result<()> {
    if path.exists() {
        anyhow::bail!("File already exists: {}", path.display());
    }
    atomic_write_string(path, initial_content)
}

pub fn create_adaptive_file(dest: &Path) -> Result<()> {
    if dest.exists() {
        anyhow::bail!("File already exists: {}", dest.display());
    }
    let kind = detect_file_kind(dest);
    let ext = dest
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let title = dest
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Untitled");

    match kind {
        FileKind::Markdown | FileKind::Mdx => create_new_file(dest, &format!("# {title}\n\n")),
        FileKind::Canvas => {
            let mut doc = crate::formats::canvas::CanvasDoc::default();
            doc.add_text_node(0.0, 0.0, format!("# {title}\n\nDouble-click to edit card."));
            create_new_file(dest, &doc.to_json_pretty())
        }
        FileKind::Kanban => {
            let k_title = title.strip_suffix(".kanban").unwrap_or(title);
            let starter = format!(
                "# {k_title}\n\n## Todo\n- [ ] First task\n\n## Doing\n\n## Done\n"
            );
            create_new_file(dest, &starter)
        }
        FileKind::Mermaid => {
            let starter = "flowchart TD\n    A[Start] --> B{Ready?}\n    B -->|Yes| C([Launch])\n    B -->|No| D[(Revise)]\n";
            create_new_file(dest, starter)
        }
        FileKind::Svg => {
            let starter = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"240\" height=\"160\" viewBox=\"0 0 240 160\">\n  <rect x=\"20\" y=\"20\" width=\"200\" height=\"120\" rx=\"14\" fill=\"#222020\" stroke=\"#8ba4b0\" stroke-width=\"3\"/>\n  <text x=\"120\" y=\"86\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"18\" fill=\"#c5c9c5\">VKarma SVG</text>\n</svg>\n";
            create_new_file(dest, starter)
        }
        FileKind::Typst => create_new_file(
            dest,
            &format!(
                "#set page(paper: \"a4\", margin: 2cm)\n#set text(size: 11pt)\n\n= {title}\n\nWrite Typst content here.\n"
            ),
        ),
        FileKind::Spreadsheet => {
            if ext == "xlsx" {
                if let Some(parent) = dest.parent()
                    && !parent.as_os_str().is_empty()
                {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("create parent dir {}", parent.display()))?;
                }
                crate::spreadsheet::SpreadsheetDoc::new_empty(dest)
                    .and_then(|mut doc| doc.save(dest))
            } else if ext == "tsv" {
                create_new_file(dest, "Col1\tCol2\n")
            } else {
                create_new_file(dest, "Col1,Col2\n")
            }
        }
        FileKind::Fountain => {
            let f_title = if title == "Untitled" {
                "Untitled Screenplay"
            } else {
                title
            };
            let starter = format!(
                "Title: {f_title}\nCredit: Written by\nAuthor: \nDraft date: \n\n===\n\nFADE IN:\n\nINT. LOCATION - DAY\n\nOpening beat.\n\nCHARACTER\nFirst line of dialogue.\n\nFADE OUT.\n"
            );
            create_new_file(dest, &starter)
        }
        FileKind::Subtitle => {
            if ext == "vtt" {
                create_new_file(
                    dest,
                    "WEBVTT\n\n00:00:00.000 --> 00:00:03.000\nFirst subtitle.\n",
                )
            } else {
                create_new_file(
                    dest,
                    "1\n00:00:00,000 --> 00:00:03,000\nFirst subtitle.\n",
                )
            }
        }
        FileKind::Code(_) | FileKind::PlainText | _ => create_new_file(dest, ""),
    }
}

/// Finds all non-overlapping UTF-8 byte ranges in `haystack` matching `query`.
pub fn find_all_byte_ranges(
    haystack: &str,
    query: &str,
    case_sensitive: bool,
) -> Vec<std::ops::Range<usize>> {
    if query.is_empty() || haystack.len() < (if case_sensitive { query.len() } else { 1 }) {
        return Vec::new();
    }
    if case_sensitive {
        let mut out = Vec::new();
        let mut pos = 0;
        while let Some(rel) = haystack[pos..].find(query) {
            let start = pos + rel;
            let end = start + query.len();
            out.push(start..end);
            pos = end;
        }
        return out;
    }
    if haystack.is_ascii() && query.is_ascii() {
        let hb = haystack.as_bytes();
        let qb = query.as_bytes();
        let qlen = qb.len();
        let mut out = Vec::new();
        let mut i = 0;
        while i + qlen <= hb.len() {
            if hb[i..i + qlen].eq_ignore_ascii_case(qb) {
                out.push(i..i + qlen);
                i += qlen;
            } else {
                i += 1;
            }
        }
        return out;
    }

    let q_chars: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if q_chars.is_empty() {
        return Vec::new();
    }
    let mut lowered: Vec<(usize, usize, char)> = Vec::with_capacity(haystack.len());
    for (byte_start, ch) in haystack.char_indices() {
        let byte_end = byte_start + ch.len_utf8();
        for lc in ch.to_lowercase() {
            lowered.push((byte_start, byte_end, lc));
        }
    }
    let qlen = q_chars.len();
    let mut out = Vec::new();
    let mut k = 0;
    while k + qlen <= lowered.len() {
        let starts_char = k == 0 || lowered[k].0 != lowered[k - 1].0;
        let end_k = k + qlen;
        let ends_char = end_k == lowered.len() || lowered[end_k].0 != lowered[end_k - 1].0;
        if starts_char
            && ends_char
            && lowered[k..end_k]
                .iter()
                .zip(q_chars.iter())
                .all(|(a, b)| a.2 == *b)
        {
            out.push(lowered[k].0..lowered[end_k - 1].1);
            k = end_k;
        } else {
            k += 1;
        }
    }
    out
}

/// Merges base highlight styles with search match ranges into strictly sorted, non-overlapping spans.
pub fn merge_highlight_ranges<T: Clone>(
    base: &[(std::ops::Range<usize>, T)],
    matches: &[std::ops::Range<usize>],
    active_rel: Option<usize>,
    make_style: impl Fn(Option<&T>, bool) -> T,
) -> Vec<(std::ops::Range<usize>, T)> {
    if matches.is_empty() {
        return base.to_vec();
    }
    let mut pts = Vec::with_capacity((base.len() + matches.len()) * 2);
    for (r, _) in base {
        if r.start < r.end {
            pts.push(r.start);
            pts.push(r.end);
        }
    }
    for r in matches {
        if r.start < r.end {
            pts.push(r.start);
            pts.push(r.end);
        }
    }
    if pts.len() < 2 {
        return Vec::new();
    }
    pts.sort_unstable();
    pts.dedup();

    let mut out = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let base_style = base
            .iter()
            .rev()
            .find(|(r, _)| r.start <= a && r.end >= b)
            .map(|(_, s)| s);
        let match_ix = matches.iter().position(|r| r.start <= a && r.end >= b);
        match (base_style, match_ix) {
            (None, None) => continue,
            (Some(bs), None) => out.push((a..b, bs.clone())),
            (bs, Some(ix)) => out.push((a..b, make_style(bs, active_rel == Some(ix)))),
        }
    }
    out
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

pub fn move_entry(src_path: &Path, dest_dir: &Path) -> Result<PathBuf> {
    let src_canon = fs::canonicalize(src_path).unwrap_or_else(|_| src_path.to_path_buf());
    let dest_canon = fs::canonicalize(dest_dir).unwrap_or_else(|_| dest_dir.to_path_buf());
    if !dest_canon.is_dir() {
        anyhow::bail!("Destination is not a folder: {}", dest_dir.display());
    }
    if src_canon == dest_canon || (src_canon.is_dir() && dest_canon.starts_with(&src_canon)) {
        anyhow::bail!("Cannot move a folder into itself or a subfolder");
    }
    if src_canon.parent() == Some(dest_canon.as_path()) {
        return Ok(src_canon);
    }
    let Some(file_name) = src_canon.file_name() else {
        anyhow::bail!("Invalid source path: {}", src_path.display());
    };
    let dest_path = dest_canon.join(file_name);
    if dest_path.exists() {
        anyhow::bail!("Target already exists: {}", dest_path.display());
    }
    fs::rename(&src_canon, &dest_path).with_context(|| {
        format!(
            "move {} to {}",
            src_canon.display(),
            dest_path.display()
        )
    })?;
    Ok(dest_path)
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

    #[test]
    fn searches_both_titles_and_content_with_title_priority() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Exact title match without query in body (plus YAML frontmatter preview skip)
        fs::write(
            root.join("Alpha.md"),
            "---\ntags: [notes]\n---\n\nFirst body summary line\n",
        )
        .unwrap();

        // Partial title match
        fs::write(root.join("AlphaNotes.md"), "Body does not mention it").unwrap();

        // Multi-word filename match with underscore
        fs::write(root.join("Raid_Prep.md"), "Flasks and food\n").unwrap();

        // Image & PDF filename match (no text body)
        fs::write(root.join("alpha_banner.png"), [0x89, 0x50, 0x4E, 0x47]).unwrap();
        fs::write(root.join("alpha_resume.pdf"), b"%PDF-1.7\n").unwrap();

        // Content-only match
        fs::write(
            root.join("Zed.md"),
            "Mentioning alpha inside content only\nAnother alpha line\n",
        )
        .unwrap();

        let hits = search_content(root, "alpha", false, 20);
        assert!(hits.len() >= 4);

        // Title hits rank ahead of content hits, exact match first
        assert_eq!(hits[0].rel_path, "Alpha.md");
        assert!(hits[0].is_title_match);
        assert_eq!(hits[0].line_num, 0);
        assert_eq!(hits[0].line_text, "First body summary line");

        assert!(hits[1].is_title_match);
        assert_eq!(hits[1].rel_path, "AlphaNotes.md");

        assert!(hits.iter().any(|h| h.is_title_match && h.rel_path == "alpha_banner.png" && h.line_text == "Image"));
        assert!(hits.iter().any(|h| h.is_title_match && h.rel_path == "alpha_resume.pdf" && h.line_text == "PDF document"));
        assert!(hits.iter().any(|h| !h.is_title_match && h.rel_path == "Zed.md" && h.line_num == 1));

        // Multi-word title match on underscore filename
        let multi = search_content(root, "raid prep", false, 10);
        assert_eq!(multi.len(), 1);
        assert!(multi[0].is_title_match);
        assert_eq!(multi[0].rel_path, "Raid_Prep.md");

        // Title match still surfaces even when max_hits would otherwise be exhausted by content matches
        fs::write(
            root.join("aaa_filler.md"),
            "omega 1\nomega 2\nomega 3\nomega 4\nomega 5\n",
        )
        .unwrap();
        fs::write(root.join("zzz_omega.md"), "no keyword in body\n").unwrap();
        let capped = search_content(root, "omega", false, 3);
        assert_eq!(capped.len(), 3);
        assert!(capped[0].is_title_match);
        assert_eq!(capped[0].rel_path, "zzz_omega.md");
    }

    #[test]
    fn move_entry_handles_valid_and_invalid_moves() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let sub_a = root.join("Alpha");
        let sub_b = root.join("Beta");
        let nested = sub_a.join("Nested");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&sub_b).unwrap();
        fs::write(root.join("Note.md"), "# Note").unwrap();
        fs::write(sub_b.join("Collision.md"), "existing").unwrap();
        fs::write(root.join("Collision.md"), "incoming").unwrap();

        // Move file into subfolder
        let moved_file = move_entry(&root.join("Note.md"), &sub_a).unwrap();
        assert_eq!(moved_file, sub_a.join("Note.md"));
        assert!(moved_file.is_file());

        // No-op when dropping into same parent
        let same = move_entry(&moved_file, &sub_a).unwrap();
        assert_eq!(same, fs::canonicalize(&moved_file).unwrap());

        // Move file back to root
        let back = move_entry(&moved_file, root).unwrap();
        assert_eq!(back, fs::canonicalize(root).unwrap().join("Note.md"));

        // Move folder into sibling folder
        let moved_dir = move_entry(&nested, &sub_b).unwrap();
        assert!(moved_dir.is_dir());

        // Reject moving folder into itself or descendant
        assert!(move_entry(&sub_b, &sub_b).is_err());
        assert!(move_entry(&sub_b, &moved_dir).is_err());

        // Reject colliding destination name
        assert!(move_entry(&root.join("Collision.md"), &sub_b).is_err());
    }

    #[test]
    fn wikilinks_resolve_non_markdown_and_cross_folder_consistently() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("a")).unwrap();
        fs::create_dir_all(root.join("b")).unwrap();

        let welcome = root.join("docs/welcome.md");
        let screenplay = root.join("scripts/Screenplay.fountain");
        fs::write(
            &welcome,
            "Read [[Screenplay.fountain]] and [[Screenplay]] and [[scripts/Screenplay]].\n",
        )
        .unwrap();
        fs::write(&screenplay, "Title: Demo\n\nFADE IN:\n").unwrap();

        let shared_a = root.join("a/Shared.fountain");
        let shared_b = root.join("b/Shared.fountain");
        fs::write(&shared_a, "Title: A\n").unwrap();
        fs::write(&shared_b, "Title: B\n").unwrap();
        let from_b = root.join("b/welcome.md");
        fs::write(&from_b, "See [[Shared.fountain]].\n").unwrap();

        let topic_md = root.join("a/Topic.md");
        let topic_png = root.join("b/Topic.png");
        fs::write(&topic_md, "# Topic\n").unwrap();
        fs::write(&topic_png, [0x89u8, 0x50, 0x4E, 0x47]).unwrap();

        let vault = VaultIndex::build(root, false);

        assert_eq!(
            vault.resolve_wikilink("Screenplay.fountain", &welcome),
            Some(screenplay.clone())
        );
        assert_eq!(
            vault.resolve_wikilink("screenplay.fountain|Read Script", &welcome),
            Some(screenplay.clone())
        );
        assert_eq!(
            vault.resolve_wikilink("Screenplay", &welcome),
            Some(screenplay.clone())
        );
        assert_eq!(
            vault.resolve_wikilink("scripts/Screenplay", &welcome),
            Some(screenplay.clone())
        );
        assert_eq!(
            vault.resolve_wikilink("../scripts/Screenplay.fountain", &welcome),
            Some(screenplay.clone())
        );

        // Closest sibling wins when duplicate filenames exist across folders
        assert_eq!(
            vault.resolve_wikilink("Shared.fountain", &from_b),
            Some(shared_b)
        );

        // Unqualified stem prefers Markdown over assets unless extension is specified
        assert_eq!(vault.resolve_wikilink("Topic", &welcome), Some(topic_md));
        assert_eq!(
            vault.resolve_wikilink("Topic.png", &welcome),
            Some(topic_png)
        );

        // Backlink recorded on non-markdown target too
        assert!(
            vault
                .backlinks
                .get(&screenplay)
                .is_some_and(|entries| entries.iter().any(|e| e.source_path == welcome))
        );
    }

    #[test]
    fn adaptive_file_creation_matches_extension() {
        assert_eq!(normalize_new_file_name("My Note"), "My Note.md");
        assert_eq!(normalize_new_file_name("My Note.md"), "My Note.md");
        assert_eq!(normalize_new_file_name("Player.cs"), "Player.cs");
        assert_eq!(normalize_new_file_name("main.cpp"), "main.cpp");
        assert_eq!(normalize_new_file_name("notes/Todo"), "notes/Todo.md");
        assert_eq!(normalize_new_file_name(".gitignore"), ".gitignore");
        assert_eq!(normalize_new_file_name("Makefile"), "Makefile");
        assert_eq!(normalize_new_file_name("Draft."), "Draft.md");
        assert_eq!(
            normalize_new_file_name("  /foo/Bar.typ  "),
            "foo/Bar.typ"
        );

        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let cs = root.join("Player.cs");
        create_adaptive_file(&cs).unwrap();
        assert_eq!(fs::read_to_string(&cs).unwrap(), "");
        assert_eq!(detect_file_kind(&cs), FileKind::Code("csharp"));

        let cpp = root.join("main.cpp");
        create_adaptive_file(&cpp).unwrap();
        assert_eq!(fs::read_to_string(&cpp).unwrap(), "");
        assert_eq!(detect_file_kind(&cpp), FileKind::Code("cpp"));

        let md = root.join("Note.md");
        create_adaptive_file(&md).unwrap();
        assert!(fs::read_to_string(&md).unwrap().starts_with("# Note"));

        let ftn = root.join("Script.fountain");
        create_adaptive_file(&ftn).unwrap();
        assert!(fs::read_to_string(&ftn).unwrap().contains("Title: Script"));

        let typ = root.join("Doc.typ");
        create_adaptive_file(&typ).unwrap();
        assert!(fs::read_to_string(&typ).unwrap().contains("= Doc"));

        let xlsx = root.join("Sheet.xlsx");
        create_adaptive_file(&xlsx).unwrap();
        assert!(xlsx.is_file());
        assert!(crate::spreadsheet::SpreadsheetDoc::open(&xlsx).is_ok());

        let kbn = root.join("Board.kanban");
        create_adaptive_file(&kbn).unwrap();
        assert!(fs::read_to_string(&kbn).unwrap().contains("## Todo"));

        let cnv = root.join("Ideas.canvas");
        create_adaptive_file(&cnv).unwrap();
        assert!(fs::read_to_string(&cnv).unwrap().contains("# Ideas"));

        let mmd = root.join("Flow.mmd");
        create_adaptive_file(&mmd).unwrap();
        assert!(fs::read_to_string(&mmd).unwrap().contains("flowchart TD"));

        let svg = root.join("Icon.svg");
        create_adaptive_file(&svg).unwrap();
        assert!(fs::read_to_string(&svg).unwrap().contains("<svg"));

        let gitignore = root.join(".gitignore");
        create_adaptive_file(&gitignore).unwrap();
        assert_eq!(fs::read_to_string(&gitignore).unwrap(), "");

        let makefile = root.join("Makefile");
        create_adaptive_file(&makefile).unwrap();
        assert_eq!(fs::read_to_string(&makefile).unwrap(), "");

        let nested_cs = root.join("sub/Player.cs");
        create_adaptive_file(&nested_cs).unwrap();
        assert!(nested_cs.is_file());

        // Creating an existing file fails cleanly
        assert!(create_adaptive_file(&cs).is_err());
    }

    #[test]
    fn find_all_byte_ranges_and_merge_highlights_handle_unicode_and_overlaps() {
        assert!(find_all_byte_ranges("abc", "", false).is_empty());
        assert_eq!(find_all_byte_ranges("Abc abc ABC", "abc", false), vec![0..3, 4..7, 8..11]);
        assert_eq!(find_all_byte_ranges("Abc abc ABC", "abc", true), vec![4..7]);

        // Unicode (Cyrillic + accented Latin) case-insensitive byte offsets
        let uni = "Привет мир, привет café CAFÉ";
        let cyr = find_all_byte_ranges(uni, "привет", false);
        assert_eq!(cyr.len(), 2);
        assert_eq!(&uni[cyr[0].clone()], "Привет");
        assert_eq!(&uni[cyr[1].clone()], "привет");

        let lat = find_all_byte_ranges(uni, "café", false);
        assert_eq!(lat.len(), 2);
        assert_eq!(&uni[lat[0].clone()], "café");
        assert_eq!(&uni[lat[1].clone()], "CAFÉ");

        // Merging overlapping base styles and search highlights yields sorted non-overlapping ranges
        let base = vec![(0..6, 1u8), (8..15, 2u8)];
        let matches = vec![(3..10, ), (12..14, )].into_iter().map(|t| t.0).collect::<Vec<_>>();
        let merged = merge_highlight_ranges(&base, &matches, Some(1), |base_opt, active| {
            let b = base_opt.copied().unwrap_or(0);
            if active { 100 + b } else { 10 + b }
        });
        assert_eq!(
            merged,
            vec![
                (0..3, 1),
                (3..6, 11),
                (6..8, 10),
                (8..10, 12),
                (10..12, 2),
                (12..14, 102),
                (14..15, 2),
            ]
        );
    }
}
