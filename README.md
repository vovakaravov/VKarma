<p align="center">
  <img src="assets/vkarma.svg" alt="VKarma" width="88" height="88" />
</p>

<h1 align="center">VKarma</h1>

<p align="center">
  Native, GPU-accelerated Rust + <a href="https://gpui.rs">GPUI</a> all-in-one workspace for Markdown &amp; MDX notes, Fountain screenplays, Typst documents, spreadsheets, code, and 12 native format studios — styled in <strong>Amethyst Obsidian</strong> with zero Chromium/Electron overhead.
</p>

---

## Features

- **Blender-Style Workspace Tabs:** Top-bar workspaces where each workspace binds its own root folder, sidebar/panel layout, Markdown mode defaults, open tabs, split state, and navigation history — persisting cleanly across launches and reopening in your last active workspace. Create with `+`, **drag to reorder**, or right-click to rename, rebind folder, duplicate, or delete.
- **Unified Live Markdown & MDX Editor:**
  - Native GPUI **Live Markdown Editor (`Edit`)**, **`Split`**, and **`Reading`** views plus **Source Mode (`Ctrl+M`)** for raw Markdown.
  - Inline styling and block rendering for YAML frontmatter, wikilinks (`[[Note]]` / `[[Note|Alias]]`), image embeds (`![[img.png|width|align]]`), tables, code fences, callouts, blockquotes, task checkboxes, `==highlight==`, smart typography, single-Enter line breaks, and chapter pagination for long fiction.
  - **Zen Mode (`F11` / `Ctrl+Alt+Z`):** Toggleable from the bottom-right status bar or keyboard shortcut for distraction-free writing with a readable centered column and hover-reveal exit button.
- **Sidebar, Vault Search & Multi-Pane Navigation:**
  - **Drag-and-drop** files and folders to move entries directly in the sidebar, press **`F2` or double-click** for inline rename, or right-click to reveal in your file manager.
  - **Vault Search (`Ctrl+Shift+F`)** searches both filenames and file contents across the workspace, alongside **Quick Open (`Ctrl+P`)** and Enter-to-submit modals.
  - Mix any document types side-by-side (`Ctrl+\` or middle-click in sidebar), close tabs with `Ctrl+W` or middle-click, cycle tabs with `Ctrl+Tab`, and jump back/forward via mouse side buttons or `Alt+←` / `Alt+→`.
- **Fountain Screenplay Studio (`.fountain`):** Structured screenplay parser, live formatted script preview, scene navigator, and Typst-backed PDF export.
- **Typst Live Studio (`.typ`):** Live Typst editing with side-by-side PNG preview and PDF export.
- **Native PDF Preview (`.pdf`):** Built-in multi-page PDF rendering (`pdfinfo` / `pdftoppm`) with quick external open.
- **Spreadsheets (`.xlsx`, `.csv`, `.tsv`):** Multi-sheet workbook editor powered by `ironcalc` with formula evaluation and cell formatting.
- **12 Native Format Studios:**
  - **Canvas** (`.canvas`) — infinite spatial node canvas
  - **Kanban** (`.kanban`, `.kanban.md`) — interactive columns and cards
  - **Subtitles** (`.srt`, `.vtt`) — cue timeline and subtitle editor
  - **Structured Data Tree & Table** (`.json`, `.jsonc`, `.yaml`, `.yml`, `.toml`)
  - **SVG Studio** (`.svg`) — native `resvg` vector preview & source
  - **Mermaid Diagrams** (`.mmd`, `.mermaid`) — native diagram renderer
  - **Office Documents** (`.docx`, `.odt`) — document reader
  - **EPUB Reader** (`.epub`) — book & chapter viewer
  - **SQLite Inspector** (`.sqlite`, `.sqlite3`, `.db`) — schema & table browser, SQL inspection, and CSV export
  - **Archive Browser** (`.zip`, `.tar`, `.tar.gz`, `.tgz`) — archive contents viewer
  - **Audio Studio** (`.mp3`, `.wav`, `.ogg`, `.flac`, `.m4a`, `.opus`) — metadata/waveform view and PipeWire/PulseAudio playback (`pw-play` / `paplay`)
  - **Font Inspector** (`.ttf`, `.otf`) — specimen and glyph viewer via `ttf-parser`
- **Code & Config Editor:** Tree-sitter syntax highlighting across Rust, C#, C/C++, Python, TypeScript/TSX, JavaScript, Go, Zig, Lua, SQL, Bash, TOML, YAML, JSON, HTML, CSS, Make, CMake, Diff, and Typst.

---

## Keybindings

| Shortcut | Action |
| :--- | :--- |
| `Ctrl+Alt+1..9` | Switch to Workspace 1–9 |
| `Ctrl+PageUp` / `Ctrl+PageDown` | Previous / Next Workspace |
| `Ctrl+Alt+N` | Create New Workspace |
| `Ctrl+P` | Quick Open File in Active Workspace |
| `Ctrl+Shift+F` | Search Filenames & Contents in Active Workspace |
| `Ctrl+E` | Cycle View Mode (Edit / Split / Reading) |
| `Ctrl+M` | Toggle Markdown Source Mode |
| `F11` / `Ctrl+Alt+Z` | Toggle Zen Mode |
| `Ctrl+\` | Split Active Tab to Right Pane |
| `Ctrl+Tab` | Switch to Next Tab |
| `Ctrl+W` / Middle-click | Close Tab |
| `Alt+←` / `Alt+→` / Mouse4 / Mouse5 | Navigate Back / Forward |
| `F2` / Double-click | Rename Selected Sidebar Entry Inline |
| `Ctrl+B` / `Ctrl+Shift+B` | Toggle Left Sidebar / Right Outline & Backlinks Panel |
| `Ctrl+S` / `Ctrl+Shift+S` | Save Active Tab / Save All |
| `Ctrl+Shift+O` | Bind Current Workspace Folder |
| `Ctrl+O` / `Ctrl+N` | Open File(s) / New Note |
| `Escape` | Close Active Modal |

---

## Building & Installing

Requires a Rust toolchain (Edition 2024) and system dependencies for GPUI on Linux (Vulkan, Wayland/X11, Fontconfig).

Optional runtime tools for specialized studios:
- `typst` on `PATH` for `.typ` / `.fountain` preview & PDF export
- `poppler` (`pdfinfo`, `pdftoppm`) for native `.pdf` preview
- `sqlite3` for `.sqlite` / `.db` inspection
- `ffmpeg` / `ffprobe` and `pw-play` or `paplay` for the Audio Studio

```bash
git clone https://github.com/vovakaravov/VKarma.git
cd VKarma
cargo test
cargo build --release

mkdir -p ~/.local/bin
ln -sf "$(pwd)/target/release/vkarma" ~/.local/bin/vkarma
```

### Optional: Linux `.desktop` Entry (KDE / Wayland)

VKarma sets its Wayland `app_id` to `vkarma` so desktop environments match its icon cleanly:

```bash
mkdir -p ~/.local/share/icons/hicolor/scalable/apps ~/.local/share/applications
cp assets/vkarma.svg ~/.local/share/icons/hicolor/scalable/apps/vkarma.svg

cat > ~/.local/share/applications/vkarma.desktop <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=VKarma
GenericName=Workspace & Office Suite
Comment=Native GPU-accelerated Markdown Vault, Typst Studio, Spreadsheet & Code Workspace
Exec=$HOME/.local/bin/vkarma %F
Icon=$HOME/.local/share/icons/hicolor/scalable/apps/vkarma.svg
Terminal=false
Categories=Office;TextEditor;
Keywords=markdown;obsidian;mdx;fountain;screenplay;typst;spreadsheet;xlsx;csv;notes;editor;
MimeType=text/markdown;text/plain;text/csv;application/vnd.openxmlformats-officedocument.spreadsheetml.sheet;
StartupNotify=true
StartupWMClass=vkarma
EOF
```

---

## License

MIT — see [LICENSE](LICENSE).
