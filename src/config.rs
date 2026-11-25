use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MdViewMode {
    Edit,
    Split,
    Reading,
}

impl Default for MdViewMode {
    fn default() -> Self {
        Self::Split
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WorkspacePreset {
    #[default]
    Notes,
    Writing,
    Code,
    Sheets,
    Typst,
    Library,
}

impl WorkspacePreset {
    pub fn all() -> &'static [WorkspacePreset] {
        &[
            Self::Notes,
            Self::Writing,
            Self::Code,
            Self::Sheets,
            Self::Typst,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Notes => "Notes",
            Self::Writing => "Writing",
            Self::Code => "Code",
            Self::Sheets => "Sheets",
            Self::Typst => "Typst",
            Self::Library => "Library",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Notes => "Left file tree, right outline & backlinks, Live Preview editing",
            Self::Writing => "Distraction-ready Live Preview with outline & backlinks open",
            Self::Code => "Focused left file tree, Source mode default, compact sidebars",
            Self::Sheets => "Spreadsheet & CSV workbench with split Markdown reference",
            Self::Typst => "Typst live studio with side-by-side preview and outline",
            Self::Library => "E-book bookshelf, EPUB metadata & cover editor, and reader",
        }
    }

    /// Returns `(sidebar_visible, right_panel_visible, default_md_view, default_md_source_mode)`.
    pub fn defaults(self) -> (bool, bool, MdViewMode, bool) {
        match self {
            Self::Notes => (true, true, MdViewMode::Edit, false),
            Self::Writing => (true, true, MdViewMode::Edit, false),
            Self::Code => (true, false, MdViewMode::Edit, true),
            Self::Sheets => (true, false, MdViewMode::Split, false),
            Self::Typst => (true, true, MdViewMode::Split, false),
            Self::Library => (false, false, MdViewMode::Edit, false),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceConfig {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    pub preset: WorkspacePreset,
    pub sidebar_visible: bool,
    pub right_panel_visible: bool,
    pub default_md_view: MdViewMode,
    pub default_md_source_mode: bool,
    pub left_tabs: Vec<PathBuf>,
    pub left_active_tab: usize,
    pub right_tabs: Vec<PathBuf>,
    pub right_active_tab: usize,
    pub split_open: bool,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self::from_preset("notes", "Notes", default_workspace_path(), WorkspacePreset::Notes)
    }
}

impl WorkspaceConfig {
    pub fn from_preset(
        id: impl Into<String>,
        name: impl Into<String>,
        root: PathBuf,
        preset: WorkspacePreset,
    ) -> Self {
        let (sidebar_visible, right_panel_visible, default_md_view, default_md_source_mode) =
            preset.defaults();
        Self {
            id: id.into(),
            name: name.into(),
            root,
            preset,
            sidebar_visible,
            right_panel_visible,
            default_md_view,
            default_md_source_mode,
            left_tabs: Vec::new(),
            left_active_tab: 0,
            right_tabs: Vec::new(),
            right_active_tab: 0,
            split_open: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub last_workspace: Option<PathBuf>,
    pub recent_workspaces: Vec<PathBuf>,
    pub show_hidden_files: bool,
    pub sidebar_visible: bool,
    pub right_panel_visible: bool,
    pub default_md_view: MdViewMode,
    pub default_md_source_mode: bool,
    pub autosave: bool,
    pub workspaces: Vec<WorkspaceConfig>,
    pub active_workspace_id: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            last_workspace: None,
            recent_workspaces: Vec::new(),
            show_hidden_files: false,
            sidebar_visible: true,
            right_panel_visible: false,
            default_md_view: MdViewMode::Split,
            default_md_source_mode: false,
            autosave: false,
            workspaces: Vec::new(),
            active_workspace_id: None,
        }
    }
}

pub fn default_starter_workspaces(fallback_root: Option<PathBuf>) -> Vec<WorkspaceConfig> {
    let root = fallback_root
        .filter(|p| p.is_dir())
        .unwrap_or_else(default_workspace_path);
    let canon = fs::canonicalize(&root).unwrap_or(root);
    let name = canon
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Notes")
        .to_string();
    let id = make_unique_workspace_id(&name, &[]);
    vec![WorkspaceConfig::from_preset(
        id,
        name,
        canon,
        WorkspacePreset::Notes,
    )]
}

pub fn make_unique_workspace_id(name: &str, existing: &[WorkspaceConfig]) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug = "workspace".to_string();
    }

    if !existing.iter().any(|w| w.id == slug) {
        return slug;
    }
    for suffix in 2usize.. {
        let candidate = format!("{slug}-{suffix}");
        if !existing.iter().any(|w| w.id == candidate) {
            return candidate;
        }
    }
    slug
}

pub fn find_workspace_for_path(workspaces: &[WorkspaceConfig], target: &Path) -> Option<usize> {
    let canon_target = fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    workspaces
        .iter()
        .enumerate()
        .filter_map(|(idx, ws)| {
            let canon_root = fs::canonicalize(&ws.root).unwrap_or_else(|_| ws.root.clone());
            if canon_target.starts_with(&canon_root) {
                Some((idx, canon_root.components().count()))
            } else {
                None
            }
        })
        .max_by_key(|(_, depth)| *depth)
        .map(|(idx, _)| idx)
}

pub fn default_workspace_path() -> PathBuf {
    if let Some(docs) = dirs::document_dir() {
        let notes = docs.join("Notes");
        if notes.is_dir() {
            return notes;
        }
        if docs.is_dir() {
            return docs;
        }
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vkarma")
        .join("config.json")
}

impl AppConfig {
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(raw) = fs::read_to_string(&path)
            && let Ok(cfg) = serde_json::from_str::<AppConfig>(&raw)
        {
            return cfg;
        }
        Self::default()
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create config dir {}", parent.display()))?;
        }
        let body = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.vkarma-tmp");
        fs::write(&tmp, &body)
            .with_context(|| format!("write temp config {}", tmp.display()))?;
        if fs::rename(&tmp, &path).is_err() {
            let _ = fs::remove_file(&tmp);
            fs::write(&path, &body)
                .with_context(|| format!("write config {}", path.display()))?;
        }
        Ok(())
    }

    pub fn remember_workspace(&mut self, path: PathBuf) {
        self.last_workspace = Some(path.clone());
        self.recent_workspaces.retain(|p| p != &path);
        self.recent_workspaces.insert(0, path);
        self.recent_workspaces.truncate(10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_json_deserializes_with_empty_workspaces() {
        let legacy = r#"{
            "last_workspace": "/tmp/notes",
            "recent_workspaces": ["/tmp/notes"],
            "show_hidden_files": true,
            "sidebar_visible": false,
            "right_panel_visible": true,
            "default_md_view": "Edit",
            "default_md_source_mode": false,
            "autosave": true
        }"#;
        let parsed: AppConfig = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.last_workspace, Some(PathBuf::from("/tmp/notes")));
        assert!(parsed.autosave);
        assert!(parsed.workspaces.is_empty());
        assert_eq!(parsed.active_workspace_id, None);
    }

    #[test]
    fn workspace_config_roundtrip_and_unique_id_generation() {
        let ws1 = WorkspaceConfig::from_preset(
            make_unique_workspace_id("My Notes!", &[]),
            "My Notes!",
            PathBuf::from("/tmp/notes"),
            WorkspacePreset::Notes,
        );
        assert_eq!(ws1.id, "my-notes");
        assert!(ws1.sidebar_visible);
        assert!(ws1.right_panel_visible);
        assert_eq!(ws1.default_md_view, MdViewMode::Edit);
        assert!(!ws1.default_md_source_mode);

        let id2 = make_unique_workspace_id("My Notes?", &[ws1.clone()]);
        assert_eq!(id2, "my-notes-2");
        let ws2 = WorkspaceConfig::from_preset(
            id2,
            "My Notes?",
            PathBuf::from("/tmp/notes-2"),
            WorkspacePreset::Code,
        );
        assert!(ws2.default_md_source_mode);
        assert!(!ws2.right_panel_visible);

        let id3 = make_unique_workspace_id("My Notes", &[ws1.clone(), ws2.clone()]);
        assert_eq!(id3, "my-notes-3");

        let mut cfg = AppConfig::default();
        cfg.workspaces = vec![ws1, ws2];
        cfg.active_workspace_id = Some("my-notes-2".to_string());
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.workspaces, cfg.workspaces);
        assert_eq!(back.active_workspace_id.as_deref(), Some("my-notes-2"));
    }

    #[test]
    fn find_workspace_for_path_prefers_deepest_matching_root() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("work");
        let nested = parent.join("project-a");
        let deep_file = nested.join("src").join("main.rs");
        fs::create_dir_all(deep_file.parent().unwrap()).unwrap();
        fs::write(&deep_file, "fn main() {}\n").unwrap();

        let workspaces = vec![
            WorkspaceConfig::from_preset("work", "Work", parent.clone(), WorkspacePreset::Notes),
            WorkspaceConfig::from_preset("proj", "Proj", nested.clone(), WorkspacePreset::Code),
        ];
        assert_eq!(find_workspace_for_path(&workspaces, &deep_file), Some(1));
        assert_eq!(find_workspace_for_path(&workspaces, &parent), Some(0));
        assert_eq!(
            find_workspace_for_path(&workspaces, Path::new("/nonexistent/other")),
            None
        );
    }
}
