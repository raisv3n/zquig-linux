# Security

## Reporting a problem

Please report security issues privately through GitHub's **Report a vulnerability** button on the [Security tab](https://github.com/raisv3n/zquig-rs/security), not in a public issue. Include what you found, how to reproduce it and which version you used. I'll reply as soon as I can.

## What Zquig does and doesn't do

- **No network access.** Zquig contains no networking code. Screenshots, images and settings never leave your computer.
- **No keylogging.** The screenshot shortcuts use Windows' `RegisterHotKey`, so Zquig is only told when one of its own shortcuts is pressed. It never sees other keystrokes.
- **Screen capture only on request.** The screen is captured only when you press a shortcut or choose a capture command.
- **No admin rights.** Zquig and its installer only write to your own user profile: `%LOCALAPPDATA%\Programs\Zquig`, `%APPDATA%\Zquig`, your Start menu, and per-user registry keys (`HKEY_CURRENT_USER`) for "Open with" and "Start with Windows".
- **Files it writes:** screenshots in `Pictures\Screenshots`, files you save, preferences in `%APPDATA%\Zquig`, and short-lived PNGs in `%TEMP%` while you drag an image into another app (removed 30 seconds after the drop).

## Hardening

- Anything Zquig reads from another app is treated as untrusted: annotations and text pasted from the clipboard, images, PDFs, and requests from a second Zquig launch. Sizes are capped so a crafted file or clipboard entry can't make it allocate unbounded memory.
- The channel a second launch uses to hand files to the running copy has a random per-user name stored in `%APPDATA%`, so other accounts on the same PC can't take it over or watch which files you open. It only accepts existing local file paths.
- Dependencies are pinned in `Cargo.lock` and checked against the RustSec advisory database on every dependency change and weekly. Accepted advisories, with reasons, are listed in [.cargo/audit.toml](.cargo/audit.toml).
- GitHub Actions use read-only tokens except where a release is published, and every action is pinned to a full commit SHA.

## Verifying a download

Each release lists SHA-256 checksums in `SHA256SUMS.txt` and carries a signed build-provenance attestation showing it was built by this repository's release workflow. To check a download in PowerShell:

```powershell
Get-FileHash .\Zquig-windows.zip -Algorithm SHA256
```

Or, with the [GitHub CLI](https://cli.github.com):

```powershell
gh attestation verify .\Zquig-windows.zip --repo raisv3n/zquig-rs
```

The exe isn't code-signed yet, so Windows SmartScreen may warn the first time you run it.
