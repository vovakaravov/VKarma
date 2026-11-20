# VKarma

A native, GPU-accelerated Rust + [GPUI](https://www.gpui.rs/) all-in-one workspace for Markdown & MDX notes, Fountain screenplays, Typst documents, spreadsheets, and code — styled in **Kanagawa Dragon** with zero Chromium/Electron overhead.

## Features

- **Blender-Style Workspace Tabs:** Top-bar workspaces where each workspace binds its own root folder, sidebar/panel layout, Markdown mode defaults, open tabs, split state, and navigation history — persisting cleanly across launches and reopening in your last active workspace. Create with `+`, right-click to rename, rebind folder, duplicate, reorder, or delete.
- **Obsidian-Style Markdown & MDX Editing:**
  - **Live Preview (`Edit`), `Split`, and `Reading`** views plus **Source Mode (`Ctrl+M`)** for raw Markdown.
  - Inline block editing with virtualized list scrolling, wikilinks (`[[Note]]` / `[[Note|Alias]]`), image embeds (`![[img.png|width|align]]`), callouts, task checkboxes, `==highlight==`, smart typography, single-Enter line breaks, and chapter pagination for long fiction.
- **Fountain Screenplay Studio (`.fountain`):** Structured screenplay parser, live formatted script preview, scene navigator, and Typst-backed PDF export.
- **Typst Live Studio (`.typ`):** Live Typst editing and side-by-side PNG preview + PDF export.
- **Spreadsheets (`.xlsx`, `.csv`):** Multi-sheet workbook editor powered by `ironcalc` with formula evaluation and cell formatting.
- **Code & Config Editor:** Tree-sitter syntax highlighting across Rust, C#, C/C++, Python, TypeScript/TSX, JavaScript, Go, Zig, Lua, SQL, Bash, TOML, YAML, HTML, CSS, Make, CMake, Diff, and Typst.
- **Multi-Pane Splits & Navigation:** Mix any file types side-by-side (`Ctrl+\` or middle-click file in sidebar), close tabs via `X` or middle-click, and jump back/forward across documents via mouse side buttons or `Alt+←` / `Alt+→`.

## Keybindings

| Shortcut | Action |
| --- | --- |
| `Ctrl+Alt+1..9` | Switch to Workspace 1–9 |
| `Ctrl+PageUp` / `Ctrl+PageDown` | Previous / Next Workspace |
| `Ctrl+Alt+N` | Create New Workspace |
| `Ctrl+P` | Quick Open File in Active Workspace |
| `Ctrl+Shift+F` | Search in Active Workspace |
| `Ctrl+E` | Cycle View Mode (Edit / Split / Reading) |
| `Ctrl+M` | Toggle Markdown Source Mode |
| `Ctrl+\` | Split Active Tab to Right Pane |
| `Ctrl+W` / Middle-click | Close Tab |
| `Alt+←` / `Alt+→` / Mouse4 / Mouse5 | Navigate Back / Forward |
| `Ctrl+B` / `Ctrl+Shift+B` | Toggle Left Sidebar / Right Outline & Backlinks Panel |
| `Ctrl+S` / `Ctrl+Shift+S` | Save Active Tab / Save All |
| `Ctrl+Shift+O` | Bind Current Workspace Folder |
| `Ctrl+O` / `Ctrl+N` | Open File(s) / New Note |

## Building & Installing

Requires a Rust toolchain (Edition 2024) and system dependencies for GPUI on Linux (Vulkan, Wayland/X11, Fontconfig) plus `typst` on `PATH` for `.typ` / `.fountain` PDF/preview compilation.

```bash
cargo test
cargo build --release
ln -sf "$(pwd)/target/release/vkarma" ~/.local/bin/vkarma
```

## License

MIT — see [LICENSE](LICENSE).
