# Security

## Reporting a problem

Please report security issues privately through GitHub's **Report a vulnerability** button on the [Security tab](https://github.com/raisv3n/zquig-linux/security), not in a public issue. Include what you found, how to reproduce it and which version you used. I'll reply as soon as I can.

## What Zquig does and doesn't do

- **No network access.** Zquig contains no networking code. Screenshots, images and settings never leave your computer.
- **No keylogging.** The screenshot shortcuts are X11 key grabs, so Zquig is only told when one of its own shortcuts is pressed. The exception is [Space to preview](README.md#space-to-preview): while a file manager window is focused, Zquig receives Space presses. When one arrives it holds the keyboard for at most half a second (usually a few hundredths) while it checks the selection, then passes any keys typed in that time on to the file manager, in order. If focus moved to another window meanwhile, those keys are dropped rather than typed into the wrong window. Nothing is stored or logged.
- **Clipboard use for Space to preview.** To learn which file is selected, Zquig sends the file manager Ctrl+C and reads the copied file list, then restores what was on the clipboard before. It reads the clipboard only at that moment, and only while a file manager is focused.
- **Screen capture only on request.** The screen is captured only when you press a shortcut or choose a capture command.
- **No root.** Zquig runs as you and only writes to your home folder. Only installing the `.deb` needs `sudo`.
- **Files it writes:** screenshots in `~/Pictures/Screenshots` (readable only by you), files you save, preferences in `~/.config/zquig`, a login item in `~/.config/autostart` if you turn on **Start at Login**, and, when not installed from the `.deb`, a menu entry and icon in `~/.local/share`. While a PDF opens, its pages are rendered into a private, randomly named folder in `/tmp` that is removed straight away.

## Hardening

- Anything Zquig reads from another app is treated as untrusted: annotations and text pasted from the clipboard, images, PDFs, and requests from a second Zquig launch. Sizes are capped so a crafted file or clipboard entry can't make it allocate unbounded memory.
- PDFs are rendered by poppler's command-line tools in a separate process, with arguments passed directly (no shell), a 60-second time limit and capped output.
- Files are saved through a freshly created temporary file that is then renamed into place, so a file or symlink planted next to the target can't redirect the write.
- The channel a second launch uses to hand files to the running copy is an abstract socket with a random per-user name kept in `~/.config/zquig/instance-key` (readable only by you), so other accounts on the same computer can't take it over or watch which files you open. It only accepts existing local file paths.
- Desktop entries are written with quoted, escaped paths, and refused if a path contains control characters.
- Dependencies are pinned in `Cargo.lock` and checked against the RustSec advisory database on every dependency change and weekly. Accepted advisories, with reasons, are listed in [.cargo/audit.toml](.cargo/audit.toml).
- GitHub Actions use read-only tokens except where a release is published, every action is pinned to a full commit SHA, and the tools the workflows install are pinned to exact versions.

## Verifying a download

Each release lists SHA-256 checksums in `SHA256SUMS.txt` and carries a signed build-provenance attestation showing it was built by this repository's release workflow. To check a download:

```sh
sha256sum -c SHA256SUMS.txt --ignore-missing
```

Or, with the [GitHub CLI](https://cli.github.com):

```sh
gh attestation verify zquig_0.1.0-1_amd64.deb --repo raisv3n/zquig-linux
```

The packages aren't GPG-signed, and there's no apt repository yet, so updates are installed by downloading the new `.deb`.
