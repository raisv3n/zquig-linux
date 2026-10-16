# Zquig for Linux

A lightweight screenshot annotation app for Linux (X11), written in Rust.

Press a shortcut to grab your screen, a region or a window, then mark it up with arrows, boxes, text, numbered steps, a highlighter or pixelation, and save or paste it anywhere. Zquig also opens existing images and PDFs, and adds a Quick Look-style **Space to preview** to your file manager.

This is the Linux port of [Zquig for Windows](https://github.com/raisv3n/zquig-rs), itself a port of [Zquig for macOS](https://github.com/raisv3n/Zquig). It has the same editor plus macOS-style global screenshot shortcuts.

[![CI](https://github.com/raisv3n/zquig-linux/actions/workflows/ci.yml/badge.svg)](https://github.com/raisv3n/zquig-linux/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/raisv3n/zquig-linux)](https://github.com/raisv3n/zquig-linux/releases/latest)

## Contents

- [Install](#install)
- [Screenshot shortcuts](#screenshot-shortcuts)
- [Space to preview](#space-to-preview)
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

**Requirements:** a 64-bit (x86-64) Linux desktop running an **X11 (Xorg) session**. Wayland isn't supported yet. Tested on Lubuntu 24.04 (LXQt); it should work on any X11 desktop with a system tray.

### Debian, Ubuntu and derivatives (.deb)

1. Download `zquig_<version>-1_amd64.deb` from the [latest release](https://github.com/raisv3n/zquig-linux/releases/latest).
2. Install it:

   ```sh
   sudo apt install ./zquig_*_amd64.deb
   ```

3. Start **Zquig** from the app menu. It lives in the system tray.

- **Update:** install the new `.deb` the same way.
- **Uninstall:** `sudo apt remove zquig`. Your settings in `~/.config/zquig` are left for you to delete if you want.

### Any distribution (tarball)

Download `Zquig-linux-x86_64.tar.gz`, then:

```sh
tar -xzf Zquig-linux-x86_64.tar.gz
cd Zquig
./install.sh
```

This copies Zquig to `~/.local/bin`, adds it to the app menu and the file manager's **Open with** list, and starts it in the tray. No root needed. Run `./install.sh --uninstall` to remove it.

You need GTK 3 and the usual X11 libraries, which every desktop has. Opening PDFs needs `poppler-utils` (`sudo apt install poppler-utils`).

Check your download first if you like; see [Verifying a download](SECURITY.md#verifying-a-download).

## Screenshot shortcuts

While Zquig is running (it lives in the tray), these work from any app:

| Shortcut | What it captures | Where it goes |
| --- | --- | --- |
| **Ctrl+Shift+1** | A region you drag (press Space to pick a window) | Clipboard |
| **Ctrl+Shift+2** | The full screen | Clipboard |
| **Ctrl+Shift+3** or **Print Screen** | The full screen | JPEG in `~/Pictures/Screenshots` |
| **Ctrl+Shift+4** | A region you drag (press Space to pick a window) | JPEG in `~/Pictures/Screenshots` |

These mirror the Mac's Cmd+Shift+3 and Cmd+Shift+4. Files are named like macOS screenshots, for example `Screenshot 2026-10-15 at 22.09.33.jpg`, and only you can read them. The Pictures folder follows your desktop's setting (`xdg-user-dir PICTURES`).

**Picking a region:** the screen freezes and dims. Drag to select; the size in pixels follows the cursor. Press **Space** to switch to window mode and click a window to capture it. Press Space again to go back to dragging. **Esc** or a right-click cancels.

**After each capture**, a thumbnail appears in the bottom-right corner for about five seconds. Click it to open the shot in the editor. Hovering keeps it on screen.

**Multiple monitors:** full-screen captures include every monitor in one image, and regions can be picked on any monitor.

**Print Screen** is usually taken by the desktop's own screenshot tool (on LXQt, ScreenGrab). Zquig then leaves it alone, and the tray's **Shortcuts** menu says so. To use it for Zquig, remove or change the Print entry in your desktop's shortcut settings (LXQt: **Preferences → LXQt Settings → Shortcut Keys**), then restart Zquig.

## Space to preview

Like Quick Look on the Mac: in the file manager, select an image and press **Space** to see it in a preview window.

- **Space** or **Esc** closes the preview. **← / →** step through a multiple selection.
- **Enter**, a double-click or **Open in Zquig** opens the file in the editor.
- It works in PCManFM-Qt (including the desktop), PCManFM, Thunar, Nemo, Caja and Dolphin.

**It only previews files that Zquig opens by default.** If double-clicking a file opens it in another app, Space does what it always did in your file manager. To include a file type, make Zquig its default: right-click a file → **Properties** (or **Open with**) → choose Zquig and set it as the default.

**How it works:** file managers don't tell other apps which file is selected, so when you press Space in one, Zquig sends it Ctrl+C, reads the copied file and puts your previous clipboard back. When Space isn't for a preview (a folder, another app's file, or typing while renaming or in the path bar), Zquig passes the Space on, along with anything you typed in the meantime, in order. Two side effects: files you had *cut* come back as *copied*, and formatted text on the clipboard comes back as plain text.

## The editor

An editor window opens when you start Zquig, open a file, paste an image, capture from the editor's menus, click a capture thumbnail or choose **Open in Zquig** in a preview.

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

**In a Space preview:** Space or Esc closes it, ← / → step through files, Enter opens the file in the editor.

## Opening and saving files

| | Formats |
| --- | --- |
| **Open** | PNG, JPEG, BMP, GIF, TIFF, WebP, ICO, PDF |
| **Save** | PNG, JPEG, TIFF, PDF |

- **Ways to open:** File → Open, **Open with → Zquig** in the file manager, a Space preview's **Open in Zquig**, or dropping a file onto an editor window (which replaces its image).
- **PDFs** open with up to 50 pages stacked into one tall image, rendered by poppler (`pdftoppm`). Password-protected PDFs can't be opened.
- **JPEG and PDF** have no transparency, so transparent areas are saved as white.
- If Zquig is already running, opening a file from the file manager hands it to the running copy instead of starting a second one.

## The tray icon

Zquig keeps running in the tray after you close every editor window, so the shortcuts stay available. Left-click the icon to bring windows forward, or open a new one if none are open. Right-click it for:

- New from Clipboard
- Capture Region, Capture Window, Capture Full Screen (into the editor)
- Open…, Open Screenshots Folder
- **Shortcuts:** a reminder of the global shortcuts
- **Start at Login:** starts Zquig quietly in the tray when you log in (`~/.config/autostart/zquig.desktop`)
- Quit Zquig

The tray icon uses the StatusNotifierItem standard, which LXQt, KDE, Xfce, Cinnamon and MATE panels support. GNOME needs the AppIndicator extension.

## Privacy and security

- **Nothing leaves your computer.** Zquig has no networking code at all.
- **No keylogging.** The global shortcuts are X11 key grabs, so Zquig only learns when its own shortcuts are pressed. The one exception is Space to preview: while a file manager is focused, Zquig receives Space presses, and while it checks one (a fraction of a second) it holds the keys you type so it can pass them on in order. It never stores or records them.
- **The screen is captured only when you ask:** by a shortcut, a menu command or the tray.
- **Runs without root.** Only installing the `.deb` needs `sudo`; Zquig itself only writes to your own home folder.

See [SECURITY.md](SECURITY.md) for details, how to verify downloads, and how to report a vulnerability.

## Troubleshooting

**"Some screenshot shortcuts are unavailable" when Zquig starts.** Another app already uses that combination. Change it in that app or in your desktop's shortcut settings, then restart Zquig.

**"Zquig captures the screen through X11" when taking a screenshot.** You're in a Wayland session. Log out and pick an X11 (Xorg) session on the login screen.

**No tray icon.** Your panel needs a StatusNotifierItem tray: on LXQt, add the **Status Notifier** widget to the panel; on GNOME, install the AppIndicator extension.

**Space doesn't preview.** Check that Zquig is the default app for that file type (see [Space to preview](#space-to-preview)) and that you're in a supported file manager. Nautilus has its own preview (`gnome-sushi`) and isn't hooked.

**PDFs don't open.** Install poppler: `sudo apt install poppler-utils`.

**The editor window is blank, or Zquig doesn't start.** Zquig draws with Vulkan, falling back to OpenGL. Make sure your graphics drivers are installed (`mesa-vulkan-drivers` on most Intel and AMD systems). To force a backend for testing, set the environment variable `WGPU_BACKEND` to `vulkan` or `gl` before starting Zquig.

## Building from source

You need [Rust](https://rustup.rs) 1.88 or later, a C toolchain and the GTK 3 and xkbcommon headers. On Debian or Ubuntu:

```sh
sudo apt install build-essential pkg-config libgtk-3-dev libxkbcommon-dev poppler-utils
git clone https://github.com/raisv3n/zquig-linux
cd zquig-linux
cargo build --release
./target/release/zquig
```

Other useful commands:

| Command | What it does |
| --- | --- |
| `cargo test` | Runs the tests: the editing model (undo/redo, crop, text editing, paste), rendering of every tool, hostile clipboard data, and saving and reloading each format. PDF is checked by reading the exported file back through poppler. |
| `./install.sh` | Installs the release build you just made into `~/.local/bin` |
| `./package.sh` | Builds the release and creates the `.deb` and the tarball in `dist/` (needs `cargo install cargo-deb`) |
| `cargo audit` | Checks dependencies for known vulnerabilities (needs `cargo install cargo-audit`) |

### Releases

Releases are built by GitHub Actions ([release.yml](.github/workflows/release.yml)). To publish one, set `version` in `Cargo.toml`, commit, then push a matching tag:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The workflow runs the tests, builds the `.deb` and the tarball, writes `SHA256SUMS.txt`, records a signed build-provenance attestation, and publishes the release.

### UI testing

Debug builds can replay scripted input (clicks, drags, typing, keys) into the first editor window, so the real UI code can be exercised without moving the mouse. Set `ZQUIG_REPLAY` to a script file; [src/replay.rs](src/replay.rs) documents the format. This hook is compiled out of release builds.

## How it's built

Zquig shares its editor with the Windows version; everything that talks to the operating system is in one file per platform ([src/os/linux.rs](src/os/linux.rs), [src/os/windows.rs](src/os/windows.rs)).

| Part | macOS version | Linux version |
| --- | --- | --- |
| Windows and UI | AppKit | [winit](https://github.com/rust-windowing/winit) + [egui](https://github.com/emilk/egui), drawn with [wgpu](https://wgpu.rs) (Vulkan, or OpenGL) |
| Drawing annotations | Core Graphics | [tiny-skia](https://github.com/linebender/tiny-skia), shared by the screen and exports so what you see is what you save |
| Text | NSString drawing, SF Pro | [cosmic-text](https://github.com/pop-os/cosmic-text), the desktop's sans-serif (via fontconfig) |
| Screen capture | `screencapture` | X11 through [x11rb](https://github.com/psychon/x11rb): one root-window grab, RandR monitors, the window manager's window list |
| Global shortcuts | — | X11 key grabs via [global-hotkey](https://github.com/tauri-apps/global-hotkey) |
| Space to preview | Quick Look | X11 key grab + XTEST + XFixes clipboard tracking |
| PDF | PDFKit | poppler (`pdfinfo`, `pdftoppm`) to open, a small built-in writer to save |
| Tray, dialogs, clipboard | — | [tray-icon](https://github.com/tauri-apps/tray-icon) (StatusNotifierItem), [rfd](https://github.com/PolyMeilex/rfd) (GTK 3), [arboard](https://github.com/1Password/arboard) |

### Project layout

```
src/
  main.rs        App: windows, tray icon, global shortcuts, capture flow, opening files
  document.rs    Editor window: menus, toolbar, style panel, zoomable canvas, saving
  canvas.rs      Document model: annotations, undo/redo, selection, mouse behaviour, text editing
  annotation.rs  Tools, annotation geometry, hit-testing, clipboard format
  render.rs      Draws annotations (shared by the screen and exports)
  text.rs        Text layout and glyph rasterising
  capture.rs     Capture modes, the frozen desktop, Screenshots file names
  overlay.rs     Full-screen region/window picker
  toast.rs       Thumbnail shown after a shortcut capture
  peek.rs        The Space-to-preview window
  imageio.rs     Image/PDF loading, PNG/JPEG/TIFF/PDF export, clipboard images
  system.rs      Preferences and single instance
  tiles.rs       Shows large images as GPU tiles
  replay.rs      Debug builds only: scripted input for UI testing
  os/linux.rs    X11 capture, desktop entries, poppler, file-manager preview
  os/windows.rs  The Windows equivalents (kept so fixes move easily between the two repos)
packaging/       Desktop entry and icons for the .deb
install.sh       Per-user installer and uninstaller
package.sh       Builds the .deb and the tarball
```

Settings (last colour, thickness and text border choice) are stored in `~/.config/zquig/prefs.json`.

## License

Personal use only. See [LICENSE](LICENSE). This is not an open-source license.
