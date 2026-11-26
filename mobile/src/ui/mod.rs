pub mod live_editor;
pub mod notes_view;
pub mod settings_view;

use crate::live_md::LinkTarget;
use crate::vault::{
    MarkdownFormatAction, MobileWorkspaceState, QuickSwitchHit, SearchHit, TreeRow, VaultIndex,
    VaultSort, WikilinkRef, app_storage_dir, atomic_write_string, can_read_dir,
    count_direct_matching_files, create_folder, detect_default_vault_root,
    discover_vault_candidates, extract_tags, extract_wikilinks, list_subdirectories,
    normalize_rel_path, note_stats, rename_note_file, sanitize_note_filename, search_vault,
    today_daily_note_name,
};
use gpui::{prelude::*, *};
use gpui_kit::component::input::{InputEvent, InputState};
use live_editor::{LiveEditorEvent, LiveMarkdownEditor};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Default)]
pub struct SafeArea {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSurface {
    Notes,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawerCreateKind {
    Note,
    Folder,
}

#[derive(Debug, Clone)]
pub enum ItemContextTarget {
    Note {
        path: PathBuf,
        rel_path: String,
        display_name: String,
    },
    Folder {
        path: PathBuf,
        rel_path: String,
        display_name: String,
    },
}

#[derive(Debug, Clone)]
pub struct FolderPickerState {
    pub browsing_dir: PathBuf,
    pub subdirs: Vec<(String, PathBuf)>,
    pub direct_match_count: usize,
    pub error: Option<String>,
    pub create_candidate: Option<PathBuf>,
}

fn picker_match_count(dir: &Path) -> usize {
    count_direct_matching_files(dir, &["md", "markdown", "txt"])
}

fn expand_picker_path(raw: &str) -> PathBuf {
    let mut trimmed = raw.trim().to_string();
    while trimmed.len() > 1 && trimmed.ends_with('/') {
        trimmed.pop();
    }
    if (trimmed == "~" || trimmed.starts_with("~/"))
        && let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
    {
        return if trimmed == "~" {
            home
        } else {
            home.join(&trimmed[2..])
        };
    }
    PathBuf::from(trimmed)
}

fn ensure_active_candidate(
    candidates: &mut Vec<(String, PathBuf)>,
    active: &Path,
    fallback_label: &str,
) {
    if can_read_dir(active) && !candidates.iter().any(|(_, p)| p == active) {
        candidates.insert(0, (fallback_label.to_string(), active.to_path_buf()));
    }
}

#[derive(Debug, Clone)]
pub struct DrawerSnapAnim {
    pub from_progress: f32,
    pub to_progress: f32,
    pub started_at: Instant,
    pub duration_ms: u64,
    pub target_open: bool,
}

pub(crate) fn resolve_drawer_snap_open(
    started_open: bool,
    progress: f32,
    vel_x: f32,
    ended_cleanly: bool,
) -> bool {
    if !ended_cleanly {
        return progress >= 0.5;
    }
    if !started_open {
        progress >= 0.28 || (vel_x >= 320.0 && progress >= 0.04)
    } else {
        !(progress <= 0.72 || (vel_x <= -320.0 && progress <= 0.96))
    }
}

#[derive(Debug, Clone)]
pub struct ActiveNote {
    pub path: PathBuf,
    pub title: String,
    pub filename: String,
    pub rel_folder: String,
    pub content: String,
    pub wikilinks: Vec<WikilinkRef>,
    pub tags: Vec<String>,
    pub stats: (usize, usize, usize),
    pub reading_only: bool,
    pub dirty: bool,
    pub saved_badge: Option<String>,
    pub find_open: bool,
    pub delete_armed: bool,
}

pub struct VKarmaMobileApp {
    pub image_cache: Entity<RetainAllImageCache>,
    pub surface: AppSurface,
    pub safe_area: SafeArea,

    pub vault_root: PathBuf,
    pub has_storage_access: bool,
    pub vault_candidates: Vec<(String, PathBuf)>,
    pub vault_index: VaultIndex,
    pub vault_indexing: bool,
    pub expanded_dirs: HashSet<String>,
    pub sort_mode: VaultSort,
    pub visible_tree: Rc<Vec<TreeRow>>,

    pub active_note: Option<ActiveNote>,
    pub live_editor: Entity<LiveMarkdownEditor>,
    pub live_editor_focused: bool,
    pub recent_notes: Vec<PathBuf>,
    pub history_back: Vec<PathBuf>,
    pub history_forward: Vec<PathBuf>,

    pub drawer_open: bool,
    pub drawer_progress: f32,
    pub drawer_drag_active: bool,
    pub drawer_drag_started_open: bool,
    pub drawer_drag_vel_x: f32,
    pub drawer_drag_last_at: Option<Instant>,
    pub drawer_anim: Option<DrawerSnapAnim>,
    pub drawer_creating: Option<DrawerCreateKind>,
    pub drawer_target_dir: Option<PathBuf>,

    pub quick_switcher_open: bool,
    pub quick_switcher_content_mode: bool,
    pub quick_hits: Vec<QuickSwitchHit>,
    pub content_hits: Vec<SearchHit>,
    pub content_searching: bool,
    pub search_gen: u64,

    pub more_sheet_open: bool,
    pub rename_mode: bool,
    pub new_note_modal_open: bool,
    pub item_context_menu: Option<ItemContextTarget>,
    pub context_rename_mode: bool,
    pub context_delete_armed: bool,

    pub edit_revision: u64,

    pub folder_picker: Option<FolderPickerState>,

    pub status_banner: Option<String>,

    pub drawer_filter_input: Entity<InputState>,
    pub drawer_create_input: Entity<InputState>,
    pub quick_switcher_input: Entity<InputState>,
    pub new_note_input: Entity<InputState>,
    pub rename_note_input: Entity<InputState>,
    pub note_find_input: Entity<InputState>,
    pub folder_picker_input: Entity<InputState>,

    pub tree_scroll: UniformListScrollHandle,
    pub switcher_scroll: UniformListScrollHandle,
    pub home_scroll: ScrollHandle,
    pub toolbar_scroll: ScrollHandle,
    pub settings_scroll: ScrollHandle,
    pub folder_picker_scroll: ScrollHandle,

    last_back_at: Option<Instant>,
    last_resume_epoch: u64,
    open_note_gen: u64,
    _vault_task: Option<Task<()>>,
    _autosave_task: Option<Task<()>>,
    _open_note_task: Option<Task<()>>,
    _search_task: Option<Task<()>>,
    _perm_poll_task: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl VKarmaMobileApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let image_cache = RetainAllImageCache::new(cx);
        let ws = MobileWorkspaceState::load();
        let (detected_root, has_storage_access) = detect_default_vault_root();
        let vault_root = ws
            .vault_root
            .filter(|p| can_read_dir(p))
            .unwrap_or(detected_root);
        let mut vault_candidates = discover_vault_candidates();
        ensure_active_candidate(&mut vault_candidates, &vault_root, "Current Vault Folder");

        let drawer_filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter files…"));
        let drawer_create_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Name…"));
        let quick_switcher_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Find or create note…"));
        let new_note_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Note title (e.g. Ideas)"));
        let rename_note_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("New title…"));
        let live_editor = cx.new(|cx| LiveMarkdownEditor::new(String::new(), window, cx));
        let note_find_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Find in note…"));
        let folder_picker_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("/sdcard/Documents"));

        let mut subs = Vec::new();

        subs.push(
            cx.subscribe_in(&drawer_filter_input, window, |this, _, ev, window, cx| match ev {
                InputEvent::Focus => {}
                InputEvent::Blur => {}
                InputEvent::Change => {
                    this.rebuild_visible_tree(cx);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    window.blur(cx);
                    gpui_mobile::hide_keyboard();
                }
            }),
        );

        subs.push(
            cx.subscribe_in(&drawer_create_input, window, |this, _, ev, window, cx| match ev {
                InputEvent::Focus | InputEvent::Blur | InputEvent::Change => {}
                InputEvent::PressEnter { .. } => this.submit_drawer_create(window, cx),
            }),
        );

        subs.push(
            cx.subscribe_in(&quick_switcher_input, window, |this, inp, ev, window, cx| match ev {
                InputEvent::Focus | InputEvent::Blur => {}
                InputEvent::Change => {
                    let q = inp.read(cx).value().to_string();
                    this.on_quick_switcher_query_changed(q, cx);
                }
                InputEvent::PressEnter { .. } => {
                    if !this.quick_switcher_content_mode {
                        if let Some(first) = this.quick_hits.first() {
                            let path = first.path.clone();
                            this.open_note(path, false, true, window, cx);
                            return;
                        }
                        let raw = inp.read(cx).value().to_string();
                        if !raw.trim().is_empty() {
                            this.create_note_by_name(&raw, None, window, cx);
                            return;
                        }
                    }
                    window.blur(cx);
                    gpui_mobile::hide_keyboard();
                }
            }),
        );

        subs.push(
            cx.subscribe_in(&new_note_input, window, |this, _, ev, window, cx| match ev {
                InputEvent::Focus | InputEvent::Blur | InputEvent::Change => {}
                InputEvent::PressEnter { .. } => this.submit_new_note_modal(window, cx),
            }),
        );

        subs.push(
            cx.subscribe_in(&rename_note_input, window, |this, _, ev, window, cx| match ev {
                InputEvent::Focus | InputEvent::Blur | InputEvent::Change => {}
                InputEvent::PressEnter { .. } => {
                    if this.context_rename_mode {
                        this.submit_context_rename(window, cx);
                    } else {
                        this.submit_rename_note(window, cx);
                    }
                }
            }),
        );

        subs.push(
            cx.subscribe_in(&live_editor, window, |this, _, ev, window, cx| match ev {
                LiveEditorEvent::FocusChanged(focused) => {
                    this.live_editor_focused = *focused;
                    if !*focused {
                        this.flush_active_note_sync(cx);
                    }
                    cx.notify();
                }
                LiveEditorEvent::Changed => {
                    let flipped_dirty = if let Some(note) = &mut this.active_note {
                        let was_dirty = note.dirty;
                        note.dirty = true;
                        note.saved_badge = None;
                        !was_dirty
                    } else {
                        false
                    };
                    this.schedule_autosave(cx);
                    if flipped_dirty {
                        cx.notify();
                    }
                }
                LiveEditorEvent::OpenLink(LinkTarget::Wikilink(target)) => {
                    let t = target.clone();
                    this.open_wikilink(&t, window, cx);
                }
                LiveEditorEvent::OpenLink(LinkTarget::Url(url)) => {
                    if url.starts_with("http://") || url.starts_with("https://") {
                        cx.open_url(url);
                    }
                }
            }),
        );

        subs.push(cx.subscribe_in(&note_find_input, window, |_, _, ev, window, cx| match ev {
            InputEvent::Focus | InputEvent::Blur => {}
            InputEvent::PressEnter { .. } => {
                window.blur(cx);
                gpui_mobile::hide_keyboard();
                cx.notify();
            }
            InputEvent::Change => cx.notify(),
        }));

        subs.push(
            cx.subscribe_in(&folder_picker_input, window, |this, _, ev, window, cx| {
                match ev {
                    InputEvent::Focus | InputEvent::Blur | InputEvent::Change => {}
                    InputEvent::PressEnter { .. } => {
                        this.apply_folder_picker_input(window, cx);
                    }
                }
            }),
        );

        let weak_app = cx.entity().downgrade();
        subs.push(cx.intercept_keystrokes(move |ev, window, cx| {
            if ev.keystroke.key == "escape"
                && !ev.keystroke.modifiers.control
                && !ev.keystroke.modifiers.alt
                && !ev.keystroke.modifiers.platform
            {
                cx.stop_propagation();
                let _ = weak_app.update(cx, |this, cx| {
                    this.handle_back_action(window, cx);
                });
            }
        }));

        let expanded_dirs: HashSet<String> = ws.expanded_dirs.into_iter().collect();
        let recent_notes: Vec<PathBuf> = ws
            .recent_notes
            .into_iter()
            .filter(|p| p.is_file())
            .take(20)
            .collect();
        let cached_index = VaultIndex::load_cached(&vault_root)
            .unwrap_or_else(|| VaultIndex::empty(vault_root.clone()));

        let mut app = Self {
            image_cache,
            surface: AppSurface::Notes,
            safe_area: Self::query_safe_area(),
            vault_root,
            has_storage_access,
            vault_candidates,
            vault_index: cached_index,
            vault_indexing: false,
            expanded_dirs,
            sort_mode: ws.sort_mode,
            visible_tree: Rc::new(Vec::new()),
            active_note: None,
            live_editor,
            live_editor_focused: false,
            recent_notes,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            drawer_open: false,
            drawer_progress: 0.0,
            drawer_drag_active: false,
            drawer_drag_started_open: false,
            drawer_drag_vel_x: 0.0,
            drawer_drag_last_at: None,
            drawer_anim: None,
            drawer_creating: None,
            drawer_target_dir: None,
            quick_switcher_open: false,
            quick_switcher_content_mode: false,
            quick_hits: Vec::new(),
            content_hits: Vec::new(),
            content_searching: false,
            search_gen: 0,
            more_sheet_open: false,
            rename_mode: false,
            new_note_modal_open: false,
            item_context_menu: None,
            context_rename_mode: false,
            context_delete_armed: false,
            edit_revision: 0,
            folder_picker: None,
            status_banner: None,
            drawer_filter_input,
            drawer_create_input,
            quick_switcher_input,
            new_note_input,
            rename_note_input,
            note_find_input,
            folder_picker_input,
            tree_scroll: UniformListScrollHandle::new(),
            switcher_scroll: UniformListScrollHandle::new(),
            home_scroll: ScrollHandle::new(),
            toolbar_scroll: ScrollHandle::new(),
            settings_scroll: ScrollHandle::new(),
            folder_picker_scroll: ScrollHandle::new(),
            last_back_at: None,
            last_resume_epoch: crate::android_resume_epoch(),
            open_note_gen: 0,
            _vault_task: None,
            _autosave_task: None,
            _open_note_task: None,
            _search_task: None,
            _perm_poll_task: None,
            _subs: subs,
        };

        app.rebuild_visible_tree(cx);
        app.rebuild_quick_hits(cx);

        if let Some(last_path) = ws.last_note_path
            && last_path.is_file()
        {
            app.open_note(last_path, false, false, window, cx);
        }
        app.refresh_vault_async(cx);
        app
    }

    pub fn query_safe_area() -> SafeArea {
        #[cfg(target_os = "android")]
        {
            use gpui_mobile::android::jni;
            if let Some(platform) = jni::platform()
                && let Some(win) = platform.primary_window()
            {
                let insets = win.safe_area_insets_logical();
                return SafeArea {
                    top: insets.top.max(0.0),
                    bottom: insets.bottom.max(0.0),
                    left: insets.left.max(0.0),
                    right: insets.right.max(0.0),
                };
            }
        }
        SafeArea::default()
    }

    pub fn persist_workspace_state_async(&self, cx: &mut Context<Self>) {
        let state = MobileWorkspaceState {
            vault_root: Some(self.vault_root.clone()),
            last_note_path: self.active_note.as_ref().map(|n| n.path.clone()),
            recent_notes: self.recent_notes.iter().take(20).cloned().collect(),
            expanded_dirs: self.expanded_dirs.iter().cloned().collect(),
            sort_mode: self.sort_mode,
        };
        cx.background_executor()
            .spawn(async move {
                let _ = state.save();
            })
            .detach();
    }

    pub fn rebuild_visible_tree(&mut self, cx: &App) {
        let filter = self.drawer_filter_input.read(cx).value().to_string();
        self.visible_tree = Rc::new(
            self.vault_index
                .visible_tree_rows(&self.expanded_dirs, self.sort_mode, &filter),
        );
    }

    pub fn rebuild_quick_hits(&mut self, cx: &App) {
        let q = self.quick_switcher_input.read(cx).value().to_string();
        self.quick_hits = self.vault_index.quick_switch(&q, &self.recent_notes, 60);
    }

    pub fn expand_ancestors_for_path(&mut self, path: &Path) {
        let rel = normalize_rel_path(&self.vault_root, path);
        let mut parent = rel.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        while !parent.is_empty() {
            self.expanded_dirs.insert(parent.to_string());
            parent = parent.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        }
    }

    pub fn refresh_vault_async(&mut self, cx: &mut Context<Self>) {
        let root = self.vault_root.clone();
        self.vault_indexing = true;
        cx.notify();

        self._vault_task = Some(cx.spawn(async move |weak_self, cx| {
            let (index, candidates, access) = cx
                .background_executor()
                .spawn(async move {
                    let index = VaultIndex::build(&root);
                    let _ = index.save_cached();
                    let candidates = discover_vault_candidates();
                    let access = detect_default_vault_root().1;
                    (index, candidates, access)
                })
                .await;

            _ = weak_self.update(cx, |this, cx| {
                if this.vault_root == index.root {
                    let mut candidates = candidates;
                    ensure_active_candidate(
                        &mut candidates,
                        &this.vault_root,
                        "Current Vault Folder",
                    );
                    this.vault_index = index;
                    this.vault_candidates = candidates;
                    this.has_storage_access = access;
                    this.vault_indexing = false;
                    if let Some(note) = &this.active_note {
                        let active_path = note.path.clone();
                        this.expand_ancestors_for_path(&active_path);
                    }
                    this.rebuild_visible_tree(cx);
                    this.rebuild_quick_hits(cx);
                    cx.notify();
                }
            });
        }));
    }

    pub fn recheck_storage_and_refresh(&mut self, cx: &mut Context<Self>) {
        let was_granted = self.has_storage_access;
        let (detected_root, access) = detect_default_vault_root();
        self.has_storage_access = access;
        let fallback_vault = app_storage_dir("vault");
        if access && (!can_read_dir(&self.vault_root) || self.vault_root == fallback_vault) {
            self.vault_root = detected_root;
        }
        if access && !was_granted {
            self.status_banner = Some("All files access granted — scanning vault…".to_string());
        }
        self.persist_workspace_state_async(cx);
        self.refresh_vault_async(cx);
    }

    pub fn request_storage_permission(&mut self, cx: &mut Context<Self>) {
        let start_epoch = crate::android_resume_epoch();
        crate::request_all_files_access();
        self.status_banner = Some(
            "Toggle \"Allow access to manage all files\", then press Back.".to_string(),
        );
        cx.notify();

        self._perm_poll_task = Some(cx.spawn(async move |weak_self, cx| {
            for _ in 0..340u32 {
                cx.background_executor()
                    .timer(Duration::from_millis(350))
                    .await;
                if crate::android_resume_epoch() != start_epoch || crate::has_all_files_access() {
                    let _ = weak_self.update(cx, |this, cx| {
                        this.last_resume_epoch = crate::android_resume_epoch();
                        this.recheck_storage_and_refresh(cx);
                    });
                    break;
                }
            }
        }));
    }

    pub fn set_vault_root(&mut self, new_root: PathBuf, cx: &mut Context<Self>) {
        if !can_read_dir(&new_root) {
            self.status_banner = Some(format!("Cannot read {}", new_root.display()));
            cx.notify();
            return;
        }
        self.flush_active_note_sync(cx);
        self.vault_root = new_root.clone();
        ensure_active_candidate(&mut self.vault_candidates, &new_root, "Current Vault Folder");
        self.expanded_dirs.clear();
        self.content_hits.clear();
        self.visible_tree = Rc::new(Vec::new());
        self.quick_hits.clear();
        self.drawer_target_dir = Some(new_root);
        self.status_banner = Some(format!("Vault: {}", self.vault_root.display()));
        self.persist_workspace_state_async(cx);
        self.refresh_vault_async(cx);
        cx.notify();
    }

    pub fn open_surface_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_active_note_sync(cx);
        window.blur(cx);
        gpui_mobile::hide_keyboard();
        self.close_drawer_immediately();
        self.quick_switcher_open = false;
        self.more_sheet_open = false;
        self.new_note_modal_open = false;
        self.surface = AppSurface::Settings;
        cx.notify();
    }

    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.blur(cx);
        gpui_mobile::hide_keyboard();
        self.surface = AppSurface::Notes;
        cx.notify();
    }

    pub fn open_folder_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_active_note_sync(cx);
        window.blur(cx);
        gpui_mobile::hide_keyboard();
        self.close_drawer_immediately();
        self.quick_switcher_open = false;
        self.more_sheet_open = false;
        self.new_note_modal_open = false;

        let browsing_dir = if can_read_dir(&self.vault_root) {
            self.vault_root.clone()
        } else {
            #[cfg(target_os = "android")]
            {
                let sd = PathBuf::from("/sdcard");
                if can_read_dir(&sd) {
                    sd
                } else {
                    app_storage_dir("")
                }
            }
            #[cfg(not(target_os = "android"))]
            {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|p| can_read_dir(p))
                    .unwrap_or_else(|| app_storage_dir(""))
            }
        };

        let subdirs = list_subdirectories(&browsing_dir);
        let direct_match_count = picker_match_count(&browsing_dir);
        let path_str = browsing_dir.to_string_lossy().into_owned();
        self.folder_picker_input
            .update(cx, |inp, cx| inp.set_value(path_str, window, cx));
        self.folder_picker_scroll.set_offset(point(px(0.), px(0.)));
        self.folder_picker = Some(FolderPickerState {
            browsing_dir,
            subdirs,
            direct_match_count,
            error: None,
            create_candidate: None,
        });
        cx.notify();
    }

    pub fn browse_folder_picker_to(
        &mut self,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.folder_picker.as_mut() else {
            return;
        };
        if !can_read_dir(&dir) {
            picker.error = Some(format!("Cannot read {}", dir.display()));
            picker.create_candidate = None;
            cx.notify();
            return;
        }
        picker.browsing_dir = dir.clone();
        picker.subdirs = list_subdirectories(&dir);
        picker.direct_match_count = picker_match_count(&dir);
        picker.error = None;
        picker.create_candidate = None;
        let path_str = dir.to_string_lossy().into_owned();
        self.folder_picker_input
            .update(cx, |inp, cx| inp.set_value(path_str, window, cx));
        self.folder_picker_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    pub fn folder_picker_go_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.folder_picker.as_ref().map(|p| p.browsing_dir.clone()) else {
            return;
        };
        if let Some(parent) = current.parent().map(Path::to_path_buf) {
            if can_read_dir(&parent) {
                self.browse_folder_picker_to(parent, window, cx);
                return;
            }
            #[cfg(target_os = "android")]
            {
                let sdcard = PathBuf::from("/sdcard");
                if current != sdcard && current.starts_with(&sdcard) && can_read_dir(&sdcard) {
                    self.browse_folder_picker_to(sdcard, window, cx);
                    return;
                }
            }
            if let Some(p) = self.folder_picker.as_mut() {
                p.error = Some(format!("Already at top accessible folder ({})", current.display()));
                p.create_candidate = None;
                cx.notify();
            }
        }
    }

    pub fn apply_folder_picker_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.folder_picker_input.read(cx).value().to_string();
        let Some(fallback_dir) = self.folder_picker.as_ref().map(|p| p.browsing_dir.clone()) else {
            return;
        };

        let candidate = if raw.trim().is_empty() {
            fallback_dir
        } else {
            expand_picker_path(&raw)
        };

        if can_read_dir(&candidate) {
            self.folder_picker = None;
            window.blur(cx);
            gpui_mobile::hide_keyboard();
            self.set_vault_root(candidate, cx);
            return;
        }

        if let Some(p) = self.folder_picker.as_mut() {
            if !candidate.exists() {
                p.error = Some(format!(
                    "Folder does not exist: {}",
                    candidate.display()
                ));
                p.create_candidate = Some(candidate);
            } else {
                p.error = Some(format!(
                    "Cannot read folder {} (check path or storage permission)",
                    candidate.display()
                ));
                p.create_candidate = None;
            }
        }
        cx.notify();
    }

    pub fn create_and_apply_folder_picker_path(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self
            .folder_picker
            .as_ref()
            .and_then(|p| p.create_candidate.clone())
        else {
            return;
        };
        match fs::create_dir_all(&path) {
            Ok(()) if can_read_dir(&path) => {
                self.folder_picker = None;
                window.blur(cx);
                gpui_mobile::hide_keyboard();
                self.set_vault_root(path, cx);
            }
            Ok(()) => {
                if let Some(p) = self.folder_picker.as_mut() {
                    p.error = Some(format!("Created {}, but folder is not readable", path.display()));
                    p.create_candidate = None;
                }
                cx.notify();
            }
            Err(err) => {
                if let Some(p) = self.folder_picker.as_mut() {
                    p.error = Some(format!("Create failed: {err}"));
                }
                cx.notify();
            }
        }
    }

    pub fn close_folder_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.folder_picker = None;
        window.blur(cx);
        gpui_mobile::hide_keyboard();
        cx.notify();
    }

    pub fn handle_back_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        if let Some(prev) = self.last_back_at
            && now.duration_since(prev) < Duration::from_millis(120)
        {
            return;
        }
        self.last_back_at = Some(now);

        if self.folder_picker.is_some() {
            self.close_folder_picker(window, cx);
            return;
        }

        match self.surface {
            AppSurface::Settings => {
                self.close_settings(window, cx);
            }
            AppSurface::Notes => {
                if self.item_context_menu.is_some() {
                    if self.context_rename_mode {
                        self.context_rename_mode = false;
                        self.context_delete_armed = false;
                        window.blur(cx);
                        gpui_mobile::hide_keyboard();
                        cx.notify();
                    } else {
                        self.close_item_context_menu(window, cx);
                    }
                    return;
                }
                if self.new_note_modal_open {
                    self.new_note_modal_open = false;
                    window.blur(cx);
                    gpui_mobile::hide_keyboard();
                    cx.notify();
                    return;
                }
                if self.more_sheet_open {
                    if self.rename_mode {
                        self.rename_mode = false;
                        window.blur(cx);
                        gpui_mobile::hide_keyboard();
                    } else {
                        self.more_sheet_open = false;
                    }
                    cx.notify();
                    return;
                }
                if self.quick_switcher_open {
                    self.close_quick_switcher(window, cx);
                    return;
                }
                if self.is_drawer_visible() {
                    if self.drawer_creating.is_some() {
                        self.drawer_creating = None;
                        window.blur(cx);
                        gpui_mobile::hide_keyboard();
                        cx.notify();
                    } else {
                        self.toggle_drawer(false, window, cx);
                    }
                    return;
                }
                if let Some(note) = &mut self.active_note
                    && note.find_open
                {
                    note.find_open = false;
                    window.blur(cx);
                    gpui_mobile::hide_keyboard();
                    cx.notify();
                    return;
                }
                if self.live_editor_focused {
                    self.flush_active_note_sync(cx);
                    self.live_editor_focused = false;
                    window.blur(cx);
                    gpui_mobile::hide_keyboard();
                    cx.notify();
                    return;
                }
                if !self.history_back.is_empty() {
                    self.navigate_history_back(window, cx);
                    return;
                }
                self.flush_active_note_sync(cx);
                self.persist_workspace_state_async(cx);
                crate::minimize_android_app();
            }
        }
    }

    pub fn drawer_width_for_window(&self, window: &Window) -> f32 {
        let vp_w =
            f32::from(window.viewport_size().width) - self.safe_area.left - self.safe_area.right;
        (vp_w * 0.84).clamp(240.0, 316.0)
    }

    pub fn is_drawer_visible(&self) -> bool {
        self.drawer_open
            || self.drawer_progress > 0.001
            || self.drawer_drag_active
            || self.drawer_anim.is_some()
    }

    pub fn has_non_drawer_modal_overlay(&self) -> bool {
        self.quick_switcher_open
            || self.new_note_modal_open
            || self.more_sheet_open
            || self.item_context_menu.is_some()
            || self.folder_picker.is_some()
    }

    pub fn has_modal_overlay(&self) -> bool {
        self.is_drawer_visible() || self.has_non_drawer_modal_overlay()
    }

    fn prepare_drawer_for_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_active_note_sync(cx);
        self.live_editor_focused = false;
        window.blur(cx);
        gpui_mobile::hide_keyboard();
        self.quick_switcher_open = false;
        self.more_sheet_open = false;
        self.new_note_modal_open = false;
        self.drawer_creating = None;
        if self.drawer_target_dir.is_none() {
            self.drawer_target_dir = self
                .active_note
                .as_ref()
                .and_then(|n| n.path.parent().map(Path::to_path_buf))
                .or_else(|| Some(self.vault_root.clone()));
        }
        self.rebuild_visible_tree(cx);
    }

    fn close_drawer_immediately(&mut self) {
        self.drawer_open = false;
        self.drawer_progress = 0.0;
        self.drawer_drag_active = false;
        self.drawer_drag_last_at = None;
        self.drawer_anim = None;
        self.drawer_creating = None;
    }

    pub fn start_drawer_snap(
        &mut self,
        target_open: bool,
        vel_x: f32,
        drawer_w: f32,
        cx: &mut Context<Self>,
    ) {
        let from = self.drawer_progress.clamp(0.0, 1.0);
        let to = if target_open { 1.0 } else { 0.0 };
        let dist = (to - from).abs();
        if dist < 0.005 {
            self.drawer_progress = to;
            self.drawer_open = target_open;
            self.drawer_anim = None;
            if !target_open {
                self.drawer_creating = None;
            }
            cx.notify();
            return;
        }

        let base_ms = (120.0 + 130.0 * dist).round() as u64;
        let aligned = (to > from && vel_x > 200.0) || (to < from && vel_x < -200.0);
        let duration_ms = if aligned {
            let vel_ms = ((dist * drawer_w) / vel_x.abs() * 1000.0 * 1.45).clamp(110.0, 220.0);
            base_ms.min(vel_ms.round() as u64)
        } else {
            base_ms
        };

        self.drawer_open = target_open;
        self.drawer_anim = Some(DrawerSnapAnim {
            from_progress: from,
            to_progress: to,
            started_at: Instant::now(),
            duration_ms,
            target_open,
        });
        cx.notify();
    }

    fn drive_drawer_anim_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(anim) = self.drawer_anim.clone() else {
            return;
        };
        let raw = (anim.started_at.elapsed().as_secs_f32() * 1000.0
            / anim.duration_ms.max(1) as f32)
            .clamp(0.0, 1.0);
        if raw >= 1.0 {
            self.drawer_progress = anim.to_progress;
            self.drawer_open = anim.target_open;
            if !anim.target_open {
                self.drawer_creating = None;
            }
            self.drawer_anim = None;
            return;
        }
        let eased = 1.0 - (1.0 - raw).powi(5);
        self.drawer_progress = anim.from_progress + (anim.to_progress - anim.from_progress) * eased;
        let weak = cx.entity().downgrade();
        window.on_next_frame(move |_, cx| {
            let _ = weak.update(cx, |_, cx| cx.notify());
        });
    }

    pub fn handle_drawer_scroll_gesture(
        &mut self,
        ev: &ScrollEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if ev.source != ScrollSource::Touch
            || self.surface != AppSurface::Notes
            || self.has_non_drawer_modal_overlay()
        {
            return false;
        }

        let delta = ev.delta.pixel_delta(px(24.));
        let dx = f32::from(delta.x);
        let dy = f32::from(delta.y);

        let toolbar_visible = self
            .active_note
            .as_ref()
            .is_some_and(|n| !n.reading_only)
            && (self.live_editor_focused
                || gpui_mobile::keyboard_height() > self.safe_area.bottom + 40.0);
        if toolbar_visible && !self.is_drawer_visible() {
            let tb_bounds = self.toolbar_scroll.bounds();
            if tb_bounds.size.width > px(0.)
                && tb_bounds.size.height > px(0.)
                && tb_bounds.contains(&ev.position)
            {
                return false;
            }
        }

        let drawer_w = self.drawer_width_for_window(window).max(1.0);
        match ev.touch_phase {
            TouchPhase::Started => {
                if dx.abs() <= 0.01 || dx.abs() < dy.abs() * 1.5 {
                    return false;
                }
                let visible = self.is_drawer_visible();
                if !visible && dx <= 0.0 {
                    return false;
                }
                if visible && dx >= 0.0 && self.drawer_progress >= 0.999 {
                    return false;
                }
                if !visible {
                    self.prepare_drawer_for_open(window, cx);
                }
                self.drawer_anim = None;
                self.drawer_drag_active = true;
                self.drawer_drag_started_open = self.drawer_progress >= 0.5;
                self.drawer_progress = (self.drawer_progress + dx / drawer_w).clamp(0.0, 1.0);
                self.drawer_drag_vel_x = 0.0;
                self.drawer_drag_last_at = Some(Instant::now());
                cx.notify();
                true
            }
            TouchPhase::Moved => {
                if self.drawer_drag_active {
                    let now = Instant::now();
                    if let Some(prev) = self.drawer_drag_last_at {
                        let dt = now.duration_since(prev).as_secs_f32().max(0.004);
                        let inst = dx / dt;
                        self.drawer_drag_vel_x = self.drawer_drag_vel_x * 0.5 + inst * 0.5;
                    }
                    self.drawer_drag_last_at = Some(now);
                    self.drawer_progress = (self.drawer_progress + dx / drawer_w).clamp(0.0, 1.0);
                    cx.notify();
                    true
                } else {
                    dx.abs() > 0.01 && dy.abs() <= 0.01
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if self.drawer_drag_active {
                    let ended = ev.touch_phase == TouchPhase::Ended;
                    if ended && dx.abs() > 0.01 {
                        self.drawer_progress =
                            (self.drawer_progress + dx / drawer_w).clamp(0.0, 1.0);
                    }
                    let target_open = resolve_drawer_snap_open(
                        self.drawer_drag_started_open,
                        self.drawer_progress,
                        self.drawer_drag_vel_x,
                        ended,
                    );
                    let vel_x = self.drawer_drag_vel_x;
                    self.drawer_drag_active = false;
                    self.drawer_drag_last_at = None;
                    self.start_drawer_snap(target_open, vel_x, drawer_w, cx);
                    true
                } else {
                    dx.abs() > 0.01 && dy.abs() <= 0.01
                }
            }
        }
    }

    pub fn toggle_drawer(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.drawer_drag_active = false;
        self.drawer_drag_last_at = None;
        let drawer_w = self.drawer_width_for_window(window);
        if open {
            self.prepare_drawer_for_open(window, cx);
            self.start_drawer_snap(true, 0.0, drawer_w, cx);
        } else {
            window.blur(cx);
            gpui_mobile::hide_keyboard();
            self.drawer_creating = None;
            self.start_drawer_snap(false, 0.0, drawer_w, cx);
        }
    }

    pub fn toggle_tree_dir(&mut self, rel_path: String, cx: &mut Context<Self>) {
        if !self.expanded_dirs.remove(&rel_path) {
            self.expanded_dirs.insert(rel_path);
        }
        self.rebuild_visible_tree(cx);
        self.persist_workspace_state_async(cx);
        cx.notify();
    }

    pub fn cycle_sort_mode(&mut self, cx: &mut Context<Self>) {
        self.sort_mode = self.sort_mode.cycle();
        self.rebuild_visible_tree(cx);
        self.persist_workspace_state_async(cx);
        cx.notify();
    }

    pub fn open_quick_switcher(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flush_active_note_sync(cx);
        self.close_drawer_immediately();
        self.more_sheet_open = false;
        self.new_note_modal_open = false;
        self.quick_switcher_open = true;
        self.quick_switcher_content_mode = false;
        self.content_hits.clear();
        self.content_searching = false;
        self.quick_switcher_input
            .update(cx, |inp, cx| inp.set_value("", window, cx));
        self.rebuild_quick_hits(cx);
        self.switcher_scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.quick_switcher_input
            .update(cx, |inp, cx| inp.focus(window, cx));
        gpui_mobile::show_keyboard();
        cx.notify();
    }

    pub fn close_quick_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.quick_switcher_open = false;
        self.content_searching = false;
        self._search_task = None;
        window.blur(cx);
        gpui_mobile::hide_keyboard();
        cx.notify();
    }

    pub fn set_quick_switcher_mode(&mut self, content_mode: bool, cx: &mut Context<Self>) {
        self.quick_switcher_content_mode = content_mode;
        let q = self.quick_switcher_input.read(cx).value().to_string();
        self.on_quick_switcher_query_changed(q, cx);
    }

    fn on_quick_switcher_query_changed(&mut self, q: String, cx: &mut Context<Self>) {
        self.rebuild_quick_hits(cx);
        if !self.quick_switcher_content_mode {
            self.content_searching = false;
            self._search_task = None;
            cx.notify();
            return;
        }

        let trimmed = q.trim().to_string();
        if trimmed.is_empty() {
            self.content_hits.clear();
            self.content_searching = false;
            self._search_task = None;
            cx.notify();
            return;
        }

        self.search_gen = self.search_gen.wrapping_add(1);
        let seq = self.search_gen;
        let root = self.vault_root.clone();
        self.content_searching = true;
        cx.notify();

        self._search_task = Some(cx.spawn(async move |weak_self, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(160))
                .await;
            let hits = cx
                .background_executor()
                .spawn(async move { search_vault(&root, &trimmed, 60) })
                .await;
            _ = weak_self.update(cx, |this, cx| {
                if this.search_gen == seq && this.quick_switcher_open {
                    this.content_hits = hits;
                    this.content_searching = false;
                    cx.notify();
                }
            });
        }));
    }

    pub fn open_note(
        &mut self,
        path: PathBuf,
        start_in_edit: bool,
        push_history: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flush_active_note_sync(cx);
        self.open_note_gen = self.open_note_gen.wrapping_add(1);
        let note_seq = self.open_note_gen;
        let path_bg = path.clone();

        let bg_read = cx.background_executor().spawn(async move {
            let content = fs::read_to_string(&path_bg).map_err(|e| e.to_string())?;
            let wikilinks = extract_wikilinks(&content);
            let tags = extract_tags(&content);
            let stats = note_stats(&content);
            Ok::<_, String>((content, wikilinks, tags, stats))
        });

        self._open_note_task = Some(cx.spawn_in(window, async move |weak_self, cx| {
            let loaded = bg_read.await;
            _ = weak_self.update_in(cx, |this, window, cx| {
                if this.open_note_gen != note_seq {
                    return;
                }
                let (content, wikilinks, tags, stats) = match loaded {
                    Ok(data) => data,
                    Err(err) => {
                        this.status_banner = Some(format!("Cannot read note: {err}"));
                        cx.notify();
                        return;
                    }
                };

                if push_history
                    && let Some(prev) = &this.active_note
                    && prev.path != path
                {
                    if this.history_back.last() != Some(&prev.path) {
                        this.history_back.push(prev.path.clone());
                        if this.history_back.len() > 40 {
                            this.history_back.remove(0);
                        }
                    }
                    this.history_forward.clear();
                }

                this.recent_notes.retain(|p| p != &path);
                this.recent_notes.insert(0, path.clone());
                if this.recent_notes.len() > 20 {
                    this.recent_notes.truncate(20);
                }

                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Note.md")
                    .to_string();
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let title = if matches!(ext.as_str(), "md" | "markdown") {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&filename)
                        .to_string()
                } else {
                    filename.clone()
                };
                let rel = normalize_rel_path(&this.vault_root, &path);
                let rel_folder = rel
                    .rsplit_once('/')
                    .map(|(dir, _)| dir.to_string())
                    .unwrap_or_else(|| {
                        this.vault_root
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("Vault")
                            .to_string()
                    });

                this.live_editor
                    .update(cx, |ed, cx| ed.set_text(content.clone(), cx));
                this.note_find_input
                    .update(cx, |inp, cx| inp.set_value("", window, cx));

                this.expand_ancestors_for_path(&path);
                this.drawer_target_dir = path.parent().map(Path::to_path_buf);
                if this.is_drawer_visible() {
                    let dw = this.drawer_width_for_window(window);
                    this.drawer_creating = None;
                    this.drawer_drag_active = false;
                    this.drawer_drag_last_at = None;
                    this.start_drawer_snap(false, -650.0, dw, cx);
                } else {
                    this.close_drawer_immediately();
                }
                this.quick_switcher_open = false;
                this.more_sheet_open = false;
                this.rename_mode = false;
                this.new_note_modal_open = false;
                this.surface = AppSurface::Notes;
                this.status_banner = None;
                this.live_editor_focused = start_in_edit;

                this.active_note = Some(ActiveNote {
                    path,
                    title,
                    filename,
                    rel_folder,
                    content,
                    wikilinks,
                    tags,
                    stats,
                    reading_only: false,
                    dirty: false,
                    saved_badge: None,
                    find_open: false,
                    delete_armed: false,
                });

                this.rebuild_visible_tree(cx);
                this.persist_workspace_state_async(cx);

                if start_in_edit {
                    this.live_editor.update(cx, |ed, cx| ed.focus(window, cx));
                } else {
                    window.blur(cx);
                    gpui_mobile::hide_keyboard();
                }

                cx.notify();
            });
        }));
    }

    pub fn close_active_note_to_home(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_active_note_sync(cx);
        window.blur(cx);
        gpui_mobile::hide_keyboard();
        self.more_sheet_open = false;
        self.rename_mode = false;
        self.active_note = None;
        self.live_editor_focused = false;
        self.persist_workspace_state_async(cx);
        cx.notify();
    }

    pub fn navigate_history_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(prev) = self.history_back.pop() {
            if prev.is_file() {
                if let Some(curr) = &self.active_note {
                    self.history_forward.push(curr.path.clone());
                }
                self.open_note(prev, false, false, window, cx);
                return;
            }
        }
    }

    pub fn navigate_history_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(next) = self.history_forward.pop() {
            if next.is_file() {
                if let Some(curr) = &self.active_note {
                    self.history_back.push(curr.path.clone());
                }
                self.open_note(next, false, false, window, cx);
                return;
            }
        }
    }

    pub fn open_wikilink(
        &mut self,
        raw_target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing) = self.vault_index.resolve_wikilink(raw_target) {
            self.open_note(existing, false, true, window, cx);
        } else {
            self.create_note_by_name(raw_target, None, window, cx);
        }
    }

    pub fn toggle_note_reading_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(currently_reading) = self.active_note.as_ref().map(|n| n.reading_only) else {
            return;
        };
        if !currently_reading {
            self.flush_active_note_sync(cx);
            let latest = self.live_editor.read(cx).text().to_string();
            if let Some(note) = &mut self.active_note {
                note.content = latest;
                note.wikilinks = extract_wikilinks(&note.content);
                note.tags = extract_tags(&note.content);
                note.stats = note_stats(&note.content);
                note.reading_only = true;
            }
            self.live_editor_focused = false;
            window.blur(cx);
            gpui_mobile::hide_keyboard();
        } else {
            if let Some(note) = &mut self.active_note {
                note.reading_only = false;
            }
            window.blur(cx);
            gpui_mobile::hide_keyboard();
        }
        cx.notify();
    }

    pub fn scroll_note_to_line(
        &mut self,
        line_0based: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(note) = &mut self.active_note {
            note.reading_only = false;
        }
        self.live_editor
            .update(cx, |ed, cx| ed.scroll_to_line(line_0based, window, cx));
        cx.notify();
    }

    pub fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        self.edit_revision = self.edit_revision.wrapping_add(1);
        let rev = self.edit_revision;

        self._autosave_task = Some(cx.spawn(async move |weak_self, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(700))
                .await;

            let maybe_write = weak_self
                .update(cx, |this, cx| {
                    if this.edit_revision != rev {
                        return None;
                    }
                    let note = this.active_note.as_ref()?;
                    if !note.dirty {
                        return None;
                    }
                    let latest = this.live_editor.read(cx).text().to_string();
                    Some((note.path.clone(), latest))
                })
                .ok()
                .flatten();

            let Some((path, latest)) = maybe_write else {
                return;
            };

            let write_path = path.clone();
            let write_text = latest.clone();
            let res = cx
                .background_executor()
                .spawn(async move {
                    atomic_write_string(&write_path, &write_text).map_err(|e| e.to_string())?;
                    let wikilinks = extract_wikilinks(&write_text);
                    let tags = extract_tags(&write_text);
                    let stats = note_stats(&write_text);
                    Ok::<_, String>((wikilinks, tags, stats))
                })
                .await;

            _ = weak_self.update(cx, |this, cx| {
                if let Some(note) = &mut this.active_note
                    && note.path == path
                {
                    match res {
                        Ok((wikilinks, tags, stats)) => {
                            if this.edit_revision == rev {
                                note.content = latest;
                                note.wikilinks = wikilinks;
                                note.tags = tags;
                                note.stats = stats;
                                note.dirty = false;
                                note.saved_badge = Some("Saved".to_string());
                            }
                        }
                        Err(err) => {
                            note.saved_badge = Some(format!("Save error: {err}"));
                        }
                    }
                    cx.notify();
                }
            });
        }));
    }

    pub fn flush_active_note_sync(&mut self, cx: &mut Context<Self>) {
        let Some(note) = &mut self.active_note else {
            return;
        };
        if !note.dirty {
            return;
        }
        let latest = self.live_editor.read(cx).text().to_string();
        match atomic_write_string(&note.path, &latest) {
            Ok(()) => {
                note.content = latest;
                note.wikilinks = extract_wikilinks(&note.content);
                note.tags = extract_tags(&note.content);
                note.stats = note_stats(&note.content);
                note.dirty = false;
                note.saved_badge = Some("Saved".to_string());
            }
            Err(err) => {
                note.saved_badge = Some(format!("Save error: {err}"));
            }
        }
    }

    pub fn apply_editor_format(
        &mut self,
        action: MarkdownFormatAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.live_editor
            .update(cx, |ed, cx| ed.apply_format(action, window, cx));
    }

    pub fn editor_undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.live_editor.update(cx, |ed, cx| {
            ed.focus(window, cx);
            ed.undo(cx);
        });
    }

    pub fn editor_redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.live_editor.update(cx, |ed, cx| {
            ed.focus(window, cx);
            ed.redo(cx);
        });
    }

    pub fn create_note_by_name(
        &mut self,
        raw_title: &str,
        target_dir: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flush_active_note_sync(cx);
        let base_dir = target_dir
            .filter(|d| d.is_dir())
            .or_else(|| self.drawer_target_dir.clone().filter(|d| d.is_dir()))
            .unwrap_or_else(|| self.vault_root.clone());
        let fname = sanitize_note_filename(raw_title);
        let path = base_dir.join(&fname);
        if !path.exists() {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled");
            let initial = format!("# {stem}\n\n");
            if let Err(err) = atomic_write_string(&path, &initial) {
                self.status_banner = Some(format!("Create failed: {err}"));
                cx.notify();
                return;
            }
        }
        self.refresh_vault_async(cx);
        self.open_note(path, true, true, window, cx);
    }

    pub fn open_today_daily_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let daily_dir = self.vault_root.join("Daily");
        let target_dir = if daily_dir.is_dir() {
            daily_dir
        } else {
            self.vault_root.clone()
        };
        let fname = today_daily_note_name();
        let path = target_dir.join(&fname);
        if !path.exists() {
            let date_stem = fname.trim_end_matches(".md");
            let initial = format!("# {date_stem}\n\n- [ ] \n");
            if let Err(err) = atomic_write_string(&path, &initial) {
                self.status_banner = Some(format!("Daily note failed: {err}"));
                cx.notify();
                return;
            }
            self.refresh_vault_async(cx);
        }
        self.open_note(path, true, true, window, cx);
    }

    pub fn open_new_note_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_active_note_sync(cx);
        self.close_drawer_immediately();
        self.quick_switcher_open = false;
        self.more_sheet_open = false;
        self.new_note_modal_open = true;
        self.new_note_input
            .update(cx, |inp, cx| inp.set_value("", window, cx));
        self.new_note_input
            .update(cx, |inp, cx| inp.focus(window, cx));
        gpui_mobile::show_keyboard();
        cx.notify();
    }

    pub fn submit_new_note_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.new_note_input.read(cx).value().to_string();
        self.new_note_modal_open = false;
        self.create_note_by_name(&raw, None, window, cx);
    }

    pub fn start_drawer_create(
        &mut self,
        kind: DrawerCreateKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drawer_creating = Some(kind);
        self.drawer_create_input
            .update(cx, |inp, cx| inp.set_value("", window, cx));
        self.drawer_create_input
            .update(cx, |inp, cx| inp.focus(window, cx));
        gpui_mobile::show_keyboard();
        cx.notify();
    }

    pub fn submit_drawer_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(kind) = self.drawer_creating.take() else {
            return;
        };
        let raw = self.drawer_create_input.read(cx).value().to_string();
        let base_dir = self
            .drawer_target_dir
            .clone()
            .filter(|d| d.is_dir())
            .unwrap_or_else(|| self.vault_root.clone());

        match kind {
            DrawerCreateKind::Note => {
                self.create_note_by_name(&raw, Some(base_dir), window, cx);
            }
            DrawerCreateKind::Folder => {
                match create_folder(&base_dir, &raw) {
                    Ok(folder_path) => {
                        let rel = normalize_rel_path(&self.vault_root, &folder_path);
                        if !rel.is_empty() {
                            self.expanded_dirs.insert(rel);
                        }
                        self.drawer_target_dir = Some(folder_path);
                        self.refresh_vault_async(cx);
                    }
                    Err(err) => {
                        self.status_banner = Some(format!("Folder error: {err}"));
                    }
                }
                window.blur(cx);
                gpui_mobile::hide_keyboard();
                cx.notify();
            }
        }
    }

    pub fn begin_rename_active_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = &self.active_note else {
            return;
        };
        let cur_title = note.title.clone();
        self.rename_mode = true;
        self.rename_note_input
            .update(cx, |inp, cx| inp.set_value(cur_title, window, cx));
        self.rename_note_input
            .update(cx, |inp, cx| inp.focus(window, cx));
        gpui_mobile::show_keyboard();
        cx.notify();
    }

    pub fn submit_rename_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_active_note_sync(cx);
        let new_title = self.rename_note_input.read(cx).value().to_string();
        let Some(old_path) = self.active_note.as_ref().map(|n| n.path.clone()) else {
            return;
        };
        match rename_note_file(&old_path, &new_title) {
            Ok(new_path) => {
                self.history_back.retain(|p| p != &old_path);
                self.history_forward.retain(|p| p != &old_path);
                self.recent_notes.retain(|p| p != &old_path);
                self.rename_mode = false;
                self.more_sheet_open = false;
                self.refresh_vault_async(cx);
                self.open_note(new_path, false, false, window, cx);
            }
            Err(err) => {
                self.status_banner = Some(format!("Rename failed: {err}"));
                cx.notify();
            }
        }
    }

    pub fn delete_active_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = &mut self.active_note else {
            return;
        };
        if !note.delete_armed {
            note.delete_armed = true;
            cx.notify();
            return;
        }
        let target_path = note.path.clone();
        if let Err(err) = fs::remove_file(&target_path) {
            self.status_banner = Some(format!("Delete failed: {err}"));
            note.delete_armed = false;
            cx.notify();
            return;
        }
        self.history_back.retain(|p| p != &target_path);
        self.history_forward.retain(|p| p != &target_path);
        self.recent_notes.retain(|p| p != &target_path);
        self.active_note = None;
        self.more_sheet_open = false;
        self.rename_mode = false;
        window.blur(cx);
        gpui_mobile::hide_keyboard();
        self.persist_workspace_state_async(cx);
        self.refresh_vault_async(cx);
        cx.notify();
    }

    pub fn open_item_context_menu(
        &mut self,
        target: ItemContextTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.blur(cx);
        gpui_mobile::hide_keyboard();
        self.item_context_menu = Some(target);
        self.context_rename_mode = false;
        self.context_delete_armed = false;
        cx.notify();
    }

    pub fn close_item_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.item_context_menu = None;
        self.context_rename_mode = false;
        self.context_delete_armed = false;
        window.blur(cx);
        gpui_mobile::hide_keyboard();
        cx.notify();
    }

    pub fn begin_context_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cur_name = match &self.item_context_menu {
            Some(ItemContextTarget::Note { display_name, .. })
            | Some(ItemContextTarget::Folder { display_name, .. }) => display_name.clone(),
            None => return,
        };
        self.context_rename_mode = true;
        self.context_delete_armed = false;
        self.rename_note_input
            .update(cx, |inp, cx| inp.set_value(cur_name, window, cx));
        self.rename_note_input
            .update(cx, |inp, cx| inp.focus(window, cx));
        gpui_mobile::show_keyboard();
        cx.notify();
    }

    pub fn submit_context_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.item_context_menu.clone() else {
            return;
        };
        let new_name = self.rename_note_input.read(cx).value().trim().to_string();
        if new_name.is_empty() {
            return;
        }
        match target {
            ItemContextTarget::Note { path, .. } => {
                self.flush_active_note_sync(cx);
                match rename_note_file(&path, &new_name) {
                    Ok(new_path) => {
                        for p in &mut self.history_back {
                            if *p == path {
                                *p = new_path.clone();
                            }
                        }
                        for p in &mut self.history_forward {
                            if *p == path {
                                *p = new_path.clone();
                            }
                        }
                        for p in &mut self.recent_notes {
                            if *p == path {
                                *p = new_path.clone();
                            }
                        }
                        let was_active = self
                            .active_note
                            .as_ref()
                            .is_some_and(|n| n.path == path);
                        self.item_context_menu = None;
                        self.context_rename_mode = false;
                        self.context_delete_armed = false;
                        window.blur(cx);
                        gpui_mobile::hide_keyboard();
                        self.refresh_vault_async(cx);
                        if was_active {
                            self.open_note(new_path, false, false, window, cx);
                        } else {
                            self.persist_workspace_state_async(cx);
                            cx.notify();
                        }
                    }
                    Err(err) => {
                        self.status_banner = Some(format!("Rename failed: {err}"));
                        cx.notify();
                    }
                }
            }
            ItemContextTarget::Folder {
                path,
                rel_path: old_rel,
                ..
            } => {
                if new_name.contains('/') || new_name.contains('\\') || new_name == "." || new_name == ".." {
                    self.status_banner = Some("Invalid folder name".to_string());
                    cx.notify();
                    return;
                }
                let Some(parent) = path.parent() else {
                    return;
                };
                let new_path = parent.join(&new_name);
                if new_path == path {
                    self.close_item_context_menu(window, cx);
                    return;
                }
                if new_path.exists() {
                    self.status_banner = Some(format!("Folder '{}' already exists", new_name));
                    cx.notify();
                    return;
                }
                self.flush_active_note_sync(cx);
                match fs::rename(&path, &new_path) {
                    Ok(()) => {
                        let new_rel = normalize_rel_path(&self.vault_root, &new_path);
                        let remap_under_old = |p: &mut PathBuf| {
                            if let Ok(suffix) = p.strip_prefix(&path) {
                                *p = new_path.join(suffix);
                            }
                        };
                        for p in &mut self.history_back {
                            remap_under_old(p);
                        }
                        for p in &mut self.history_forward {
                            remap_under_old(p);
                        }
                        for p in &mut self.recent_notes {
                            remap_under_old(p);
                        }
                        if self.expanded_dirs.remove(&old_rel) && !new_rel.is_empty() {
                            self.expanded_dirs.insert(new_rel);
                        }
                        if self
                            .drawer_target_dir
                            .as_ref()
                            .is_some_and(|d| d.starts_with(&path))
                        {
                            if let Some(d) = self.drawer_target_dir.as_mut() {
                                remap_under_old(d);
                            }
                        }
                        let active_moved = self
                            .active_note
                            .as_ref()
                            .and_then(|n| n.path.strip_prefix(&path).ok().map(|s| new_path.join(s)));
                        self.item_context_menu = None;
                        self.context_rename_mode = false;
                        self.context_delete_armed = false;
                        window.blur(cx);
                        gpui_mobile::hide_keyboard();
                        self.refresh_vault_async(cx);
                        if let Some(moved) = active_moved {
                            self.open_note(moved, false, false, window, cx);
                        } else {
                            self.persist_workspace_state_async(cx);
                            cx.notify();
                        }
                    }
                    Err(err) => {
                        self.status_banner = Some(format!("Rename folder failed: {err}"));
                        cx.notify();
                    }
                }
            }
        }
    }

    pub fn copy_context_wikilink(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ItemContextTarget::Note { display_name, .. }) = &self.item_context_menu else {
            return;
        };
        let wl = format!("[[{display_name}]]");
        cx.write_to_clipboard(ClipboardItem::new_string(wl.clone()));
        self.status_banner = Some(format!("Copied {wl}"));
        self.close_item_context_menu(window, cx);
    }

    pub fn duplicate_context_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ItemContextTarget::Note { path, .. }) = self.item_context_menu.clone() else {
            return;
        };
        self.flush_active_note_sync(cx);
        let Some(parent) = path.parent() else {
            return;
        };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Note");
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("md");
        let mut candidate = parent.join(format!("{stem} copy.{ext}"));
        let mut n = 2usize;
        while candidate.exists() {
            candidate = parent.join(format!("{stem} copy {n}.{ext}"));
            n += 1;
        }
        match fs::read_to_string(&path).and_then(|body| fs::write(&candidate, body)) {
            Ok(()) => {
                self.status_banner = Some(format!(
                    "Duplicated as {}",
                    candidate.file_name().and_then(|s| s.to_str()).unwrap_or("copy")
                ));
                self.close_item_context_menu(window, cx);
                self.refresh_vault_async(cx);
            }
            Err(err) => {
                self.status_banner = Some(format!("Duplicate failed: {err}"));
                cx.notify();
            }
        }
    }

    pub fn delete_context_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.context_delete_armed {
            self.context_delete_armed = true;
            cx.notify();
            return;
        }
        let Some(target) = self.item_context_menu.clone() else {
            return;
        };
        match target {
            ItemContextTarget::Note { path, .. } => {
                if let Err(err) = fs::remove_file(&path) {
                    self.status_banner = Some(format!("Delete failed: {err}"));
                    self.context_delete_armed = false;
                    cx.notify();
                    return;
                }
                self.history_back.retain(|p| p != &path);
                self.history_forward.retain(|p| p != &path);
                self.recent_notes.retain(|p| p != &path);
                if self.active_note.as_ref().is_some_and(|n| n.path == path) {
                    self.active_note = None;
                }
            }
            ItemContextTarget::Folder { path, rel_path, .. } => {
                if path == self.vault_root {
                    self.status_banner = Some("Cannot delete vault root folder".to_string());
                    self.context_delete_armed = false;
                    cx.notify();
                    return;
                }
                if let Err(err) = fs::remove_dir_all(&path) {
                    self.status_banner = Some(format!("Delete folder failed: {err}"));
                    self.context_delete_armed = false;
                    cx.notify();
                    return;
                }
                self.history_back.retain(|p| !p.starts_with(&path));
                self.history_forward.retain(|p| !p.starts_with(&path));
                self.recent_notes.retain(|p| !p.starts_with(&path));
                self.expanded_dirs.remove(&rel_path);
                if self
                    .drawer_target_dir
                    .as_ref()
                    .is_some_and(|d| d.starts_with(&path))
                {
                    self.drawer_target_dir = Some(self.vault_root.clone());
                }
                if self
                    .active_note
                    .as_ref()
                    .is_some_and(|n| n.path.starts_with(&path))
                {
                    self.active_note = None;
                }
            }
        }
        self.close_item_context_menu(window, cx);
        self.persist_workspace_state_async(cx);
        self.refresh_vault_async(cx);
    }

    pub fn context_create_in_folder(
        &mut self,
        kind: DrawerCreateKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ItemContextTarget::Folder { path, rel_path, .. }) =
            self.item_context_menu.clone()
        else {
            return;
        };
        if !rel_path.is_empty() {
            self.expanded_dirs.insert(rel_path);
        }
        self.drawer_target_dir = Some(path);
        self.item_context_menu = None;
        self.context_rename_mode = false;
        self.context_delete_armed = false;
        self.drawer_open = true;
        self.drawer_progress = 1.0;
        self.drawer_anim = None;
        self.drawer_drag_active = false;
        self.drawer_drag_last_at = None;
        self.start_drawer_create(kind, window, cx);
    }
}

impl Render for VKarmaMobileApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let epoch = crate::android_resume_epoch();
        if epoch != self.last_resume_epoch {
            self.last_resume_epoch = epoch;
            let access = crate::has_all_files_access();
            if access != self.has_storage_access {
                self.recheck_storage_and_refresh(cx);
            }
        }
        self.safe_area = Self::query_safe_area();
        self.drive_drawer_anim_frame(window, cx);
        let kb_height = gpui_mobile::keyboard_height();
        let bottom_inset = self.safe_area.bottom.max(kb_height);
        let swipe_entity = cx.entity().downgrade();

        let top_bg = crate::theme::kd::BG_DEEP;

        let body = match self.surface {
            AppSurface::Notes => {
                notes_view::render_notes_workspace(self, bottom_inset, window, cx)
                    .into_any_element()
            }
            AppSurface::Settings => {
                settings_view::render_settings_tab(self, cx).into_any_element()
            }
        };

        div()
            .image_cache(self.image_cache.clone())
            .size_full()
            .flex()
            .flex_col()
            .bg(crate::theme::hex_color(crate::theme::kd::BG_MAIN))
            .text_color(crate::theme::hex_color(crate::theme::kd::FG))
            .child(
                div()
                    .w_full()
                    .h(px(self.safe_area.top))
                    .flex_shrink_0()
                    .bg(crate::theme::hex_color(top_bg)),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .pl(px(self.safe_area.left))
                    .pr(px(self.safe_area.right))
                    .flex()
                    .flex_col()
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _cx| {
                                let app_swipe = swipe_entity.clone();
                                window.on_mouse_event(move |ev: &ScrollEvent, phase, window, cx| {
                                    if phase != DispatchPhase::Capture {
                                        return;
                                    }
                                    let _ = app_swipe.update(cx, |this, cx| {
                                        if this.drawer_drag_active || bounds.contains(&ev.position)
                                        {
                                            if this.handle_drawer_scroll_gesture(ev, window, cx) {
                                                cx.stop_propagation();
                                            }
                                        }
                                    });
                                });
                            },
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .flex()
                            .flex_col()
                            .child(body),
                    )
                    .when(
                        self.surface == AppSurface::Notes && self.is_drawer_visible(),
                        |frame| {
                            frame.child(notes_view::render_sidebar_drawer(
                                self, window, cx,
                            ))
                        },
                    )
                    .when(
                        self.surface == AppSurface::Notes && self.item_context_menu.is_some(),
                        |frame| {
                            frame.child(notes_view::render_item_context_menu(
                                self,
                                bottom_inset,
                                cx,
                            ))
                        },
                    )
                    .when(
                        self.surface == AppSurface::Notes && self.quick_switcher_open,
                        |frame| {
                            frame.child(notes_view::render_quick_switcher(
                                self,
                                bottom_inset,
                                window,
                                cx,
                            ))
                        },
                    )
                    .when(
                        self.surface == AppSurface::Notes && self.new_note_modal_open,
                        |frame| {
                            frame.child(notes_view::render_new_note_modal(
                                self,
                                bottom_inset,
                                cx,
                            ))
                        },
                    )
                    .when(
                        self.surface == AppSurface::Notes && self.more_sheet_open,
                        |frame| {
                            frame.child(notes_view::render_more_sheet(
                                self,
                                bottom_inset,
                                cx,
                            ))
                        },
                    )
                    .when(self.folder_picker.is_some(), |frame| {
                        frame.child(settings_view::render_folder_picker_modal(
                            self,
                            bottom_inset,
                            cx,
                        ))
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_drawer_snap_open;

    #[test]
    fn resolve_drawer_snap_open_handles_drags_flicks_and_cancellations() {
        assert!(!resolve_drawer_snap_open(false, 0.20, 40.0, true));
        assert!(resolve_drawer_snap_open(false, 0.30, 0.0, true));
        assert!(resolve_drawer_snap_open(false, 0.06, 420.0, true));
        assert!(!resolve_drawer_snap_open(false, 0.02, 500.0, true));

        assert!(resolve_drawer_snap_open(true, 0.80, -40.0, true));
        assert!(!resolve_drawer_snap_open(true, 0.70, 0.0, true));
        assert!(!resolve_drawer_snap_open(true, 0.94, -420.0, true));
        assert!(resolve_drawer_snap_open(true, 0.98, -500.0, true));

        assert!(!resolve_drawer_snap_open(false, 0.40, 800.0, false));
        assert!(resolve_drawer_snap_open(true, 0.60, -800.0, false));
    }
}
