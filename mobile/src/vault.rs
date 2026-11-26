use anyhow::{Context, Result, bail};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

const NOTE_EXTS: &[&str] = &[
    "md", "markdown", "txt", "fountain", "org", "json", "toml", "yaml", "yml", "csv", "tsv",
    "ini", "conf", "log",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultEntry {
    pub path: PathBuf,
    pub rel_path: String,
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub modified_secs: u64,
    pub ext: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: PathBuf,
    pub rel_path: String,
    pub name: String,
    pub line_num: Option<usize>,
    pub snippet: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VaultSort {
    #[default]
    NameAsc,
    NameDesc,
    ModifiedDesc,
}

impl VaultSort {
    pub fn cycle(self) -> Self {
        match self {
            Self::NameAsc => Self::NameDesc,
            Self::NameDesc => Self::ModifiedDesc,
            Self::ModifiedDesc => Self::NameAsc,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NameAsc => "A-Z",
            Self::NameDesc => "Z-A",
            Self::ModifiedDesc => "Recent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultNode {
    pub path: PathBuf,
    pub rel_path: String,
    pub name: String,
    pub display_name: String,
    pub lower_name: String,
    pub lower_rel: String,
    pub parent_rel: String,
    pub depth: usize,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub modified_secs: u64,
    pub child_count: usize,
    pub ext: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub path: PathBuf,
    pub rel_path: String,
    pub display_name: String,
    pub ext: String,
    pub depth: usize,
    pub is_dir: bool,
    pub is_expanded: bool,
    pub child_count: usize,
    pub modified_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickSwitchHit {
    pub path: PathBuf,
    pub rel_path: String,
    pub display_name: String,
    pub folder_hint: String,
    pub score: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikilinkRef {
    pub target: String,
    pub display: String,
}

pub use vkarma_core::live_md::MarkdownFormatAction;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MobileWorkspaceState {
    pub vault_root: Option<PathBuf>,
    pub last_note_path: Option<PathBuf>,
    pub recent_notes: Vec<PathBuf>,
    pub expanded_dirs: Vec<String>,
    pub sort_mode: VaultSort,
}

impl MobileWorkspaceState {
    pub fn state_file_path() -> PathBuf {
        app_storage_dir("").join("workspace-state.json")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::state_file_path())
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(raw) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::state_file_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .context("Failed to serialize MobileWorkspaceState")?;
        atomic_write_string(path, &json)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultIndex {
    pub root: PathBuf,
    pub entries: Vec<VaultNode>,
    pub notes: Vec<VaultNode>,
}

impl VaultIndex {
    pub fn empty(root: PathBuf) -> Self {
        Self {
            root,
            entries: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn cache_file_path() -> PathBuf {
        app_cache_dir("").join("vault-index-cache.json")
    }

    pub fn load_cached(root: &Path) -> Option<Self> {
        Self::load_cached_from(&Self::cache_file_path(), root)
    }

    pub fn load_cached_from(cache_path: &Path, root: &Path) -> Option<Self> {
        let raw = fs::read_to_string(cache_path).ok()?;
        let cached: Self = serde_json::from_str(&raw).ok()?;
        (cached.root == root).then_some(cached)
    }

    pub fn save_cached(&self) -> Result<()> {
        self.save_cached_to(&Self::cache_file_path())
    }

    pub fn save_cached_to(&self, cache_path: &Path) -> Result<()> {
        let json = serde_json::to_string(self).context("Failed to serialize VaultIndex cache")?;
        atomic_write_string(cache_path, &json)
    }

    pub fn note_count(&self) -> usize {
        self.notes.len()
    }

    pub fn build(root: &Path) -> Self {
        if !root.is_dir() {
            return Self::empty(root.to_path_buf());
        }

        let mut entries = Vec::new();
        let mut dir_note_counts: HashMap<String, usize> = HashMap::new();

        let walker = WalkDir::new(root).follow_links(false).into_iter();
        for entry in walker.filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_str().unwrap_or_default();
            if e.file_type().is_dir() {
                return !is_ignored_dir(name);
            }
            !name.starts_with('.')
        }) {
            let Ok(entry) = entry else {
                continue;
            };
            if entry.depth() == 0 {
                continue;
            }
            let is_dir = entry.file_type().is_dir();
            let path = entry.path().to_path_buf();
            let name = entry.file_name().to_string_lossy().into_owned();
            let ext = if is_dir {
                String::new()
            } else {
                path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase()
            };
            // Filter non-note extensions BEFORE calling entry.metadata() to avoid slow FUSE stats.
            if !is_dir && !is_readable_note_ext(&ext) {
                continue;
            }

            let (size_bytes, modified_secs) = if is_dir {
                (0, 0)
            } else if let Ok(meta) = entry.metadata() {
                let m = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                (meta.len(), m)
            } else {
                (0, 0)
            };

            let rel_path = normalize_rel_path(root, &path);
            let parent_rel = rel_path
                .rsplit_once('/')
                .map(|(p, _)| p.to_string())
                .unwrap_or_default();
            let depth = entry.depth().saturating_sub(1);
            let display_name = if is_dir {
                name.clone()
            } else {
                strip_note_ext(&name, &ext)
            };

            if !is_dir {
                let mut ancestor = parent_rel.as_str();
                while !ancestor.is_empty() {
                    *dir_note_counts.entry(ancestor.to_string()).or_insert(0) += 1;
                    ancestor = ancestor.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
                }
            }

            entries.push(VaultNode {
                lower_name: display_name.to_lowercase(),
                lower_rel: rel_path.to_lowercase(),
                path,
                rel_path,
                name,
                display_name,
                parent_rel,
                depth,
                is_dir,
                size_bytes,
                modified_secs,
                child_count: 0,
                ext,
            });
        }

        for node in &mut entries {
            if node.is_dir {
                node.child_count = dir_note_counts.get(&node.rel_path).copied().unwrap_or(0);
            }
        }

        let mut notes: Vec<VaultNode> = entries.iter().filter(|n| !n.is_dir).cloned().collect();
        notes.sort_by(|a, b| {
            b.modified_secs
                .cmp(&a.modified_secs)
                .then_with(|| a.lower_name.cmp(&b.lower_name))
        });

        Self {
            root: root.to_path_buf(),
            entries,
            notes,
        }
    }

    pub fn visible_tree_rows(
        &self,
        expanded_dirs: &HashSet<String>,
        sort: VaultSort,
        filter: &str,
    ) -> Vec<TreeRow> {
        let trimmed = filter.trim().to_lowercase();
        if !trimmed.is_empty() {
            let mut scored: Vec<(i32, &VaultNode)> = self
                .notes
                .iter()
                .filter_map(|node| {
                    score_note_match(&trimmed, &node.lower_name, &node.lower_rel)
                        .map(|s| (s, node))
                })
                .collect();
            scored.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then_with(|| a.1.lower_name.cmp(&b.1.lower_name))
            });
            return scored
                .into_iter()
                .map(|(_, n)| TreeRow {
                    path: n.path.clone(),
                    rel_path: n.rel_path.clone(),
                    display_name: n.display_name.clone(),
                    ext: n.ext.clone(),
                    depth: 0,
                    is_dir: false,
                    is_expanded: false,
                    child_count: 0,
                    modified_secs: n.modified_secs,
                })
                .collect();
        }

        let mut children_map: HashMap<&str, Vec<usize>> = HashMap::new();
        for (ix, node) in self.entries.iter().enumerate() {
            children_map
                .entry(node.parent_rel.as_str())
                .or_default()
                .push(ix);
        }

        for bucket in children_map.values_mut() {
            bucket.sort_by(|&ia, &ib| {
                let a = &self.entries[ia];
                let b = &self.entries[ib];
                b.is_dir.cmp(&a.is_dir).then_with(|| match sort {
                    VaultSort::NameAsc => a.lower_name.cmp(&b.lower_name),
                    VaultSort::NameDesc => b.lower_name.cmp(&a.lower_name),
                    VaultSort::ModifiedDesc => b
                        .modified_secs
                        .cmp(&a.modified_secs)
                        .then_with(|| a.lower_name.cmp(&b.lower_name)),
                })
            });
        }

        fn push_subtree<'a>(
            parent: &'a str,
            depth: usize,
            entries: &'a [VaultNode],
            children_map: &HashMap<&'a str, Vec<usize>>,
            expanded_dirs: &HashSet<String>,
            out: &mut Vec<TreeRow>,
        ) {
            let Some(indices) = children_map.get(parent) else {
                return;
            };
            for &idx in indices {
                let node = &entries[idx];
                let is_expanded = node.is_dir && expanded_dirs.contains(&node.rel_path);
                out.push(TreeRow {
                    path: node.path.clone(),
                    rel_path: node.rel_path.clone(),
                    display_name: node.display_name.clone(),
                    ext: node.ext.clone(),
                    depth,
                    is_dir: node.is_dir,
                    is_expanded,
                    child_count: node.child_count,
                    modified_secs: node.modified_secs,
                });
                if is_expanded {
                    push_subtree(
                        node.rel_path.as_str(),
                        depth + 1,
                        entries,
                        children_map,
                        expanded_dirs,
                        out,
                    );
                }
            }
        }

        let mut rows = Vec::with_capacity(self.entries.len());
        push_subtree("", 0, &self.entries, &children_map, expanded_dirs, &mut rows);
        rows
    }

    pub fn quick_switch(
        &self,
        query: &str,
        recent_paths: &[PathBuf],
        limit: usize,
    ) -> Vec<QuickSwitchHit> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            let mut out = Vec::with_capacity(limit.min(self.notes.len()));
            let mut seen = HashSet::new();

            for recent in recent_paths {
                if let Some(node) = self.notes.iter().find(|n| &n.path == recent)
                    && seen.insert(node.path.clone())
                {
                    out.push(QuickSwitchHit {
                        path: node.path.clone(),
                        rel_path: node.rel_path.clone(),
                        display_name: node.display_name.clone(),
                        folder_hint: folder_hint_from_parent(&node.parent_rel),
                        score: 2000,
                    });
                    if out.len() >= limit {
                        return out;
                    }
                }
            }

            for node in &self.notes {
                if seen.insert(node.path.clone()) {
                    out.push(QuickSwitchHit {
                        path: node.path.clone(),
                        rel_path: node.rel_path.clone(),
                        display_name: node.display_name.clone(),
                        folder_hint: folder_hint_from_parent(&node.parent_rel),
                        score: 0,
                    });
                    if out.len() >= limit {
                        break;
                    }
                }
            }
            return out;
        }

        let recent_set: HashSet<&Path> = recent_paths.iter().map(PathBuf::as_path).collect();
        let mut hits: Vec<(i32, usize, &VaultNode)> = self
            .notes
            .iter()
            .filter_map(|node| {
                let mut base = score_note_match(&q, &node.lower_name, &node.lower_rel)?;
                if recent_set.contains(node.path.as_path()) {
                    base += 45;
                }
                Some((base, node.display_name.chars().count(), node))
            })
            .collect();

        hits.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| b.2.modified_secs.cmp(&a.2.modified_secs))
                .then_with(|| a.2.lower_name.cmp(&b.2.lower_name))
        });

        hits.into_iter()
            .take(limit)
            .map(|(score, _, node)| QuickSwitchHit {
                path: node.path.clone(),
                rel_path: node.rel_path.clone(),
                display_name: node.display_name.clone(),
                folder_hint: folder_hint_from_parent(&node.parent_rel),
                score,
            })
            .collect()
    }

    pub fn resolve_wikilink(&self, raw_target: &str) -> Option<PathBuf> {
        let clean = normalize_wikilink_target(raw_target);
        if clean.is_empty() {
            return None;
        }
        let target_lower = clean.to_lowercase();
        let target_no_ext = target_lower
            .strip_suffix(".md")
            .or_else(|| target_lower.strip_suffix(".markdown"))
            .unwrap_or(&target_lower);

        // 1. Exact relative path without extension (e.g. "folder/sub/note")
        if let Some(n) = self.notes.iter().find(|n| {
            let rel_no_ext = strip_note_ext(&n.lower_rel, &n.ext);
            rel_no_ext == target_no_ext || n.lower_rel == target_lower
        }) {
            return Some(n.path.clone());
        }

        // 2. Exact stem match anywhere in vault (shortest rel_path first)
        let mut stem_matches: Vec<&VaultNode> = self
            .notes
            .iter()
            .filter(|n| n.lower_name == target_no_ext || n.lower_name == target_lower)
            .collect();
        stem_matches.sort_by_key(|n| n.rel_path.len());
        if let Some(first) = stem_matches.first() {
            return Some(first.path.clone());
        }

        // 3. Suffix path match ("sub/note" matches "root/sub/note.md")
        let suffix = format!("/{target_no_ext}");
        let mut suffix_matches: Vec<&VaultNode> = self
            .notes
            .iter()
            .filter(|n| strip_note_ext(&n.lower_rel, &n.ext).ends_with(&suffix))
            .collect();
        suffix_matches.sort_by_key(|n| n.rel_path.len());
        suffix_matches.first().map(|n| n.path.clone())
    }

    pub fn has_exact_note_name(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return false;
        }
        let q_stem = q
            .strip_suffix(".md")
            .or_else(|| q.strip_suffix(".markdown"))
            .unwrap_or(&q);
        self.notes
            .iter()
            .any(|n| n.lower_name == q_stem || n.lower_name == q)
    }

    pub fn find_backlinks(&self, active_path: &Path, limit: usize) -> Vec<SearchHit> {
        let Some(stem) = active_path.file_stem().and_then(|s| s.to_str()) else {
            return Vec::new();
        };
        let target_lower = stem.to_lowercase();
        let needle_plain = format!("[[{target_lower}]]");
        let needle_pipe = format!("[[{target_lower}|");
        let needle_head = format!("[[{target_lower}#");

        let mut out = Vec::new();
        for node in &self.notes {
            if node.path == active_path || node.size_bytes > 512 * 1024 {
                continue;
            }
            let Ok(text) = fs::read_to_string(&node.path) else {
                continue;
            };
            for (ix, line) in text.lines().enumerate() {
                let lower = line.to_lowercase();
                if lower.contains(&needle_plain)
                    || lower.contains(&needle_pipe)
                    || lower.contains(&needle_head)
                {
                    out.push(SearchHit {
                        path: node.path.clone(),
                        rel_path: node.rel_path.clone(),
                        name: node.display_name.clone(),
                        line_num: Some(ix + 1),
                        snippet: line.trim().chars().take(110).collect(),
                    });
                    break;
                }
            }
            if out.len() >= limit {
                break;
            }
        }
        out
    }
}

fn folder_hint_from_parent(parent_rel: &str) -> String {
    if parent_rel.is_empty() {
        "Root".to_string()
    } else {
        parent_rel.to_string()
    }
}

fn strip_note_ext(name: &str, ext: &str) -> String {
    if ext == "md" {
        if let Some(stem) = name.strip_suffix(".md").or_else(|| name.strip_suffix(".MD")) {
            return stem.to_string();
        }
    } else if ext == "markdown"
        && let Some(stem) = name
            .strip_suffix(".markdown")
            .or_else(|| name.strip_suffix(".MARKDOWN"))
    {
        return stem.to_string();
    }
    name.to_string()
}

fn score_note_match(q_lower: &str, lower_name: &str, lower_rel: &str) -> Option<i32> {
    if lower_name == q_lower {
        return Some(1000);
    }
    if lower_name.starts_with(q_lower) {
        return Some(750);
    }
    if let Some(pos) = lower_name.find(q_lower) {
        let word_boundary = pos == 0
            || lower_name[..pos]
                .chars()
                .next_back()
                .is_some_and(|c| matches!(c, ' ' | '-' | '_' | '/'));
        return Some(if word_boundary { 560 } else { 420 } - pos as i32);
    }
    if let Some(pos) = lower_rel.find(q_lower) {
        return Some(300 - (pos as i32).min(200));
    }
    fuzzy_subsequence_score(q_lower, lower_name)
        .map(|s| 180 + s)
        .or_else(|| fuzzy_subsequence_score(q_lower, lower_rel).map(|s| 90 + s))
}

fn fuzzy_subsequence_score(needle: &str, haystack: &str) -> Option<i32> {
    let mut n_chars = needle.chars();
    let mut current = n_chars.next()?;
    let mut score = 0i32;
    let mut last_match: Option<usize> = None;

    for (idx, ch) in haystack.chars().enumerate() {
        if ch == current {
            if let Some(prev) = last_match {
                if idx == prev + 1 {
                    score += 6;
                } else {
                    score -= (idx - prev - 1).min(8) as i32;
                }
            } else if idx == 0 {
                score += 8;
            }
            last_match = Some(idx);
            match n_chars.next() {
                Some(next) => current = next,
                None => return Some(score),
            }
        }
    }
    None
}

fn normalize_wikilink_target(raw: &str) -> String {
    let without_alias = raw.split('|').next().unwrap_or(raw);
    let without_heading = without_alias.split('#').next().unwrap_or(without_alias);
    without_heading
        .trim()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

pub fn extract_wikilinks(md: &str) -> Vec<WikilinkRef> {
    let body_start = frontmatter_byte_len(md);
    let mut out = Vec::new();
    let mut seen_targets = HashSet::new();
    let mut in_fence = false;

    for line in md[body_start..].lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut rest = line;
        while let Some(open) = rest.find("[[") {
            let before = &rest[..open];
            let backticks = before.matches('`').count();
            if backticks % 2 == 1 {
                rest = &rest[open + 2..];
                continue;
            }
            let after_open = &rest[open + 2..];
            let Some(close) = after_open.find("]]") else {
                break;
            };
            let inner = after_open[..close].trim();
            if !inner.is_empty() && !inner.contains('\n') {
                let (raw_target, alias) = match inner.split_once('|') {
                    Some((t, a)) => (t.trim(), a.trim()),
                    None => (inner, inner.split('#').next().unwrap_or(inner).trim()),
                };
                let target = normalize_wikilink_target(raw_target);
                let display = if alias.is_empty() {
                    target.clone()
                } else {
                    alias.to_string()
                };
                if !target.is_empty() && seen_targets.insert(target.to_lowercase()) {
                    out.push(WikilinkRef { target, display });
                }
            }
            rest = &after_open[close + 2..];
        }
    }
    out
}

pub fn preprocess_obsidian_markdown(md: &str) -> String {
    let fm_len = frontmatter_byte_len(md);
    let mut out = String::with_capacity(md.len() + 64);
    out.push_str(&md[..fm_len]);

    let mut in_fence = false;
    let body = &md[fm_len..];
    let lines: Vec<&str> = body.split('\n').collect();
    for (idx, raw_line) in lines.iter().enumerate() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push_str(line);
            if idx + 1 < lines.len() {
                out.push('\n');
            }
            continue;
        }
        if in_fence {
            out.push_str(line);
            if idx + 1 < lines.len() {
                out.push('\n');
            }
            continue;
        }
        let line_start = out.len();
        rewrite_wikilinks_in_line(line, &mut out);
        if idx + 1 < lines.len() {
            let next_raw = lines[idx + 1].trim_end_matches('\r');
            let converted = &out[line_start..];
            if should_insert_hard_break(line, converted, next_raw) {
                out.push_str("  ");
            }
            out.push('\n');
        }
    }
    out
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

fn is_block_boundary_line(trimmed: &str) -> bool {
    is_heading_line(trimmed)
        || is_thematic_break_line(trimmed)
        || trimmed.starts_with('|')
        || trimmed.starts_with('<')
}

fn is_block_starter_line(trimmed: &str) -> bool {
    is_block_boundary_line(trimmed)
        || trimmed.starts_with("```")
        || trimmed.starts_with("~~~")
        || is_list_item_line(trimmed)
}

fn frontmatter_byte_len(md: &str) -> usize {
    let Some(after_open) = md
        .strip_prefix("---\n")
        .or_else(|| md.strip_prefix("---\r\n"))
    else {
        return 0;
    };
    let open_len = md.len() - after_open.len();
    let mut offset = 0usize;
    for line in after_open.split_inclusive('\n') {
        let bare = line.trim_end_matches(&['\r', '\n'][..]);
        offset += line.len();
        if bare == "---" || bare == "..." {
            return open_len + offset;
        }
    }
    0
}

fn rewrite_wikilinks_in_line(line: &str, out: &mut String) {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    let mut in_inline_code = false;

    while i < bytes.len() {
        if bytes[i] == b'`' {
            in_inline_code = !in_inline_code;
            out.push('`');
            i += 1;
            continue;
        }
        if !in_inline_code {
            let is_embed = i + 2 < bytes.len() && bytes[i] == b'!' && &bytes[i + 1..i + 3] == b"[[";
            let is_link = i + 1 < bytes.len() && &bytes[i..i + 2] == b"[[";
            if is_embed || is_link {
                let start = if is_embed { i + 3 } else { i + 2 };
                if let Some(rel_close) = line[start..].find("]]") {
                    let inner = line[start..start + rel_close].trim();
                    if !inner.is_empty() && !inner.contains('[') && !inner.contains('\n') {
                        let (raw_target, alias) = match inner.split_once('|') {
                            Some((t, a)) => (t.trim(), a.trim()),
                            None => (inner, inner.split('#').next().unwrap_or(inner).trim()),
                        };
                        let target = normalize_wikilink_target(raw_target);
                        if !target.is_empty() {
                            let label = if alias.is_empty() {
                                target.as_str()
                            } else {
                                alias
                            };
                            let encoded =
                                utf8_percent_encode(&target, NON_ALPHANUMERIC).to_string();
                            out.push('[');
                            out.push_str(label);
                            out.push_str("](vkarma-wiki://");
                            out.push_str(&encoded);
                            out.push(')');
                            i = start + rel_close + 2;
                            continue;
                        }
                    }
                }
            }
        }
        let ch = line[i..].chars().next().expect("valid utf8");
        out.push(ch);
        i += ch.len_utf8();
    }
}

pub fn extract_tags(md: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut seen = HashSet::new();
    let fm_len = frontmatter_byte_len(md);

    if fm_len > 0 {
        let mut in_tags_list = false;
        for line in md[..fm_len].lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("tags:") {
                let rest = rest.trim();
                in_tags_list = rest.is_empty();
                if !rest.is_empty() {
                    let inner = rest.trim_start_matches('[').trim_end_matches(']');
                    for part in inner.split([',', ' ']) {
                        let clean = part.trim().trim_matches(&['"', '\'', '#'][..]);
                        if !clean.is_empty() && seen.insert(clean.to_lowercase()) {
                            tags.push(clean.to_string());
                        }
                    }
                }
            } else if in_tags_list {
                if let Some(item) = trimmed.strip_prefix("- ") {
                    let clean = item.trim().trim_matches(&['"', '\'', '#'][..]);
                    if !clean.is_empty() && seen.insert(clean.to_lowercase()) {
                        tags.push(clean.to_string());
                    }
                } else if !trimmed.is_empty() {
                    in_tags_list = false;
                }
            }
        }
    }

    let mut in_fence = false;
    for line in md[fm_len..].lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let bytes = line.as_bytes();
        let mut idx = 0usize;
        let mut in_code = false;
        while idx < bytes.len() {
            if bytes[idx] == b'`' {
                in_code = !in_code;
                idx += 1;
                continue;
            }
            if !in_code && bytes[idx] == b'#' {
                let preceded_by_boundary = idx == 0
                    || bytes[idx - 1].is_ascii_whitespace()
                    || matches!(bytes[idx - 1], b'(' | b'[' | b',');
                let next = bytes.get(idx + 1).copied();
                if preceded_by_boundary
                    && next.is_some_and(|b| b != b'#' && !b.is_ascii_whitespace())
                {
                    let rest = &line[idx + 1..];
                    let tag: String = rest
                        .chars()
                        .take_while(|c| {
                            c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
                        })
                        .collect();
                    if !tag.is_empty()
                        && !tag.chars().all(|c| c.is_ascii_digit())
                        && seen.insert(tag.to_lowercase())
                    {
                        tags.push(tag);
                        if tags.len() >= 16 {
                            return tags;
                        }
                    }
                }
            }
            let ch = line[idx..].chars().next().expect("valid utf8");
            idx += ch.len_utf8();
        }
    }
    tags
}

pub fn note_stats(md: &str) -> (usize, usize, usize) {
    let words = md.split_whitespace().count();
    let chars = md.chars().count();
    let lines = md.lines().count().max(usize::from(!md.is_empty()));
    (words, chars, lines)
}

pub fn format_selection_or_snippet(action: MarkdownFormatAction, selected: &str) -> String {
    let has_sel = !selected.is_empty();
    match action {
        MarkdownFormatAction::Heading => {
            if has_sel {
                format!("## {selected}")
            } else {
                "\n## ".to_string()
            }
        }
        MarkdownFormatAction::Bold => {
            if has_sel {
                format!("**{selected}**")
            } else {
                "**text**".to_string()
            }
        }
        MarkdownFormatAction::Italic => {
            if has_sel {
                format!("*{selected}*")
            } else {
                "*text*".to_string()
            }
        }
        MarkdownFormatAction::Checkbox => {
            if has_sel {
                format!("- [ ] {selected}")
            } else {
                "\n- [ ] ".to_string()
            }
        }
        MarkdownFormatAction::Bullet => {
            if has_sel {
                format!("- {selected}")
            } else {
                "\n- ".to_string()
            }
        }
        MarkdownFormatAction::Quote => {
            if has_sel {
                format!("> {selected}")
            } else {
                "\n> ".to_string()
            }
        }
        MarkdownFormatAction::Code => {
            if has_sel {
                format!("`{selected}`")
            } else {
                "`code`".to_string()
            }
        }
        MarkdownFormatAction::Wikilink => {
            if has_sel {
                format!("[[{selected}]]")
            } else {
                "[[]]".to_string()
            }
        }
        MarkdownFormatAction::HorizontalRule => {
            if has_sel {
                format!("{selected}\n\n---\n")
            } else {
                "\n---\n".to_string()
            }
        }
    }
}

pub fn today_daily_note_name() -> String {
    format!("{}.md", chrono::Local::now().format("%Y-%m-%d"))
}

pub fn sanitize_folder_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.trim().chars() {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t' => {
                out.push(' ');
            }
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let clean = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.is_empty() {
        "New Folder".to_string()
    } else {
        clean
    }
}

pub fn create_folder(parent: &Path, raw_name: &str) -> Result<PathBuf> {
    let clean = sanitize_folder_name(raw_name);
    let target = parent.join(clean);
    fs::create_dir_all(&target)
        .with_context(|| format!("Failed to create folder {}", target.display()))?;
    Ok(target)
}

pub fn rename_note_file(old_path: &Path, new_title: &str) -> Result<PathBuf> {
    let Some(parent) = old_path.parent() else {
        bail!("Cannot determine parent folder for {}", old_path.display());
    };
    let clean_name = sanitize_note_filename(new_title);
    let target = parent.join(clean_name);
    if target == old_path {
        return Ok(target);
    }
    if target.exists() {
        bail!("A note named '{}' already exists in this folder", target.display());
    }
    fs::rename(old_path, &target).with_context(|| {
        format!(
            "Failed to rename {} to {}",
            old_path.display(),
            target.display()
        )
    })?;
    Ok(target)
}

pub fn discover_vault_candidates() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    let (default_root, _) = detect_default_vault_root();
    if seen.insert(default_root.clone()) {
        let lbl = default_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Vault")
            .to_string();
        out.push((format!("{lbl} (Root)"), default_root.clone()));
    }

    if let Ok(rd) = fs::read_dir(&default_root) {
        let mut subdirs: Vec<PathBuf> = rd
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                (p.is_dir() && !is_ignored_dir(&name)).then_some(p)
            })
            .collect();
        subdirs.sort_by(|a, b| {
            a.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_lowercase()
                .cmp(
                    &b.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default()
                        .to_lowercase(),
                )
        });
        for p in subdirs {
            if seen.insert(p.clone()) {
                let lbl = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Folder")
                    .to_string();
                out.push((lbl, p));
            }
        }
    }

    let internal = app_storage_dir("vault");
    if seen.insert(internal.clone()) {
        out.push(("App Internal Vault".to_string(), internal));
    }

    out
}

pub fn is_ignored_dir(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.starts_with('.') {
        return true;
    }
    matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "node_modules" | "target" | "android" | ".obsidian" | ".git" | ".trash"
    )
}

pub fn is_readable_note_ext(ext: &str) -> bool {
    let lower = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    NOTE_EXTS.contains(&lower.as_str())
}

pub fn app_storage_dir(subdir: &str) -> PathBuf {
    #[cfg(target_os = "android")]
    let base = gpui_mobile::packages::path_provider::documents_directory()
        .unwrap_or_else(|_| PathBuf::from("/data/local/tmp/vkarma-mobile"));

    #[cfg(not(target_os = "android"))]
    let base = std::env::temp_dir().join("vkarma-mobile");

    let target = if subdir.trim().is_empty() {
        base
    } else {
        base.join(subdir)
    };
    let _ = fs::create_dir_all(&target);
    target
}

pub fn app_cache_dir(subdir: &str) -> PathBuf {
    #[cfg(target_os = "android")]
    let base = gpui_mobile::packages::path_provider::cache_directory()
        .unwrap_or_else(|_| app_storage_dir("cache"));

    #[cfg(not(target_os = "android"))]
    let base = std::env::temp_dir().join("vkarma-mobile-cache");

    let target = if subdir.trim().is_empty() {
        base
    } else {
        base.join(subdir)
    };
    let _ = fs::create_dir_all(&target);
    target
}

pub fn can_read_dir(path: &Path) -> bool {
    path.is_dir() && fs::read_dir(path).is_ok()
}

pub fn list_subdirectories(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut subdirs: Vec<(String, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            (p.is_dir() && !is_ignored_dir(&name)).then_some((name, p))
        })
        .collect();
    subdirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    subdirs
}

pub fn count_direct_matching_files(dir: &Path, exts: &[&str]) -> usize {
    let Ok(rd) = fs::read_dir(dir) else {
        return 0;
    };
    rd.flatten()
        .filter(|e| {
            let path = e.path();
            let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
                return false;
            };
            exts.iter().any(|allowed| ext.eq_ignore_ascii_case(allowed)) && path.is_file()
        })
        .count()
}

pub fn detect_default_vault_root() -> (PathBuf, bool) {
    #[cfg(target_os = "android")]
    {
        if !crate::has_all_files_access() {
            return (app_storage_dir("vault"), false);
        }
        for cand in [
            PathBuf::from("/sdcard/Documents"),
            PathBuf::from("/storage/emulated/0/Documents"),
        ] {
            if can_read_dir(&cand) {
                return (cand, true);
            }
        }
        (app_storage_dir("vault"), false)
    }

    #[cfg(not(target_os = "android"))]
    {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            let docs = home.join("Documents");
            if can_read_dir(&docs) {
                return (docs, true);
            }
        }
        (app_storage_dir("vault"), false)
    }
}

pub fn normalize_rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

pub fn list_dir_entries(root: &Path, current_dir: &Path) -> Result<Vec<VaultEntry>> {
    let mut entries = Vec::new();
    let rd = fs::read_dir(current_dir)
        .with_context(|| format!("Cannot read directory {}", current_dir.display()))?;

    for item in rd.flatten() {
        let path = item.path();
        let name = item.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = item.metadata() else {
            continue;
        };
        let is_dir = meta.is_dir();
        if is_dir && is_ignored_dir(&name) {
            continue;
        }
        let ext = if is_dir {
            String::new()
        } else {
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
        };
        if !is_dir && !is_readable_note_ext(&ext) {
            continue;
        }
        let modified_secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        entries.push(VaultEntry {
            rel_path: normalize_rel_path(root, &path),
            name,
            path,
            is_dir,
            size_bytes: meta.len(),
            modified_secs,
            ext,
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

pub fn search_vault(root: &Path, query: &str, limit: usize) -> Vec<SearchHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() || !root.is_dir() {
        return Vec::new();
    }

    let mut hits = Vec::new();
    let walker = WalkDir::new(root).follow_links(false).into_iter();
    for entry in walker.filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        let name = e.file_name().to_str().unwrap_or_default();
        if e.file_type().is_dir() {
            return !is_ignored_dir(name);
        }
        !name.starts_with('.')
    }) {
        if hits.len() >= limit {
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        if !is_readable_note_ext(ext) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel_path = normalize_rel_path(root, path);

        let name_matches = name.to_lowercase().contains(&q);
        let mut content_match: Option<(usize, String)> = None;
        if let Ok(meta) = entry.metadata()
            && meta.len() <= 2 * 1024 * 1024
            && let Ok(text) = fs::read_to_string(path)
        {
            for (idx, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&q) {
                    let clean: String = line.trim().chars().take(120).collect();
                    content_match = Some((idx + 1, clean));
                    break;
                }
            }
        }

        if name_matches || content_match.is_some() {
            let (line_num, snippet) = match content_match {
                Some((ln, snip)) => (Some(ln), snip),
                None => (None, rel_path.clone()),
            };
            hits.push(SearchHit {
                path: path.to_path_buf(),
                rel_path,
                name,
                line_num,
                snippet,
            });
        }
    }

    hits.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    hits
}

pub fn find_line_matches(content: &str, query: &str, limit: usize) -> Vec<(usize, String)> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    content
        .lines()
        .enumerate()
        .filter_map(|(ix, line)| {
            if line.to_lowercase().contains(&q) {
                let preview: String = line.trim().chars().take(110).collect();
                Some((ix + 1, preview))
            } else {
                None
            }
        })
        .take(limit)
        .collect()
}

pub use vkarma_core::fs_util::atomic_write_string;

pub fn sanitize_note_filename(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.trim().chars() {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t' => {
                out.push(' ');
            }
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let clean = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = if clean.is_empty() {
        "Untitled".to_string()
    } else {
        clean
    };
    if Path::new(&clean).extension().is_some() {
        clean
    } else {
        format!("{clean}.md")
    }
}

pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_dir_entries_sorts_dirs_first_and_skips_hidden() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("Notes")).unwrap();
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::write(root.join("beta.md"), "# Beta").unwrap();
        fs::write(root.join("Alpha.md"), "# Alpha").unwrap();
        fs::write(root.join(".hidden.md"), "secret").unwrap();
        fs::write(root.join("image.png"), b"png").unwrap();

        let entries = list_dir_entries(root, root).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Notes", "Alpha.md", "beta.md"]);
        assert!(entries[0].is_dir);
        assert!(!entries[1].is_dir);
    }

    #[test]
    fn search_vault_matches_unicode_filenames_and_content() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("Archive")).unwrap();
        fs::write(root.join("почитать.md"), "Список книг на осень").unwrap();
        fs::write(root.join("Archive/Idea.md"), "Rust + GPUI on Pixel 7 Pro").unwrap();

        let hits1 = search_vault(root, "почитать", 20);
        assert_eq!(hits1.len(), 1);
        assert_eq!(hits1[0].name, "почитать.md");

        let hits2 = search_vault(root, "pixel", 20);
        assert_eq!(hits2.len(), 1);
        assert_eq!(hits2[0].rel_path, "Archive/Idea.md");
        assert_eq!(hits2[0].line_num, Some(1));
    }

    #[test]
    fn atomic_write_and_sanitize_work_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("Sub/Hello.md");
        atomic_write_string(&target, "# First").unwrap();
        atomic_write_string(&target, "# Updated").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "# Updated");

        assert_eq!(sanitize_note_filename("My: Idea?"), "My Idea.md");
        assert_eq!(sanitize_note_filename("log.txt"), "log.txt");
        assert_eq!(sanitize_note_filename("   "), "Untitled.md");
    }

    #[test]
    fn vault_index_tree_expand_collapse_sort_and_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("Projects/Rust")).unwrap();
        fs::create_dir_all(root.join("Daily")).unwrap();
        fs::write(root.join("Projects/Rust/vkarma.md"), "# VKarma").unwrap();
        fs::write(root.join("Daily/2026-11-24.md"), "# Today").unwrap();
        fs::write(root.join("Inbox.md"), "# Inbox").unwrap();
        fs::write(root.join("Notes.txt"), "plain").unwrap();

        let idx = VaultIndex::build(root);
        assert_eq!(idx.note_count(), 4);

        let mut expanded = HashSet::new();
        let collapsed = idx.visible_tree_rows(&expanded, VaultSort::NameAsc, "");
        let names: Vec<(&str, usize, bool)> = collapsed
            .iter()
            .map(|r| (r.display_name.as_str(), r.depth, r.is_dir))
            .collect();
        assert_eq!(
            names,
            vec![
                ("Daily", 0, true),
                ("Projects", 0, true),
                ("Inbox", 0, false),
                ("Notes.txt", 0, false),
            ]
        );
        assert_eq!(collapsed[1].child_count, 1);

        expanded.insert("Projects".to_string());
        expanded.insert("Projects/Rust".to_string());
        let open_rows = idx.visible_tree_rows(&expanded, VaultSort::NameAsc, "");
        let open_names: Vec<(&str, usize)> = open_rows
            .iter()
            .map(|r| (r.display_name.as_str(), r.depth))
            .collect();
        assert_eq!(
            open_names,
            vec![
                ("Daily", 0),
                ("Projects", 0),
                ("Rust", 1),
                ("vkarma", 2),
                ("Inbox", 0),
                ("Notes.txt", 0),
            ]
        );

        let filtered = idx.visible_tree_rows(&expanded, VaultSort::NameAsc, "vkm");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].display_name, "vkarma");
        assert_eq!(filtered[0].depth, 0);
    }

    #[test]
    fn quick_switch_and_wikilink_resolution_work() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("Archive")).unwrap();
        fs::write(root.join("Project Plan.md"), "# Plan").unwrap();
        fs::write(root.join("Archive/熵电台.md"), "# Radio").unwrap();
        fs::write(root.join("Notes.md"), "[[Project Plan#Goals|the plan]] [[Archive/熵电台]]")
            .unwrap();

        let idx = VaultIndex::build(root);
        assert_eq!(
            idx.resolve_wikilink("Project Plan#Goals|the plan"),
            Some(root.join("Project Plan.md"))
        );
        assert_eq!(
            idx.resolve_wikilink("Archive/熵电台"),
            Some(root.join("Archive/熵电台.md"))
        );
        assert!(idx.resolve_wikilink("Missing Note").is_none());

        let hits = idx.quick_switch("ppl", &[], 10);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].display_name, "Project Plan");

        let backlinks = idx.find_backlinks(&root.join("Project Plan.md"), 10);
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].name, "Notes");
    }

    #[test]
    fn obsidian_markdown_preprocessing_tags_and_formatting() {
        let src = "---\ntitle: Spec\ntags: [rust, gpui]\n---\n\nSee [[Target Note#Top|alias]] and ![[Deck]] and #mobile.\n\n```md\n[[NotALink]] #notatag\n```\nInline `[[AlsoIgnored]]` and **done**.";
        let processed = preprocess_obsidian_markdown(src);
        assert!(processed.starts_with("---\ntitle: Spec"));
        assert!(processed.contains("[alias](vkarma-wiki://Target%20Note)"));
        assert!(processed.contains("[Deck](vkarma-wiki://Deck)"));
        assert!(processed.contains("[[NotALink]]"));
        assert!(processed.contains("`[[AlsoIgnored]]`"));

        let links = extract_wikilinks(src);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "Target Note");
        assert_eq!(links[0].display, "alias");
        assert_eq!(links[1].target, "Deck");

        let tags = extract_tags(src);
        assert_eq!(tags, vec!["rust", "gpui", "mobile"]);

        assert_eq!(
            format_selection_or_snippet(MarkdownFormatAction::Bold, "hello"),
            "**hello**"
        );
        assert_eq!(
            format_selection_or_snippet(MarkdownFormatAction::Checkbox, ""),
            "\n- [ ] "
        );
    }

    #[test]
    fn rename_and_workspace_state_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("Draft.md");
        atomic_write_string(&old, "# Draft").unwrap();
        let renamed = rename_note_file(&old, "Final Spec").unwrap();
        assert_eq!(renamed, tmp.path().join("Final Spec.md"));
        assert!(!old.exists());
        assert!(renamed.exists());

        let state_path = tmp.path().join("workspace-state.json");
        let st = MobileWorkspaceState {
            vault_root: Some(tmp.path().to_path_buf()),
            last_note_path: Some(renamed.clone()),
            recent_notes: vec![renamed],
            expanded_dirs: vec!["Projects".to_string()],
            sort_mode: VaultSort::ModifiedDesc,
        };
        st.save_to(&state_path).unwrap();
        let loaded = MobileWorkspaceState::load_from(&state_path);
        assert_eq!(loaded.vault_root, st.vault_root);
        assert_eq!(loaded.last_note_path, st.last_note_path);
        assert_eq!(loaded.sort_mode, VaultSort::ModifiedDesc);

        // Legacy workspace-state.json containing `books_root` still deserializes cleanly.
        let legacy_path = tmp.path().join("legacy-workspace-state.json");
        fs::write(
            &legacy_path,
            format!(
                r#"{{"vault_root":"{}","books_root":"/sdcard/Documents/Library","sort_mode":"NameAsc"}}"#,
                tmp.path().display()
            ),
        )
        .unwrap();
        let legacy = MobileWorkspaceState::load_from(&legacy_path);
        assert_eq!(legacy.vault_root, st.vault_root);
    }

    #[test]
    fn list_subdirectories_and_count_direct_matching_files_work() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("beta")).unwrap();
        fs::create_dir_all(root.join("Alpha")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join("one.md"), "# 1").unwrap();
        fs::write(root.join("two.EPUB"), "x").unwrap();
        fs::write(root.join("Alpha/nested.md"), "# nested").unwrap();

        let dirs = list_subdirectories(root);
        let names: Vec<&str> = dirs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["Alpha", "beta"]);
        assert_eq!(count_direct_matching_files(root, &["md", "txt"]), 1);
        assert_eq!(count_direct_matching_files(root, &["epub"]), 1);
    }

    #[test]
    fn vault_index_cache_roundtrip_and_root_check() {
        let tmp = tempfile::tempdir().unwrap();
        let vault_a = tmp.path().join("VaultA");
        let vault_b = tmp.path().join("VaultB");
        fs::create_dir_all(&vault_a).unwrap();
        fs::create_dir_all(&vault_b).unwrap();
        fs::write(vault_a.join("Note.md"), "# Note").unwrap();

        let idx = VaultIndex::build(&vault_a);
        assert_eq!(idx.note_count(), 1);
        let cache_file = tmp.path().join("vault-index-cache.json");
        idx.save_cached_to(&cache_file).unwrap();

        let hit = VaultIndex::load_cached_from(&cache_file, &vault_a).unwrap();
        assert_eq!(hit.note_count(), 1);
        assert_eq!(hit.notes[0].display_name, "Note");
        assert!(VaultIndex::load_cached_from(&cache_file, &vault_b).is_none());
    }

    #[test]
    fn preprocess_obsidian_markdown_preserves_single_newlines_without_breaking_blocks() {
        let raw = "# Heading\nFirst line\nSecond line\n- bullet one\n- bullet two\n\n```\ncode1\ncode2\n```";
        let pre = preprocess_obsidian_markdown(raw);
        assert!(pre.contains("First line  \nSecond line"));
        assert!(!pre.contains("# Heading  \n"));
        assert!(!pre.contains("- bullet one  \n"));
        assert!(pre.contains("code1\ncode2\n"));
    }
}
