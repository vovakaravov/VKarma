pub mod archive_view;
pub mod audio_view;
pub mod canvas_view;
pub mod code_view;
pub mod epub_view;
pub mod find_bar;
pub mod font_view;
pub mod fountain_view;
pub mod kanban_view;
pub mod live_editor;
pub mod markdown_view;
pub mod mermaid_view;
pub mod modals;
pub mod office_view;
pub mod pane;
pub mod pdf_view;
pub mod right_panel;
pub mod sidebar;
pub mod spreadsheet_view;
pub mod sqlite_view;
pub mod status_bar;
pub mod subtitle_view;
pub mod svg_view;
pub mod typst_view;
pub mod workspace_bar;

use archive_view::ArchiveView;
use audio_view::AudioView;
use canvas_view::{CanvasView, CanvasViewEvent};
use epub_view::EpubView;
use find_bar::EditorFindBar;
use font_view::FontView;
use kanban_view::{KanbanView, KanbanViewEvent};
use mermaid_view::{MermaidView, MermaidViewEvent};
use sqlite_view::SqliteView;
use subtitle_view::{SubtitleView, SubtitleViewEvent};
use svg_view::{SvgView, SvgViewEvent};

use crate::config::{
    AppConfig, MdViewMode, WorkspaceConfig, WorkspacePreset, default_starter_workspaces,
    find_workspace_for_path, make_unique_workspace_id,
};
use crate::fountain::{
    FountainDocument, extract_fountain_headings, fountain_to_typst, parse_fountain,
};
use crate::live_md::LinkTarget;
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
    extract_headings, find_all_byte_ranges, search_content,
};
use crate::watcher::WorkspaceWatcher;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{
    EditorState, InputEvent, InputState, Replace as InputReplace, Search as InputSearch,
};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::text::{FrontmatterPlugin, MarkdownExtensions, TaskToggleEvent};
use gpui_kit::component::{
    Disableable as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable, Hsla,
    Image, ImageCache, ImageCacheError, InteractiveElement as _, IntoElement, KeyBinding,
    MouseButton, MouseDownEvent, NavigationDirection, ParentElement, PathPromptOptions, Render,
    RenderImage, Resource, RetainAllImageCache, ScrollHandle, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, UniformListScrollHandle, Window,
    actions, div, point, prelude::FluentBuilder as _, px,
};
use live_editor::{LiveEditorEvent, LiveMarkdownEditor};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[cfg(target_env = "gnu")]
pub fn trim_allocator() {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> std::ffi::c_int;
    }
    // SAFETY: glibc's `malloc_trim(0)` is thread-safe and releases unused free
    // heap/arena pages back to the OS without affecting live allocations.
    unsafe {
        let _ = malloc_trim(0);
    }
}

#[cfg(not(target_env = "gnu"))]
pub fn trim_allocator() {}

pub struct LruImageCache {
    inner: Entity<RetainAllImageCache>,
    order: VecDeque<Resource>,
    max_entries: usize,
}

impl LruImageCache {
    pub fn new(max_entries: usize, cx: &mut App) -> Entity<Self> {
        let inner = RetainAllImageCache::new(cx);
        cx.new(|_| Self {
            inner,
            order: VecDeque::with_capacity(max_entries + 1),
            max_entries: max_entries.max(1),
        })
    }
}

impl ImageCache for LruImageCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        if let Some(pos) = self.order.iter().position(|r| r == resource) {
            self.order.remove(pos);
        }
        self.order.push_back(resource.clone());

        let result = self
            .inner
            .update(cx, |cache, cx| cache.load(resource, window, cx));

        while self.order.len() > self.max_entries {
            if let Some(oldest) = self.order.pop_front() {
                self.inner.update(cx, |cache, cx| {
                    cache.remove(&oldest, window, cx);
                });
            }
        }

        result
    }
}

static NEXT_TAB_ID: AtomicU64 = AtomicU64::new(1);

fn next_tab_id() -> u64 {
    NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed)
}

pub const NORMAL_READ_WIDTH: f32 = 820.0;
pub const ZEN_BOOK_WIDTH: f32 = 680.0;

/// Returns the centered prose column max-width in pixels for the current view mode.
pub fn column_max_width(centered: bool, zen_mode: bool) -> Option<f32> {
    if zen_mode {
        Some(ZEN_BOOK_WIDTH)
    } else if centered {
        Some(NORMAL_READ_WIDTH)
    } else {
        None
    }
}

/// Frames an `Editor` widget either full-bleed or centered at book width in Zen Mode.
pub fn zen_editor_frame(content: impl IntoElement, zen_mode: bool) -> AnyElement {
    if zen_mode {
        div()
            .size_full()
            .bg(hex_color(kd::BG_MAIN))
            .flex()
            .justify_center()
            .px_6()
            .child(
                div()
                    .w_full()
                    .max_w(px(ZEN_BOOK_WIDTH))
                    .h_full()
                    .pt(px(36.))
                    .child(content),
            )
            .into_any_element()
    } else {
        div()
            .size_full()
            .bg(hex_color(kd::BG_MAIN))
            .child(content)
            .into_any_element()
    }
}

actions!(
    vkarma,
    [
        SaveFile,
        SaveAll,
        QuickSwitcher,
        VaultSearch,
        FindInTab,
        ReplaceInTab,
        ToggleViewMode,
        ToggleSourceMode,
        ToggleZenMode,
        SplitRight,
        CloseTab,
        NextTab,
        MoveTabLeft,
        MoveTabRight,
        ToggleSidebar,
        ToggleRightPanel,
        NewNote,
        OpenFileDialog,
        OpenFolderDialog,
        EscapeModal,
        RenameSelectedTreeItem,
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
        KeyBinding::new("ctrl-f", FindInTab, None),
        KeyBinding::new("ctrl-h", ReplaceInTab, None),
        KeyBinding::new("ctrl-e", ToggleViewMode, None),
        KeyBinding::new("ctrl-m", ToggleSourceMode, None),
        KeyBinding::new("f11", ToggleZenMode, None),
        KeyBinding::new("ctrl-alt-z", ToggleZenMode, None),
        KeyBinding::new("ctrl-\\", SplitRight, None),
        KeyBinding::new("ctrl-w", CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-pageup", MoveTabLeft, None),
        KeyBinding::new("ctrl-shift-pagedown", MoveTabRight, None),
        KeyBinding::new("ctrl-b", ToggleSidebar, None),
        KeyBinding::new("ctrl-shift-b", ToggleRightPanel, None),
        KeyBinding::new("ctrl-n", NewNote, None),
        KeyBinding::new("ctrl-o", OpenFileDialog, None),
        KeyBinding::new("ctrl-shift-o", OpenFolderDialog, None),
        KeyBinding::new("escape", EscapeModal, None),
        KeyBinding::new("f2", RenameSelectedTreeItem, None),
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
        KeyBinding::new(
            "tab",
            live_editor::LiveEditorTab,
            Some(live_editor::LIVE_EDITOR_CONTEXT),
        ),
        KeyBinding::new(
            "shift-tab",
            live_editor::LiveEditorShiftTab,
            Some(live_editor::LIVE_EDITOR_CONTEXT),
        ),
    ]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightPanelTab {
    Outline,
    Backlinks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeViewMode {
    Code,
    Tree,
    Table,
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

#[derive(Debug, Clone)]
pub struct ReadFindMatch {
    pub line: usize,
    pub range: Range<usize>,
    pub snippet: String,
}

pub struct ReadFindState {
    pub open: bool,
    pub query: String,
    pub case_sensitive: bool,
    pub input: Entity<InputState>,
    pub _sub: Subscription,
    pub matches: Vec<ReadFindMatch>,
    pub active: usize,
}

pub struct SpreadsheetFindState {
    pub open: bool,
    pub query: String,
    pub case_sensitive: bool,
    pub input: Entity<InputState>,
    pub _sub: Subscription,
    pub matches: Vec<(u32, i32, i32)>,
    pub active: usize,
}

#[derive(Debug, Clone)]
pub struct OfficeFindMatch {
    pub block_ix: usize,
    pub row_ix: usize,
    pub col_ix: usize,
    pub range: Range<usize>,
}

pub struct OfficeFindState {
    pub open: bool,
    pub query: String,
    pub case_sensitive: bool,
    pub input: Entity<InputState>,
    pub _sub: Subscription,
    pub matches: Vec<OfficeFindMatch>,
    pub active: usize,
}

pub enum TabKind {
    Markdown {
        editor: Option<Entity<EditorState>>,
        editor_find: Option<Entity<EditorFindBar>>,
        live_doc: Entity<LiveMarkdownEditor>,
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
        read_find: ReadFindState,
        read_scroll: ScrollHandle,
        _live_sub: Subscription,
        _sub: Option<Subscription>,
    },
    Fountain {
        editor: Entity<EditorState>,
        find_bar: Entity<EditorFindBar>,
        view_mode: FountainViewMode,
        paper_light: bool,
        show_notes: bool,
        parsed: FountainDocument,
        headings: Vec<OutlineHeading>,
        _sub: Subscription,
    },
    Typst {
        editor: Entity<EditorState>,
        find_bar: Entity<EditorFindBar>,
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
        find: SpreadsheetFindState,
        _sub: Subscription,
    },
    Code {
        editor: Entity<EditorState>,
        find_bar: Entity<EditorFindBar>,
        language: &'static str,
        badge: &'static str,
        soft_wrap: bool,
        data_view: CodeViewMode,
        collapsed_paths: HashSet<String>,
        tree_filter: Option<Entity<InputState>>,
        _tree_filter_sub: Option<Subscription>,
        _sub: Subscription,
    },
    Canvas {
        view: Entity<CanvasView>,
        _sub: Subscription,
    },
    Kanban {
        view: Entity<KanbanView>,
        _sub: Subscription,
    },
    Subtitle {
        view: Entity<SubtitleView>,
        _sub: Subscription,
    },
    Svg {
        view: Entity<SvgView>,
        _sub: Subscription,
    },
    Mermaid {
        view: Entity<MermaidView>,
        _sub: Subscription,
    },
    Office {
        doc: crate::formats::office::OfficeDoc,
        paper_light: bool,
        font_scale: f32,
        file_size_bytes: u64,
        scroll_handle: ScrollHandle,
        find: OfficeFindState,
    },
    Epub {
        view: Entity<EpubView>,
        file_size_bytes: u64,
    },
    Sqlite {
        view: Entity<SqliteView>,
        file_size_bytes: u64,
    },
    Archive {
        view: Entity<ArchiveView>,
        file_size_bytes: u64,
    },
    Audio {
        view: Entity<AudioView>,
        file_size_bytes: u64,
    },
    Font {
        view: Entity<FontView>,
        file_size_bytes: u64,
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

pub struct InlineRenameState {
    pub target_path: PathBuf,
    pub input: Entity<InputState>,
    pub _sub: Subscription,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewItemTemplate {
    File,
    Folder,
}

impl NewItemTemplate {
    pub fn title(&self) -> &'static str {
        match self {
            Self::File => "New File",
            Self::Folder => "New Folder",
        }
    }

    pub fn placeholder(&self) -> &'static str {
        match self {
            Self::File => "Name.md, Script.fountain, Player.cs, Sheet.xlsx…",
            Self::Folder => "Folder Name",
        }
    }
}

pub fn file_kind_color(kind: FileKind) -> Hsla {
    match kind {
        FileKind::Markdown => hex_color(kd::ACCENT),
        FileKind::Mdx => hex_color(kd::ORANGE),
        FileKind::Canvas => hex_color(kd::VIOLET),
        FileKind::Kanban => hex_color(kd::AQUA),
        FileKind::Subtitle => hex_color(kd::ORANGE),
        FileKind::Svg => hex_color(kd::PINK),
        FileKind::Mermaid => hex_color(kd::AQUA),
        FileKind::Office => hex_color(kd::GREEN),
        FileKind::Epub => hex_color(kd::VIOLET),
        FileKind::Sqlite => hex_color(kd::AQUA),
        FileKind::Archive => hex_color(kd::YELLOW),
        FileKind::Audio => hex_color(kd::ORANGE),
        FileKind::Font => hex_color(kd::GREEN),
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
                ("MD", hex_color(kd::ACCENT))
            }
        }
        TabKind::Canvas { .. } => ("CNV", hex_color(kd::VIOLET)),
        TabKind::Kanban { .. } => ("KBN", hex_color(kd::AQUA)),
        TabKind::Subtitle { .. } => ("SUB", hex_color(kd::ORANGE)),
        TabKind::Svg { .. } => ("SVG", hex_color(kd::PINK)),
        TabKind::Mermaid { .. } => ("MMD", hex_color(kd::AQUA)),
        TabKind::Office { doc, .. } => (doc.format.label(), hex_color(kd::GREEN)),
        TabKind::Epub { .. } => ("EPUB", hex_color(kd::VIOLET)),
        TabKind::Sqlite { .. } => ("DB", hex_color(kd::AQUA)),
        TabKind::Archive { .. } => (
            crate::formats::archive::ArchiveFormat::from_path(&tab.path).label(),
            hex_color(kd::YELLOW),
        ),
        TabKind::Audio { .. } => ("AUD", hex_color(kd::ORANGE)),
        TabKind::Font { .. } => ("FNT", hex_color(kd::GREEN)),
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

    pub sidebar_focus: FocusHandle,
    pub selected_tree_path: Option<PathBuf>,
    pub inline_rename: Option<InlineRenameState>,
    pub right_panel_tab: RightPanelTab,
    pub expanded_dirs: HashSet<PathBuf>,

    pub search_input: Entity<InputState>,
    pub search_hits: Vec<SearchHit>,
    pub _search_sub: Subscription,

    pub quick_switcher: Option<QuickSwitcherState>,
    pub nav_history: NavHistory,
    pub status_toast: Option<String>,
    pub workspace_gen: u64,
    pub zen_mode: bool,
    pub zen_entered_fullscreen: bool,
    pub image_cache: Entity<LruImageCache>,
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

        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search…"));
        let _search_sub = cx.subscribe_in(&search_input, window, |this, state, event, _, cx| {
            if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                let query = state.read(cx).value().to_string();
                this.run_vault_search(query, cx);
            }
        });

        let weak_app_for_wrap = cx.weak_entity();
        let image_plugin = StyledImagePlugin::with_link_handler(move |url, window, cx| {
            let _ = weak_app_for_wrap.update(cx, |app, cx| {
                let side = app.active_pane;
                let tab_id = app.active_tab_ref().map(|t| t.id).unwrap_or(0);
                app.handle_markdown_link(tab_id, side, url, window, cx);
            });
        });

        let markdown_extensions = MarkdownExtensions::default()
            .frontmatter()
            .plugin(FrontmatterPlugin::new())
            .plugin(image_plugin)
            .plugin(SceneBreakPlugin)
            .parser_revision(1);

        let image_cache = LruImageCache::new(48, cx);

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
            sidebar_focus: cx.focus_handle(),
            selected_tree_path: None,
            inline_rename: None,
            right_panel_tab: RightPanelTab::Outline,
            expanded_dirs: HashSet::new(),
            search_input,
            search_hits: Vec::new(),
            _search_sub,
            quick_switcher: None,
            nav_history: NavHistory::default(),
            status_toast: None,
            workspace_gen: 1,
            zen_mode: false,
            zen_entered_fullscreen: false,
            image_cache,
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

    fn saved_tabs_for_pane(pane: &WorkspacePane) -> (Vec<PathBuf>, usize) {
        let mut paths = Vec::new();
        let mut mapped_active = 0usize;
        for (idx, tab) in pane.tabs.iter().enumerate() {
            if idx == pane.active_tab {
                mapped_active = paths.len();
            }
            paths.push(tab.path.clone());
        }
        if paths.is_empty() {
            mapped_active = 0;
        } else if mapped_active >= paths.len() {
            mapped_active = paths.len() - 1;
        }
        (paths, mapped_active)
    }

    fn slot_to_workspace_config(&self, idx: usize, slot: &WorkspaceSlot) -> WorkspaceConfig {
        let (left_tabs, left_active_tab, right_tabs, right_active_tab, split_open) =
            if idx == self.active_workspace {
                let (lt, la) = Self::saved_tabs_for_pane(&self.left_pane);
                let (rt, ra) = self
                    .right_pane
                    .as_ref()
                    .map(Self::saved_tabs_for_pane)
                    .unwrap_or_default();
                (lt, la, rt, ra, self.right_pane.is_some())
            } else if let Some(parked) = &slot.parked {
                let (lt, la) = Self::saved_tabs_for_pane(&parked.left_pane);
                let (rt, ra) = parked
                    .right_pane
                    .as_ref()
                    .map(Self::saved_tabs_for_pane)
                    .unwrap_or_default();
                (lt, la, rt, ra, parked.right_pane.is_some())
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
        self.quick_switcher = None;
        self.inline_rename = None;
        self.selected_tree_path = None;

        let search_query = self.search_input.read(cx).value().to_string();
        let old_idx = self.active_workspace;
        {
            let parked = ParkedWorkspaceState {
                vault: Arc::clone(&self.vault),
                watcher: self.watcher.take(),
                left_pane: std::mem::replace(&mut self.left_pane, WorkspacePane::empty()),
                right_pane: self.right_pane.take(),
                active_pane: self.active_pane,
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
        trim_allocator();
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
                .spawn(async move {
                    let built = Arc::new(VaultIndex::build(&root, show_hidden));
                    trim_allocator();
                    built
                })
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
                        live_doc,
                        source_mode,
                        preprocessed,
                        is_mdx,
                        ..
                    } = &mut pane.kind
                    {
                        let raw: SharedString = if *source_mode
                            && let Some(ed) = editor.as_ref()
                        {
                            ed.read(cx).value()
                        } else {
                            live_doc.read(cx).text().to_string().into()
                        };
                        *preprocessed = preprocess_markdown_with_options(
                            raw.as_ref(),
                            &pane.path,
                            &vault,
                            0,
                            *is_mdx,
                        )
                        .into();
                    } else if let TabKind::Canvas { view, .. } = &pane.kind {
                        let vault_clone = Arc::clone(&vault);
                        view.update(cx, |v, _| v.update_vault(vault_clone));
                    }
                }
                let active_query = app.search_input.read(cx).value().to_string();
                if active_query.trim().chars().count() >= 2 {
                    app.run_vault_search(active_query, cx);
                }
                trim_allocator();
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
        self.inline_rename = None;
        self.selected_tree_path = None;
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
                live_doc,
                view_mode,
                source_mode,
                ..
            } => {
                if *source_mode
                    && let Some(ed) = editor
                {
                    ed.focus_handle(cx).focus(window, cx);
                } else if *view_mode != MdViewMode::Reading {
                    live_doc.focus_handle(cx).focus(window, cx);
                } else {
                    self.focus_handle.focus(window, cx);
                }
            }
            TabKind::Fountain { editor, .. }
            | TabKind::Typst { editor, .. }
            | TabKind::Code { editor, .. } => {
                editor.focus_handle(cx).focus(window, cx);
            }
            TabKind::Canvas { view, .. } => {
                view.focus_handle(cx).focus(window, cx);
            }
            TabKind::Kanban { view, .. } => {
                view.focus_handle(cx).focus(window, cx);
            }
            TabKind::Subtitle { view, .. } => {
                view.focus_handle(cx).focus(window, cx);
            }
            TabKind::Svg { view, .. } => {
                view.focus_handle(cx).focus(window, cx);
            }
            TabKind::Mermaid { view, .. } => {
                view.focus_handle(cx).focus(window, cx);
            }
            TabKind::Epub { view, .. } => {
                view.focus_handle(cx).focus(window, cx);
            }
            TabKind::Sqlite { view, .. } => {
                view.focus_handle(cx).focus(window, cx);
            }
            TabKind::Archive { view, .. } => {
                view.focus_handle(cx).focus(window, cx);
            }
            TabKind::Audio { view, .. } => {
                view.focus_handle(cx).focus(window, cx);
            }
            TabKind::Font { view, .. } => {
                view.focus_handle(cx).focus(window, cx);
            }
            TabKind::Spreadsheet { grid_focus, .. } => {
                grid_focus.focus(window, cx);
            }
            TabKind::Image { .. } | TabKind::Pdf { .. } | TabKind::Office { .. } => {}
        }
    }

    fn create_md_source_editor(
        side: PaneSide,
        tab_id: u64,
        raw: String,
        line_numbers: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<EditorState>, Entity<EditorFindBar>, Subscription) {
        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .searchable(false)
                .language("markdown")
                .soft_wrap(true)
                .line_number(line_numbers)
                .default_value(raw);
            state.set_editor_paddings(gpui_kit::Edges {
                top: px(10.),
                right: px(16.),
                bottom: px(24.),
                left: px(12.),
            });
            state
        });
        let editor_find = cx.new(|cx| EditorFindBar::new(editor.clone(), window, cx));
        let sub = cx.subscribe_in(&editor, window, move |this, _ed, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_markdown_changed(side, tab_id, window, cx);
            }
        });
        (editor, editor_find, sub)
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

                let (editor, editor_find, sub) = if source_mode {
                    let (ed, ef, s) =
                        Self::create_md_source_editor(side, tab_id, raw.clone(), true, window, cx);
                    (Some(ed), Some(ef), Some(s))
                } else {
                    (None, None, None)
                };

                let live_doc = cx.new(|cx| {
                    LiveMarkdownEditor::new(
                        raw,
                        path.to_path_buf(),
                        Arc::clone(&self.vault),
                        is_mdx,
                        window,
                        cx,
                    )
                });

                let _live_sub =
                    cx.subscribe_in(&live_doc, window, move |this, _ed, event, window, cx| {
                        match event {
                            LiveEditorEvent::Changed => {
                                this.on_live_doc_changed(side, tab_id, window, cx);
                            }
                            LiveEditorEvent::OpenLink {
                                target,
                                in_right_split,
                            } => {
                                let dest = if *in_right_split {
                                    PaneSide::Right
                                } else {
                                    side
                                };
                                let url: SharedString = match target {
                                    LinkTarget::Wikilink(note) => format!(
                                        "{}{}",
                                        crate::markdown::VKARMA_NOTE_SCHEME,
                                        crate::markdown::encode_uri_payload(note)
                                    )
                                    .into(),
                                    LinkTarget::Url(u) => u.clone().into(),
                                };
                                this.handle_markdown_link(tab_id, dest, &url, window, cx);
                            }
                            LiveEditorEvent::PasteImage(item) => {
                                this.handle_live_image_paste(side, tab_id, item, window, cx);
                            }
                        }
                    });

                let read_find_input = cx.new(|cx| {
                    InputState::new(window, cx).placeholder("Find in document…")
                });
                let read_find_sub = cx.subscribe_in(
                    &read_find_input,
                    window,
                    move |this, inp, event, _window, cx| match event {
                        InputEvent::Change => {
                            let val = inp.read(cx).value().to_string();
                            if let Some(t) = this.find_tab_mut(side, tab_id)
                                && let TabKind::Markdown { read_find, .. } = &mut t.kind
                            {
                                read_find.query = val;
                            }
                            this.recompute_md_read_find(side, tab_id, true, cx);
                            cx.notify();
                        }
                        InputEvent::PressEnter {
                            secondary, shift, ..
                        } => {
                            this.step_md_read_find(side, tab_id, !(*shift || *secondary), cx);
                        }
                        _ => {}
                    },
                );

                TabKind::Markdown {
                    editor,
                    editor_find,
                    live_doc,
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
                    read_find: ReadFindState {
                        open: false,
                        query: String::new(),
                        case_sensitive: false,
                        input: read_find_input,
                        _sub: read_find_sub,
                        matches: Vec::new(),
                        active: 0,
                    },
                    read_scroll: ScrollHandle::new(),
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
                        .searchable(false)
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
                let find_bar = cx.new(|cx| EditorFindBar::new(editor.clone(), window, cx));
                let sub = cx.subscribe_in(&editor, window, move |this, _ed, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.on_fountain_changed(side, tab_id, window, cx);
                    }
                });
                TabKind::Fountain {
                    editor,
                    find_bar,
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
                        .searchable(false)
                        .language("typst")
                        .soft_wrap(true)
                        .line_number(true)
                        .default_value(raw.clone())
                });
                let find_bar = cx.new(|cx| EditorFindBar::new(editor.clone(), window, cx));

                let sub = cx.subscribe_in(&editor, window, move |this, _ed, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.on_typst_changed(side, tab_id, window, cx);
                    }
                });

                // Trigger immediate initial compile on load
                self.schedule_typst_compile(side, tab_id, 1, raw, path.to_path_buf(), 144, false, cx);

                TabKind::Typst {
                    editor,
                    find_bar,
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

                let sheet_find_input = cx.new(|cx| {
                    InputState::new(window, cx).placeholder("Find in workbook…")
                });
                let sheet_find_sub = cx.subscribe_in(
                    &sheet_find_input,
                    window,
                    move |this, inp, event, _window, cx| match event {
                        InputEvent::Change => {
                            let val = inp.read(cx).value().to_string();
                            if let Some(t) = this.find_tab_mut(side, tab_id)
                                && let TabKind::Spreadsheet { find, .. } = &mut t.kind
                            {
                                find.query = val;
                            }
                            this.recompute_spreadsheet_find(side, tab_id, true, cx);
                            cx.notify();
                        }
                        InputEvent::PressEnter {
                            secondary, shift, ..
                        } => {
                            this.step_spreadsheet_find(side, tab_id, !(*shift || *secondary), cx);
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
                    find: SpreadsheetFindState {
                        open: false,
                        query: String::new(),
                        case_sensitive: false,
                        input: sheet_find_input,
                        _sub: sheet_find_sub,
                        matches: Vec::new(),
                        active: 0,
                    },
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
            FileKind::Canvas => {
                let raw = fs::read_to_string(path)?;
                let doc = crate::formats::canvas::CanvasDoc::parse(&raw)?;
                let doc_path = path.to_path_buf();
                let vault = Arc::clone(&self.vault);
                let view =
                    cx.new(|cx| CanvasView::new(doc, raw, doc_path, vault, window, cx));
                let sub = cx.subscribe_in(&view, window, move |this, _, event, window, cx| {
                    match event {
                        CanvasViewEvent::Changed => this.on_code_changed(side, tab_id, window, cx),
                        CanvasViewEvent::OpenFile(target_path) => {
                            let pane = this.active_pane;
                            this.open_file(target_path.clone(), pane, window, cx);
                        }
                    }
                });
                TabKind::Canvas { view, _sub: sub }
            }
            FileKind::Kanban => {
                let raw = fs::read_to_string(path)?;
                let view = cx.new(|cx| KanbanView::new(raw, window, cx));
                let sub = cx.subscribe_in(&view, window, move |this, _, event, window, cx| {
                    if matches!(event, KanbanViewEvent::Changed) {
                        this.on_code_changed(side, tab_id, window, cx);
                    }
                });
                TabKind::Kanban { view, _sub: sub }
            }
            FileKind::Subtitle => {
                let raw = fs::read_to_string(path)?;
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let default_fmt = if ext == "vtt" {
                    crate::formats::subtitle::SubtitleFormat::Vtt
                } else {
                    crate::formats::subtitle::SubtitleFormat::Srt
                };
                let view = cx.new(|cx| SubtitleView::new(raw, default_fmt, window, cx));
                let sub = cx.subscribe_in(&view, window, move |this, _, event, window, cx| {
                    if matches!(event, SubtitleViewEvent::Changed) {
                        this.on_code_changed(side, tab_id, window, cx);
                    }
                });
                TabKind::Subtitle { view, _sub: sub }
            }
            FileKind::Svg => {
                let raw = fs::read_to_string(path)?;
                let doc_path = path.to_path_buf();
                let view = cx.new(|cx| SvgView::new(raw, doc_path, window, cx));
                let sub = cx.subscribe_in(&view, window, move |this, _, event, window, cx| {
                    if matches!(event, SvgViewEvent::Changed) {
                        this.on_code_changed(side, tab_id, window, cx);
                    }
                });
                TabKind::Svg { view, _sub: sub }
            }
            FileKind::Mermaid => {
                let raw = fs::read_to_string(path)?;
                let doc_path = path.to_path_buf();
                let view = cx.new(|cx| MermaidView::new(raw, doc_path, window, cx));
                let sub = cx.subscribe_in(&view, window, move |this, _, event, window, cx| {
                    if matches!(event, MermaidViewEvent::Changed) {
                        this.on_code_changed(side, tab_id, window, cx);
                    }
                });
                TabKind::Mermaid { view, _sub: sub }
            }
            FileKind::Office => {
                let meta = fs::metadata(path)?;
                let doc = crate::formats::office::OfficeDoc::load(path)?;
                let office_find_input = cx.new(|cx| {
                    InputState::new(window, cx).placeholder("Find in document…")
                });
                let office_find_sub = cx.subscribe_in(
                    &office_find_input,
                    window,
                    move |this, inp, event, _window, cx| match event {
                        InputEvent::Change => {
                            let val = inp.read(cx).value().to_string();
                            if let Some(t) = this.find_tab_mut(side, tab_id)
                                && let TabKind::Office { find, .. } = &mut t.kind
                            {
                                find.query = val;
                            }
                            this.recompute_office_find(side, tab_id, true, cx);
                            cx.notify();
                        }
                        InputEvent::PressEnter {
                            secondary, shift, ..
                        } => {
                            this.step_office_find(side, tab_id, !(*shift || *secondary), cx);
                        }
                        _ => {}
                    },
                );
                TabKind::Office {
                    doc,
                    paper_light: false,
                    font_scale: 1.0,
                    file_size_bytes: meta.len(),
                    scroll_handle: ScrollHandle::new(),
                    find: OfficeFindState {
                        open: false,
                        query: String::new(),
                        case_sensitive: false,
                        input: office_find_input,
                        _sub: office_find_sub,
                        matches: Vec::new(),
                        active: 0,
                    },
                }
            }
            FileKind::Epub => {
                let meta = fs::metadata(path)?;
                let book = crate::formats::epub::EpubBook::load(path)?;
                let view = cx.new(|cx| EpubView::new(path.to_path_buf(), book, window, cx));
                TabKind::Epub {
                    view,
                    file_size_bytes: meta.len(),
                }
            }
            FileKind::Sqlite => {
                let meta = fs::metadata(path)?;
                let db_path = path.to_path_buf();
                let view = cx.new(|cx| SqliteView::new(db_path, window, cx));
                TabKind::Sqlite {
                    view,
                    file_size_bytes: meta.len(),
                }
            }
            FileKind::Archive => {
                let meta = fs::metadata(path)?;
                let arc_path = path.to_path_buf();
                let view = cx.new(|cx| ArchiveView::new(arc_path, window, cx));
                TabKind::Archive {
                    view,
                    file_size_bytes: meta.len(),
                }
            }
            FileKind::Audio => {
                let meta = fs::metadata(path)?;
                let audio_path = path.to_path_buf();
                let view = cx.new(|cx| AudioView::new(audio_path, window, cx));
                TabKind::Audio {
                    view,
                    file_size_bytes: meta.len(),
                }
            }
            FileKind::Font => {
                let meta = fs::metadata(path)?;
                let info = crate::formats::font::FontInfo::load(path)?;
                let view = cx.new(|cx| FontView::new(info, window, cx));
                TabKind::Font {
                    view,
                    file_size_bytes: meta.len(),
                }
            }
            FileKind::Code(lang) => {
                let raw = fs::read_to_string(path)?;
                let editor = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .searchable(false)
                        .language(lang)
                        .soft_wrap(false)
                        .line_number(true)
                        .default_value(raw)
                });
                let find_bar = cx.new(|cx| EditorFindBar::new(editor.clone(), window, cx));
                let sub = cx.subscribe_in(&editor, window, move |this, _ed, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.on_code_changed(side, tab_id, window, cx);
                    }
                });
                let (tree_filter, tree_filter_sub) =
                    if crate::formats::data_tree::is_structured_lang(lang) {
                        let inp =
                            cx.new(|cx| InputState::new(window, cx).placeholder("Filter keys or values…"));
                        let fsub = cx.subscribe(&inp, |_, _, event, cx| {
                            if matches!(event, InputEvent::Change) {
                                cx.notify();
                            }
                        });
                        (Some(inp), Some(fsub))
                    } else {
                        (None, None)
                    };
                TabKind::Code {
                    editor,
                    find_bar,
                    language: lang,
                    badge: file_kind.short_badge(),
                    soft_wrap: false,
                    data_view: CodeViewMode::Code,
                    collapsed_paths: HashSet::new(),
                    tree_filter,
                    _tree_filter_sub: tree_filter_sub,
                    _sub: sub,
                }
            }
            FileKind::PlainText => {
                let raw = fs::read_to_string(path)?;
                let editor = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .searchable(false)
                        .language("text")
                        .soft_wrap(true)
                        .line_number(true)
                        .default_value(raw)
                });
                let find_bar = cx.new(|cx| EditorFindBar::new(editor.clone(), window, cx));
                let sub = cx.subscribe_in(&editor, window, move |this, _ed, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.on_code_changed(side, tab_id, window, cx);
                    }
                });
                TabKind::Code {
                    editor,
                    find_bar,
                    language: "text",
                    badge: "TXT",
                    soft_wrap: true,
                    data_view: CodeViewMode::Code,
                    collapsed_paths: HashSet::new(),
                    tree_filter: None,
                    _tree_filter_sub: None,
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
        let other = match side {
            PaneSide::Left => PaneSide::Right,
            PaneSide::Right => PaneSide::Left,
        };
        let in_primary = self
            .pane(side)
            .is_some_and(|p| p.tabs.iter().any(|t| t.id == tab_id));
        let target_side = if in_primary { side } else { other };
        self.pane_mut(target_side)?
            .tabs
            .iter_mut()
            .find(|t| t.id == tab_id)
    }

    fn find_tab(&self, side: PaneSide, tab_id: u64) -> Option<&DocumentTab> {
        let other = match side {
            PaneSide::Left => PaneSide::Right,
            PaneSide::Right => PaneSide::Left,
        };
        self.pane(side)
            .and_then(|p| p.tabs.iter().find(|t| t.id == tab_id))
            .or_else(|| {
                self.pane(other)
                    .and_then(|p| p.tabs.iter().find(|t| t.id == tab_id))
            })
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

    fn on_live_doc_changed(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw_opt = self.find_tab(side, tab_id).and_then(|t| match &t.kind {
            TabKind::Markdown {
                live_doc,
                source_mode,
                ..
            } if !*source_mode => Some(SharedString::from(live_doc.read(cx).text().to_string())),
            _ => None,
        });
        if let Some(raw) = raw_opt {
            self.update_markdown_derived_state(side, tab_id, raw, window, cx);
        }
    }

    fn on_markdown_changed(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ed_opt = self.find_tab(side, tab_id).and_then(|t| match &t.kind {
            TabKind::Markdown {
                editor,
                source_mode,
                ..
            } if *source_mode => editor.clone(),
            _ => None,
        });
        if let Some(ed) = ed_opt {
            let raw = ed.read(cx).value();
            self.update_markdown_derived_state(side, tab_id, raw, window, cx);
        }
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
        let _ = window;
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
                TabKind::Markdown {
                    editor,
                    live_doc,
                    source_mode,
                    ..
                } => {
                    if *source_mode
                        && let Some(ed) = editor.as_ref()
                    {
                        let content = ed.read(cx).value();
                        atomic_write_string(&path, content.as_ref())
                    } else {
                        let content = live_doc.read(cx).text();
                        atomic_write_string(&path, content)
                    }
                }
                TabKind::Fountain { editor, .. }
                | TabKind::Typst { editor, .. }
                | TabKind::Code { editor, .. } => {
                    let content = editor.read(cx).value();
                    atomic_write_string(&path, content.as_ref())
                }
                TabKind::Canvas { view, .. } => {
                    let content = view.read(cx).to_json(cx);
                    atomic_write_string(&path, &content)
                }
                TabKind::Kanban { view, .. } => {
                    let content = view.read(cx).to_markdown(cx);
                    atomic_write_string(&path, &content)
                }
                TabKind::Subtitle { view, .. } => {
                    let content = view.read(cx).to_text(cx);
                    atomic_write_string(&path, &content)
                }
                TabKind::Svg { view, .. } => {
                    let content = view.read(cx).value(cx);
                    atomic_write_string(&path, &content)
                }
                TabKind::Mermaid { view, .. } => {
                    let content = view.read(cx).value(cx);
                    atomic_write_string(&path, &content)
                }
                TabKind::Spreadsheet { doc, .. } => doc.save(&path),
                TabKind::Image { .. }
                | TabKind::Pdf { .. }
                | TabKind::Office { .. }
                | TabKind::Epub { .. }
                | TabKind::Sqlite { .. }
                | TabKind::Archive { .. }
                | TabKind::Audio { .. }
                | TabKind::Font { .. } => Ok(()),
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
        trim_allocator();
        cx.notify();
    }

    pub fn move_tab(&mut self, side: PaneSide, tab_idx: usize, delta: isize, cx: &mut Context<Self>) {
        let Some(pane) = self.pane_mut(side) else {
            return;
        };
        let len = pane.tabs.len();
        if tab_idx >= len {
            return;
        }
        let target_isize = tab_idx as isize + delta;
        if target_isize < 0 || target_isize as usize >= len {
            return;
        }
        if reorder_with_active(
            &mut pane.tabs,
            &mut pane.active_tab,
            tab_idx,
            target_isize as usize,
        ) {
            self.persist_workspaces();
            cx.notify();
        }
    }

    pub fn reorder_tab(
        &mut self,
        from_side: PaneSide,
        from_idx: usize,
        to_side: PaneSide,
        to_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if from_side == to_side {
            let Some(pane) = self.pane_mut(from_side) else {
                return;
            };
            if pane.tabs.is_empty() {
                return;
            }
            let clamped_to = to_idx.min(pane.tabs.len() - 1);
            if reorder_with_active(&mut pane.tabs, &mut pane.active_tab, from_idx, clamped_to) {
                self.active_pane = from_side;
                self.persist_workspaces();
                cx.notify();
            }
            return;
        }

        let Some(src_path) = self
            .pane(from_side)
            .and_then(|p| p.tabs.get(from_idx))
            .map(|t| t.path.clone())
        else {
            return;
        };

        if to_side == PaneSide::Right {
            self.ensure_right_pane();
        }

        // Guard duplicate path in target pane: activate existing instead of duplicating.
        if let Some(existing_idx) = self
            .pane(to_side)
            .and_then(|p| p.tabs.iter().position(|t| t.path == src_path))
        {
            self.activate_tab(to_side, existing_idx, window, cx);
            return;
        }

        let moved = {
            let (from_pane, to_pane) = match (from_side, to_side) {
                (PaneSide::Left, PaneSide::Right) => {
                    let Some(rp) = self.right_pane.as_mut() else {
                        return;
                    };
                    (&mut self.left_pane, rp)
                }
                (PaneSide::Right, PaneSide::Left) => {
                    let Some(rp) = self.right_pane.as_mut() else {
                        return;
                    };
                    (rp, &mut self.left_pane)
                }
                _ => return,
            };
            move_tab_between_vecs(
                &mut from_pane.tabs,
                &mut from_pane.active_tab,
                from_idx,
                &mut to_pane.tabs,
                &mut to_pane.active_tab,
                to_idx,
            )
        };
        if !moved {
            return;
        }

        if from_side == PaneSide::Right
            && self.right_pane.as_ref().is_some_and(|p| p.tabs.is_empty())
        {
            self.right_pane = None;
        }
        self.active_pane = to_side;
        if let Some(new_idx) = self.pane(to_side).map(|p| p.active_tab) {
            self.focus_tab_widget(to_side, new_idx, window, cx);
        }
        self.persist_workspaces();
        cx.notify();
    }

    pub fn move_tab_to_other_pane(
        &mut self,
        from_side: PaneSide,
        tab_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let other = match from_side {
            PaneSide::Left => PaneSide::Right,
            PaneSide::Right => PaneSide::Left,
        };
        let target_end = self.pane(other).map(|p| p.tabs.len()).unwrap_or(0);
        self.reorder_tab(from_side, tab_idx, other, target_end, window, cx);
    }

    pub fn close_other_tabs(&mut self, side: PaneSide, keep_idx: usize, cx: &mut Context<Self>) {
        let Some(pane) = self.pane_mut(side) else {
            return;
        };
        if keep_idx >= pane.tabs.len() || pane.tabs.len() <= 1 {
            return;
        }
        let mut idx = 0usize;
        pane.tabs.retain(|_| {
            let keep = idx == keep_idx;
            idx += 1;
            keep
        });
        pane.active_tab = 0;
        self.persist_workspaces();
        trim_allocator();
        cx.notify();
    }

    pub fn close_tabs_to_right(&mut self, side: PaneSide, keep_idx: usize, cx: &mut Context<Self>) {
        let Some(pane) = self.pane_mut(side) else {
            return;
        };
        if keep_idx + 1 >= pane.tabs.len() {
            return;
        }
        pane.tabs.truncate(keep_idx + 1);
        pane.active_tab = pane.active_tab.min(pane.tabs.len().saturating_sub(1));
        self.persist_workspaces();
        trim_allocator();
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

    pub fn toggle_md_view_mode(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let side = self.active_pane;
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
        let info = {
            let Some(tab) = self.pane_mut(side).and_then(WorkspacePane::active_mut) else {
                return;
            };
            let tab_id = tab.id;
            let TabKind::Markdown {
                editor,
                live_doc,
                source_mode,
                ..
            } = &mut tab.kind
            else {
                return;
            };
            *source_mode = !*source_mode;
            (*source_mode, tab_id, editor.clone(), live_doc.clone())
        };
        let (sm, tab_id, existing_ed, live_doc) = info;

        if sm {
            let (raw, line, col) = {
                let ld = live_doc.read(cx);
                let (l, c) = ld.cursor_line_col();
                (ld.text().to_string(), l, c)
            };
            let pos = gpui_kit::component::input::Position::new(line as u32, col as u32);
            if let Some(editor) = existing_ed {
                editor.update(cx, |s, cx| {
                    s.set_value(raw, window, cx);
                    s.set_line_number(true, window, cx);
                    s.set_editor_paddings(gpui_kit::Edges {
                        top: px(10.),
                        right: px(16.),
                        bottom: px(24.),
                        left: px(12.),
                    });
                    s.set_cursor_position(pos, window, cx);
                    s.focus(window, cx);
                });
            } else {
                let (editor, editor_find, sub) =
                    Self::create_md_source_editor(side, tab_id, raw, true, window, cx);
                editor.update(cx, |s, cx| {
                    s.set_cursor_position(pos, window, cx);
                    s.focus(window, cx);
                });
                if let Some(tab) = self.find_tab_mut(side, tab_id)
                    && let TabKind::Markdown {
                        editor: slot,
                        editor_find: slot_find,
                        _sub,
                        ..
                    } = &mut tab.kind
                {
                    *slot = Some(editor);
                    *slot_find = Some(editor_find);
                    *_sub = Some(sub);
                }
            }
        } else {
            if let Some(editor) = existing_ed {
                let (raw, cursor_line, cursor_col) = {
                    let ed = editor.read(cx);
                    let pos = ed.cursor_position();
                    (ed.value().to_string(), pos.line as usize, pos.character as usize)
                };
                live_doc.update(cx, |ld, cx| {
                    ld.set_text(raw, cursor_line, cursor_col, cx);
                });
            }
            if let Some(tab) = self.find_tab_mut(side, tab_id)
                && let TabKind::Markdown {
                    editor: slot,
                    editor_find: slot_find,
                    _sub,
                    ..
                } = &mut tab.kind
            {
                *slot = None;
                *slot_find = None;
                *_sub = None;
            }
            live_doc.focus_handle(cx).focus(window, cx);
            trim_allocator();
        }
        cx.notify();
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

            // Create missing wikilink target cleanly with extension awareness
            let mut clean = target
                .split('|')
                .next()
                .unwrap_or(&target)
                .split('#')
                .next()
                .unwrap_or(&target)
                .trim();
            while let Some(rest) = clean.strip_prefix("./").or_else(|| clean.strip_prefix(".\\")) {
                clean = rest;
            }
            if !clean.is_empty() {
                let rel_name = crate::vault::normalize_new_file_name(clean);
                if !rel_name.is_empty() {
                    let dest = current_file
                        .parent()
                        .unwrap_or(&self.workspace_root)
                        .join(&rel_name);
                    if crate::vault::create_adaptive_file(&dest).is_ok() {
                        self.refresh_vault_index(cx);
                        self.open_file(dest, side, window, cx);
                    }
                }
            }
            return;
        }

        if url_str.starts_with("http://") || url_str.starts_with("https://") {
            let _ = Command::new("xdg-open").arg(url_str).spawn();
        } else if !url_str.starts_with("mailto:") && !url_str.starts_with('#') {
            let current_file = self
                .find_tab(side, tab_id)
                .map(|t| t.path.clone())
                .unwrap_or_else(|| self.workspace_root.join("note.md"));
            if let Some(resolved) = self.vault.resolve_wikilink(url_str, &current_file) {
                self.open_file(resolved, side, window, cx);
            }
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
            live_doc,
            source_mode,
            preprocessed,
            ..
        } = &tab.kind
        {
            let raw: SharedString = if *source_mode
                && let Some(ed) = editor.as_ref()
            {
                ed.read(cx).value()
            } else {
                live_doc.read(cx).text().to_string().into()
            };
            if let Some(updated) = toggle_nth_task_checkbox(
                raw.as_ref(),
                preprocessed.as_ref(),
                event.marker_range().start,
                event.checked(),
            ) {
                let ed = editor.clone();
                let ld = live_doc.clone();
                let is_source = *source_mode;
                if is_source
                    && let Some(ed_ent) = ed
                {
                    ed_ent.update(cx, |s, cx| s.set_value(updated.clone(), window, cx));
                } else {
                    ld.update(cx, |s, cx| {
                        let (l, c) = s.cursor_line_col();
                        s.set_text(updated.clone(), l, c, cx);
                    });
                }
                self.update_markdown_derived_state(side, tab_id, updated.into(), window, cx);
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
            && let TabKind::Markdown {
                editor: Some(editor),
                ..
            } = &tab.kind
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
            && let TabKind::Markdown { live_doc, .. } = &tab.kind
        {
            let embed = format!("![[{filename}]]");
            live_doc.clone().update(cx, |ed, cx| {
                ed.insert_text_at_caret(&embed, cx);
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

    pub fn trigger_find_in_active_tab(
        &mut self,
        replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let side = self.active_pane;
        let Some(tab) = self.pane(side).and_then(WorkspacePane::active) else {
            return;
        };
        let tab_id = tab.id;
        let zen_mode = self.zen_mode;

        enum FindAction {
            None,
            MdLive(Entity<LiveMarkdownEditor>),
            MdRead,
            MdSwitchToEditLive(Entity<LiveMarkdownEditor>),
            MdSwitchToEditSource(Entity<EditorFindBar>),
            OpenFindBar(Entity<EditorFindBar>),
            FocusInput(Entity<InputState>),
            SwitchTypstAndFind(Entity<EditorFindBar>),
            SwitchFountainAndFind(Entity<EditorFindBar>),
            SvgView(Entity<SvgView>),
            MermaidView(Entity<MermaidView>),
            KanbanView(Entity<KanbanView>),
            CanvasView(Entity<CanvasView>),
            SubtitleView(Entity<SubtitleView>),
            EpubView(Entity<EpubView>),
            SqliteView(Entity<SqliteView>),
            ArchiveView(Entity<ArchiveView>),
            FontView(Entity<FontView>),
            Spreadsheet,
            Office,
        }

        let action = match &tab.kind {
            TabKind::Markdown {
                editor_find,
                live_doc,
                view_mode,
                source_mode,
                ..
            } => {
                let eff = if zen_mode && *view_mode == MdViewMode::Split {
                    MdViewMode::Edit
                } else {
                    *view_mode
                };
                if eff == MdViewMode::Reading {
                    if replace {
                        if *source_mode
                            && let Some(fb) = editor_find.as_ref()
                        {
                            FindAction::MdSwitchToEditSource(fb.clone())
                        } else {
                            FindAction::MdSwitchToEditLive(live_doc.clone())
                        }
                    } else {
                        FindAction::MdRead
                    }
                } else if *source_mode
                    && let Some(fb) = editor_find.as_ref()
                {
                    FindAction::OpenFindBar(fb.clone())
                } else {
                    FindAction::MdLive(live_doc.clone())
                }
            }
            TabKind::Code {
                find_bar,
                data_view,
                tree_filter,
                ..
            } => {
                if !replace
                    && *data_view == CodeViewMode::Tree
                    && let Some(tf) = tree_filter.as_ref()
                {
                    FindAction::FocusInput(tf.clone())
                } else {
                    FindAction::OpenFindBar(find_bar.clone())
                }
            }
            TabKind::Typst {
                find_bar,
                view_mode,
                ..
            } => {
                if *view_mode == TypstViewMode::Preview {
                    FindAction::SwitchTypstAndFind(find_bar.clone())
                } else {
                    FindAction::OpenFindBar(find_bar.clone())
                }
            }
            TabKind::Fountain {
                find_bar,
                view_mode,
                ..
            } => {
                if *view_mode == FountainViewMode::Script {
                    FindAction::SwitchFountainAndFind(find_bar.clone())
                } else {
                    FindAction::OpenFindBar(find_bar.clone())
                }
            }
            TabKind::Svg { view, .. } => FindAction::SvgView(view.clone()),
            TabKind::Mermaid { view, .. } => FindAction::MermaidView(view.clone()),
            TabKind::Kanban { view, .. } => FindAction::KanbanView(view.clone()),
            TabKind::Canvas { view, .. } => FindAction::CanvasView(view.clone()),
            TabKind::Subtitle { view, .. } => FindAction::SubtitleView(view.clone()),
            TabKind::Epub { view, .. } => FindAction::EpubView(view.clone()),
            TabKind::Sqlite { view, .. } => FindAction::SqliteView(view.clone()),
            TabKind::Archive { view, .. } => FindAction::ArchiveView(view.clone()),
            TabKind::Font { view, .. } => FindAction::FontView(view.clone()),
            TabKind::Spreadsheet { .. } => FindAction::Spreadsheet,
            TabKind::Office { .. } => FindAction::Office,
            TabKind::Image { .. } | TabKind::Pdf { .. } | TabKind::Audio { .. } => FindAction::None,
        };

        match action {
            FindAction::None => {}
            FindAction::MdLive(live_doc) => {
                live_doc.update(cx, |ed, cx| ed.open_search(replace, window, cx));
            }
            FindAction::MdRead => {
                self.open_md_read_find(side, tab_id, window, cx);
            }
            FindAction::MdSwitchToEditLive(live_doc) => {
                if let Some(t) = self.find_tab_mut(side, tab_id)
                    && let TabKind::Markdown {
                        view_mode,
                        read_find,
                        ..
                    } = &mut t.kind
                {
                    *view_mode = MdViewMode::Edit;
                    read_find.open = false;
                }
                live_doc.update(cx, |ed, cx| ed.open_search(true, window, cx));
                cx.notify();
            }
            FindAction::MdSwitchToEditSource(fb) => {
                if let Some(t) = self.find_tab_mut(side, tab_id)
                    && let TabKind::Markdown {
                        view_mode,
                        read_find,
                        ..
                    } = &mut t.kind
                {
                    *view_mode = MdViewMode::Edit;
                    read_find.open = false;
                }
                fb.update(cx, |b, cx| b.open(true, window, cx));
                cx.notify();
            }
            FindAction::OpenFindBar(fb) => {
                if let Some(t) = self.find_tab_mut(side, tab_id)
                    && let TabKind::Code { data_view, .. } = &mut t.kind
                    && *data_view != CodeViewMode::Code
                {
                    *data_view = CodeViewMode::Code;
                }
                fb.update(cx, |b, cx| b.open(replace, window, cx));
                cx.notify();
            }
            FindAction::FocusInput(inp) => {
                inp.update(cx, |s, cx| {
                    let len = s.value().len();
                    s.focus(window, cx);
                    if len > 0 {
                        s.set_selected_range(0..len, cx);
                    }
                });
                cx.notify();
            }
            FindAction::SwitchTypstAndFind(fb) => {
                if let Some(t) = self.find_tab_mut(side, tab_id)
                    && let TabKind::Typst { view_mode, .. } = &mut t.kind
                {
                    *view_mode = TypstViewMode::Split;
                }
                fb.update(cx, |b, cx| b.open(replace, window, cx));
                cx.notify();
            }
            FindAction::SwitchFountainAndFind(fb) => {
                if let Some(t) = self.find_tab_mut(side, tab_id)
                    && let TabKind::Fountain { view_mode, .. } = &mut t.kind
                {
                    *view_mode = FountainViewMode::Split;
                }
                fb.update(cx, |b, cx| b.open(replace, window, cx));
                cx.notify();
            }
            FindAction::SvgView(v) => v.update(cx, |v, cx| v.open_find(replace, window, cx)),
            FindAction::MermaidView(v) => v.update(cx, |v, cx| v.open_find(replace, window, cx)),
            FindAction::KanbanView(v) => v.update(cx, |v, cx| v.open_find(replace, window, cx)),
            FindAction::CanvasView(v) => v.update(cx, |v, cx| v.open_find(replace, window, cx)),
            FindAction::SubtitleView(v) => v.update(cx, |v, cx| v.open_find(replace, window, cx)),
            FindAction::EpubView(v) => v.update(cx, |v, cx| v.open_find(window, cx)),
            FindAction::SqliteView(v) => v.update(cx, |v, cx| v.focus_filter(window, cx)),
            FindAction::ArchiveView(v) => v.update(cx, |v, cx| v.focus_filter(window, cx)),
            FindAction::FontView(v) => v.update(cx, |v, cx| v.focus_filter(window, cx)),
            FindAction::Spreadsheet => self.open_spreadsheet_find(side, tab_id, window, cx),
            FindAction::Office => self.open_office_find(side, tab_id, window, cx),
        }
    }

    fn close_active_tab_find(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let side = self.active_pane;
        let Some(tab) = self.pane(side).and_then(WorkspacePane::active) else {
            return false;
        };
        let tab_id = tab.id;
        match &tab.kind {
            TabKind::Markdown {
                editor_find,
                live_doc,
                view_mode,
                source_mode,
                read_find,
                ..
            } => {
                let eff = if self.zen_mode && *view_mode == MdViewMode::Split {
                    MdViewMode::Edit
                } else {
                    *view_mode
                };
                if eff == MdViewMode::Reading && read_find.open {
                    self.close_md_read_find(side, tab_id, Some(window), cx);
                    return true;
                }
                if *source_mode
                    && let Some(fb) = editor_find.as_ref()
                    && fb.read(cx).is_open()
                {
                    fb.clone().update(cx, |b, cx| b.close(Some(window), cx));
                    return true;
                }
                if !*source_mode && live_doc.read(cx).is_search_open() {
                    live_doc.clone().update(cx, |ed, cx| ed.close_search(window, cx));
                    return true;
                }
                false
            }
            TabKind::Code { find_bar, .. }
            | TabKind::Typst { find_bar, .. }
            | TabKind::Fountain { find_bar, .. }
                if find_bar.read(cx).is_open() =>
            {
                find_bar.clone().update(cx, |b, cx| b.close(Some(window), cx));
                true
            }
            TabKind::Svg { view, .. } if view.read(cx).is_find_open(cx) => {
                view.clone().update(cx, |v, cx| v.close_find(Some(window), cx));
                true
            }
            TabKind::Mermaid { view, .. } if view.read(cx).is_find_open(cx) => {
                view.clone().update(cx, |v, cx| v.close_find(Some(window), cx));
                true
            }
            TabKind::Subtitle { view, .. } if view.read(cx).is_find_open(cx) => {
                view.clone().update(cx, |v, cx| v.close_find(Some(window), cx));
                true
            }
            TabKind::Spreadsheet { find, .. } if find.open => {
                self.close_spreadsheet_find(side, tab_id, Some(window), cx);
                true
            }
            TabKind::Office { find, .. } if find.open => {
                self.close_office_find(side, tab_id, Some(window), cx);
                true
            }
            TabKind::Epub { view, .. } if view.read(cx).is_find_open() => {
                view.clone().update(cx, |v, cx| v.close_find(Some(window), cx));
                true
            }
            TabKind::Kanban { view, .. } if view.read(cx).is_find_open(cx) => {
                view.clone().update(cx, |v, cx| v.close_find(Some(window), cx));
                true
            }
            TabKind::Canvas { view, .. } if view.read(cx).is_find_open(cx) => {
                view.clone().update(cx, |v, cx| v.close_find(Some(window), cx));
                true
            }
            _ => false,
        }
    }

    pub fn open_md_read_find(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inp = {
            let Some(t) = self.find_tab_mut(side, tab_id) else {
                return;
            };
            let TabKind::Markdown { read_find, .. } = &mut t.kind else {
                return;
            };
            read_find.open = true;
            read_find.input.clone()
        };
        let q = inp.read(cx).value().to_string();
        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Markdown { read_find, .. } = &mut t.kind
        {
            read_find.query = q;
        }
        self.recompute_md_read_find(side, tab_id, false, cx);
        inp.update(cx, |s, cx| {
            let len = s.value().len();
            s.focus(window, cx);
            if len > 0 {
                s.set_selected_range(0..len, cx);
            }
        });
        cx.notify();
    }

    pub fn close_md_read_find(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Markdown { read_find, .. } = &mut t.kind
        {
            read_find.open = false;
        }
        if let Some(window) = window {
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    pub fn toggle_md_read_find_case(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Markdown { read_find, .. } = &mut t.kind
        {
            read_find.case_sensitive = !read_find.case_sensitive;
        }
        self.recompute_md_read_find(side, tab_id, true, cx);
        cx.notify();
    }

    pub fn clear_md_read_find_query(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Markdown { read_find, .. } = &mut t.kind
        {
            read_find.query.clear();
            read_find.matches.clear();
            read_find.active = 0;
        }
        cx.notify();
    }

    fn scroll_md_read_to_active(
        raw: &str,
        matches: &[ReadFindMatch],
        active: usize,
        read_scroll: &ScrollHandle,
    ) {
        let Some(hit) = matches.get(active) else {
            return;
        };
        let total_lines = raw.lines().count().max(1);
        let max_y = f32::from(read_scroll.max_offset().y);
        if max_y > 0.0 {
            let frac = (hit.line.saturating_sub(1) as f32 / total_lines as f32).clamp(0.0, 1.0);
            let target_y = (frac * max_y - 140.0).clamp(0.0, max_y);
            read_scroll.set_offset(point(px(0.), px(-target_y)));
        }
    }

    pub fn recompute_md_read_find(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        jump: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(t) = self.find_tab(side, tab_id) else {
            return;
        };
        let TabKind::Markdown {
            editor,
            live_doc,
            source_mode,
            read_find,
            ..
        } = &t.kind
        else {
            return;
        };
        let raw = if *source_mode
            && let Some(ed) = editor.as_ref()
        {
            ed.read(cx).value().to_string()
        } else {
            live_doc.read(cx).text().to_string()
        };
        let trimmed = read_find.query.trim().to_string();
        let case_sensitive = read_find.case_sensitive;
        let mut matches = Vec::new();
        if !trimmed.is_empty() {
            for (line_idx, line) in raw.lines().enumerate() {
                for range in find_all_byte_ranges(line, &trimmed, case_sensitive) {
                    let snip = line.trim();
                    let snippet = if snip.chars().count() > 80 {
                        let mut s: String = snip.chars().take(77).collect();
                        s.push('…');
                        s
                    } else {
                        snip.to_string()
                    };
                    matches.push(ReadFindMatch {
                        line: line_idx + 1,
                        range,
                        snippet,
                    });
                }
            }
        }

        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Markdown {
                read_find,
                read_scroll,
                ..
            } = &mut t.kind
        {
            read_find.matches = matches;
            if read_find.matches.is_empty() {
                read_find.active = 0;
            } else {
                if jump || read_find.active >= read_find.matches.len() {
                    read_find.active = 0;
                }
                if jump {
                    Self::scroll_md_read_to_active(
                        &raw,
                        &read_find.matches,
                        read_find.active,
                        read_scroll,
                    );
                }
            }
        }
    }

    pub fn step_md_read_find(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        forward: bool,
        cx: &mut Context<Self>,
    ) {
        let raw = {
            let Some(t) = self.find_tab(side, tab_id) else {
                return;
            };
            let TabKind::Markdown {
                editor,
                live_doc,
                source_mode,
                ..
            } = &t.kind
            else {
                return;
            };
            if *source_mode
                && let Some(ed) = editor.as_ref()
            {
                ed.read(cx).value().to_string()
            } else {
                live_doc.read(cx).text().to_string()
            }
        };
        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Markdown {
                read_find,
                read_scroll,
                ..
            } = &mut t.kind
        {
            let len = read_find.matches.len();
            if len == 0 {
                return;
            }
            read_find.active = if forward {
                (read_find.active + 1) % len
            } else {
                (read_find.active + len - 1) % len
            };
            Self::scroll_md_read_to_active(&raw, &read_find.matches, read_find.active, read_scroll);
        }
        cx.notify();
    }

    pub fn jump_md_read_match_to_edit(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hit_and_mode = {
            let Some(t) = self.find_tab_mut(side, tab_id) else {
                return;
            };
            let TabKind::Markdown {
                editor,
                live_doc,
                view_mode,
                source_mode,
                read_find,
                ..
            } = &mut t.kind
            else {
                return;
            };
            let Some(hit) = read_find.matches.get(read_find.active).cloned() else {
                return;
            };
            read_find.open = false;
            *view_mode = MdViewMode::Edit;
            (hit, *source_mode, editor.clone(), live_doc.clone())
        };
        let (hit, source_mode, editor_opt, live_doc) = hit_and_mode;
        let line_0 = hit.line.saturating_sub(1);
        if source_mode
            && let Some(ed) = editor_opt
        {
            let raw = ed.read(cx).value().to_string();
            let col_chars = raw
                .lines()
                .nth(line_0)
                .map(|l| {
                    let end = hit.range.start.min(l.len());
                    l[..end].chars().count()
                })
                .unwrap_or(0);
            let pos = gpui_kit::component::input::Position::new(line_0 as u32, col_chars as u32);
            ed.focus_handle(cx).focus(window, cx);
            ed.update(cx, |s, cx| s.set_cursor_position(pos, window, cx));
        } else {
            live_doc.update(cx, |ed, cx| ed.jump_to_line(line_0, window, cx));
        }
        cx.notify();
    }

    pub fn open_spreadsheet_find(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inp = {
            let Some(t) = self.find_tab_mut(side, tab_id) else {
                return;
            };
            let TabKind::Spreadsheet { find, .. } = &mut t.kind else {
                return;
            };
            find.open = true;
            find.input.clone()
        };
        let q = inp.read(cx).value().to_string();
        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Spreadsheet { find, .. } = &mut t.kind
        {
            find.query = q;
        }
        self.recompute_spreadsheet_find(side, tab_id, false, cx);
        inp.update(cx, |s, cx| {
            let len = s.value().len();
            s.focus(window, cx);
            if len > 0 {
                s.set_selected_range(0..len, cx);
            }
        });
        cx.notify();
    }

    pub fn close_spreadsheet_find(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let gf = if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Spreadsheet {
                find, grid_focus, ..
            } = &mut t.kind
        {
            find.open = false;
            Some(grid_focus.clone())
        } else {
            None
        };
        if let (Some(window), Some(gf)) = (window, gf) {
            gf.focus(window, cx);
        }
        cx.notify();
    }

    pub fn toggle_spreadsheet_find_case(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Spreadsheet { find, .. } = &mut t.kind
        {
            find.case_sensitive = !find.case_sensitive;
        }
        self.recompute_spreadsheet_find(side, tab_id, true, cx);
        cx.notify();
    }

    pub fn clear_spreadsheet_find_query(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Spreadsheet { find, .. } = &mut t.kind
        {
            find.query.clear();
            find.matches.clear();
            find.active = 0;
        }
        cx.notify();
    }

    fn jump_spreadsheet_find_active(
        tab: &mut DocumentTab,
        cx: &mut App,
    ) {
        let TabKind::Spreadsheet {
            doc,
            active_sheet,
            active_row,
            active_col,
            formula_input,
            row_scroll,
            find,
            ..
        } = &mut tab.kind
        else {
            return;
        };
        let Some(&(s_ix, r, c)) = find.matches.get(find.active) else {
            return;
        };
        *active_sheet = s_ix;
        *active_row = r.max(1);
        *active_col = c.max(1);
        let raw = doc.raw_input(s_ix, *active_row, *active_col);
        let fi = formula_input.clone();
        // Keep find input focused; update formula bar value passively without window focus steal
        let _ = fi;
        let _ = raw;
        let _ = cx;
        row_scroll.scroll_to_item((*active_row - 1).max(0) as usize, ScrollStrategy::Center);
    }

    pub fn recompute_spreadsheet_find(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        jump: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.find_tab_mut(side, tab_id) {
            let formula_update = {
                let TabKind::Spreadsheet {
                    doc,
                    active_sheet,
                    find,
                    ..
                } = &mut t.kind
                else {
                    return;
                };
                let trimmed = find.query.trim().to_string();
                let case_sensitive = find.case_sensitive;
                let cur_sheet = *active_sheet;
                let mut matches = Vec::new();
                if !trimmed.is_empty() {
                    let sheet_count = doc.sheet_names().len() as u32;
                    for s_ix in 0..sheet_count {
                        let (max_r, max_c) = doc.sheet_bounds(s_ix);
                        for r in 1..=max_r {
                            for c in 1..=max_c {
                                let disp = doc.display_value(s_ix, r, c);
                                let mut matched =
                                    !find_all_byte_ranges(&disp, &trimmed, case_sensitive)
                                        .is_empty();
                                if !matched {
                                    let raw = doc.raw_input(s_ix, r, c);
                                    if !raw.is_empty() && raw != disp {
                                        matched = !find_all_byte_ranges(
                                            &raw,
                                            &trimmed,
                                            case_sensitive,
                                        )
                                        .is_empty();
                                    }
                                }
                                if matched {
                                    matches.push((s_ix, r, c));
                                }
                            }
                        }
                    }
                }
                find.matches = matches;
                if find.matches.is_empty() {
                    find.active = 0;
                    false
                } else {
                    if jump {
                        find.active = find
                            .matches
                            .iter()
                            .position(|(s, _, _)| *s >= cur_sheet)
                            .unwrap_or(0);
                    } else if find.active >= find.matches.len() {
                        find.active = 0;
                    }
                    jump
                }
            };
            if formula_update {
                Self::jump_spreadsheet_find_active(t, cx);
            }
        }
    }

    pub fn step_spreadsheet_find(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        forward: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.find_tab_mut(side, tab_id) {
            if let TabKind::Spreadsheet { find, .. } = &mut t.kind {
                let len = find.matches.len();
                if len == 0 {
                    return;
                }
                find.active = if forward {
                    (find.active + 1) % len
                } else {
                    (find.active + len - 1) % len
                };
            }
            Self::jump_spreadsheet_find_active(t, cx);
        }
        cx.notify();
    }

    pub fn open_office_find(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inp = {
            let Some(t) = self.find_tab_mut(side, tab_id) else {
                return;
            };
            let TabKind::Office { find, .. } = &mut t.kind else {
                return;
            };
            find.open = true;
            find.input.clone()
        };
        let q = inp.read(cx).value().to_string();
        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Office { find, .. } = &mut t.kind
        {
            find.query = q;
        }
        self.recompute_office_find(side, tab_id, false, cx);
        inp.update(cx, |s, cx| {
            let len = s.value().len();
            s.focus(window, cx);
            if len > 0 {
                s.set_selected_range(0..len, cx);
            }
        });
        cx.notify();
    }

    pub fn close_office_find(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Office { find, .. } = &mut t.kind
        {
            find.open = false;
        }
        if let Some(window) = window {
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    pub fn toggle_office_find_case(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Office { find, .. } = &mut t.kind
        {
            find.case_sensitive = !find.case_sensitive;
        }
        self.recompute_office_find(side, tab_id, true, cx);
        cx.notify();
    }

    pub fn clear_office_find_query(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Office { find, .. } = &mut t.kind
        {
            find.query.clear();
            find.matches.clear();
            find.active = 0;
        }
        cx.notify();
    }

    fn scroll_office_to_active(
        total_blocks: usize,
        matches: &[OfficeFindMatch],
        active: usize,
        scroll_handle: &ScrollHandle,
    ) {
        let Some(hit) = matches.get(active) else {
            return;
        };
        let max_y = f32::from(scroll_handle.max_offset().y);
        if max_y > 0.0 {
            let frac = (hit.block_ix as f32 / total_blocks.max(1) as f32).clamp(0.0, 1.0);
            let target_y = (frac * max_y - 140.0).clamp(0.0, max_y);
            scroll_handle.set_offset(point(px(0.), px(-target_y)));
        }
    }

    pub fn recompute_office_find(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        jump: bool,
        _cx: &mut Context<Self>,
    ) {
        let Some(t) = self.find_tab_mut(side, tab_id) else {
            return;
        };
        let TabKind::Office {
            doc,
            scroll_handle,
            find,
            ..
        } = &mut t.kind
        else {
            return;
        };
        let trimmed = find.query.trim().to_string();
        let case_sensitive = find.case_sensitive;
        let mut matches = Vec::new();
        if !trimmed.is_empty() {
            for (blk_ix, blk) in doc.blocks.iter().enumerate() {
                match blk {
                    crate::formats::office::OfficeBlock::Heading { text, .. } => {
                        for range in find_all_byte_ranges(text, &trimmed, case_sensitive) {
                            matches.push(OfficeFindMatch {
                                block_ix: blk_ix,
                                row_ix: 0,
                                col_ix: 0,
                                range,
                            });
                        }
                    }
                    crate::formats::office::OfficeBlock::Paragraph { runs, list_prefix } => {
                        let text =
                            office_view::office_paragraph_text(runs, list_prefix.as_deref());
                        for range in find_all_byte_ranges(&text, &trimmed, case_sensitive) {
                            matches.push(OfficeFindMatch {
                                block_ix: blk_ix,
                                row_ix: 0,
                                col_ix: 0,
                                range,
                            });
                        }
                    }
                    crate::formats::office::OfficeBlock::Table { rows } => {
                        for (rix, row) in rows.iter().enumerate() {
                            for (cix, cell) in row.iter().enumerate() {
                                for range in find_all_byte_ranges(cell, &trimmed, case_sensitive) {
                                    matches.push(OfficeFindMatch {
                                        block_ix: blk_ix,
                                        row_ix: rix,
                                        col_ix: cix,
                                        range,
                                    });
                                }
                            }
                        }
                    }
                    crate::formats::office::OfficeBlock::Image { .. } => {}
                }
            }
        }
        find.matches = matches;
        if find.matches.is_empty() {
            find.active = 0;
        } else {
            if jump || find.active >= find.matches.len() {
                find.active = 0;
            }
            if jump {
                Self::scroll_office_to_active(
                    doc.blocks.len(),
                    &find.matches,
                    find.active,
                    scroll_handle,
                );
            }
        }
    }

    pub fn step_office_find(
        &mut self,
        side: PaneSide,
        tab_id: u64,
        forward: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.find_tab_mut(side, tab_id)
            && let TabKind::Office {
                doc,
                scroll_handle,
                find,
                ..
            } = &mut t.kind
        {
            let len = find.matches.len();
            if len == 0 {
                return;
            }
            find.active = if forward {
                (find.active + 1) % len
            } else {
                (find.active + len - 1) % len
            };
            Self::scroll_office_to_active(
                doc.blocks.len(),
                &find.matches,
                find.active,
                scroll_handle,
            );
        }
        cx.notify();
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
        let line_0based = line_1based.saturating_sub(1);
        let pos = gpui_kit::component::input::Position::new(line_0based as u32, 0);
        match &tab.kind {
            TabKind::Markdown {
                editor,
                live_doc,
                view_mode,
                source_mode,
                ..
            } => {
                if !*source_mode && *view_mode != MdViewMode::Reading {
                    live_doc.clone().update(cx, |ed, cx| {
                        ed.jump_to_line(line_0based, window, cx);
                    });
                } else if let Some(ed) = editor {
                    ed.clone().update(cx, |state, cx| {
                        state.set_cursor_position(pos, window, cx);
                    });
                } else {
                    live_doc.clone().update(cx, |ed, cx| {
                        ed.jump_to_line(line_0based, window, cx);
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
            TabKind::Epub { view, .. } => {
                view.clone().update(cx, |v, cx| {
                    v.go_to_chapter(line_0based, cx);
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

    pub fn enter_zen_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.zen_mode {
            return;
        }
        self.zen_mode = true;
        if !window.is_fullscreen() {
            window.toggle_fullscreen();
            self.zen_entered_fullscreen = true;
        } else {
            self.zen_entered_fullscreen = false;
        }
        cx.notify();
    }

    pub fn exit_zen_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.zen_mode {
            return;
        }
        self.zen_mode = false;
        if self.zen_entered_fullscreen && window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        self.zen_entered_fullscreen = false;
        cx.notify();
    }

    pub fn toggle_zen_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.zen_mode {
            self.exit_zen_mode(window, cx);
        } else {
            self.enter_zen_mode(window, cx);
        }
    }

    fn apply_path_relocation(&mut self, old_path: &Path, new_path: &Path, cx: &mut Context<Self>) {
        for tab in self
            .left_pane
            .tabs
            .iter_mut()
            .chain(self.right_pane.iter_mut().flat_map(|p| p.tabs.iter_mut()))
        {
            if let Some(updated) = relocate_path(&tab.path, old_path, new_path) {
                tab.path = updated.clone();
                tab.title = updated
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Untitled")
                    .to_string();
                tab.rel_path = updated
                    .strip_prefix(&self.workspace_root)
                    .unwrap_or(&updated)
                    .to_string_lossy()
                    .to_string();
                if let TabKind::Markdown { live_doc, .. } = &tab.kind {
                    live_doc.update(cx, |ed, _| ed.set_doc_path(updated));
                } else if let TabKind::Canvas { view, .. } = &tab.kind {
                    let vault = Arc::clone(&self.vault);
                    view.update(cx, |v, _| v.relocate_doc(updated, vault));
                } else if let TabKind::Svg { view, .. } = &tab.kind {
                    view.update(cx, |v, _| v.relocate_doc(updated));
                } else if let TabKind::Mermaid { view, .. } = &tab.kind {
                    view.update(cx, |v, _| v.relocate_doc(updated));
                } else if let TabKind::Sqlite { view, .. } = &tab.kind {
                    view.update(cx, |v, _| v.relocate_db(updated));
                } else if let TabKind::Archive { view, .. } = &tab.kind {
                    view.update(cx, |v, _| v.relocate_archive(updated));
                } else if let TabKind::Audio { view, .. } = &tab.kind {
                    view.update(cx, |v, _| v.relocate_audio(updated));
                } else if let TabKind::Epub { view, .. } = &tab.kind {
                    view.update(cx, |v, _| v.relocate_book(updated));
                }
            }
        }

        let prev_expanded: Vec<PathBuf> = self.expanded_dirs.drain().collect();
        for dir in prev_expanded {
            if let Some(updated) = relocate_path(&dir, old_path, new_path) {
                self.expanded_dirs.insert(updated);
            } else {
                self.expanded_dirs.insert(dir);
            }
        }

        if let Some(sel) = &self.selected_tree_path
            && let Some(updated) = relocate_path(sel, old_path, new_path)
        {
            self.selected_tree_path = Some(updated);
        }

        for entry in &mut self.nav_history.entries {
            if let Some(updated) = relocate_path(&entry.path, old_path, new_path) {
                entry.path = updated;
            }
        }

        self.persist_workspaces();
        self.refresh_vault_index(cx);
    }

    pub fn start_inline_rename(
        &mut self,
        target: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !target.exists() {
            return;
        }
        self.config.sidebar_visible = true;
        if !self.search_input.read(cx).value().is_empty() {
            self.search_input
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.search_hits.clear();
        }
        self.selected_tree_path = Some(target.clone());

        let mut cur = target.parent();
        while let Some(dir) = cur {
            if !dir.starts_with(&self.workspace_root) || dir == self.workspace_root.as_path() {
                break;
            }
            self.expanded_dirs.insert(dir.to_path_buf());
            cur = dir.parent();
        }

        let cur_name = target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let sel_end = if !target.is_dir()
            && !cur_name.starts_with('.')
            && let Some(dot) = cur_name.rfind('.')
            && dot > 0
        {
            dot
        } else {
            cur_name.len()
        };

        let input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).default_value(cur_name);
            st.set_selected_range(0..sel_end, cx);
            st
        });
        let _sub = cx.subscribe_in(&input, window, |this, _state, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.commit_inline_rename(window, cx);
            }
        });
        input.focus_handle(cx).focus(window, cx);
        self.inline_rename = Some(InlineRenameState {
            target_path: target,
            input,
            _sub,
        });
        cx.notify();
    }

    pub fn commit_inline_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(st) = self.inline_rename.take() else {
            return;
        };
        let new_name = st.input.read(cx).value().trim().to_string();
        let old_name = st
            .target_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if new_name.is_empty() || new_name == old_name {
            cx.notify();
            return;
        }
        match crate::vault::rename_entry(&st.target_path, &new_name) {
            Ok(new_path) => {
                self.selected_tree_path = Some(new_path.clone());
                self.apply_path_relocation(&st.target_path, &new_path, cx);
                self.set_status(format!("Renamed to {new_name}"), cx);
            }
            Err(err) => {
                self.toast_err(format!("{err}"), window, cx);
            }
        }
    }

    pub fn cancel_inline_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inline_rename.take().is_some() {
            self.sidebar_focus.focus(window, cx);
            cx.notify();
        }
    }

    pub fn move_fs_entry(
        &mut self,
        src: PathBuf,
        dest_dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inline_rename = None;
        let src_canon = fs::canonicalize(&src).unwrap_or_else(|_| src.clone());
        match crate::vault::move_entry(&src, &dest_dir) {
            Ok(new_path) => {
                if new_path != src_canon {
                    let dest_is_visible =
                        dest_dir == self.workspace_root || self.expanded_dirs.contains(&dest_dir);
                    let name = new_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("item")
                        .to_string();
                    self.selected_tree_path = Some(if dest_is_visible {
                        new_path.clone()
                    } else {
                        dest_dir.clone()
                    });
                    self.apply_path_relocation(&src, &new_path, cx);
                    let msg = if dest_dir != self.workspace_root {
                        let dest_name = dest_dir
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("folder");
                        format!("Moved {name} → {dest_name}")
                    } else {
                        format!("Moved {name}")
                    };
                    self.set_status(msg, cx);
                }
            }
            Err(err) => {
                self.toast_err(format!("{err}"), window, cx);
            }
        }
    }
}

impl Render for VKarmaApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app_bg = hex_color(kd::BG_MAIN);
        let toolbar_bg = hex_color(kd::BG_SIDEBAR);
        let border = hex_color(kd::BORDER_SUBTLE);
        let fg = hex_color(kd::FG);

        let main_body = if self.zen_mode {
            let zen_side = if self.pane(self.active_pane).is_some() {
                self.active_pane
            } else {
                PaneSide::Left
            };
            let zen_words: Option<usize> = self
                .pane(zen_side)
                .and_then(|p| p.active())
                .and_then(|t| match &t.kind {
                    TabKind::Markdown { word_count, .. } => Some(*word_count),
                    _ => None,
                });

            let zen_exit_overlay = div()
                .id("zen-exit-zone")
                .group("zen-exit-zone")
                .absolute()
                .bottom_0()
                .right_0()
                .w(px(220.))
                .h(px(76.))
                .p_4()
                .flex()
                .items_end()
                .justify_end()
                .child(
                    div()
                        .id("zen-exit-reveal")
                        .opacity(0.)
                        .group_hover("zen-exit-zone", |s| s.opacity(1.))
                        .child(
                            div()
                                .id("zen-exit-btn")
                                .px_3()
                                .py_1p5()
                                .rounded(px(8.))
                                .bg(hex_color(kd::BG_SIDEBAR))
                                .border_1()
                                .border_color(hex_color(kd::BORDER_SUBTLE))
                                .shadow_md()
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap_2()
                                .hover(|s| {
                                    s.bg(hex_color(kd::BG_PANEL))
                                        .border_color(hex_color(kd::ACCENT))
                                })
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.exit_zen_mode(window, cx);
                                }))
                                .when_some(zen_words, |d, wc| {
                                    d.child(
                                        div()
                                            .text_xs()
                                            .text_color(hex_color(kd::FG_MUTED))
                                            .child(format!("{wc}w")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(hex_color(kd::BORDER_SUBTLE))
                                            .child("·"),
                                    )
                                })
                                .child(
                                    Icon::new(IconName::Minimize2)
                                        .xsmall()
                                        .text_color(hex_color(kd::FG)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(hex_color(kd::FG))
                                        .child("Exit Zen"),
                                )
                                .child(
                                    div()
                                        .px_1()
                                        .py_0p5()
                                        .rounded(px(4.))
                                        .bg(hex_color(kd::BG_DEEP))
                                        .text_size(px(10.))
                                        .text_color(hex_color(kd::FG_MUTED))
                                        .child("F11"),
                                ),
                        ),
                );

            v_flex()
                .relative()
                .size_full()
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .child(pane::render_pane(self, zen_side, window, cx)),
                )
                .child(zen_exit_overlay)
                .into_any_element()
        } else {
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
                        Button::new("new-file-btn")
                            .ghost()
                            .small()
                            .icon(IconName::FilePlus)
                            .label("+ New File")
                            .tooltip("New File (Ctrl+N) — adapts to extension (.md, .cs, .cpp, .fountain, .typ, .xlsx…)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                let entity = cx.entity();
                                let dir = this.workspace_root.clone();
                                modals::open_new_item_dialog(
                                    entity,
                                    dir,
                                    NewItemTemplate::File,
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
                    .size(px(276.))
                    .size_range(px(200.)..px(440.))
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

            v_flex()
                .size_full()
                .child(workspace_bar::render_workspace_bar(self, cx))
                .child(top_bar)
                .child(div().flex_1().min_h_0().w_full().child(workspace_split))
                .child(status_bar::render_status_bar(self, cx))
                .into_any_element()
        };

        v_flex()
            .image_cache(self.image_cache.clone())
            .relative()
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
                this.exit_zen_mode(window, cx);
                this.config.sidebar_visible = true;
                this.persist_workspaces();
                this.search_input.focus_handle(cx).focus(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FindInTab, window, cx| {
                this.trigger_find_in_active_tab(false, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ReplaceInTab, window, cx| {
                this.trigger_find_in_active_tab(true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &InputSearch, window, cx| {
                this.trigger_find_in_active_tab(false, window, cx);
            }))
            .on_action(cx.listener(|this, _: &InputReplace, window, cx| {
                this.trigger_find_in_active_tab(true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleViewMode, window, cx| {
                this.toggle_md_view_mode(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSourceMode, window, cx| {
                this.toggle_md_source_mode(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleZenMode, window, cx| {
                this.toggle_zen_mode(window, cx);
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
            .on_action(cx.listener(|this, _: &MoveTabLeft, _, cx| {
                let side = this.active_pane;
                if let Some(idx) = this.pane(side).map(|p| p.active_tab) {
                    this.move_tab(side, idx, -1, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &MoveTabRight, _, cx| {
                let side = this.active_pane;
                if let Some(idx) = this.pane(side).map(|p| p.active_tab) {
                    this.move_tab(side, idx, 1, cx);
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
                modals::open_new_item_dialog(entity, dir, NewItemTemplate::File, window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenFileDialog, window, cx| {
                this.prompt_open_files(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenFolderDialog, window, cx| {
                this.prompt_open_folder(window, cx);
            }))
            .on_action(cx.listener(|this, _: &EscapeModal, window, cx| {
                if this.inline_rename.is_some() {
                    this.cancel_inline_rename(window, cx);
                } else if this.quick_switcher.is_some() {
                    this.quick_switcher = None;
                    cx.notify();
                } else if this.close_active_tab_find(window, cx) {
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &RenameSelectedTreeItem, window, cx| {
                if let Some((side, tab_id, row, col)) = this.pane(this.active_pane).and_then(|p| {
                    p.active().and_then(|t| match &t.kind {
                        TabKind::Spreadsheet {
                            grid_focus,
                            active_row,
                            active_col,
                            ..
                        } if grid_focus.is_focused(window) => {
                            Some((this.active_pane, t.id, *active_row, *active_col))
                        }
                        _ => None,
                    })
                }) {
                    this.select_spreadsheet_cell(side, tab_id, row, col, true, window, cx);
                    return;
                }
                if this.zen_mode {
                    return;
                }
                let target = this
                    .selected_tree_path
                    .clone()
                    .or_else(|| this.active_tab_ref().map(|t| t.path.clone()))
                    .filter(|p| p.starts_with(&this.workspace_root));
                if let Some(target_path) = target {
                    this.start_inline_rename(target_path, window, cx);
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
            .child(main_body)
            .when_some(self.quick_switcher.as_ref(), |this, qs| {
                this.child(modals::render_quick_switcher_overlay(self, qs, cx))
            })
            .into_any_element()
    }
}

pub fn relocate_path(path: &Path, old_base: &Path, new_base: &Path) -> Option<PathBuf> {
    if path == old_base {
        return Some(new_base.to_path_buf());
    }
    let rel = path.strip_prefix(old_base).ok()?;
    Some(new_base.join(rel))
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

pub(crate) fn move_tab_between_vecs<T>(
    from_tabs: &mut Vec<T>,
    from_active: &mut usize,
    from_idx: usize,
    to_tabs: &mut Vec<T>,
    to_active: &mut usize,
    to_idx: usize,
) -> bool {
    if from_idx >= from_tabs.len() {
        return false;
    }
    let item = from_tabs.remove(from_idx);
    if from_tabs.is_empty() {
        *from_active = 0;
    } else if from_idx < *from_active || *from_active >= from_tabs.len() {
        *from_active = from_active.saturating_sub(1).min(from_tabs.len() - 1);
    }
    let insert_idx = to_idx.min(to_tabs.len());
    to_tabs.insert(insert_idx, item);
    *to_active = insert_idx;
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

    #[test]
    fn zen_column_max_width_rules() {
        assert_eq!(column_max_width(true, false), Some(NORMAL_READ_WIDTH));
        assert_eq!(column_max_width(false, false), None);
        assert_eq!(column_max_width(true, true), Some(ZEN_BOOK_WIDTH));
        assert_eq!(column_max_width(false, true), Some(ZEN_BOOK_WIDTH));
    }

    #[test]
    fn relocate_path_rewrites_files_and_nested_descendants() {
        let old_dir = PathBuf::from("/vault/Notes");
        let new_dir = PathBuf::from("/vault/Archive/Notes");
        assert_eq!(relocate_path(&old_dir, &old_dir, &new_dir), Some(new_dir.clone()));
        assert_eq!(
            relocate_path(&PathBuf::from("/vault/Notes/Sub/A.md"), &old_dir, &new_dir),
            Some(PathBuf::from("/vault/Archive/Notes/Sub/A.md"))
        );
        assert_eq!(
            relocate_path(&PathBuf::from("/vault/Notes2/B.md"), &old_dir, &new_dir),
            None
        );
    }

    #[test]
    fn move_tab_between_vecs_tracks_source_and_target_active_indices() {
        let mut left = vec!["A", "B", "C"];
        let mut left_active = 2; // "C"
        let mut right = vec!["X", "Y"];
        let mut right_active = 0; // "X"

        // Move "A" before active "C" into middle of right pane -> shifts left_active down and activates inserted tab in right
        assert!(move_tab_between_vecs(
            &mut left,
            &mut left_active,
            0,
            &mut right,
            &mut right_active,
            1
        ));
        assert_eq!(left, vec!["B", "C"]);
        assert_eq!(left[left_active], "C");
        assert_eq!(right, vec!["X", "A", "Y"]);
        assert_eq!(right[right_active], "A");

        // Move active "C" (last index) out of left pane to end of right pane
        assert!(move_tab_between_vecs(
            &mut left,
            &mut left_active,
            1,
            &mut right,
            &mut right_active,
            99
        ));
        assert_eq!(left, vec!["B"]);
        assert_eq!(left_active, 0);
        assert_eq!(right, vec!["X", "A", "Y", "C"]);
        assert_eq!(right[right_active], "C");

        // Move last remaining item "B" out of left pane -> left becomes empty cleanly
        assert!(move_tab_between_vecs(
            &mut left,
            &mut left_active,
            0,
            &mut right,
            &mut right_active,
            0
        ));
        assert!(left.is_empty());
        assert_eq!(left_active, 0);
        assert_eq!(right, vec!["B", "X", "A", "Y", "C"]);
        assert_eq!(right[right_active], "B");

        // Out-of-bounds source index is a safe no-op
        assert!(!move_tab_between_vecs(
            &mut left,
            &mut left_active,
            0,
            &mut right,
            &mut right_active,
            0
        ));
    }
}
