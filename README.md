# Zquig for Windows

A lightweight screenshot annotation app for Windows 10 and 11, written in Rust.

Press a shortcut to grab your screen, a region or a window, then mark it up with arrows, boxes, text, numbered steps, a highlighter or pixelation, and save or paste it anywhere. Zquig also opens existing images and PDFs.

This is the Windows port of [Zquig for macOS](https://github.com/raisv3n/Zquig). It has the same editor plus macOS-style global screenshot shortcuts.

[![CI](https://github.com/raisv3n/zquig-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/raisv3n/zquig-rs/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/raisv3n/zquig-rs)](https://github.com/raisv3n/zquig-rs/releases/latest)

## Contents

- [Install](#install)
- [Screenshot shortcuts](#screenshot-shortcuts)
- [The editor](#the-editor)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Opening and saving files](#opening-and-saving-files)
- [The tray icon](#the-tray-icon)
- [Privacy and security](#privacy-and-security)
- [Troubleshooting](#troubleshooting)
- [Building from source](#building-from-source)
- [How it's built](#how-its-built)
- [License](#license)

## Install

**Requirements:** Windows 10 or 11, 64-bit (x64). No admin rights are needed.

1. Download `Zquig-windows.zip` from the [latest release](https://github.com/raisv3n/zquig-rs/releases/latest).
2. Unzip it.
3. In the unzipped folder, open PowerShell (Shift + right-click → **Open PowerShell window here**, or type `powershell` in the folder's address bar) and run:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\install.ps1
   ```

The installer copies Zquig to `%LOCALAPPDATA%\Programs\Zquig`, adds **Zquig** to the Start menu, adds it to Explorer's **Open with** menu for images and PDFs, and starts it in the system tray.

- **Update:** download the new release and run the same command. Your settings are kept.
- **Uninstall:** run `.\install.ps1 -Uninstall`. This removes the app, the Start menu shortcut and its registry entries. Your settings in `%APPDATA%\Zquig` are left for you to delete if you want.
- **Without installing:** `zquig.exe` is a single self-contained file, so you can also just run it from anywhere.

The exe isn't code-signed yet, so the first time you run it Windows SmartScreen may say "Windows protected your PC". Choose **More info → Run anyway**. You can check that your download is genuine first; see [Verifying a download](SECURITY.md#verifying-a-download).

## Screenshot shortcuts

While Zquig is running (it lives in the tray), these work from any app:

| Shortcut | What it captures | Where it goes |
| --- | --- | --- |
| **Ctrl+Shift+1** | A region you drag (press Space to pick a window) | Clipboard |
| **Ctrl+Shift+2** | The full screen | Clipboard |
| **Ctrl+Shift+3** or **Print Screen** | The full screen | JPEG in `Pictures\Screenshots` |
| **Ctrl+Shift+4** | A region you drag (press Space to pick a window) | JPEG in `Pictures\Screenshots` |

These mirror the Mac's Cmd+Shift+3 and Cmd+Shift+4. Files are named like macOS screenshots, for example `Screenshot 2026-10-15 at 22.09.33.jpg`.

**Picking a region:** the screen freezes and dims. Drag to select; the size in pixels follows the cursor. Press **Space** to switch to window mode and click a window to capture it. Press Space again to go back to dragging. **Esc** or a right-click cancels.

**After each capture**, a thumbnail appears in the bottom-right corner for about five seconds. Click it to open the shot in the editor. Hovering keeps it on screen.

**Multiple monitors:** full-screen captures include every monitor in one image, and regions can be picked on any monitor.

**Print Screen** on Windows 11 opens the Snipping Tool by default. To use it for Zquig, turn off **Settings → Accessibility → Keyboard → "Use the Print screen key to open screen capture"**. Zquig picks the key up automatically; no restart needed. Until then, the tray's **Shortcuts** menu shows that the Snipping Tool has the key.

## The editor

An editor window opens when you start Zquig, open a file, paste an image, capture from the editor's menus or click a capture thumbnail.

### Tools

| Tool | Key | What it does |
| --- | --- | --- |
| Select | A | Click to select, drag to move, drag the round handles to resize |
| Arrow | Q | Drag to draw an arrow. Hold Shift to snap horizontal or vertical |
| Line | | Like the arrow, without a head |
| Rectangle | | Drag to draw an outlined box |
| Ellipse | | Drag to draw an outlined ellipse |
| Pen | | Freehand drawing |
| Highlighter | | A translucent marker box that keeps the text underneath readable |
| Text | T | Click to place text. Return adds a line; Ctrl+Enter, Esc or clicking elsewhere finishes |
| Numbered stamp | | Click to drop numbered circles (1, 2, 3…) for step-by-step guides |
| Pixelate | | Drag over anything you want to hide, like names or email addresses |
| Crop | | Drag the area to keep |

### Working with annotations

- Everything you draw stays editable: select a shape to move it, resize it, recolour it or change its thickness.
- **Double-click** text to edit it. Drag a text box's side handles to change where it wraps.
- **Alt+drag** draws a new shape even when you start on top of an existing one.
- **Arrow keys** nudge the selected shape by 1 px, or by 10 px with Shift.
- **Copy and paste** shapes, within a window or between windows. Pasted copies are offset slightly so they don't sit exactly on top.
- **Double-click** an empty spot to start typing text there.
- Pasting text from another app adds it as a text annotation. Pasting an image opens it in a new window.

### Colour and thickness

Click the coloured circle in the toolbar for the style panel:

- **Colour:** nine presets (red, orange, mustard, green, blue, purple, black, white, gray) plus a custom colour picker.
- **Thickness:** five sizes. Text size follows the thickness.
- **Text border & shadow:** a white outline and soft shadow that keep text readable on any background. On by default.

Changing the style applies to the selected shape and to new ones. Zquig remembers your last colour, thickness and text border setting.

### Viewing

Large images are zoomed to fit when they open. **Ctrl+mouse wheel** zooms around the cursor, and the **View** menu has Zoom In, Zoom Out, Actual Size and Zoom to Fit.

### Getting the image out

- **Save** (Ctrl+S) overwrites the file you opened. Captures and pasted images ask where to save.
- **Save As** (Ctrl+Shift+S) saves PNG, JPEG, TIFF or PDF.
- **Copy Whole Image** (Ctrl+Shift+C) copies the annotated image to the clipboard. Ctrl+C does the same when nothing is selected.
- **Drag out:** drag the share icon at the right of the toolbar into Explorer, an email, a chat app or a document to drop in the annotated image as a PNG.

Closing a window with unsaved changes asks whether to save them.

## Keyboard shortcuts

**Anywhere (global):** see [Screenshot shortcuts](#screenshot-shortcuts).

**In an editor window:**

| Action | Shortcut |
| --- | --- |
| New from clipboard (or a blank canvas) | Ctrl+N |
| Capture region / window / full screen into a new editor window | Ctrl+Alt+R / Ctrl+Alt+W / Ctrl+Alt+F |
| Open | Ctrl+O |
| Save / Save As | Ctrl+S / Ctrl+Shift+S |
| Copy whole image | Ctrl+Shift+C |
| Undo / Redo | Ctrl+Z / Ctrl+Y or Ctrl+Shift+Z |
| Cut / Copy / Paste a shape | Ctrl+X / Ctrl+C / Ctrl+V |
| Delete selected shape | Delete or Backspace |
| Deselect | Esc |
| Select / Text / Arrow tool | A / T / Q |
| Nudge selection | Arrow keys (Shift for 10 px) |
| Zoom in / out | Ctrl+= / Ctrl+- or Ctrl+mouse wheel |
| Zoom to fit / Actual size | Ctrl+0 / Ctrl+1 |
| Close window | Ctrl+W |
| Quit Zquig | Ctrl+Q |

The editor's capture commands hide Zquig's own windows first, so they don't end up in the shot.

## Opening and saving files

| | Formats |
| --- | --- |
| **Open** | PNG, JPEG, BMP, GIF, TIFF, WebP, ICO, PDF |
| **Save** | PNG, JPEG, TIFF, PDF |

- **Ways to open:** File → Open, **Open with → Zquig** in Explorer, or dropping a file onto an editor window (which replaces its image).
- **PDFs** open with up to 50 pages stacked into one tall image, rendered by Windows' built-in PDF engine. Password-protected PDFs can't be opened.
- **JPEG and PDF** have no transparency, so transparent areas are saved as white.
- If Zquig is already running, opening a file from Explorer hands it to the running copy instead of starting a second one.

## The tray icon

Zquig keeps running in the tray after you close every editor window, so the shortcuts stay available. Left-click the icon to bring windows forward, or open a new one if none are open. Right-click it for:

- New from Clipboard
- Capture Region, Capture Window, Capture Full Screen (into the editor)
- Open…, Open Screenshots Folder
- **Shortcuts:** a reminder of the global shortcuts
- **Start with Windows:** starts Zquig quietly in the tray when you sign in
- Quit Zquig

## Privacy and security

- **Nothing leaves your computer.** Zquig has no networking code at all.
- **No keylogging.** Global shortcuts use Windows' hotkey registration, so Zquig only learns when its own shortcuts are pressed.
- **The screen is captured only when you ask:** by a shortcut, a menu command or the tray.
- **No admin rights**, and it only writes to your own user profile.

See [SECURITY.md](SECURITY.md) for details, how to verify downloads, and how to report a vulnerability.

## Troubleshooting

**"Some screenshot shortcuts are unavailable" when Zquig starts.** Another app already uses that combination. Close or reconfigure that app, then restart Zquig.

**Print Screen does nothing.** See [Print Screen](#screenshot-shortcuts) above: Windows 11 gives it to the Snipping Tool unless you turn that off.

**Windows protected your PC / SmartScreen warning.** The exe isn't code-signed yet. Choose **More info → Run anyway**, after [verifying the download](SECURITY.md#verifying-a-download) if you like.

**The Start menu or taskbar shows an old icon after updating.** That's Windows' icon cache. Unpin and re-pin, or run `ie4uinit.exe -show`.

**The editor window is blank, or Zquig doesn't start.** Zquig draws with DirectX 12, falling back to OpenGL. Update your graphics driver. To force a backend for testing, set the environment variable `WGPU_BACKEND` to `dx12` or `gl` before starting Zquig.

## Building from source

You need:

- [Rust](https://rustup.rs) 1.88 or later (stable, MSVC toolchain)
- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the **Desktop development with C++** workload

```powershell
git clone https://github.com/raisv3n/zquig-rs
cd zquig-rs
cargo build --release
.\target\release\zquig.exe
```

If Cargo says `link.exe` isn't found even though the Build Tools are installed, build from the **x64 Native Tools Command Prompt for VS 2022**. Don't build from Git Bash: its `link` command shadows the MSVC linker.

Other useful commands:

| Command | What it does |
| --- | --- |
| `cargo test` | Runs the tests: the editing model (undo/redo, crop, text editing, paste), rendering of every tool, hostile clipboard data, and saving and reloading each format. PDF is checked by reading the exported file back through Windows' PDF engine. |
| `.\install.ps1` | Installs the release build you just made |
| `.\package.ps1` | Builds the release and creates `dist\Zquig-windows.zip` |
| `cargo audit` | Checks dependencies for known vulnerabilities (needs `cargo install cargo-audit`) |

### Releases

Releases are built by GitHub Actions ([release.yml](.github/workflows/release.yml)). To publish one, set `version` in `Cargo.toml`, commit, then push a matching tag:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

The workflow runs the tests, builds the zip, writes `SHA256SUMS.txt`, records a signed build-provenance attestation, and publishes the release.

### UI testing

Debug builds can replay scripted input (clicks, drags, typing, keys) into the first editor window, so the real UI code can be exercised without moving the mouse. Set `ZQUIG_REPLAY` to a script file; [src/replay.rs](src/replay.rs) documents the format. This hook is compiled out of release builds.

## How it's built

Zquig is a port of the Swift/AppKit app. It keeps the same document model and drawing code, with Windows equivalents for the macOS pieces:

| Part | macOS version | Windows version |
| --- | --- | --- |
| Windows and UI | AppKit | [winit](https://github.com/rust-windowing/winit) + [egui](https://github.com/emilk/egui), drawn with [wgpu](https://wgpu.rs) (DirectX 12) |
| Drawing annotations | Core Graphics | [tiny-skia](https://github.com/linebender/tiny-skia), shared by the screen and exports so what you see is what you save |
| Text | NSString drawing, SF Pro | [cosmic-text](https://github.com/pop-os/cosmic-text), Segoe UI |
| Screen capture | `screencapture` | GDI desktop capture plus DWM window bounds |
| Global shortcuts | — | `RegisterHotKey` via [global-hotkey](https://github.com/tauri-apps/global-hotkey) |
| PDF | PDFKit | Windows.Data.Pdf to open, a small built-in writer to save |
| Tray, dialogs, clipboard | — | [tray-icon](https://github.com/tauri-apps/tray-icon), [rfd](https://github.com/PolyMeilex/rfd), [arboard](https://github.com/1Password/arboard) |

### Project layout

```
src/
  main.rs        App: windows, tray icon, global shortcuts, capture flow, opening files
  document.rs    Editor window: menus, toolbar, style panel, zoomable canvas, saving
  canvas.rs      Document model: annotations, undo/redo, selection, mouse behaviour, text editing
  annotation.rs  Tools, annotation geometry, hit-testing, clipboard format
  render.rs      Draws annotations (shared by the screen and exports)
  text.rs        Text layout and glyph rasterising
  capture.rs     Screen capture, window list, monitors, Screenshots folder
  overlay.rs     Full-screen region/window picker
  toast.rs       Thumbnail shown after a shortcut capture
  imageio.rs     Image/PDF loading, PNG/JPEG/TIFF/PDF export, clipboard images
  system.rs      Preferences, Open-with registration, start-up entry, single instance
  tiles.rs       Shows large images as GPU tiles
  replay.rs      Debug builds only: scripted input for UI testing
assets/          App icon and Windows manifest
build.rs         Builds the .ico and embeds the icon and manifest into the exe
install.ps1      Per-user installer and uninstaller
package.ps1      Builds the release zip
```

Settings (last colour, thickness and text border choice) are stored in `%APPDATA%\Zquig\prefs.json`.

## License

Personal use only. See [LICENSE](LICENSE). This is not an open-source license.
