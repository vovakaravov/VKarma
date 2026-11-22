pub mod code_view;
pub mod fountain_view;
pub mod live_preview_view;
pub mod markdown_view;
pub mod modals;
pub mod pane;
pub mod pdf_view;
pub mod right_panel;
pub mod sidebar;
pub mod spreadsheet_view;
pub mod status_bar;
pub mod typst_view;
pub mod workspace_bar;

use crate::config::{
    AppConfig, MdViewMode, WorkspaceConfig, WorkspacePreset, default_starter_workspaces,
    find_workspace_for_path, make_unique_workspace_id,
};
use crate::fountain::{
    FountainDocument, extract_fountain_headings, fountain_to_typst, parse_fountain,
};
use crate::live_preview::{
    LiveBlock, LiveBlockKind, LiveEnterAction, block_index_for_line, byte_offset_to_line_col,
    clamp_to_char_boundary, classify_single_line, click_pos_to_raw_byte, compute_smart_enter,
    line_col_to_byte_offset, parse_live_blocks, splice_block_in_doc, toggle_task_in_line,
};
use live_preview_view::LiveScrollbarHandle;
use crate::markdown::{
    SceneBreakPlugin, StyledImagePlugin, preprocess_markdown_with_options, toggle_nth_task_checkbox,
    word_and_char_count,
};
use crate::pdf_engine::{DEFAULT_PDF_DPI, DEFAULT_PDF_PAGE_LIMIT, render_pdf_pages};
use crate::spreadsheet::SpreadsheetDoc;
use crate::theme::{hex_color, kd};
use crate::typst_engine::{compile_typst_png, empty_typst_debounce, export_typst_pdf};
use crate::vault::{
    FileKind, OutlineHeading, SearchHit, VaultIndex, atomic_write_string, detect_file_kind,
    extract_headings, search_content,
};
use crate::watcher::WorkspaceWatcher;
use gpui_kit::assets::IconName;
use gpui_kit::base::input::{BufferPoint, DiagnosticColors, InputEditorStyle};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{EditorState, InputEvent, InputState, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::text::{FrontmatterPlugin, MarkdownExtensions, TaskToggleEvent};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, AppContext as _, Bounds, ClipboardEntry, ClipboardItem, Context, Entity, FocusHandle,
    Focusable, Hsla, Image, InteractiveElement as _, IntoElement, KeyBinding, ListAlignment,
    ListState, MouseButton, MouseDownEvent, NavigationDirection, ParentElement, PathPromptOptions,
    Pixels, Point, Render, SharedString, Styled, Subscription, UniformListScrollHandle, Window,
    actions, div, prelude::FluentBuilder as _, px,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

static NEXT_TAB_ID: AtomicU64 = AtomicU64::new(1);

fn next_tab_id() -> u64 {
    NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed)
}

actions!(
    vkarma,
    [
        SaveFile,
        SaveAll,
        QuickSwitcher,
        VaultSearch,
        ToggleViewMode,
        ToggleSourceMode,
        SplitRight,
        CloseTab,
        NextTab,
        ToggleSidebar,
        ToggleRightPanel,
        NewNote,
        OpenFileDialog,
        OpenFolderDialog,
        EscapeModal,
        NavigateBack,
        NavigateForward,
        SwitchWorkspace1,
        SwitchWorkspace2,
        SwitchWorkspace3,
        SwitchWorkspace4,
        SwitchWorkspace5,
        SwitchWorkspace6,
        SwitchWorkspace7,
        SwitchWorkspace8,
        SwitchWorkspace9,
        PrevWorkspace,
        NextWorkspace,
        NewWorkspaceDialog,
    ]
);

pub fn register_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-s", SaveFile, None),
        KeyBinding::new("ctrl-shift-s", SaveAll, None),
        KeyBinding::new("ctrl-p", QuickSwitcher, None),
        KeyBinding::new("ctrl-shift-f", VaultSearch, None),
        KeyBinding::new("ctrl-e", ToggleViewMode, None),
        KeyBinding::new("ctrl-m", ToggleSourceMode, None),
        KeyBinding::new("ctrl-\\", SplitRight, None),
        KeyBinding::new("ctrl-w", CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-b", ToggleSidebar, None),
        KeyBinding::new("ctrl-shift-b", ToggleRightPanel, None),
        KeyBinding::new("ctrl-n", NewNote, None),
        KeyBinding::new("ctrl-o", OpenFileDialog, None),
        KeyBinding::new("ctrl-shift-o", OpenFolderDialog, None),
        KeyBinding::new("escape", EscapeModal, None),
        KeyBinding::new("alt-left", NavigateBack, None),
        KeyBinding::new("alt-right", NavigateForward, None),
        KeyBinding::new("ctrl-alt-1", SwitchWorkspace1, None),
        KeyBinding::new("ctrl-alt-2", SwitchWorkspace2, None),
        KeyBinding::new("ctrl-alt-3", SwitchWorkspace3, None),
        KeyBinding::new("ctrl-alt-4", SwitchWorkspace4, None),
        KeyBinding::new("ctrl-alt-5", SwitchWorkspace5, None),
        KeyBinding::new("ctrl-alt-6", SwitchWorkspace6, None),
        KeyBinding::new("ctrl-alt-7", SwitchWorkspace7, None),
        KeyBinding::new("ctrl-alt-8", SwitchWorkspace8, None),
        KeyBinding::new("ctrl-alt-9", SwitchWorkspace9, None),
        KeyBinding::new("ctrl-pageup", PrevWorkspace, None),
        KeyBinding::new("ctrl-pagedown", NextWorkspace, None),
        KeyBinding::new("ctrl-alt-n", NewWorkspaceDialog, None),
    ]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Files,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightPanelTab {
    Outline,
    Backlinks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypstViewMode {
    Edit,
    Split,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FountainViewMode {
    Edit,
    Split,
    Script,
}

pub enum TabKind {
    Markdown {
        editor: Entity<EditorState>,
        view_mode: MdViewMode,
        source_mode: bool,
        preprocessed: SharedString,
        word_count: usize,
        char_count: usize,
        headings: Vec<OutlineHeading>,
        sync_gen: u64,
        is_mdx: bool,
        chapter_mode: bool,
        active_chapter: usize,
        live_blocks: Arc<Vec<LiveBlock>>,
        live_active_block: Option<usize>,
        live_editor: Entity<TextareaState>,
        live_list_state: ListState,
        live_scrollbar: LiveScrollbarHandle,
        live_syncing: Arc<AtomicBool>,
        live_row_bounds: Rc<RefCell<HashMap<usize, Bounds<Pixels>>>>,
        live_last_wrap_rows: usize,
        live_dirty_block: bool,
        live_edit_gen: u64,
        _live_sub: Subscription,
        _sub: Subscription,
    },
    Fountain {
        editor: Entity<EditorState>,
        view_mode: FountainViewMode,
        paper_light: bool,
        show_notes: bool,
        parsed: FountainDocument,
        headings: Vec<OutlineHeading>,
        _sub: Subscription,
    },
    Typst {
        editor: Entity<EditorState>,
        view_mode: TypstViewMode,
        zoom: f32,
        pages: Vec<Arc<Image>>,
        compile_ms: Option<u128>,
        last_error: Option<String>,
        compiling: bool,
        compile_gen: u64,
        headings: Vec<OutlineHeading>,
        _sub: Subscription,
    },
    Spreadsheet {
        doc: SpreadsheetDoc,
        active_sheet: u32,
        active_row: i32,
        active_col: i32,
        formula_input: Entity<InputState>,
        row_scroll: UniformListScrollHandle,
        grid_focus: FocusHandle,
        _sub: Subscription,
    },
    Code {
        editor: Entity<EditorState>,
        language: &'static str,
        badge: &'static str,
        soft_wrap: bool,
        _sub: Subscription,
    },
    Image {
        zoom: f32,
        fit_to_view: bool,
        file_size_bytes: u64,
    },
    Pdf {
        zoom: f32,
        pages: Vec<Arc<Image>>,
        total_pages: usize,
        rendered_limit: Option<usize>,
        render_ms: Option<u128>,
        last_error: Option<String>,
        rendering: bool,
        render_gen: u64,
        file_size_bytes: u64,
        modified: Option<std::time::SystemTime>,
    },
}

pub struct DocumentTab {
    pub id: u64,
    pub path: PathBuf,
    pub title: String,
    pub rel_path: String,
    pub dirty: bool,
    pub last_saved: Option<String>,
    pub kind: TabKind,
}

pub struct WorkspacePane {
    pub tabs: Vec<DocumentTab>,
    pub active_tab: usize,
}

impl WorkspacePane {
    pub fn empty() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: 0,
        }
    }

    pub fn active(&self) -> Option<&DocumentTab> {
        self.tabs.get(self.active_tab)
    }

    pub fn active_mut(&mut self) -> Option<&mut DocumentTab> {
        self.tabs.get_mut(self.active_tab)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavEntry {
    pub pane: PaneSide,
    pub path: PathBuf,
}

const MAX_NAV_HISTORY: usize = 128;

#[derive(Debug, Clone, Default)]
pub struct NavHistory {
    entries: Vec<NavEntry>,
    cursor: Option<usize>,
}

impl NavHistory {
    pub fn push(&mut self, pane: PaneSide, path: PathBuf) {
        let entry = NavEntry { pane, path };
        if let Some(cur) = self.cursor {
            if self.entries.get(cur) == Some(&entry) {
                return;
            }
            self.entries.truncate(cur + 1);
        }
        self.entries.push(entry);
        if self.entries.len() > MAX_NAV_HISTORY {
            let overflow = self.entries.len() - MAX_NAV_HISTORY;
            self.entries.drain(0..overflow);
        }
        self.cursor = Some(self.entries.len() - 1);
    }

    pub fn can_back(&self) -> bool {
        self.cursor.is_some_and(|c| c > 0)
    }

    pub fn can_forward(&self) -> bool {
        self.cursor.is_some_and(|c| c + 1 < self.entries.len())
    }

    pub fn go_back_matching<F>(&mut self, mut accept: F) -> Option<NavEntry>
    where
        F: FnMut(&NavEntry) -> bool,
    {
        let mut idx = self.cursor?;
        while idx > 0 {
            idx -= 1;
            if accept(&self.entries[idx]) {
                self.cursor = Some(idx);
                return Some(self.entries[idx].clone());
            }
        }
        None
    }

    pub fn go_forward_matching<F>(&mut self, mut accept: F) -> Option<NavEntry>
    where
        F: FnMut(&NavEntry) -> bool,
    {
        let mut idx = self.cursor?;
        while idx + 1 < self.entries.len() {
            idx += 1;
            if accept(&self.entries[idx]) {
                self.cursor = Some(idx);
                return Some(self.entries[idx].clone());
            }
        }
        None
    }
}

pub struct QuickSwitcherState {
    pub query_input: Entity<InputState>,
    pub query: String,
    pub selected: usize,
    pub _sub: Subscription,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewItemTemplate {
    Markdown,
    Typst,
    Spreadsheet,
    Fountain,
    Folder,
}

impl NewItemTemplate {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Markdown => "New Markdown Note",
            Self::Typst => "New Typst Document",
            Self::Spreadsheet => "New Spreadsheet (.xlsx)",
            Self::Fountain => "New Fountain Screenplay",
            Self::Folder => "New Folder",
        }
    }

    pub fn placeholder(&self) -> &'static str {
        match self {
            Self::Markdown => "My Note.md",
            Self::Typst => "Document.typ",
            Self::Spreadsheet => "Sheet.xlsx",
            Self::Fountain => "Screenplay.fountain",
            Self::Folder => "Folder Name",
        }
    }

    pub fn extension(&self) -> Option<&'static str> {
        match self {
            Self::Markdown => Some("md"),
            Self::Typst => Some("typ"),
            Self::Spreadsheet => Some("xlsx"),
            Self::Fountain => Some("fountain"),
            Self::Folder => None,
        }
    }
}

pub fn file_kind_color(kind: FileKind) -> Hsla {
    match kind {
        FileKind::Markdown => hex_color(kd::BLUE),
        FileKind::Mdx => hex_color(kd::ORANGE),
        FileKind::Fountain => hex_color(kd::VIOLET),
        FileKind::Typst => hex_color(kd::AQUA),
        FileKind::Spreadsheet => hex_color(kd::GREEN),
        FileKind::Image => hex_color(kd::PINK),
        FileKind::Pdf => hex_color(kd::RED),
        FileKind::Code(_) => hex_color(kd::YELLOW),
        FileKind::PlainText => hex_color(kd::FG_MUTED),
    }
}

pub fn tab_badge_and_color(tab: &DocumentTab) -> (&'static str, Hsla) {
    match &tab.kind {
        TabKind::Markdown { is_mdx, .. } => {
            if *is_mdx {
                ("MDX", hex_color(kd::ORANGE))
            } else {
                ("MD", hex_color(kd::BLUE))
            }
        }
        TabKind::Fountain { .. } => ("FTN", hex_color(kd::VIOLET)),
        TabKind::Typst { .. } => ("TYP", hex_color(kd::AQUA)),
        TabKind::Spreadsheet { doc, .. } => (doc.format.label(), hex_color(kd::GREEN)),
        TabKind::Code { badge, .. } => (*badge, hex_color(kd::YELLOW)),
        TabKind::Image { .. } => ("IMG", hex_color(kd::PINK)),
        TabKind::Pdf { .. } => ("PDF", hex_color(kd::RED)),
    }
}

pub struct ParkedWorkspaceState {
    pub vault: Arc<VaultIndex>,
    pub watcher: Option<WorkspaceWatcher>,
    pub left_pane: WorkspacePane,
    pub right_pane: Option<WorkspacePane>,
    pub active_pane: PaneSide,
    pub sidebar_tab: SidebarTab,
    pub right_panel_tab: RightPanelTab,
    pub expanded_dirs: HashSet<PathBuf>,
    pub search_query: String,
    pub search_hits: Vec<SearchHit>,
    pub nav_history: NavHistory,
}

pub struct WorkspaceSlot {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    pub preset: WorkspacePreset,
    pub sidebar_visible: bool,
    pub right_panel_visible: bool,
    pub default_md_view: MdViewMode,
    pub default_md_source_mode: bool,
    pub pending_left_tabs: Vec<PathBuf>,
    pub pending_left_active: usize,
    pub pending_right_tabs: Vec<PathBuf>,
    pub pending_right_active: usize,
    pub pending_split_open: bool,
    pub hydrated: bool,
    pub parked: Option<ParkedWorkspaceState>,
}

impl WorkspaceSlot {
    pub fn from_config(cfg: WorkspaceConfig) -> Self {
        Self {
            id: cfg.id,
            name: cfg.name,
            root: cfg.root,
            preset: cfg.preset,
            sidebar_visible: cfg.sidebar_visible,
            right_panel_visible: cfg.right_panel_visible,
            default_md_view: cfg.default_md_view,
            default_md_source_mode: cfg.default_md_source_mode,
            pending_left_tabs: cfg.left_tabs,
            pending_left_active: cfg.left_active_tab,
            pending_right_tabs: cfg.right_tabs,
            pending_right_active: cfg.right_active_tab,
            pending_split_open: cfg.split_open,
            hydrated: false,
            parked: None,
        }
    }
}

pub struct VKarmaApp {
    pub focus_handle: FocusHandle,
    pub config: AppConfig,
    pub workspaces: Vec<WorkspaceSlot>,
    pub active_workspace: usize,
    pub workspace_root: PathBuf,
    pub vault: Arc<VaultIndex>,
    pub watcher: Option<WorkspaceWatcher>,
    pub markdown_extensions: MarkdownExtensions,

    pub left_pane: WorkspacePane,
    pub right_pane: Option<WorkspacePane>,
    pub active_pane: PaneSide,

    pub sidebar_tab: SidebarTab,
    pub right_panel_tab: RightPanelTab,
    pub expanded_dirs: HashSet<PathBuf>,

    pub search_input: Entity<InputState>,
    pub search_hits: Vec<SearchHit>,
    pub _search_sub: Subscription,

    pub quick_switcher: Option<QuickSwitcherState>,
    pub nav_history: NavHistory,
    pub status_toast: Option<String>,
    pub workspace_gen: u64,
}

impl Focusable for VKarmaApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl VKarmaApp {
    pub fn new(initial_target: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut config = AppConfig::load();
        if config.workspaces.is_empty() {
            config.workspaces = default_starter_workspaces(config.last_workspace.clone());
        }

        let mut workspaces: Vec<WorkspaceSlot> = config
            .workspaces
            .iter()
            .cloned()
            .map(WorkspaceSlot::from_config)
            .collect();

        let saved_idx = config
            .active_workspace_id
            .as_deref()
            .and_then(|id| workspaces.iter().position(|w| w.id == id))
            .unwrap_or(0);

        let (active_idx, initial_file) = match initial_target {
            Some(path) if path.is_file() => {
                let canon_file = fs::canonicalize(&path).unwrap_or(path);
                let idx = find_workspace_for_path(&config.workspaces, &canon_file)
                    .unwrap_or(saved_idx);
                (idx, Some(canon_file))
            }
            Some(path) if path.is_dir() => {
                let canon_dir = fs::canonicalize(&path).unwrap_or(path);
                if let Some(idx) = workspaces.iter().position(|w| {
                    fs::canonicalize(&w.root).unwrap_or_else(|_| w.root.clone()) == canon_dir
                }) {
                    (idx, None)
                } else {
                    let dir_name = canon_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Workspace")
                        .to_string();
                    let id = make_unique_workspace_id(&dir_name, &config.workspaces);
                    let cfg = WorkspaceConfig::from_preset(
                        id,
                        dir_name,
                        canon_dir,
                        WorkspacePreset::Notes,
                    );
                    workspaces.push(WorkspaceSlot::from_config(cfg));
                    (workspaces.len() - 1, None)
                }
            }
            _ => (saved_idx, None),
        };

        let active_slot = &workspaces[active_idx];
        let initial_ws = fs::canonicalize(&active_slot.root)
            .unwrap_or_else(|_| active_slot.root.clone());
        let ws_title = format!("VKarma — {}", active_slot.name);
        config.sidebar_visible = active_slot.sidebar_visible;
        config.right_panel_visible = active_slot.right_panel_visible;
        config.default_md_view = active_slot.default_md_view;
        config.default_md_source_mode = active_slot.default_md_source_mode;
        config.remember_workspace(initial_ws.clone());

        let vault = Arc::new(VaultIndex::build(&initial_ws, config.show_hidden_files));
        let watcher = WorkspaceWatcher::start(&initial_ws).ok();

        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search in vault…"));
        let _search_sub = cx.subscribe_in(&search_input, window, |this, state, event, _, cx| {
            if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                let query = state.read(cx).value().to_string();
                this.run_vault_search(query, cx);
            }
        });

        let markdown_extensions = MarkdownExtensions::default()
            .frontmatter()
            .plugin(FrontmatterPlugin::new())
            .plugin(StyledImagePlugin)
            .plugin(SceneBreakPlugin)
            .parser_revision(1);

        let mut app = Self {
            focus_handle: cx.focus_handle(),
            config,
            workspaces,
            active_workspace: active_idx,
            workspace_root: initial_ws,
            vault,
            watcher,
            markdown_extensions,
            left_pane: WorkspacePane::empty(),
            right_pane: None,
            active_pane: PaneSide::Left,
            sidebar_tab: SidebarTab::Files,
            right_panel_tab: RightPanelTab::Outline,
            expanded_dirs: HashSet::new(),
            search_input,
            search_hits: Vec::new(),
            _search_sub,
            quick_switcher: None,
            nav_history: NavHistory::default(),
            status_toast: None,
            workspace_gen: 1,
        };

        window.set_window_title(&ws_title);
        app.hydrate_active_workspace_tabs(window, cx);
        if let Some(file_path) = initial_file {
            app.open_file(file_path, PaneSide::Left, window, cx);
        } else {
            app.persist_workspaces();
        }

        // Background watcher loop to refresh file tree & backlinks when files change externally
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(600))
                    .await;
                let changed = this
                    .update(cx, |app, _| {
                        app.watcher
                            .as_ref()
                            .is_some_and(WorkspaceWatcher::poll_changed)
                    })
                    .unwrap_or(false);
                if changed {
                    let _ = this.update(cx, |app, cx| {
                        app.refresh_vault_index(cx);
                        app.reload_modified_pdf_tabs(cx);
                    });
                }
            }
        })
        .detach();

        app
    }

    pub fn workspace_has_dirty_tabs(&self, idx: usize) -> bool {
        if idx == self.active_workspace {
            return self.left_pane.tabs.iter().any(|t| t.dirty)
                || self
                    .right_pane
                    .as_ref()
                    .is_some_and(|p| p.tabs.iter().any(|t| t.dirty));
        }
        self.workspaces.get(idx).is_some_and(|ws| {
            ws.parked.as_ref().is_some_and(|p| {
                p.left_pane.tabs.iter().any(|t| t.dirty)
                    || p.right_pane
                        .as_ref()
                        .is_some_and(|rp| rp.tabs.iter().any(|t| t.dirty))
            })
        })
    }

    fn slot_to_workspace_config(&self, idx: usize, slot: &WorkspaceSlot) -> WorkspaceConfig {
        let (left_tabs, left_active_tab, right_tabs, right_active_tab, split_open) =
            if idx == self.active_workspace {
                (
                    self.left_pane.tabs.iter().map(|t| t.path.clone()).collect(),
                    self.left_pane.active_tab,
                    self.right_pane
                        .as_ref()
                        .map(|p| p.tabs.iter().map(|t| t.path.clone()).collect())
                        .unwrap_or_default(),
                    self.right_pane.as_ref().map(|p| p.active_tab).unwrap_or(0),
                    self.right_pane.is_some(),
                )
            } else if let Some(parked) = &slot.parked {
                (
                    parked.left_pane.tabs.iter().map(|t| t.path.clone()).collect(),
                    parked.left_pane.active_tab,
                    parked
                        .right_pane
                        .as_ref()
                        .map(|p| p.tabs.iter().map(|t| t.path.clone()).collect())
                        .unwrap_or_default(),
                    parked.right_pane.as_ref().map(|p| p.active_tab).unwrap_or(0),
                    parked.right_pane.is_some(),
                )
            } else {
                (
                    slot.pending_left_tabs.clone(),
                    slot.pending_left_active,
                    slot.pending_right_tabs.clone(),
                    slot.pending_right_active,
                    slot.pending_split_open,
                )
            };

        WorkspaceConfig {
            id: slot.id.clone(),
            name: slot.name.clone(),
            root: slot.root.clone(),
            preset: slot.preset,
            sidebar_visible: slot.sidebar_visible,
            right_panel_visible: slot.right_panel_visible,
            default_md_view: slot.default_md_view,
            default_md_source_mode: slot.default_md_source_mode,
            left_tabs,
            left_active_tab,
            right_tabs,
            right_active_tab,
            split_open,
        }
    }

    pub fn persist_workspaces(&mut self) {
        if let Some(active) = self.workspaces.get_mut(self.active_workspace) {
            active.root = self.workspace_root.clone();
            active.sidebar_visible = self.config.sidebar_visible;
            active.right_panel_visible = self.config.right_panel_visible;
            active.default_md_view = self.config.default_md_view;
            active.default_md_source_mode = self.config.default_md_source_mode;
        }
        self.config.workspaces = self
            .workspaces
            .iter()
            .enumerate()
            .map(|(idx, slot)| self.slot_to_workspace_config(idx, slot))
            .collect();
        self.config.active_workspace_id =
            self.workspaces.get(self.active_workspace).map(|w| w.id.clone());
        self.config.remember_workspace(self.workspace_root.clone());
        let _ = self.config.save();
    }

    fn hydrate_active_workspace_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (left_tabs, left_active, right_tabs, right_active, split_open) = {
            let slot = &mut self.workspaces[self.active_workspace];
            slot.hydrated = true;
            (
                std::mem::take(&mut slot.pending_left_tabs),
                slot.pending_left_active,
                std::mem::take(&mut slot.pending_right_tabs),
                slot.pending_right_active,
                slot.pending_split_open,
            )
        };

        for p in left_tabs {
            if p.is_file() {
                self.open_file_inner(p, PaneSide::Left, false, false, window, cx);
            }
        }
        if !self.left_pane.tabs.is_empty() {
            self.left_pane.active_tab = left_active.min(self.left_pane.tabs.len() - 1);
            if let Some(active_path) = self.left_pane.active().map(|t| t.path.clone()) {
                self.nav_history.push(PaneSide::Left, active_path);
            }
        }

        if split_open || !right_tabs.is_empty() {
            self.ensure_right_pane();
            for p in right_tabs {
                if p.is_file() {
                    self.open_file_inner(p, PaneSide::Right, false, false, window, cx);
                }
            }
            if let Some(rp) = &mut self.right_pane
                && !rp.tabs.is_empty()
            {
                rp.active_tab = right_active.min(rp.tabs.len() - 1);
                if let Some(active_path) = rp.active().map(|t| t.path.clone()) {
                    self.nav_history.push(PaneSide::Right, active_path);
                }
            }
        }

        self.active_pane = if !self.left_pane.tabs.is_empty() {
            PaneSide::Left
        } else if self.right_pane.as_ref().is_some_and(|p| !p.tabs.is_empty()) {
            PaneSide::Right
        } else {
            PaneSide::Left
        };
        if let Some(idx) = self.pane(self.active_pane).map(|p| p.active_tab) {
            self.focus_tab_widget(self.active_pane, idx, window, cx);
        }
    }

    pub fn switch_workspace(
        &mut self,
        target_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if target_idx >= self.workspaces.len() || target_idx == self.active_workspace {
            return;
        }
        for side in [PaneSide::Left, PaneSide::Right] {
            if let Some(tab_id) = self.pane(side).and_then(WorkspacePane::active).map(|t| t.id) {
                self.commit_active_live_block(side, tab_id, window, cx);
            }
        }
        self.quick_switcher = None;

        let search_query = self.search_input.read(cx).value().to_string();
        let old_idx = self.active_workspace;
        {
            let parked = ParkedWorkspaceState {
                vault: Arc::clone(&self.vault),
                watcher: self.watcher.take(),
                left_pane: std::mem::replace(&mut self.left_pane, WorkspacePane::empty()),
                right_pane: self.right_pane.take(),
                active_pane: self.active_pane,
                sidebar_tab: self.sidebar_tab,
                right_panel_tab: self.right_panel_tab,
                expanded_dirs: std::mem::take(&mut self.expanded_dirs),
                search_query,
                search_hits: std::mem::take(&mut self.search_hits),
                nav_history: std::mem::take(&mut self.nav_history),
            };
            let old_slot = &mut self.workspaces[old_idx];
            old_slot.root = self.workspace_root.clone();
            old_slot.sidebar_visible = self.config.sidebar_visible;
            old_slot.right_panel_visible = self.config.right_panel_visible;
            old_slot.default_md_view = self.config.default_md_view;
            old_slot.default_md_source_mode = self.config.default_md_source_mode;
            old_slot.hydrated = true;
            old_slot.parked = Some(parked);
        }

        self.active_workspace = target_idx;
        self.workspace_gen = self.workspace_gen.wrapping_add(1);

        let (target_root, target_name, parked_opt) = {
            let slot = &mut self.workspaces[target_idx];
            let canon = fs::canonicalize(&slot.root).unwrap_or_else(|_| slot.root.clone());
            slot.root = canon.clone();
            self.config.sidebar_visible = slot.sidebar_visible;
            self.config.right_panel_visible = slot.right_panel_visible;
            self.config.default_md_view = slot.default_md_view;
            self.config.default_md_source_mode = slot.default_md_source_mode;
            (canon, slot.name.clone(), slot.parked.take())
        };
        self.workspace_root = target_root.clone();

        if let Some(parked) = parked_opt {
            self.vault = parked.vault;
            self.watcher = parked.watcher;
            self.left_pane = parked.left_pane;
            self.right_pane = parked.right_pane;
            self.active_pane = parked.active_pane;
            self.sidebar_tab = parked.sidebar_tab;
            self.right_panel_tab = parked.right_panel_tab;
            self.expanded_dirs = parked.expanded_dirs;
            self.search_hits = parked.search_hits;
            self.nav_history = parked.nav_history;
            let q = parked.search_query;
            self.search_input.update(cx, |s, cx| s.set_value(q, window, cx));

            if self.vault.root != self.workspace_root {
                self.watcher = WorkspaceWatcher::start(&self.workspace_root).ok();
                self.refresh_vault_index(cx);
            } else if self.watcher.as_ref().is_some_and(WorkspaceWatcher::poll_changed) {
                self.refresh_vault_index(cx);
            }
        } else {
            let show_hidden = self.config.show_hidden_files;
            self.vault = Arc::new(VaultIndex::build(&target_root, show_hidden));
            self.watcher = WorkspaceWatcher::start(&target_root).ok();
            self.active_pane = PaneSide::Left;
            self.sidebar_tab = SidebarTab::Files;
            self.right_panel_tab = RightPanelTab::Outline;
            self.search_input
                .update(cx, |s, cx| s.set_value(String::new(), window, cx));
            self.hydrate_active_workspace_tabs(window, cx);
        }

        window.set_window_title(&format!("VKarma — {target_name}"));
        self.persist_workspaces();
        if let Some(active_tab) = self.pane(self.active_pane).map(|p| p.active_tab) {
            self.focus_tab_widget(self.active_pane, active_tab, window, cx);
        }
        self.set_status(format!("Workspace: {target_name}"), cx);
        cx.notify();
    }

    fn next_workspace_id(&self, name: &str) -> String {
        let cfgs: Vec<WorkspaceConfig> = self
            .workspaces
            .iter()
            .enumerate()
            .map(|(idx, slot)| self.slot_to_workspace_config(idx, slot))
            .collect();
        make_unique_workspace_id(name, &cfgs)
    }

    pub fn add_workspace(
        &mut self,
        name: String,
        root: PathBuf,
        preset: WorkspacePreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !root.is_dir() {
            self.toast_err(format!("Folder not found: {}", root.display()), window, cx);
            return;
        }
        let canon = fs::canonicalize(&root).unwrap_or(root);
        let trimmed = name.trim();
        let final_name = if trimmed.is_empty() {
            canon
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("Workspace")
                .to_string()
        } else {
            trimmed.to_string()
        };
        let id = self.next_workspace_id(&final_name);
        let cfg = WorkspaceConfig::from_preset(id, final_name, canon, preset);
        self.workspaces.push(WorkspaceSlot::from_config(cfg));
        let target_idx = self.workspaces.len() - 1;
        self.switch_workspace(target_idx, window, cx);
    }

    pub fn rename_workspace(
        &mut self,
        idx: usize,
        new_name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let trimmed = new_name.trim().to_string();
        if trimmed.is_empty() || idx >= self.workspaces.len() {
            return;
        }
        self.workspaces[idx].name = trimmed.clone();
        if idx == self.active_workspace {
            window.set_window_title(&format!("VKarma — {trimmed}"));
        }
        self.persist_workspaces();
        cx.notify();
    }

    pub fn rebind_workspace_folder(
        &mut self,
        idx: usize,
        new_root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !new_root.is_dir() || idx >= self.workspaces.len() {
            self.toast_err(format!("Folder not found: {}", new_root.display()), window, cx);
            return;
        }
        if idx == self.active_workspace {
            self.open_workspace(new_root, cx);
            return;
        }
        let canon = fs::canonicalize(&new_root).unwrap_or(new_root);
        let slot = &mut self.workspaces[idx];
        slot.root = canon;
        if let Some(parked) = &mut slot.parked {
            parked.watcher = None;
            parked.expanded_dirs.clear();
            parked.search_hits.clear();
        }
        self.persist_workspaces();
        cx.notify();
    }

    pub fn prompt_rebind_workspace_folder(
        &mut self,
        idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select Workspace Folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(dir) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |app, window, cx| {
                    app.rebind_workspace_folder(idx, dir, window, cx);
                });
            }
        })
        .detach();
    }

    pub fn duplicate_workspace(
        &mut self,
        idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(src) = self.workspaces.get(idx) else {
            return;
        };
        let mut cfg = self.slot_to_workspace_config(idx, src);
        let copy_name = format!("{} Copy", cfg.name);
        cfg.id = self.next_workspace_id(&copy_name);
        cfg.name = copy_name;
        self.workspaces.push(WorkspaceSlot::from_config(cfg));
        let new_idx = self.workspaces.len() - 1;
        self.switch_workspace(new_idx, window, cx);
    }

    pub fn move_workspace(&mut self, idx: usize, delta: isize, cx: &mut Context<Self>) {
        let len = self.workspaces.len();
        if idx >= len {
            return;
        }
        let target_isize = idx as isize + delta;
        if target_isize < 0 || target_isize as usize >= len {
            return;
        }
        let target = target_isize as usize;
        self.workspaces.swap(idx, target);
        if self.active_workspace == idx {
            self.active_workspace = target;
        } else if self.active_workspace == target {
            self.active_workspace = idx;
        }
        self.persist_workspaces();
        cx.notify();
    }

    pub fn reorder_workspace(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if reorder_with_active(&mut self.workspaces, &mut self.active_workspace, from, to) {
            self.persist_workspaces();
            cx.notify();
        }
    }

    pub fn remove_workspace(
        &mut self,
        idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspaces.len() <= 1 || idx >= self.workspaces.len() {
            self.toast_err("Cannot delete the last workspace.", window, cx);
            return;
        }
        if idx == self.active_workspace {
            let next_idx = if idx > 0 { idx - 1 } else { 1 };
            self.switch_workspace(next_idx, window, cx);
        }
        self.workspaces.remove(idx);
        if idx < self.active_workspace {
            self.active_workspace -= 1;
        }
        self.persist_workspaces();
        cx.notify();
    }

    pub fn toast_ok(&self, msg: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::success(msg.into()), cx);
    }

    pub fn toast_err(&self, msg: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::error(msg.into()), cx);
    }

    pub fn set_status(&mut self, msg: impl Into<String>, cx: &mut Context<Self>) {
        self.status_toast = Some(msg.into());
        cx.notify();
    }

    pub fn refresh_vault_index(&mut self, cx: &mut Context<Self>) {
        let root = self.workspace_root.clone();
        let show_hidden = self.config.show_hidden_files;
        let ws_gen = self.workspace_gen;

        cx.spawn(async move |this, cx| {
            let new_vault = cx
                .background_executor()
                .spawn(async move { Arc::new(VaultIndex::build(&root, show_hidden)) })
                .await;

            let _ = this.update(cx, |app, cx| {
                if app.workspace_gen != ws_gen {
                    return;
                }
                app.vault = new_vault;
                // Re-preprocess open markdown tabs cleanly if backlinks/embeds changed
                let vault = Arc::clone(&app.vault);
                for pane in app
                    .left_pane
                    .tabs
                    .iter_mut()
                    .chain(app.right_pane.iter_mut().flat_map(|p| p.tabs.iter_mut()))
                {
                    if let TabKind::Markdown {
                        editor,
                        preprocessed,
                        is_mdx,
                        ..
                    } = &mut pane.kind
                    {
                        let raw = editor.read(cx).value();
                        *preprocessed = preprocess_markdown_with_options(
                            raw.as_ref(),
                            &pane.path,
                            &vault,
                            0,
                            *is_mdx,
                        )
                        .into();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn open_workspace(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if !root.is_dir() {
            return;
        }
        let canon = fs::canonicalize(&root).unwrap_or(root);
        self.workspace_root = canon.clone();
        if let Some(slot) = self.workspaces.get_mut(self.active_workspace) {
            slot.root = canon.clone();
        }
        self.workspace_gen = self.workspace_gen.wrapping_add(1);
        self.expanded_dirs.clear();
        self.search_hits.clear();
        self.watcher = WorkspaceWatcher::start(&canon).ok();
        self.persist_workspaces();
        self.refresh_vault_index(cx);
        self.set_status(format!("Opened workspace {}", canon.display()), cx);
    }

    pub fn run_vault_search(&mut self, query: String, cx: &mut Context<Self>) {
        if query.trim().chars().count() < 2 {
            self.search_hits.clear();
            cx.notify();
            return;
        }
        let root = self.workspace_root.clone();
        let show_hidden = self.config.show_hidden_files;
        let ws_gen = self.workspace_gen;
        let query_snap = query.clone();

        cx.spawn(async move |this, cx| {
            let hits = cx
                .background_executor()
                .spawn(async move { search_content(&root, &query, show_hidden, 200) })
                .await;
            let _ = this.update(cx, |app, cx| {
                if app.workspace_gen == ws_gen
                    && app.search_input.read(cx).value().as_ref() == query_snap.as_str()
                {
                    app.search_hits = hits;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn pane_mut(&mut self, side: PaneSide) -> Option<&mut WorkspacePane> {
        match side {
            PaneSide::Left => Some(&mut self.left_pane),
            PaneSide::Right => self.right_pane.as_mut(),
        }
    }

    pub fn pane(&self, side: PaneSide) -> Option<&WorkspacePane> {
        match side {
            PaneSide::Left => Some(&self.left_pane),
            PaneSide::Right => self.right_pane.as_ref(),
        }
    }

    pub fn active_tab_ref(&self) -> Option<&DocumentTab> {
        self.pane(self.active_pane)
            .and_then(WorkspacePane::active)
            .or_else(|| self.left_pane.active())
    }

    pub fn ensure_right_pane(&mut self) {
        if self.right_pane.is_none() {
            self.right_pane = Some(WorkspacePane::empty());
        }
    }

    pub fn open_file(
        &mut self,
        path: PathBuf,
        target_pane: PaneSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_file_inner(path, target_pane, true, true, window, cx);
    }

    fn open_file_inner(
        &mut self,
        path: PathBuf,
        target_pane: PaneSide,
        record_history: bool,
        persist: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !path.is_file() {
            self.toast_err(format!("File not found: {}", path.display()), window, cx);
            return;
        }
        let canon = fs::canonicalize(&path).unwrap_or(path);
        if target_pane == PaneSide::Right {
            self.ensure_right_pane();
        }
        self.active_pane = target_pane;

        if let Some(pane) = self.pane_mut(target_pane)
            && let Some(existing) = pane.tabs.iter().position(|t| t.path == canon)
        {
            pane.active_tab = existing;
            if record_history {
                self.nav_history.push(target_pane, canon);
            }
            if persist {
                self.persist_workspaces();
            }
            self.focus_tab_widget(target_pane, existing, window, cx);
            cx.notify();
            return;
        }

        match self.build_document_tab(&canon, target_pane, window, cx) {
            Ok(tab) => {
                let mut opened_idx = None;
                if let Some(pane) = self.pane_mut(target_pane) {
                    pane.tabs.push(tab);
                    pane.active_tab = pane.tabs.len() - 1;
                    let idx = pane.active_tab;
                    opened_idx = Some(idx);
                    if record_history {
                        self.nav_history.push(target_pane, canon);
                    }
                }
                if let Some(idx) = opened_idx {
                    if persist {
                        self.persist_workspaces();
                    }
                    self.focus_tab_widget(target_pane, idx, window, cx);
                }
                cx.notify();
            }
            Err(err) => {
                self.toast_err(format!("Failed to open {}: {err}", canon.display()), window, cx);
            }
        }
    }

    pub fn activate_tab(
        &mut self,
        side: PaneSide,
        tab_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_pane = side;
        let path = if let Some(p) = self.pane_mut(side) {
            if tab_idx >= p.tabs.len() {
                return;
            }
            p.active_tab = tab_idx;
            Some(p.tabs[tab_idx].path.clone())
        } else {
            None
        };
        if let Some(path) = path {
            self.nav_history.push(side, path);
        }
        self.persist_workspaces();
        self.focus_tab_widget(side, tab_idx, window, cx);
        cx.notify();
    }

    pub fn navigate_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .active_tab_ref()
            .map(|t| (self.active_pane, t.path.clone()));
        let entry = self.nav_history.go_back_matching(|e| {
            e.path.is_file()
                && current
                    .as_ref()
                    .is_none_or(|(p, path)| !(*p == e.pane && *path == e.path))
        });
        if let Some(entry) = entry {
            let target_pane = if entry.pane == PaneSide::Right && self.right_pane.is_none() {
                PaneSide::Left
            } else {
                entry.pane
            };
            self.open_file_inner(entry.path, target_pane, false, true, window, cx);
        }
    }

    pub fn navigate_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .active_tab_ref()
            .map(|t| (self.active_pane, t.path.clone()));
        let entry = self.nav_history.go_forward_matching(|e| {
            e.path.is_file()
                && current
                    .as_ref()
                    .is_none_or(|(p, path)| !(*p == e.pane && *path == e.path))
        });
        if let Some(entry) = entry {
            let target_pane = if entry.pane == PaneSide::Right && self.right_pane.is_none() {
                PaneSide::Left
            } else {
                entry.pane
            };
            self.open_file_inner(entry.path, target_pane, false, true, window, cx);
        }
    }

    fn focus_tab_widget(
        &self,
        side: PaneSide,
        tab_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.pane(side).and_then(|p| p.tabs.get(tab_idx)) else {
            return;
        };
        match &tab.kind {
            TabKind::Markdown {
                editor,
                source_mode,
                live_active_block,
                live_editor,
                ..
            } => {
                if *source_mode {
                    editor.focus_handle(cx).focus(window, cx);
                } else if live_active_block.is_some() {
                    live_editor.focus_handle(cx).focus(window, cx);
                } else {
                    self.focus_handle.focus(window, cx);
                }
            }
            TabKind::Fountain { editor, .. }
            | TabKind::Typst { editor, .. }
            | TabKind::Code { editor, .. } => {
                editor.focus_handle(cx).focus(window, cx);
            }
            TabKind::Spreadsheet { grid_focus, .. } => {
                grid_focus.focus(window, cx);
            }
            TabKind::Image { .. } | TabKind::Pdf { .. } => {}
        }
    }

    fn build_document_tab(
        &self,
        path: &Path,
        side: PaneSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<DocumentTab> {
        let tab_id = next_tab_id();
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Untitled")
            .to_string();
        let rel_path = path
            .strip_prefix(&self.workspace_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let file_kind = detect_file_kind(path);

        let kind = match file_kind {
            FileKind::Markdown | FileKind::Mdx => {
                let is_mdx = file_kind == FileKind::Mdx;
                let raw = fs::read_to_string(path)?;
                let (word_count, char_count) = word_and_char_count(&raw);
                let headings = extract_headings(&raw, false);
                let preprocessed: SharedString =
                    preprocess_markdown_with_options(&raw, path, &self.vault, 0, is_mdx).into();
                let source_mode = self.config.default_md_source_mode;
                let view_mode = self.config.default_md_view;

                let live_blocks = Arc::new(parse_live_blocks(&raw, is_mdx));
                let live_list_state =
                    ListState::new(live_blocks.len() + 1, ListAlignment::Top, px(1200.));
                let live_scrollbar =
                    LiveScrollbarHandle::new(live_list_state.clone(), Arc::clone(&live_blocks));
                let live_syncing = Arc::new(AtomicBool::new(false));
                let live_row_bounds = Rc::new(RefCell::new(HashMap::new()));

                let editor = cx.new(|cx| {
                    let mut state = EditorState::new(window, cx)
                        .language("markdown")
                        .soft_wrap(true)
                        .line_number(source_mode)
                        .default_value(raw);
                    state.set_editor_paddings(gpui_kit::Edges {
                        top: px(16.),
                        right: px(28.),
                        bottom: px(24.),
                        left: px(28.),
                    });
                    state
                });

                let theme = cx.theme().clone();
                let live_editor = cx.new(|cx| {
                    let mut st = TextareaState::new(window, cx)
                        .soft_wrap(true)
                        .auto_grow(1, 200);
                    st.set_editor_paddings(gpui_kit::Edges::all(px(0.)));
                    st.set_editor_style(InputEditorStyle {
                        foreground: hex_color(kd::FG),
                        muted_foreground: hex_color(kd::FG_MUTED),
                        background: gpui_kit::transparent_black(),
                        border: gpui_kit::transparent_black(),
                        selection: theme.selection,
                        caret: hex_color(kd::BLUE),
                        diagnostics: DiagnosticColors {
                            error: hex_color(kd::RED),
                            warning: hex_color(kd::YELLOW),
                            info: hex_color(kd::BLUE),
                            hint: hex_color(kd::AQUA),
                        },
                        highlight_styles: theme.highlight_theme.clone(),
                        editor_invisible: None,
                        editor_active_line: None,
                        editor_gutter_background: None,
                        fold_icon_renderer: None,
                    });
                    st
                });

                let _live_sub =
                    cx.subscribe_in(&live_editor, window, move |this, _ed, event, window, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.on_live_editor_changed(side, tab_id, window, cx);
                        }
                    });

                let sub = cx.subscribe_in(&editor, window, move |this, _ed, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.on_markdown_changed(side, tab_id, window, cx);
                    }
                });

                TabKind::Markdown {
                    editor,
                    view_mode,
                    source_mode,
                    preprocessed,
                    word_count,
                    char_count,
                    headings,
                    sync_gen: 0,
                    is_mdx,
                    chapter_mode: false,
                    active_chapter: 0,
                    live_blocks,
                    live_active_block: None,
                    live_editor,
                    live_list_state,
                    live_scrollbar,
                    live_syncing,
                    live_row_bounds,
                    live_last_wrap_rows: 1,
                    live_dirty_block: false,
                    live_edit_gen: 0,
                    _live_sub,
                    _sub: sub,
                }
            }
            FileKind::Fountain => {
                let raw = fs::read_to_string(path)?;
                let parsed = parse_fountain(&raw);
                let headings = extract_fountain_headings(&parsed);
                let editor = cx.new(|cx| {
                    let mut st = EditorState::new(window, cx)
                        .soft_wrap(true)
                        .line_number(true)
                        .default_value(raw);
                    st.set_editor_paddings(gpui_kit::Edges {
                        top: px(16.),
                        right: px(24.),
                        bottom: px(24.),
                        left: px(16.),
                    });
                    st
                });
                let sub = cx.subscribe_in(&editor, window, move |this, _ed, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.on_fountain_changed(side, tab_id, window, cx);
                    }
                });
                TabKind::Fountain {
                    editor,
                    view_mode: FountainViewMode::Split,
                    paper_light: false,
                    show_notes: true,
                    parsed,
                    headings,
                    _sub: sub,
                }
            }
            FileKind::Typst => {
                let raw = fs::read_to_string(path)?;
                let headings = extract_headings(&raw, true);
                let editor = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .language("typst")
                        .soft_wrap(true)
                        .line_number(true)
                        .default_value(raw.clone())
                });

                let sub = cx.subscribe_in(&editor, window, move |this, _ed, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.on_typst_changed(side, tab_id, window, cx);
                    }
                });

                // Trigger immediate initial compile on load
                self.schedule_typst_compile(side, tab_id, 1, raw, path.to_path_buf(), 144, false, cx);

                TabKind::Typst {
                    editor,
                    view_mode: TypstViewMode::Split,
                    zoom: 1.0,
                    pages: Vec::new(),
                    compile_ms: None,
                    last_error: None,
                    compiling: true,
                    compile_gen: 1,
                    headings,
                    _sub: sub,
                }
            }
            FileKind::Spreadsheet => {
                let doc = SpreadsheetDoc::open(path)?;
                let initial_formula = doc.raw_input(0, 1, 1);
                let formula_input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("Cell value or formula (e.g. =SUM(A1:A10))")
                        .default_value(initial_formula)
                });
                let sub = cx.subscribe_in(
                    &formula_input,
                    window,
                    move |this, _input, event, window, cx| match event {
                        InputEvent::PressEnter { .. } => {
                            this.commit_formula_bar(side, tab_id, true, window, cx);
                        }
                        _ => {}
                    },
                );

                TabKind::Spreadsheet {
                    doc,
                    active_sheet: 0,
                    active_row: 1,
                    active_col: 1,
                    formula_input,
                    row_scroll: UniformListScrollHandle::new(),
                    grid_focus: cx.focus_handle(),
                    _sub: sub,
                }
            }
            FileKind::Image => {
                let meta = fs::metadata(path)?;
                TabKind::Image {
                    zoom: 1.0,
                    fit_to_view: true,
                    file_size_bytes: meta.len(),
                }
            }
            FileKind::Pdf => {
                let meta = fs::metadata(path)?;
                self.schedule_pdf_render(
                    side,
                    tab_id,
                    1,
                    path.to_path_buf(),
                    DEFAULT_PDF_DPI,
                    Some(DEFAULT_PDF_PAGE_LIMIT),
                    cx,
                );
                TabKind::Pdf {
                    zoom: 1.0,
                    pages: Vec::new(),
                    total_pages: 0,
                    rendered_limit: Some(DEFAULT_PDF_PAGE_LIMIT),
                    render_ms: None,
                    last_error: None,
                    rendering: true,
                    render_gen: 1,
                    file_size_bytes: meta.len(),
                    modified: meta.modified().ok(),
                }
            }
            FileKind::Code(lang) => {
                let raw = fs::read_to_string(path)?;
                let editor = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .language(lang)
                        .soft_wrap(false)
                        .line_number(true)
                        .default_value(raw)
                });
                let sub = cx.subscribe_in(&editor, window, move |this, _ed, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.on_code_changed(side, tab_id, window, cx);
                    }
                });
                TabKind::Code {
                    editor,
                    language: lang,
                    badge: file_kind.short_badge(),
                    soft_wrap: false,
                    _sub: sub,
                }
            }
            FileKind::PlainText => {
                let raw = fs::read_to_string(path)?;
                let editor = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .language("text")
                        .soft_wrap(true)
                        .line_number(true)
                        .default_value(raw)
                });
                let sub = cx.subscribe_in(&editor, window, move |this, _ed, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.on_code_changed(side, tab_id, window, cx);
                    }
                });
                TabKind::Code {
                    editor,
                    language: "text",
                    badge: "TXT",
                    soft_wrap: true,
                    _sub: sub,
                }
            }
        };

        Ok(DocumentTab {
            id: tab_id,
            path: path.to_path_buf(),
            title,
            rel_path,
            dirty: false,
            last_saved: None,
            kind,
        })
    }

    fn find_tab_mut(&mut self, side: PaneSide, tab_id: u64) -> Option<&mut DocumentTab> {
        self.pane_mut(side)?
            .tabs
            .iter_mut()
            .find(|t| t.id == tab_id)
    }

    fn find_tab(&self, side: PaneSide, tab_id: u64) -> Option<&DocumentTab> {
        self.pane(side)?.tabs.iter().find(|t| t.id == tab_id)
    }

    fn update_markdown_derived_state(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        raw: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let vault = Arc::clone(&self.vault);
        let Some(tab) = self.find_tab_mut(side, tab_id) else {
            return;
        };
        tab.dirty = true;
        let path = tab.path.clone();
        if let TabKind::Markdown {
            word_count,
            char_count,
            headings,
            sync_gen,
            preprocessed,
            is_mdx,
            ..
        } = &mut tab.kind
        {
            let (wc, cc) = word_and_char_count(raw.as_ref());
            *word_count = wc;
            *char_count = cc;
            *headings = extract_headings(raw.as_ref(), false);
            *sync_gen = sync_gen.wrapping_add(1);
            let cur_gen = *sync_gen;
            let mdx = *is_mdx;

            if raw.len() < 32_768 {
                *preprocessed =
                    preprocess_markdown_with_options(raw.as_ref(), &path, &vault, 0, mdx).into();
            } else {
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(Duration::from_millis(80)).await;
                    let pre = cx
                        .background_executor()
                        .spawn(async move {
                            preprocess_markdown_with_options(raw.as_ref(), &path, &vault, 0, mdx)
                        })
                        .await;
                    let _ = this.update(cx, |app, cx| {
                        if let Some(tab) = app.find_tab_mut(side, tab_id)
                            && let TabKind::Markdown {
                                sync_gen,
                                preprocessed,
                                ..
                            } = &mut tab.kind
                            && *sync_gen == cur_gen
                        {
                            *preprocessed = pre.into();
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        }

        if self.config.autosave {
            self.schedule_autosave(side, tab_id, window, cx);
        }
        cx.notify();
    }

    pub fn commit_active_live_block(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let info = {
            let Some(tab) = self.find_tab(side, tab_id) else {
                return;
            };
            let TabKind::Markdown {
                editor,
                is_mdx,
                live_blocks,
                live_active_block,
                live_editor,
                live_syncing,
                live_dirty_block,
                ..
            } = &tab.kind
            else {
                return;
            };
            if !*live_dirty_block {
                return;
            }
            let Some(ix) = *live_active_block else {
                return;
            };
            let Some(block) = live_blocks.get(ix) else {
                return;
            };
            (
                editor.clone(),
                live_editor.clone(),
                Arc::clone(live_syncing),
                Arc::clone(live_blocks),
                *is_mdx,
                ix,
                block.clone(),
            )
        };
        let (editor, live_editor, live_syncing, old_blocks, is_mdx, ix, block) = info;

        let new_text = live_editor.read(cx).value().to_string();
        let raw_doc = editor.read(cx).value();
        let new_doc =
            splice_block_in_doc(raw_doc.as_ref(), block.byte_start, block.byte_end, &new_text);

        live_syncing.store(true, Ordering::SeqCst);
        editor.update(cx, |state, cx| {
            state.set_value(new_doc.clone(), window, cx);
        });
        live_syncing.store(false, Ordering::SeqCst);

        let new_byte_end = block.byte_start + new_text.len();
        let delta = new_byte_end as isize - block.byte_end as isize;
        let new_kind = if block.kind.is_multiline() {
            block.kind
        } else {
            classify_single_line(&new_text, is_mdx)
        };

        let mut updated_vec = (*old_blocks).clone();
        if let Some(cur) = updated_vec.get_mut(ix) {
            cur.kind = new_kind;
            cur.byte_end = new_byte_end;
            if block.kind.is_multiline() {
                let line_count = new_text.bytes().filter(|&b| b == b'\n').count();
                let old_lines = cur.end_line - cur.start_line;
                cur.end_line = cur.start_line + line_count;
                let line_delta = line_count as isize - old_lines as isize;
                cur.raw = new_text;
                for tail in updated_vec.iter_mut().skip(ix + 1) {
                    tail.byte_start = tail.byte_start.saturating_add_signed(delta);
                    tail.byte_end = tail.byte_end.saturating_add_signed(delta);
                    tail.start_line = tail.start_line.saturating_add_signed(line_delta);
                    tail.end_line = tail.end_line.saturating_add_signed(line_delta);
                }
            } else {
                cur.raw = new_text;
                for tail in updated_vec.iter_mut().skip(ix + 1) {
                    tail.byte_start = tail.byte_start.saturating_add_signed(delta);
                    tail.byte_end = tail.byte_end.saturating_add_signed(delta);
                }
            }
        }
        let new_blocks = Arc::new(updated_vec);
        if let Some(tab) = self.find_tab_mut(side, tab_id)
            && let TabKind::Markdown {
                live_blocks,
                live_dirty_block,
                ..
            } = &mut tab.kind
        {
            *live_blocks = new_blocks;
            *live_dirty_block = false;
        }
        self.update_markdown_derived_state(side, tab_id, new_doc.into(), window, cx);
    }

    fn sync_live_blocks_from_master(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        raw: &str,
        clear_active: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.find_tab_mut(side, tab_id) else {
            return;
        };
        if let TabKind::Markdown {
            is_mdx,
            live_blocks,
            live_active_block,
            live_list_state,
            live_scrollbar,
            live_editor,
            live_dirty_block,
            ..
        } = &mut tab.kind
        {
            let old_active = *live_active_block;
            if clear_active {
                *live_active_block = None;
            }
            *live_dirty_block = false;
            let new_blocks = Arc::new(parse_live_blocks(raw, *is_mdx));
            let active_focus = (*live_active_block).and_then(|ix| {
                if ix < new_blocks.len() {
                    Some((ix, live_editor.focus_handle(cx)))
                } else {
                    None
                }
            });
            if active_focus.is_none() {
                *live_active_block = None;
            }
            splice_list_diff(
                live_list_state,
                live_scrollbar,
                live_blocks,
                &new_blocks,
                old_active,
                active_focus,
            );
            *live_blocks = new_blocks;
        }
    }

    pub fn activate_live_block(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        ix: usize,
        byte_offset: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_live_block_inner(side, tab_id, ix, byte_offset, true, window, cx);
    }

    pub fn activate_live_block_no_scroll(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        ix: usize,
        byte_offset: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_live_block_inner(side, tab_id, ix, byte_offset, false, window, cx);
    }

    #[allow(clippy::too_many_arguments)]
    fn activate_live_block_inner(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        ix: usize,
        byte_offset: usize,
        reveal: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_pane = side;
        let current_active = self.find_tab(side, tab_id).and_then(|t| match &t.kind {
            TabKind::Markdown {
                live_active_block,
                live_dirty_block,
                ..
            } => Some((*live_active_block, *live_dirty_block)),
            _ => None,
        });
        if let Some((Some(old_ix), true)) = current_active
            && old_ix != ix
        {
            self.commit_active_live_block(side, tab_id, window, cx);
        }

        let extracted = {
            let Some(tab) = self.find_tab(side, tab_id) else {
                return;
            };
            let TabKind::Markdown {
                live_blocks,
                live_active_block,
                live_editor,
                live_list_state,
                live_scrollbar,
                live_syncing,
                live_row_bounds,
                ..
            } = &tab.kind
            else {
                return;
            };
            if ix >= live_blocks.len() {
                return;
            }
            let row_w = live_row_bounds
                .borrow()
                .get(&ix)
                .map(|b| f32::from(b.size.width))
                .unwrap_or(760.0);
            (
                *live_active_block,
                live_blocks[ix].kind,
                live_blocks[ix].raw.clone(),
                live_editor.clone(),
                live_list_state.clone(),
                live_scrollbar.clone(),
                Arc::clone(live_syncing),
                row_w,
            )
        };
        let (
            old_active,
            block_kind,
            block_raw,
            live_editor,
            live_list_state,
            live_scrollbar,
            live_syncing,
            row_w,
        ) = extracted;

        let font = {
            let mut f = window.text_style().font();
            if !matches!(
                block_kind,
                LiveBlockKind::CodeFence
                    | LiveBlockKind::Frontmatter
                    | LiveBlockKind::Table
                    | LiveBlockKind::MdxBlock
            ) {
                f.family = cx.theme().font_family.clone();
            }
            f
        };
        let font_size = live_block_font_size(block_kind);

        live_syncing.store(true, Ordering::SeqCst);
        let pos = clamp_to_char_boundary(&block_raw, byte_offset);
        live_editor.update(cx, |state, cx| {
            state.set_value(block_raw, window, cx);
            state.set_selected_range(pos..pos, cx);
            state.prewrap(font, font_size, px((row_w - 10.0).max(120.0)), cx);
            state.focus(window, cx);
        });
        live_syncing.store(false, Ordering::SeqCst);

        let wrap_rows = live_editor.read(cx).display_map().display_row_count().max(1);
        let fh = live_editor.focus_handle(cx);

        let saved_scroll = live_list_state.logical_scroll_top();
        if let Some(old_ix) = old_active
            && old_ix != ix
        {
            live_list_state.splice_focusable(old_ix..old_ix + 1, [None]);
        }
        live_list_state.splice_focusable(ix..ix + 1, [Some(fh)]);
        if saved_scroll.offset_in_item > px(0.)
            && (old_active == Some(saved_scroll.item_ix) || ix == saved_scroll.item_ix)
        {
            live_list_state.scroll_to(saved_scroll);
        }
        if reveal {
            live_scrollbar.reveal_item(ix);
        }

        if let Some(tab) = self.find_tab_mut(side, tab_id)
            && let TabKind::Markdown {
                live_active_block,
                live_last_wrap_rows,
                live_dirty_block,
                ..
            } = &mut tab.kind
        {
            *live_active_block = Some(ix);
            *live_last_wrap_rows = wrap_rows;
            *live_dirty_block = false;
        }
        cx.notify();
    }

    pub fn activate_live_block_from_click(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        ix: usize,
        click_pos: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let byte_offset = {
            let Some(tab) = self.find_tab(side, tab_id) else {
                return;
            };
            let TabKind::Markdown {
                live_blocks,
                live_row_bounds,
                ..
            } = &tab.kind
            else {
                return;
            };
            let Some(block) = live_blocks.get(ix) else {
                return;
            };
            let (col_left, col_top, row_width) = live_row_bounds
                .borrow()
                .get(&ix)
                .map(|b| {
                    (
                        f32::from(b.origin.x),
                        f32::from(b.origin.y),
                        f32::from(b.size.width),
                    )
                })
                .unwrap_or((f32::from(click_pos.x), f32::from(click_pos.y), 760.0));
            let (avg_char_px, line_height_px) = match block.kind {
                LiveBlockKind::Heading(1) => (14.5, 27.0 * 1.25),
                LiveBlockKind::Heading(2) => (11.2, 20.0 * 1.35),
                LiveBlockKind::Heading(3) => (9.2, 17.0 * 1.35),
                LiveBlockKind::Heading(_) => (8.2, 15.0 * 1.35),
                LiveBlockKind::CodeFence
                | LiveBlockKind::Frontmatter
                | LiveBlockKind::Table
                | LiveBlockKind::MdxBlock => (8.1, 13.5 * 1.5),
                _ => (7.8, 15.0 * 1.65),
            };
            let rel_x = f32::from(click_pos.x) - col_left;
            let rel_y = f32::from(click_pos.y) - col_top;
            click_pos_to_raw_byte(block, rel_x, rel_y, row_width, avg_char_px, line_height_px)
        };
        self.activate_live_block_no_scroll(side, tab_id, ix, byte_offset, window, cx);
    }

    pub fn activate_or_append_bottom_line(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_active_live_block(side, tab_id, window, cx);
        let state_info = {
            let Some(tab) = self.find_tab(side, tab_id) else {
                return;
            };
            let TabKind::Markdown {
                editor,
                live_blocks,
                live_syncing,
                ..
            } = &tab.kind
            else {
                return;
            };
            (
                editor.clone(),
                Arc::clone(live_blocks),
                Arc::clone(live_syncing),
            )
        };
        let (editor, old_blocks, live_syncing) = state_info;
        if let Some(last) = old_blocks.last()
            && last.kind == LiveBlockKind::Blank
        {
            self.activate_live_block_no_scroll(side, tab_id, old_blocks.len() - 1, 0, window, cx);
            return;
        }

        let mut new_doc = editor.read(cx).value().to_string();
        new_doc.push('\n');
        live_syncing.store(true, Ordering::SeqCst);
        editor.update(cx, |state, cx| {
            state.set_value(new_doc.clone(), window, cx);
        });
        live_syncing.store(false, Ordering::SeqCst);

        self.sync_live_blocks_from_master(side, tab_id, &new_doc, false, cx);
        self.update_markdown_derived_state(side, tab_id, new_doc.into(), window, cx);

        let new_ix = self
            .find_tab(side, tab_id)
            .and_then(|t| match &t.kind {
                TabKind::Markdown { live_blocks, .. } => Some(live_blocks.len().saturating_sub(1)),
                _ => None,
            })
            .unwrap_or(0);
        self.activate_live_block(side, tab_id, new_ix, 0, window, cx);
    }

    pub fn deactivate_live_block(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_active_live_block(side, tab_id, window, cx);
        if let Some(tab) = self.find_tab_mut(side, tab_id)
            && let TabKind::Markdown {
                live_active_block,
                live_list_state,
                ..
            } = &mut tab.kind
            && let Some(old_ix) = live_active_block.take()
        {
            live_list_state.splice_focusable(old_ix..old_ix + 1, [None]);
            self.focus_handle.focus(window, cx);
            cx.notify();
        }
    }

    fn on_live_editor_changed(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let snapshot = {
            let Some(tab) = self.find_tab(side, tab_id) else {
                return;
            };
            let TabKind::Markdown {
                editor,
                is_mdx,
                live_blocks,
                live_active_block,
                live_editor,
                live_syncing,
                ..
            } = &tab.kind
            else {
                return;
            };
            if live_syncing.load(Ordering::SeqCst) {
                return;
            }
            let Some(ix) = *live_active_block else {
                return;
            };
            let Some(block) = live_blocks.get(ix) else {
                return;
            };
            (
                editor.clone(),
                live_editor.clone(),
                Arc::clone(live_syncing),
                *is_mdx,
                ix,
                block.clone(),
            )
        };
        let (editor, live_editor, live_syncing, is_mdx, ix, block) = snapshot;

        let new_text = live_editor.read(cx).value().to_string();
        if !block.kind.is_multiline() && new_text.contains('\n') {
            // Multi-line paste into a single-line block: commit structurally and land at pasted cursor.
            let pasted_cursor = live_editor.read(cx).cursor();
            let raw_doc = editor.read(cx).value();
            let new_doc = splice_block_in_doc(
                raw_doc.as_ref(),
                block.byte_start,
                block.byte_end,
                &new_text,
            );
            live_syncing.store(true, Ordering::SeqCst);
            editor.update(cx, |state, cx| {
                state.set_value(new_doc.clone(), window, cx);
            });
            live_syncing.store(false, Ordering::SeqCst);

            let new_blocks = Arc::new(parse_live_blocks(&new_doc, is_mdx));
            let abs_cursor = block.byte_start + pasted_cursor;
            let (target_line, target_col) = byte_offset_to_line_col(&new_doc, abs_cursor);
            let target_ix = block_index_for_line(&new_blocks, target_line);
            let target_line_in_block = target_line.saturating_sub(new_blocks[target_ix].start_line);
            let target_offset =
                line_col_to_byte_offset(&new_blocks[target_ix].raw, target_line_in_block, target_col);

            if let Some(tab) = self.find_tab_mut(side, tab_id)
                && let TabKind::Markdown {
                    live_blocks,
                    live_active_block,
                    live_list_state,
                    live_scrollbar,
                    live_dirty_block,
                    ..
                } = &mut tab.kind
            {
                let old_active = *live_active_block;
                *live_active_block = None;
                *live_dirty_block = false;
                let fh = live_editor.focus_handle(cx);
                splice_list_diff(
                    live_list_state,
                    live_scrollbar,
                    live_blocks,
                    &new_blocks,
                    old_active,
                    Some((target_ix, fh)),
                );
                *live_blocks = new_blocks;
            }
            self.update_markdown_derived_state(side, tab_id, new_doc.into(), window, cx);
            self.activate_live_block(side, tab_id, target_ix, target_offset, window, cx);
            return;
        }

        // Fast O(1) keystroke path: do not re-sync whole file on every character.
        let new_kind = if block.kind.is_multiline() {
            block.kind
        } else {
            classify_single_line(&new_text, is_mdx)
        };
        let kind_changed = new_kind != block.kind;
        let wrap_rows = live_editor.read(cx).display_map().display_row_count().max(1);
        let fh = live_editor.focus_handle(cx);

        let gen_num = if let Some(tab) = self.find_tab_mut(side, tab_id) {
            tab.dirty = true;
            if let TabKind::Markdown {
                live_blocks,
                live_list_state,
                live_last_wrap_rows,
                live_dirty_block,
                live_edit_gen,
                ..
            } = &mut tab.kind
            {
                *live_dirty_block = true;
                *live_edit_gen = live_edit_gen.wrapping_add(1);
                if kind_changed {
                    Arc::make_mut(live_blocks)[ix].kind = new_kind;
                }
                if wrap_rows != *live_last_wrap_rows
                    || kind_changed
                    || new_kind == LiveBlockKind::ImageEmbed
                {
                    *live_last_wrap_rows = wrap_rows;
                    live_list_state.splice_focusable(ix..ix + 1, [Some(fh)]);
                }
                *live_edit_gen
            } else {
                0
            }
        } else {
            return;
        };

        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let _ = this.update_in(cx, |app, window, cx| {
                let should_commit = app.find_tab(side, tab_id).is_some_and(|t| {
                    matches!(
                        &t.kind,
                        TabKind::Markdown {
                            live_edit_gen,
                            live_dirty_block,
                            ..
                        } if *live_edit_gen == gen_num && *live_dirty_block
                    )
                });
                if should_commit {
                    app.commit_active_live_block(side, tab_id, window, cx);
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn live_cursor_geometry(
        &self,
        side: PaneSide,
        tab_id: u64,
        cx: &App,
    ) -> Option<(String, std::ops::Range<usize>, usize, usize, usize)> {
        let tab = self.find_tab(side, tab_id)?;
        let TabKind::Markdown { live_editor, .. } = &tab.kind else {
            return None;
        };
        let state = live_editor.read(cx);
        let val = state.value().to_string();
        let sel = state.selected_range();
        let (line, col) = byte_offset_to_line_col(&val, sel.start);
        let display_row = state
            .display_map()
            .buffer_pos_to_display_pos(BufferPoint::new(line, col))
            .row;
        let total_rows = state.display_map().display_row_count().max(1);
        Some((val, sel, display_row, total_rows, col))
    }

    pub fn handle_live_move_up(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_val, _sel, display_row, _total_rows, cursor_col)) =
            self.live_cursor_geometry(side, tab_id, cx)
        else {
            return;
        };
        if display_row == 0 && ix > 0 {
            cx.stop_propagation();
            self.commit_active_live_block(side, tab_id, window, cx);
            let target_offset = self
                .find_tab(side, tab_id)
                .and_then(|t| match &t.kind {
                    TabKind::Markdown { live_blocks, .. } => live_blocks.get(ix - 1).map(|prev| {
                        let last_line = prev.raw.bytes().filter(|&b| b == b'\n').count();
                        line_col_to_byte_offset(&prev.raw, last_line, cursor_col)
                    }),
                    _ => None,
                })
                .unwrap_or(0);
            self.activate_live_block(side, tab_id, ix - 1, target_offset, window, cx);
        }
    }

    pub fn handle_live_move_down(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_val, _sel, display_row, total_rows, cursor_col)) =
            self.live_cursor_geometry(side, tab_id, cx)
        else {
            return;
        };
        let total_blocks = self
            .find_tab(side, tab_id)
            .and_then(|t| match &t.kind {
                TabKind::Markdown { live_blocks, .. } => Some(live_blocks.len()),
                _ => None,
            })
            .unwrap_or(0);
        if display_row + 1 >= total_rows && ix + 1 < total_blocks {
            cx.stop_propagation();
            self.commit_active_live_block(side, tab_id, window, cx);
            let target_offset = self
                .find_tab(side, tab_id)
                .and_then(|t| match &t.kind {
                    TabKind::Markdown { live_blocks, .. } => live_blocks
                        .get(ix + 1)
                        .map(|next| line_col_to_byte_offset(&next.raw, 0, cursor_col)),
                    _ => None,
                })
                .unwrap_or(0);
            self.activate_live_block(side, tab_id, ix + 1, target_offset, window, cx);
        }
    }

    pub fn handle_live_move_left(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_val, sel, _display_row, _total_rows, _col)) =
            self.live_cursor_geometry(side, tab_id, cx)
        else {
            return;
        };
        if sel.start == 0 && sel.end == 0 && ix > 0 {
            cx.stop_propagation();
            self.commit_active_live_block(side, tab_id, window, cx);
            let end_len = self
                .find_tab(side, tab_id)
                .and_then(|t| match &t.kind {
                    TabKind::Markdown { live_blocks, .. } => {
                        live_blocks.get(ix - 1).map(|prev| prev.raw.len())
                    }
                    _ => None,
                })
                .unwrap_or(0);
            self.activate_live_block(side, tab_id, ix - 1, end_len, window, cx);
        }
    }

    pub fn handle_live_move_right(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((cur_val, sel, _display_row, _total_rows, _col)) =
            self.live_cursor_geometry(side, tab_id, cx)
        else {
            return;
        };
        let total_blocks = self
            .find_tab(side, tab_id)
            .and_then(|t| match &t.kind {
                TabKind::Markdown { live_blocks, .. } => Some(live_blocks.len()),
                _ => None,
            })
            .unwrap_or(0);
        if sel.start == cur_val.len() && sel.end == cur_val.len() && ix + 1 < total_blocks {
            cx.stop_propagation();
            self.commit_active_live_block(side, tab_id, window, cx);
            self.activate_live_block(side, tab_id, ix + 1, 0, window, cx);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn handle_live_enter(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        ix: usize,
        secondary: bool,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_active_live_block(side, tab_id, window, cx);
        let snapshot = {
            let Some(tab) = self.find_tab(side, tab_id) else {
                return;
            };
            let TabKind::Markdown {
                editor,
                is_mdx,
                live_blocks,
                live_editor,
                live_scrollbar,
                live_syncing,
                ..
            } = &tab.kind
            else {
                return;
            };
            let Some(block) = live_blocks.get(ix).cloned() else {
                return;
            };
            (
                editor.clone(),
                live_editor.clone(),
                live_scrollbar.clone(),
                Arc::clone(live_syncing),
                live_blocks.len(),
                *is_mdx,
                block,
            )
        };
        let (editor, live_editor, live_scrollbar, live_syncing, blocks_len, is_mdx, cur_block) =
            snapshot;

        if cur_block.kind.is_multiline() {
            if secondary {
                cx.stop_propagation();
                if ix + 1 < blocks_len {
                    self.activate_live_block(side, tab_id, ix + 1, 0, window, cx);
                } else {
                    self.activate_or_append_bottom_line(side, tab_id, window, cx);
                }
            }
            return;
        }

        if shift || secondary {
            return;
        }

        cx.stop_propagation();
        let (cur_val, sel) = {
            let st = live_editor.read(cx);
            (st.value().to_string(), st.selected_range())
        };
        match compute_smart_enter(&cur_val, sel.start, sel.end) {
            LiveEnterAction::ClearLine => {
                live_editor.update(cx, |s, cx| {
                    s.set_value(String::new(), window, cx);
                    s.set_selected_range(0..0, cx);
                });
            }
            LiveEnterAction::ExpandCodeFence {
                text,
                cursor_offset,
            } => {
                let raw_doc = editor.read(cx).value();
                let new_doc = splice_block_in_doc(
                    raw_doc.as_ref(),
                    cur_block.byte_start,
                    cur_block.byte_end,
                    &text,
                );
                live_syncing.store(true, Ordering::SeqCst);
                editor.update(cx, |s, cx| s.set_value(new_doc.clone(), window, cx));
                live_editor.update(cx, |s, cx| {
                    s.set_value(text, window, cx);
                    let pos = clamp_to_char_boundary(s.value().as_ref(), cursor_offset);
                    s.set_selected_range(pos..pos, cx);
                    s.focus(window, cx);
                });
                live_syncing.store(false, Ordering::SeqCst);

                self.update_markdown_derived_state(side, tab_id, new_doc.clone().into(), window, cx);
                self.sync_live_blocks_from_master(side, tab_id, &new_doc, false, cx);
                live_scrollbar.reveal_item(ix);
            }
            LiveEnterAction::SplitLine {
                left,
                right,
                right_cursor,
            } => {
                let raw_doc = editor.read(cx).value();
                let new_doc = splice_block_in_doc(
                    raw_doc.as_ref(),
                    cur_block.byte_start,
                    cur_block.byte_end,
                    &format!("{left}\n{right}"),
                );
                live_syncing.store(true, Ordering::SeqCst);
                editor.update(cx, |s, cx| s.set_value(new_doc.clone(), window, cx));
                live_syncing.store(false, Ordering::SeqCst);

                self.update_markdown_derived_state(side, tab_id, new_doc.clone().into(), window, cx);
                let new_blocks = Arc::new(parse_live_blocks(&new_doc, is_mdx));
                let target_ix = (ix + 1).min(new_blocks.len().saturating_sub(1));
                if let Some(tab) = self.find_tab_mut(side, tab_id)
                    && let TabKind::Markdown {
                        live_blocks,
                        live_active_block,
                        live_list_state,
                        live_scrollbar,
                        ..
                    } = &mut tab.kind
                {
                    let old_active = *live_active_block;
                    *live_active_block = None;
                    let fh = live_editor.focus_handle(cx);
                    splice_list_diff(
                        live_list_state,
                        live_scrollbar,
                        live_blocks,
                        &new_blocks,
                        old_active,
                        Some((target_ix, fh)),
                    );
                    *live_blocks = new_blocks;
                }
                self.activate_live_block(side, tab_id, target_ix, right_cursor, window, cx);
            }
        }
    }

    pub fn handle_live_backspace(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_cur_val, sel, _, _, _)) = self.live_cursor_geometry(side, tab_id, cx) else {
            return;
        };
        if sel.start != 0 || sel.end != 0 || ix == 0 {
            return;
        }
        cx.stop_propagation();
        self.commit_active_live_block(side, tab_id, window, cx);

        let snapshot = {
            let Some(tab) = self.find_tab(side, tab_id) else {
                return;
            };
            let TabKind::Markdown {
                editor,
                live_blocks,
                live_syncing,
                ..
            } = &tab.kind
            else {
                return;
            };
            let (Some(prev), Some(cur_block)) = (live_blocks.get(ix - 1), live_blocks.get(ix)) else {
                return;
            };
            (
                editor.clone(),
                Arc::clone(live_syncing),
                prev.clone(),
                cur_block.clone(),
            )
        };
        let (editor, live_syncing, prev, cur_block) = snapshot;

        if prev.kind == LiveBlockKind::Blank {
            let raw_doc = editor.read(cx).value();
            let new_doc =
                splice_block_in_doc(raw_doc.as_ref(), prev.byte_start, cur_block.byte_start, "");
            live_syncing.store(true, Ordering::SeqCst);
            editor.update(cx, |s, cx| s.set_value(new_doc.clone(), window, cx));
            live_syncing.store(false, Ordering::SeqCst);

            self.update_markdown_derived_state(side, tab_id, new_doc.clone().into(), window, cx);
            self.sync_live_blocks_from_master(side, tab_id, &new_doc, false, cx);
            self.activate_live_block(side, tab_id, ix - 1, 0, window, cx);
        } else if !prev.kind.is_multiline() && !cur_block.kind.is_multiline() {
            let merge_cursor = prev.raw.len();
            let merged = format!("{}{}", prev.raw, cur_block.raw);
            let raw_doc = editor.read(cx).value();
            let new_doc = splice_block_in_doc(
                raw_doc.as_ref(),
                prev.byte_start,
                cur_block.byte_end,
                &merged,
            );
            live_syncing.store(true, Ordering::SeqCst);
            editor.update(cx, |s, cx| s.set_value(new_doc.clone(), window, cx));
            live_syncing.store(false, Ordering::SeqCst);

            self.update_markdown_derived_state(side, tab_id, new_doc.clone().into(), window, cx);
            self.sync_live_blocks_from_master(side, tab_id, &new_doc, false, cx);
            self.activate_live_block(side, tab_id, ix - 1, merge_cursor, window, cx);
        } else if cur_block.kind == LiveBlockKind::Blank {
            let raw_doc = editor.read(cx).value();
            let new_doc =
                splice_block_in_doc(raw_doc.as_ref(), prev.byte_end, cur_block.byte_end, "");
            live_syncing.store(true, Ordering::SeqCst);
            editor.update(cx, |s, cx| s.set_value(new_doc.clone(), window, cx));
            live_syncing.store(false, Ordering::SeqCst);

            self.update_markdown_derived_state(side, tab_id, new_doc.clone().into(), window, cx);
            self.sync_live_blocks_from_master(side, tab_id, &new_doc, false, cx);
            self.activate_live_block(side, tab_id, ix - 1, prev.raw.len(), window, cx);
        } else {
            self.activate_live_block(side, tab_id, ix - 1, prev.raw.len(), window, cx);
        }
    }

    pub fn handle_live_delete(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((cur_val, sel, _, _, _)) = self.live_cursor_geometry(side, tab_id, cx) else {
            return;
        };
        if sel.start != cur_val.len() || sel.end != cur_val.len() {
            return;
        }
        self.commit_active_live_block(side, tab_id, window, cx);
        let snapshot = {
            let Some(tab) = self.find_tab(side, tab_id) else {
                return;
            };
            let TabKind::Markdown {
                editor,
                live_blocks,
                live_syncing,
                ..
            } = &tab.kind
            else {
                return;
            };
            let (Some(cur_block), Some(next)) = (live_blocks.get(ix), live_blocks.get(ix + 1)) else {
                return;
            };
            if cur_block.kind.is_multiline() || next.kind.is_multiline() {
                return;
            }
            (
                editor.clone(),
                Arc::clone(live_syncing),
                cur_block.clone(),
                next.clone(),
            )
        };
        let (editor, live_syncing, cur_block, next) = snapshot;

        cx.stop_propagation();
        let keep_cursor = cur_block.raw.len();
        let merged = format!("{}{}", cur_block.raw, next.raw);
        let raw_doc = editor.read(cx).value();
        let new_doc =
            splice_block_in_doc(raw_doc.as_ref(), cur_block.byte_start, next.byte_end, &merged);
        live_syncing.store(true, Ordering::SeqCst);
        editor.update(cx, |s, cx| s.set_value(new_doc.clone(), window, cx));
        live_syncing.store(false, Ordering::SeqCst);

        self.update_markdown_derived_state(side, tab_id, new_doc.clone().into(), window, cx);
        self.sync_live_blocks_from_master(side, tab_id, &new_doc, false, cx);
        self.activate_live_block(side, tab_id, ix, keep_cursor, window, cx);
    }

    pub fn handle_live_escape(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.deactivate_live_block(side, tab_id, window, cx);
    }

    pub fn handle_live_paste_action(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let has_image = item
            .entries()
            .iter()
            .any(|e| matches!(e, ClipboardEntry::Image(_)));
        if has_image {
            cx.stop_propagation();
            self.handle_live_image_paste(side, tab_id, &item, window, cx);
        }
    }

    pub fn handle_live_task_toggle(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        ix: usize,
        checked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_active_live_block(side, tab_id, window, cx);
        let info = {
            let Some(tab) = self.find_tab(side, tab_id) else {
                return;
            };
            let TabKind::Markdown {
                editor,
                live_blocks,
                live_syncing,
                ..
            } = &tab.kind
            else {
                return;
            };
            let Some(block) = live_blocks.get(ix) else {
                return;
            };
            let Some(updated_line) = toggle_task_in_line(&block.raw, checked) else {
                return;
            };
            (
                editor.clone(),
                Arc::clone(live_syncing),
                Arc::clone(live_blocks),
                block.byte_start,
                block.byte_end,
                updated_line,
            )
        };
        let (editor, live_syncing, old_blocks, byte_start, byte_end, updated_line) = info;

        let raw_doc = editor.read(cx).value();
        let new_doc = splice_block_in_doc(raw_doc.as_ref(), byte_start, byte_end, &updated_line);

        live_syncing.store(true, Ordering::SeqCst);
        editor.update(cx, |s, cx| s.set_value(new_doc.clone(), window, cx));
        live_syncing.store(false, Ordering::SeqCst);

        let mut updated_vec = (*old_blocks).clone();
        if let Some(cur) = updated_vec.get_mut(ix) {
            cur.raw = updated_line;
        }
        let new_blocks = Arc::new(updated_vec);
        if let Some(tab) = self.find_tab_mut(side, tab_id)
            && let TabKind::Markdown {
                live_blocks,
                live_list_state,
                ..
            } = &mut tab.kind
        {
            *live_blocks = new_blocks;
            live_list_state.splice_focusable(ix..ix + 1, [None]);
        }
        self.update_markdown_derived_state(side, tab_id, new_doc.into(), window, cx);
    }

    fn on_markdown_changed(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state_info = {
            let Some(tab) = self.find_tab(side, tab_id) else {
                return;
            };
            let TabKind::Markdown {
                editor,
                live_syncing,
                ..
            } = &tab.kind
            else {
                return;
            };
            if live_syncing.load(Ordering::SeqCst) {
                return;
            }
            editor.clone()
        };
        let raw = state_info.read(cx).value();
        self.sync_live_blocks_from_master(side, tab_id, raw.as_ref(), true, cx);
        self.update_markdown_derived_state(side, tab_id, raw, window, cx);
    }

    fn on_fountain_changed(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.find_tab_mut(side, tab_id) else {
            return;
        };
        tab.dirty = true;
        if let TabKind::Fountain {
            editor,
            parsed,
            headings,
            ..
        } = &mut tab.kind
        {
            let raw = editor.read(cx).value();
            let doc = parse_fountain(raw.as_ref());
            *headings = extract_fountain_headings(&doc);
            *parsed = doc;
        }
        if self.config.autosave {
            self.schedule_autosave(side, tab_id, window, cx);
        }
        cx.notify();
    }

    pub fn export_fountain_current_pdf(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.find_tab(side, tab_id) else {
            return;
        };
        let TabKind::Fountain { editor, .. } = &tab.kind else {
            return;
        };
        let raw = editor.read(cx).value();
        let doc = parse_fountain(raw.as_ref());
        let typst_source = fountain_to_typst(&doc);
        let doc_path = tab.path.clone();
        let out_pdf = doc_path.with_extension("pdf");
        let ws_root = self.workspace_root.clone();
        self.set_status("Exporting screenplay PDF…", cx);

        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move {
                    export_typst_pdf(&typst_source, &doc_path, &ws_root, &out_pdf)
                })
                .await;
            let _ = this.update(cx, |app, cx| match res {
                Ok(saved) => {
                    app.set_status(format!("Exported screenplay PDF: {}", saved.display()), cx)
                }
                Err(err) => app.set_status(format!("Screenplay PDF export failed: {err}"), cx),
            });
        })
        .detach();
    }

    fn on_typst_changed(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.find_tab_mut(side, tab_id) else {
            return;
        };
        tab.dirty = true;
        let path = tab.path.clone();
        let next_compile = if let TabKind::Typst {
            editor,
            compiling,
            compile_gen,
            headings,
            ..
        } = &mut tab.kind
        {
            let raw = editor.read(cx).value();
            *headings = extract_headings(raw.as_ref(), true);
            *compiling = true;
            *compile_gen = compile_gen.wrapping_add(1);
            Some((*compile_gen, raw.to_string()))
        } else {
            None
        };

        if let Some((gen_num, source)) = next_compile {
            self.schedule_typst_compile(side, tab_id, gen_num, source, path, 144, true, cx);
        }
        if self.config.autosave {
            self.schedule_autosave(side, tab_id, window, cx);
        }
        cx.notify();
    }

    fn schedule_typst_compile(
        &self,
        side: PaneSide,
        tab_id: u64,
        gen_num: u64,
        source: String,
        doc_path: PathBuf,
        ppi: u32,
        debounce: bool,
        cx: &mut Context<Self>,
    ) {
        let ws_root = self.workspace_root.clone();
        cx.spawn(async move |this, cx| {
            if debounce {
                cx.background_executor()
                    .timer(empty_typst_debounce())
                    .await;
                let still_current = this
                    .update(cx, |app, _| {
                        app.find_tab(side, tab_id)
                            .is_some_and(|t| matches!(&t.kind, TabKind::Typst { compile_gen, .. } if *compile_gen == gen_num))
                    })
                    .unwrap_or(false);
                if !still_current {
                    return;
                }
            }

            let compiled = cx
                .background_executor()
                .spawn(async move { compile_typst_png(&source, &doc_path, &ws_root, ppi) })
                .await;

            let _ = this.update(cx, |app, cx| {
                let Some(tab) = app.find_tab_mut(side, tab_id) else {
                    return;
                };
                if let TabKind::Typst {
                    pages,
                    compile_ms,
                    last_error,
                    compiling,
                    compile_gen,
                    ..
                } = &mut tab.kind
                {
                    if *compile_gen != gen_num {
                        return;
                    }
                    *compiling = false;
                    *compile_ms = Some(compiled.duration_ms);
                    *last_error = compiled.error;
                    // Preserve last valid pages on syntax error while editing!
                    if !compiled.pages.is_empty() || last_error.is_none() {
                        *pages = compiled.pages;
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    #[allow(clippy::too_many_arguments)]
    fn schedule_pdf_render(
        &self,
        side: PaneSide,
        tab_id: u64,
        gen_num: u64,
        path: PathBuf,
        dpi: u32,
        max_pages: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let ws_gen = self.workspace_gen;
        cx.spawn(async move |this, cx| {
            let rendered = cx
                .background_executor()
                .spawn(async move {
                    let meta = fs::metadata(&path).ok();
                    let out = render_pdf_pages(&path, dpi, max_pages);
                    (
                        out,
                        meta.as_ref().map(|m| m.len()),
                        meta.and_then(|m| m.modified().ok()),
                    )
                })
                .await;

            let _ = this.update(cx, |app, cx| {
                if app.workspace_gen != ws_gen {
                    return;
                }
                let Some(tab) = app.find_tab_mut(side, tab_id) else {
                    return;
                };
                if let TabKind::Pdf {
                    pages,
                    total_pages,
                    render_ms,
                    last_error,
                    rendering,
                    render_gen,
                    file_size_bytes,
                    modified,
                    ..
                } = &mut tab.kind
                {
                    if *render_gen != gen_num {
                        return;
                    }
                    let (out, new_sz, new_mod) = rendered;
                    *rendering = false;
                    *render_ms = Some(out.duration_ms);
                    *last_error = out.error;
                    if out.total_pages > 0 || last_error.is_none() {
                        *total_pages = out.total_pages;
                    }
                    if !out.pages.is_empty() || last_error.is_none() {
                        *pages = out.pages;
                    }
                    if let Some(sz) = new_sz {
                        *file_size_bytes = sz;
                    }
                    if new_mod.is_some() {
                        *modified = new_mod;
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn reload_pdf_tab(&mut self, side: PaneSide, tab_id: u64, cx: &mut Context<Self>) {
        let params = self.find_tab_mut(side, tab_id).and_then(|tab| {
            if let TabKind::Pdf {
                rendered_limit,
                rendering,
                render_gen,
                ..
            } = &mut tab.kind
            {
                *render_gen = render_gen.wrapping_add(1);
                *rendering = true;
                Some((*render_gen, tab.path.clone(), *rendered_limit))
            } else {
                None
            }
        });
        if let Some((gen_num, path, limit)) = params {
            self.schedule_pdf_render(side, tab_id, gen_num, path, DEFAULT_PDF_DPI, limit, cx);
            cx.notify();
        }
    }

    pub fn load_all_pdf_pages(&mut self, side: PaneSide, tab_id: u64, cx: &mut Context<Self>) {
        let params = self.find_tab_mut(side, tab_id).and_then(|tab| {
            if let TabKind::Pdf {
                rendered_limit,
                rendering,
                render_gen,
                ..
            } = &mut tab.kind
            {
                *rendered_limit = None;
                *render_gen = render_gen.wrapping_add(1);
                *rendering = true;
                Some((*render_gen, tab.path.clone()))
            } else {
                None
            }
        });
        if let Some((gen_num, path)) = params {
            self.schedule_pdf_render(side, tab_id, gen_num, path, DEFAULT_PDF_DPI, None, cx);
            cx.notify();
        }
    }

    pub fn reload_modified_pdf_tabs(&mut self, cx: &mut Context<Self>) {
        let mut to_reload = Vec::new();
        for (side, pane_opt) in [
            (PaneSide::Left, Some(&self.left_pane)),
            (PaneSide::Right, self.right_pane.as_ref()),
        ] {
            let Some(pane) = pane_opt else {
                continue;
            };
            for tab in &pane.tabs {
                if let TabKind::Pdf {
                    modified, rendering, ..
                } = &tab.kind
                    && !*rendering
                    && let Ok(meta) = fs::metadata(&tab.path)
                    && let Ok(disk_mod) = meta.modified()
                    && modified.as_ref() != Some(&disk_mod)
                {
                    to_reload.push((side, tab.id));
                }
            }
        }
        for (side, tab_id) in to_reload {
            self.reload_pdf_tab(side, tab_id, cx);
        }
    }

    fn on_code_changed(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.find_tab_mut(side, tab_id) {
            tab.dirty = true;
        }
        if self.config.autosave {
            self.schedule_autosave(side, tab_id, window, cx);
        }
        cx.notify();
    }

    fn schedule_autosave(
        &self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.find_tab(side, tab_id) else {
            return;
        };
        let prev_gen = match &tab.kind {
            TabKind::Markdown { sync_gen, .. } => *sync_gen,
            TabKind::Typst { compile_gen, .. } => *compile_gen,
            _ => 0,
        };
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1200))
                .await;
            let _ = this.update_in(cx, |app, window, cx| {
                if !app.config.autosave {
                    return;
                }
                let matches_gen = app.find_tab(side, tab_id).is_some_and(|t| match &t.kind {
                    TabKind::Markdown { sync_gen, .. } => *sync_gen == prev_gen,
                    TabKind::Typst { compile_gen, .. } => *compile_gen == prev_gen,
                    _ => true,
                });
                if matches_gen {
                    app.save_tab(side, tab_id, window, cx);
                }
            });
        })
        .detach();
    }

    pub fn save_tab(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_active_live_block(side, tab_id, window, cx);
        let Some(idx) = self
            .pane(side)
            .and_then(|p| p.tabs.iter().position(|t| t.id == tab_id))
        else {
            return;
        };

        let save_res = {
            let Some(tab) = self.pane_mut(side).and_then(|p| p.tabs.get_mut(idx)) else {
                return;
            };
            let path = tab.path.clone();
            match &mut tab.kind {
                TabKind::Markdown { editor, .. }
                | TabKind::Fountain { editor, .. }
                | TabKind::Typst { editor, .. }
                | TabKind::Code { editor, .. } => {
                    let content = editor.read(cx).value();
                    atomic_write_string(&path, content.as_ref())
                }
                TabKind::Spreadsheet { doc, .. } => doc.save(&path),
                TabKind::Image { .. } | TabKind::Pdf { .. } => Ok(()),
            }
        };

        match save_res {
            Ok(()) => {
                let mut saved_title = None;
                if let Some(tab) = self.pane_mut(side).and_then(|p| p.tabs.get_mut(idx)) {
                    tab.dirty = false;
                    tab.last_saved = Some(chrono::Local::now().format("%H:%M:%S").to_string());
                    saved_title = Some(tab.title.clone());
                }
                if let Some(title) = saved_title {
                    self.set_status(format!("Saved {title}"), cx);
                }
                self.refresh_vault_index(cx);
            }
            Err(err) => {
                self.toast_err(format!("Save failed: {err}"), window, cx);
            }
        }
        cx.notify();
    }

    pub fn save_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_ref() {
            let id = tab.id;
            self.save_tab(self.active_pane, id, window, cx);
        }
    }

    pub fn save_all_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let left_ids: Vec<u64> = self
            .left_pane
            .tabs
            .iter()
            .filter(|t| t.dirty)
            .map(|t| t.id)
            .collect();
        for id in left_ids {
            self.save_tab(PaneSide::Left, id, window, cx);
        }
        let right_ids: Vec<u64> = self
            .right_pane
            .as_ref()
            .map(|p| p.tabs.iter().filter(|t| t.dirty).map(|t| t.id).collect())
            .unwrap_or_default();
        for id in right_ids {
            self.save_tab(PaneSide::Right, id, window, cx);
        }
    }

    pub fn close_tab(&mut self, side: PaneSide, tab_idx: usize, cx: &mut Context<Self>) {
        let Some(pane) = self.pane_mut(side) else {
            return;
        };
        if tab_idx >= pane.tabs.len() {
            return;
        }
        pane.tabs.remove(tab_idx);
        if pane.tabs.is_empty() {
            pane.active_tab = 0;
            if side == PaneSide::Right {
                self.right_pane = None;
                self.active_pane = PaneSide::Left;
            }
        } else if pane.active_tab >= pane.tabs.len() {
            pane.active_tab = pane.tabs.len() - 1;
        }
        self.persist_workspaces();
        cx.notify();
    }

    pub fn split_active_to_right(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.right_pane.is_some() {
            if let Some(tab) = self.pane(PaneSide::Left).and_then(WorkspacePane::active) {
                let path = tab.path.clone();
                self.open_file(path, PaneSide::Right, window, cx);
                return;
            }
        }
        self.ensure_right_pane();
        if let Some(tab) = self.pane(PaneSide::Left).and_then(WorkspacePane::active) {
            let path = tab.path.clone();
            self.open_file(path, PaneSide::Right, window, cx);
        } else {
            self.active_pane = PaneSide::Right;
            self.persist_workspaces();
            cx.notify();
        }
    }

    pub fn toggle_md_view_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let side = self.active_pane;
        if let Some(tab_id) = self.pane(side).and_then(WorkspacePane::active).map(|t| t.id) {
            self.commit_active_live_block(side, tab_id, window, cx);
        }
        if let Some(tab) = self.pane_mut(side).and_then(WorkspacePane::active_mut) {
            match &mut tab.kind {
                TabKind::Markdown { view_mode, .. } => {
                    *view_mode = match *view_mode {
                        MdViewMode::Split => MdViewMode::Reading,
                        MdViewMode::Reading => MdViewMode::Edit,
                        MdViewMode::Edit => MdViewMode::Split,
                    };
                    cx.notify();
                }
                TabKind::Typst { view_mode, .. } => {
                    *view_mode = match *view_mode {
                        TypstViewMode::Split => TypstViewMode::Preview,
                        TypstViewMode::Preview => TypstViewMode::Edit,
                        TypstViewMode::Edit => TypstViewMode::Split,
                    };
                    cx.notify();
                }
                TabKind::Fountain { view_mode, .. } => {
                    *view_mode = match *view_mode {
                        FountainViewMode::Split => FountainViewMode::Script,
                        FountainViewMode::Script => FountainViewMode::Edit,
                        FountainViewMode::Edit => FountainViewMode::Split,
                    };
                    cx.notify();
                }
                _ => {}
            }
        }
    }

    pub fn toggle_md_source_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let side = self.active_pane;
        if let Some(tab_id) = self.pane(side).and_then(WorkspacePane::active).map(|t| t.id) {
            self.commit_active_live_block(side, tab_id, window, cx);
        }
        let info = {
            let Some(tab) = self.pane_mut(side).and_then(WorkspacePane::active_mut) else {
                return;
            };
            let tab_id = tab.id;
            let TabKind::Markdown {
                editor,
                source_mode,
                live_blocks,
                live_active_block,
                live_list_state,
                live_editor,
                ..
            } = &mut tab.kind
            else {
                return;
            };
            *source_mode = !*source_mode;
            let sm = *source_mode;
            let active_before = *live_active_block;
            if sm {
                *live_active_block = None;
                if let Some(old_ix) = active_before {
                    live_list_state.splice_focusable(old_ix..old_ix + 1, [None]);
                }
            }
            (
                tab_id,
                sm,
                editor.clone(),
                Arc::clone(live_blocks),
                active_before,
                live_editor.clone(),
            )
        };
        let (tab_id, sm, editor, old_blocks, active_before, live_editor) = info;

        if sm {
            let target_line = active_before
                .and_then(|ix| old_blocks.get(ix))
                .map(|b| {
                    let live_pos = live_editor.read(cx).cursor_position().line as usize;
                    b.start_line + live_pos
                })
                .unwrap_or(0);
            editor.update(cx, |s, cx| {
                s.set_line_number(true, window, cx);
                s.set_editor_paddings(gpui_kit::Edges {
                    top: px(10.),
                    right: px(16.),
                    bottom: px(24.),
                    left: px(12.),
                });
                s.set_cursor_position(
                    gpui_kit::component::input::Position::new(target_line as u32, 0),
                    window,
                    cx,
                );
            });
            cx.notify();
        } else {
            let (raw, cursor_line, cursor_col) = {
                let ed = editor.read(cx);
                let pos = ed.cursor_position();
                (ed.value().to_string(), pos.line as usize, pos.character as usize)
            };
            editor.update(cx, |s, cx| {
                s.set_line_number(false, window, cx);
                s.set_editor_paddings(gpui_kit::Edges {
                    top: px(16.),
                    right: px(28.),
                    bottom: px(24.),
                    left: px(28.),
                });
            });
            self.sync_live_blocks_from_master(side, tab_id, &raw, true, cx);
            let target = self.find_tab(side, tab_id).and_then(|t| match &t.kind {
                TabKind::Markdown { live_blocks, .. } => {
                    let ix = block_index_for_line(live_blocks, cursor_line);
                    let line_in_block = cursor_line.saturating_sub(live_blocks[ix].start_line);
                    let off = line_col_to_byte_offset(&live_blocks[ix].raw, line_in_block, cursor_col);
                    Some((ix, off))
                }
                _ => None,
            });
            if let Some((ix, off)) = target {
                self.activate_live_block(side, tab_id, ix, off, window, cx);
            } else {
                cx.notify();
            }
        }
    }

    pub fn handle_markdown_link(
        &mut self,
        tab_id: u64,
        side: PaneSide,
        raw_url: &SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let url_str = raw_url.as_ref();
        if let Some(target) = crate::markdown::decode_uri_payload(url_str, crate::markdown::VKARMA_NOTE_SCHEME) {
            let current_file = self
                .find_tab(side, tab_id)
                .map(|t| t.path.clone())
                .unwrap_or_else(|| self.workspace_root.join("note.md"));

            if let Some(resolved) = self.vault.resolve_wikilink(&target, &current_file) {
                self.open_file(resolved, side, window, cx);
                return;
            }

            // Create missing wikilink note cleanly like Obsidian
            let clean = target
                .split('|')
                .next()
                .unwrap_or(&target)
                .split('#')
                .next()
                .unwrap_or(&target)
                .trim();
            if !clean.is_empty() {
                let rel_name = if clean.ends_with(".md") {
                    clean.to_string()
                } else {
                    format!("{clean}.md")
                };
                let dest = current_file
                    .parent()
                    .unwrap_or(&self.workspace_root)
                    .join(&rel_name);
                let title = dest
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("New Note");
                if crate::vault::create_new_file(&dest, &format!("# {title}\n\n")).is_ok() {
                    self.refresh_vault_index(cx);
                    self.open_file(dest, side, window, cx);
                }
            }
            return;
        }

        if url_str.starts_with("http://") || url_str.starts_with("https://") {
            let _ = Command::new("xdg-open").arg(url_str).spawn();
        }
    }

    pub fn handle_task_toggle(
        &mut self,
        tab_id: u64,
        side: PaneSide,
        event: &TaskToggleEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.find_tab(side, tab_id) else {
            return;
        };
        if let TabKind::Markdown {
            editor,
            preprocessed,
            ..
        } = &tab.kind
        {
            let raw = editor.read(cx).value();
            if let Some(updated) = toggle_nth_task_checkbox(
                raw.as_ref(),
                preprocessed.as_ref(),
                event.marker_range().start,
                event.checked(),
            ) {
                let ed = editor.clone();
                ed.update(cx, |s, cx| s.set_value(updated, window, cx));
                self.on_markdown_changed(side, tab_id, window, cx);
            }
        }
    }

    fn save_clipboard_image_attachment(
        &mut self,
        item: &ClipboardItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let img_entry = item.entries().iter().find_map(|e| match e {
            gpui_kit::ClipboardEntry::Image(img) => Some(img),
            _ => None,
        })?;

        let attach_dir = self.workspace_root.join("attachments");
        if let Err(err) = fs::create_dir_all(&attach_dir) {
            self.toast_err(format!("Could not create attachments folder: {err}"), window, cx);
            return None;
        }

        let ext = img_entry.format().extension();
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let mut filename = format!("pasted-{stamp}.{ext}");
        let mut dest = attach_dir.join(&filename);
        let mut counter = 1u32;
        while dest.exists() {
            filename = format!("pasted-{stamp}-{counter}.{ext}");
            dest = attach_dir.join(&filename);
            counter += 1;
        }

        if let Err(err) = fs::write(&dest, img_entry.bytes()) {
            self.toast_err(format!("Could not save pasted image: {err}"), window, cx);
            return None;
        }

        self.refresh_vault_index(cx);
        self.toast_ok(format!("Saved image attachments/{filename}"), window, cx);
        Some(filename)
    }

    pub fn handle_markdown_image_paste(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        item: &ClipboardItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let has_img = item
            .entries()
            .iter()
            .any(|e| matches!(e, gpui_kit::ClipboardEntry::Image(_)));
        if !has_img {
            return false;
        }
        let Some(filename) = self.save_clipboard_image_attachment(item, window, cx) else {
            return true;
        };
        if let Some(tab) = self.find_tab(side, tab_id)
            && let TabKind::Markdown { editor, .. } = &tab.kind
        {
            let embed = format!("![[{filename}]]");
            editor.clone().update(cx, |state, cx| {
                state.insert(embed, window, cx);
            });
        }
        true
    }

    pub fn handle_live_image_paste(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        item: &ClipboardItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let has_img = item
            .entries()
            .iter()
            .any(|e| matches!(e, gpui_kit::ClipboardEntry::Image(_)));
        if !has_img {
            return false;
        }
        let Some(filename) = self.save_clipboard_image_attachment(item, window, cx) else {
            return true;
        };
        if let Some(tab) = self.find_tab(side, tab_id)
            && let TabKind::Markdown { live_editor, .. } = &tab.kind
        {
            let embed = format!("![[{filename}]]");
            live_editor.clone().update(cx, |state, cx| {
                state.insert(embed, window, cx);
            });
        }
        true
    }

    pub fn commit_formula_bar(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        move_down: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.find_tab_mut(side, tab_id) else {
            return;
        };
        if let TabKind::Spreadsheet {
            doc,
            active_sheet,
            active_row,
            active_col,
            formula_input,
            grid_focus,
            ..
        } = &mut tab.kind
        {
            let value = formula_input.read(cx).value().to_string();
            if let Err(err) = doc.set_cell(*active_sheet, *active_row, *active_col, &value) {
                self.toast_err(format!("Cell update failed: {err}"), window, cx);
                return;
            }
            tab.dirty = doc.dirty;
            if move_down {
                *active_row += 1;
                let next_raw = doc.raw_input(*active_sheet, *active_row, *active_col);
                formula_input
                    .clone()
                    .update(cx, |s, cx| s.set_value(next_raw, window, cx));
                grid_focus.focus(window, cx);
            }
            cx.notify();
        }
    }

    pub fn select_spreadsheet_cell(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        row: i32,
        col: i32,
        focus_formula: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.find_tab_mut(side, tab_id) else {
            return;
        };
        if let TabKind::Spreadsheet {
            doc,
            active_sheet,
            active_row,
            active_col,
            formula_input,
            row_scroll,
            grid_focus,
            ..
        } = &mut tab.kind
        {
            *active_row = row.max(1);
            *active_col = col.max(1);
            let raw = doc.raw_input(*active_sheet, *active_row, *active_col);
            let input_clone = formula_input.clone();
            input_clone.update(cx, |s, cx| s.set_value(raw, window, cx));
            row_scroll.scroll_to_item((*active_row - 1) as usize, gpui_kit::ScrollStrategy::Nearest);
            if focus_formula {
                input_clone.focus_handle(cx).focus(window, cx);
            } else {
                grid_focus.focus(window, cx);
            }
            cx.notify();
        }
    }

    pub fn export_typst_current_pdf(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.find_tab(side, tab_id) else {
            return;
        };
        let TabKind::Typst { editor, .. } = &tab.kind else {
            return;
        };
        let source = editor.read(cx).value().to_string();
        let doc_path = tab.path.clone();
        let out_pdf = doc_path.with_extension("pdf");
        let ws_root = self.workspace_root.clone();

        cx.spawn_in(window, async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { export_typst_pdf(&source, &doc_path, &ws_root, &out_pdf) })
                .await;
            let _ = this.update_in(cx, |app, window, cx| match res {
                Ok(pdf_path) => {
                    app.toast_ok(format!("Exported {}", pdf_path.display()), window, cx);
                    app.refresh_vault_index(cx);
                }
                Err(err) => {
                    app.toast_err(format!("PDF export failed: {err}"), window, cx);
                }
            });
        })
        .detach();
    }

    pub fn jump_to_heading_line(
        &mut self,
        line_1based: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let side = self.active_pane;
        let Some(tab) = self.pane(side).and_then(WorkspacePane::active) else {
            return;
        };
        let tab_id = tab.id;
        let line_0based = line_1based.saturating_sub(1);
        let pos = gpui_kit::component::input::Position::new(line_0based as u32, 0);
        match &tab.kind {
            TabKind::Markdown {
                view_mode,
                source_mode,
                live_blocks,
                ..
            } => {
                if !*source_mode && *view_mode != MdViewMode::Reading {
                    let ix = block_index_for_line(live_blocks, line_0based);
                    self.activate_live_block(side, tab_id, ix, 0, window, cx);
                } else if let Some(TabKind::Markdown { editor, .. }) =
                    self.pane(side).and_then(WorkspacePane::active).map(|t| &t.kind)
                {
                    editor.clone().update(cx, |state, cx| {
                        state.set_cursor_position(pos, window, cx);
                    });
                }
            }
            TabKind::Fountain { editor, .. }
            | TabKind::Typst { editor, .. }
            | TabKind::Code { editor, .. } => {
                editor.clone().update(cx, |state, cx| {
                    state.set_cursor_position(pos, window, cx);
                });
            }
            _ => {}
        }
    }

    pub fn prompt_open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select Workspace Folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(dir) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |app, _, cx| {
                    app.open_workspace(dir, cx);
                });
            }
        })
        .detach();
    }

    pub fn prompt_open_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Open File(s) in VKarma".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                let _ = this.update_in(cx, |app, window, cx| {
                    let pane = app.active_pane;
                    for p in paths {
                        app.open_file(p, pane, window, cx);
                    }
                });
            }
        })
        .detach();
    }

    pub fn open_quick_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query_input = cx.new(|cx| InputState::new(window, cx).placeholder("Jump to file…"));
        let _sub = cx.subscribe_in(&query_input, window, |this, state, event, window, cx| {
            match event {
                InputEvent::Change => {
                    let q = state.read(cx).value().to_string();
                    if let Some(qs) = &mut this.quick_switcher {
                        qs.query = q;
                        qs.selected = 0;
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { secondary, .. } => {
                    let target_pane = if *secondary {
                        PaneSide::Right
                    } else {
                        this.active_pane
                    };
                    this.confirm_quick_switcher(target_pane, window, cx);
                }
                _ => {}
            }
        });
        query_input.focus_handle(cx).focus(window, cx);
        self.quick_switcher = Some(QuickSwitcherState {
            query_input,
            query: String::new(),
            selected: 0,
            _sub,
        });
        cx.notify();
    }

    pub fn confirm_quick_switcher(
        &mut self,
        target_pane: PaneSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(qs) = &self.quick_switcher else {
            return;
        };
        let matches = self.vault.fuzzy_search_files(&qs.query, 15);
        let chosen = matches.get(qs.selected).or_else(|| matches.first()).map(|f| f.path.clone());
        self.quick_switcher = None;
        if let Some(path) = chosen {
            self.open_file(path, target_pane, window, cx);
        }
        cx.notify();
    }
}

impl Render for VKarmaApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app_bg = hex_color(kd::BG_MAIN);
        let toolbar_bg = hex_color(kd::BG_SIDEBAR);
        let border = hex_color(kd::BORDER_SUBTLE);
        let fg = hex_color(kd::FG);
        let fg_muted = hex_color(kd::FG_MUTED);

        let top_bar = h_flex()
            .h(px(40.))
            .w_full()
            .px_2()
            .gap_2()
            .items_center()
            .justify_between()
            .bg(toolbar_bg)
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("toggle-sidebar-btn")
                            .ghost()
                            .small()
                            .icon(IconName::PanelLeft)
                            .tooltip("Toggle Left Sidebar (Ctrl+B)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.config.sidebar_visible = !this.config.sidebar_visible;
                                this.persist_workspaces();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("nav-back-btn")
                            .ghost()
                            .small()
                            .icon(IconName::ChevronLeft)
                            .disabled(!self.nav_history.can_back())
                            .tooltip("Back (Mouse4 / Alt+←)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.navigate_back(window, cx);
                            })),
                    )
                    .child(
                        Button::new("nav-forward-btn")
                            .ghost()
                            .small()
                            .icon(IconName::ChevronRight)
                            .disabled(!self.nav_history.can_forward())
                            .tooltip("Forward (Mouse5 / Alt+→)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.navigate_forward(window, cx);
                            })),
                    )
                    .child(
                        Button::new("open-folder-btn")
                            .ghost()
                            .small()
                            .icon(IconName::FolderOpen)
                            .label("Folder")
                            .tooltip("Bind Current Workspace Folder (Ctrl+Shift+O)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.prompt_open_folder(window, cx);
                            })),
                    )
                    .child(
                        Button::new("open-file-btn")
                            .ghost()
                            .small()
                            .icon(IconName::FilePlus)
                            .label("Open File")
                            .tooltip("Open Any File (Ctrl+O)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.prompt_open_files(window, cx);
                            })),
                    ),
            )
            .child(
                Button::new("quick-switcher-pill")
                    .outline()
                    .small()
                    .w(px(280.))
                    .icon(IconName::Search)
                    .label("Quick Open…  Ctrl+P")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_quick_switcher(window, cx);
                    })),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("new-md-btn")
                            .ghost()
                            .small()
                            .icon(IconName::FileText)
                            .label("+ Note")
                            .tooltip("New Markdown Note (Ctrl+N)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                let entity = cx.entity();
                                let dir = this.workspace_root.clone();
                                modals::open_new_item_dialog(
                                    entity,
                                    dir,
                                    NewItemTemplate::Markdown,
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("new-typ-btn")
                            .ghost()
                            .small()
                            .icon(IconName::BookOpen)
                            .label("+ Typst")
                            .on_click(cx.listener(|this, _, window, cx| {
                                let entity = cx.entity();
                                let dir = this.workspace_root.clone();
                                modals::open_new_item_dialog(
                                    entity,
                                    dir,
                                    NewItemTemplate::Typst,
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("new-sheet-btn")
                            .ghost()
                            .small()
                            .icon(IconName::FileSpreadsheet)
                            .label("+ Sheet")
                            .on_click(cx.listener(|this, _, window, cx| {
                                let entity = cx.entity();
                                let dir = this.workspace_root.clone();
                                modals::open_new_item_dialog(
                                    entity,
                                    dir,
                                    NewItemTemplate::Spreadsheet,
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("new-script-btn")
                            .ghost()
                            .small()
                            .icon(IconName::FileText)
                            .label("+ Script")
                            .on_click(cx.listener(|this, _, window, cx| {
                                let entity = cx.entity();
                                let dir = this.workspace_root.clone();
                                modals::open_new_item_dialog(
                                    entity,
                                    dir,
                                    NewItemTemplate::Fountain,
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("split-pane-btn")
                            .with_variant(if self.right_pane.is_some() {
                                gpui_kit::component::button::ButtonVariant::Primary
                            } else {
                                gpui_kit::component::button::ButtonVariant::Ghost
                            })
                            .small()
                            .icon(IconName::Columns2)
                            .tooltip("Split Right / Mix Tabs Side-by-Side (Ctrl+\\)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.right_pane.is_some()
                                    && this
                                        .right_pane
                                        .as_ref()
                                        .is_some_and(|p| p.tabs.is_empty())
                                {
                                    this.right_pane = None;
                                    this.active_pane = PaneSide::Left;
                                    this.persist_workspaces();
                                    cx.notify();
                                } else {
                                    this.split_active_to_right(window, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("toggle-right-panel-btn")
                            .with_variant(if self.config.right_panel_visible {
                                gpui_kit::component::button::ButtonVariant::Primary
                            } else {
                                gpui_kit::component::button::ButtonVariant::Ghost
                            })
                            .small()
                            .icon(IconName::PanelRight)
                            .tooltip("Outline & Backlinks (Ctrl+Shift+B)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.config.right_panel_visible = !this.config.right_panel_visible;
                                this.persist_workspaces();
                                cx.notify();
                            })),
                    ),
            );

        let center_area = if self.right_pane.is_some() {
            h_resizable("workspace-panes-split")
                .child(
                    resizable_panel()
                        .size_range(px(260.)..px(2400.))
                        .child(pane::render_pane(self, PaneSide::Left, window, cx)),
                )
                .child(
                    resizable_panel()
                        .size_range(px(260.)..px(2400.))
                        .child(pane::render_pane(self, PaneSide::Right, window, cx)),
                )
                .into_any_element()
        } else {
            pane::render_pane(self, PaneSide::Left, window, cx).into_any_element()
        };

        let mut workspace_split = h_resizable("vkarma-main-layout");
        if self.config.sidebar_visible {
            workspace_split = workspace_split.child(
                resizable_panel()
                    .size(px(260.))
                    .size_range(px(180.)..px(440.))
                    .child(sidebar::render_sidebar(self, cx)),
            );
        }
        workspace_split = workspace_split.child(resizable_panel().child(center_area));
        if self.config.right_panel_visible {
            workspace_split = workspace_split.child(
                resizable_panel()
                    .size(px(260.))
                    .size_range(px(190.)..px(420.))
                    .child(right_panel::render_right_panel(self, cx)),
            );
        }

        let _ = fg_muted;

        v_flex()
            .size_full()
            .bg(app_bg)
            .text_color(fg)
            .track_focus(&self.focus_handle)
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                match event.button {
                    MouseButton::Navigate(NavigationDirection::Back) => {
                        cx.stop_propagation();
                        this.navigate_back(window, cx);
                    }
                    MouseButton::Navigate(NavigationDirection::Forward) => {
                        cx.stop_propagation();
                        this.navigate_forward(window, cx);
                    }
                    _ => {}
                }
            }))
            .on_action(cx.listener(|this, _: &SaveFile, window, cx| {
                this.save_active_tab(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SaveAll, window, cx| {
                this.save_all_tabs(window, cx);
            }))
            .on_action(cx.listener(|this, _: &QuickSwitcher, window, cx| {
                this.open_quick_switcher(window, cx);
            }))
            .on_action(cx.listener(|this, _: &VaultSearch, window, cx| {
                this.config.sidebar_visible = true;
                this.sidebar_tab = SidebarTab::Search;
                this.persist_workspaces();
                this.search_input.focus_handle(cx).focus(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleViewMode, window, cx| {
                this.toggle_md_view_mode(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSourceMode, window, cx| {
                this.toggle_md_source_mode(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SplitRight, window, cx| {
                this.split_active_to_right(window, cx);
            }))
            .on_action(cx.listener(|this, _: &CloseTab, _, cx| {
                let side = this.active_pane;
                if let Some(idx) = this.pane(side).map(|p| p.active_tab) {
                    this.close_tab(side, idx, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| {
                let side = this.active_pane;
                let next = this.pane(side).and_then(|p| {
                    if p.tabs.len() > 1 {
                        Some((p.active_tab + 1) % p.tabs.len())
                    } else {
                        None
                    }
                });
                if let Some(next_idx) = next {
                    this.activate_tab(side, next_idx, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NavigateBack, window, cx| {
                this.navigate_back(window, cx);
            }))
            .on_action(cx.listener(|this, _: &NavigateForward, window, cx| {
                this.navigate_forward(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.config.sidebar_visible = !this.config.sidebar_visible;
                this.persist_workspaces();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleRightPanel, _, cx| {
                this.config.right_panel_visible = !this.config.right_panel_visible;
                this.persist_workspaces();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NewNote, window, cx| {
                let entity = cx.entity();
                let dir = this.workspace_root.clone();
                modals::open_new_item_dialog(entity, dir, NewItemTemplate::Markdown, window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenFileDialog, window, cx| {
                this.prompt_open_files(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenFolderDialog, window, cx| {
                this.prompt_open_folder(window, cx);
            }))
            .on_action(cx.listener(|this, _: &EscapeModal, _, cx| {
                if this.quick_switcher.is_some() {
                    this.quick_switcher = None;
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &SwitchWorkspace1, window, cx| {
                this.switch_workspace(0, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SwitchWorkspace2, window, cx| {
                this.switch_workspace(1, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SwitchWorkspace3, window, cx| {
                this.switch_workspace(2, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SwitchWorkspace4, window, cx| {
                this.switch_workspace(3, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SwitchWorkspace5, window, cx| {
                this.switch_workspace(4, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SwitchWorkspace6, window, cx| {
                this.switch_workspace(5, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SwitchWorkspace7, window, cx| {
                this.switch_workspace(6, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SwitchWorkspace8, window, cx| {
                this.switch_workspace(7, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SwitchWorkspace9, window, cx| {
                this.switch_workspace(8, window, cx);
            }))
            .on_action(cx.listener(|this, _: &PrevWorkspace, window, cx| {
                if this.workspaces.len() > 1 {
                    let prev = if this.active_workspace == 0 {
                        this.workspaces.len() - 1
                    } else {
                        this.active_workspace - 1
                    };
                    this.switch_workspace(prev, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NextWorkspace, window, cx| {
                if this.workspaces.len() > 1 {
                    let next = (this.active_workspace + 1) % this.workspaces.len();
                    this.switch_workspace(next, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NewWorkspaceDialog, window, cx| {
                let entity = cx.entity();
                let dir = this.workspace_root.clone();
                modals::open_new_workspace_dialog(entity, dir, window, cx);
            }))
            .child(workspace_bar::render_workspace_bar(self, cx))
            .child(top_bar)
            .child(div().flex_1().min_h_0().w_full().child(workspace_split))
            .child(status_bar::render_status_bar(self, cx))
            .when_some(self.quick_switcher.as_ref(), |this, qs| {
                this.child(modals::render_quick_switcher_overlay(self, qs, cx))
            })
            .into_any_element()
    }
}

fn live_block_font_size(kind: LiveBlockKind) -> Pixels {
    match kind {
        LiveBlockKind::Heading(1) => px(27.),
        LiveBlockKind::Heading(2) => px(20.),
        LiveBlockKind::Heading(3) => px(17.),
        LiveBlockKind::CodeFence
        | LiveBlockKind::Frontmatter
        | LiveBlockKind::Table
        | LiveBlockKind::MdxBlock => px(13.5),
        _ => px(15.),
    }
}

fn splice_list_diff(
    list_state: &ListState,
    live_scrollbar: &LiveScrollbarHandle,
    old_blocks: &[LiveBlock],
    new_blocks: &Arc<Vec<LiveBlock>>,
    old_active: Option<usize>,
    new_active: Option<(usize, FocusHandle)>,
) {
    let mut prefix = 0usize;
    let min_blocks = old_blocks.len().min(new_blocks.len());
    while prefix < min_blocks {
        if old_active == Some(prefix)
            || new_active.as_ref().is_some_and(|(ix, _)| *ix == prefix)
            || old_blocks[prefix].kind != new_blocks[prefix].kind
            || old_blocks[prefix].raw != new_blocks[prefix].raw
            || old_blocks[prefix].byte_start != new_blocks[prefix].byte_start
        {
            break;
        }
        prefix += 1;
    }

    let mut suffix = 0usize;
    let max_suffix = (old_blocks.len() - prefix).min(new_blocks.len() - prefix);
    while suffix < max_suffix {
        let oi = old_blocks.len() - 1 - suffix;
        let ni = new_blocks.len() - 1 - suffix;
        if old_active == Some(oi)
            || new_active.as_ref().is_some_and(|(ix, _)| *ix == ni)
            || old_blocks[oi].kind != new_blocks[ni].kind
            || old_blocks[oi].raw != new_blocks[ni].raw
        {
            break;
        }
        suffix += 1;
    }

    let old_end = old_blocks.len() - suffix;
    let new_end = new_blocks.len() - suffix;
    if old_end == prefix && new_end == prefix {
        return;
    }
    list_state.splice_focusable(
        prefix..old_end,
        (prefix..new_end).map(|ni| {
            new_active.as_ref().and_then(|(active_ix, fh)| {
                if *active_ix == ni {
                    Some(fh.clone())
                } else {
                    None
                }
            })
        }),
    );
    live_scrollbar.splice_blocks(prefix..old_end, Arc::clone(new_blocks), prefix..new_end);
}

pub(crate) fn reorder_with_active<T>(
    items: &mut Vec<T>,
    active: &mut usize,
    from: usize,
    to: usize,
) -> bool {
    if from >= items.len() || to >= items.len() || from == to {
        return false;
    }
    let item = items.remove(from);
    items.insert(to, item);
    if *active == from {
        *active = to;
    } else if from < *active && *active <= to {
        *active -= 1;
    } else if to <= *active && *active < from {
        *active += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nav_history_tracks_back_forward_dedup_and_truncation() {
        let mut h = NavHistory::default();
        assert!(!h.can_back());
        assert!(!h.can_forward());

        let a = PathBuf::from("/tmp/a.md");
        let b = PathBuf::from("/tmp/b.md");
        let c = PathBuf::from("/tmp/c.md");

        h.push(PaneSide::Left, a.clone());
        h.push(PaneSide::Left, a.clone()); // consecutive dedup
        assert!(!h.can_back());

        h.push(PaneSide::Left, b.clone());
        h.push(PaneSide::Right, c.clone());
        assert!(h.can_back());
        assert!(!h.can_forward());

        let prev = h.go_back_matching(|_| true).unwrap();
        assert_eq!(prev.path, b);
        assert_eq!(prev.pane, PaneSide::Left);
        assert!(h.can_back());
        assert!(h.can_forward());

        let fwd = h.go_forward_matching(|_| true).unwrap();
        assert_eq!(fwd.path, c);
        assert!(!h.can_forward());

        // Step back to `a` (rejecting `b` like a deleted file), then push `d` to truncate forward branch.
        let skipped = h.go_back_matching(|e| e.path != b).unwrap();
        assert_eq!(skipped.path, a);
        assert!(h.can_forward());

        let d = PathBuf::from("/tmp/d.md");
        h.push(PaneSide::Left, d.clone());
        assert!(!h.can_forward());
        assert_eq!(h.go_back_matching(|_| true).unwrap().path, a);
    }

    #[test]
    fn nav_history_caps_at_max_entries() {
        let mut h = NavHistory::default();
        for i in 0..(MAX_NAV_HISTORY + 12) {
            h.push(PaneSide::Left, PathBuf::from(format!("/tmp/doc-{i}.md")));
        }
        assert_eq!(h.entries.len(), MAX_NAV_HISTORY);
        assert_eq!(h.entries.first().unwrap().path, PathBuf::from("/tmp/doc-12.md"));
        assert!(!h.can_forward());
    }

    #[test]
    fn reorder_with_active_updates_order_and_tracks_active_index() {
        // Drag inactive item right across active
        let mut items = vec!["A", "B", "C", "D"];
        let mut active = 1; // "B"
        assert!(reorder_with_active(&mut items, &mut active, 0, 2));
        assert_eq!(items, vec!["B", "C", "A", "D"]);
        assert_eq!(items[active], "B");
        assert_eq!(active, 0);

        // Drag active item right to the end
        assert!(reorder_with_active(&mut items, &mut active, 0, 3));
        assert_eq!(items, vec!["C", "A", "D", "B"]);
        assert_eq!(items[active], "B");
        assert_eq!(active, 3);

        // Drag active item left
        assert!(reorder_with_active(&mut items, &mut active, 3, 1));
        assert_eq!(items, vec!["C", "B", "A", "D"]);
        assert_eq!(items[active], "B");
        assert_eq!(active, 1);

        // Drag inactive item from right to left across active
        assert!(reorder_with_active(&mut items, &mut active, 3, 0));
        assert_eq!(items, vec!["D", "C", "B", "A"]);
        assert_eq!(items[active], "B");
        assert_eq!(active, 2);

        // Same-index and out-of-bounds are no-ops
        assert!(!reorder_with_active(&mut items, &mut active, 2, 2));
        assert!(!reorder_with_active(&mut items, &mut active, 0, 99));
        assert!(!reorder_with_active(&mut items, &mut active, 99, 0));
        assert_eq!(items, vec!["D", "C", "B", "A"]);
        assert_eq!(active, 2);
    }
}
