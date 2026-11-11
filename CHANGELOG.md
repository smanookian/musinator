# Changelog

The section for a version becomes its release notes on GitHub and in the update window (`U`).

## 0.1.5

### Fixed

- The first music folder is the one your system names (`~/Musik` on a German system, from `~/.config/user-dirs.dirs`), not always `~/Music`. Already set up? Add the folder in Settings → Library → Music folders.
- Settings → Appearance → Album art → **Kitty images** now really uses pictures, even when the terminal does not report Kitty support.
- Exported playlists are suggested in your first music folder.

## 0.1.4

### Fixed

- The update window shows its last line again ("then your queue comes back") and keeps the same size while it installs.
- Windows opened over album covers no longer leave striped lines next to them in Ghostty and Kitty.

## 0.1.3

### Fixed

- Song notifications really show the album cover now, also in the Omarchy shell. It is sent as the notification's picture, not as its icon.

## 0.1.2

### New

- Updates from inside Musinator. When a new version is out, the bottom line says so; press `U` to see what's new and install it, then restart with `r`. Your queue comes back.
- At the first start Musinator asks once whether it may check GitHub for updates once a day. Change it in Settings → Updates, or use Check now there.
- `musinator update` does the same from the command line.
- The install script checks the download's SHA-256 checksum.

### Fixed

- Song notifications show the album cover instead of the Musinator logo when the cover was not loaded yet.

## 0.1.1

- New logo and app icon.
- New demo recording.

## 0.1.0

- First release.
