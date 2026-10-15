# Ambience

macOS player for long background music and ambience files: one file at a
time, picks up where you left off.

Built with Rust and [GPUI](https://www.gpui.rs) /
[gpui-component](https://github.com/longbridge/gpui-component). Playback is
AVFoundation, so it plays what QuickTime plays (m4a, mp3, …).

## Install

macOS:

```
brew install --cask fsvehla/tap/ambience
```

Or download the macOS zip from [Releases](https://github.com/fsvehla/bgm-audio/releases).
Requires macOS 14.2. Updates through Sparkle (Ambience → Check for Updates…).

Windows (10 and 11, x64 or arm64): download `Ambience-<version>-windows-<arch>.zip`
from [Releases](https://github.com/fsvehla/bgm-audio/releases), unzip it anywhere
and run `Ambience.exe`. The exe is not code-signed, so SmartScreen asks once: More
info → Run anyway. Updates through WinSparkle (⋯ → Check for Updates…).

Not on Windows: the menu bar (the ⋯ button has the same menus), "Open With" from
Explorer, and pausing while the microphone is in use.

## Features

- Chapters from the file's metadata: list, markers on the progress bar,
  previous/next chapter
- Cover art in the window and in Now Playing
- Now Playing, media keys and Control Center. Skip buttons go to the
  previous/next chapter, or ±10 s without chapters
- Remembers the last file, the position in every file, volume, loop,
  appearance and window frame. Recent files list. Moved files are found again
- Pauses while another app uses the microphone (calls), resumes afterwards.
  Can be turned off
- Mini player mode
- Light/dark follows the system, or is set in the View menu. Accent colour
  follows the system

## Usage

Open a file with ⌘O, by dropping it on the window or the Dock icon, from
Finder (Open With → Ambience), or by passing the path:
`Ambience.app/Contents/MacOS/Ambience <file>`. Ambience does not become the
default audio player; to make it one, use Get Info → Open with → Change All.

| Key | |
|---|---|
| Space | Play/pause |
| ← / → | −10 s / +10 s |
| ⇧← / ⇧→ | Previous/next chapter |
| ↑ / ↓ | Volume |
| ⌘O | Open |
| ⌘⇧M | Mini player on/off (also Window → Mini Player) |
| ⌘M | Minimize |

State is kept in `~/Library/Application Support/bgm-audio/state.json`.

## Build

```
cargo run --release -- <file>          # no bundle, no updater
packaging/bundle.sh release            # target/release/Ambience.app
```

`bundle.sh` signs with a Developer ID certificate from the keychain if there is
one. `NOTARIZE=1` also notarizes (notarytool keychain profile `notary`).

Windows release builds run on GitHub Actions (`.github/workflows/windows.yml`):
GPUI precompiles its shaders with `fxc.exe`, which needs a Windows host. On macOS,
`cargo xwin build --target {x86_64,aarch64}-pc-windows-msvc` (with Homebrew's
`llvm` and `lld` on `PATH`) checks that the Windows code compiles.

GPUI comes from `gpui-pre` (weekly snapshots of Zed's GPUI) at the exact version
gpui-component pins; bump both together.

## Release

1. Bump the version in `Cargo.toml` and `packaging/Info.plist`
   (`CFBundleShortVersionString`, `CFBundleVersion`).
2. Write `packaging/notes/<version>.md`.
3. Commit and push, then run `packaging/release.sh` (`DRY_RUN=1` to build
   without publishing).

The script waits for the Windows CI run of the same commit, builds and notarizes
the macOS app, signs all zips for Sparkle/WinSparkle, creates the GitHub release
with the zips, `appcast.xml` and `appcast-windows.xml`, and bumps the cask in
[fsvehla/homebrew-tap](https://github.com/fsvehla/homebrew-tap).
