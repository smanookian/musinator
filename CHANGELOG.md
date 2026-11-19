# Changelog

The section for a version becomes its release notes on GitHub and in the update window (`U`).

## 0.1.12

### New

- Open songs and folders: `musinator song.flac` or `musinator ~/Music/Album`. If Musinator is already running, they play there.
- File managers can open audio files with Musinator ("Open with", or as the default app). Files outside your music folders show their tags too.

## 0.1.11

### New

- Tag editor: `ctrl+c` copies the album tags (Album, Album artist, Year, Genre) and the cover; `ctrl+v` pastes them into another song or album. Check, then `enter` saves.
- The album grid stretches its covers to fill the whole width.

### Fixed

- A changed Year is really saved in MP3s that also carry a recording date (players showed the old year before).

## 0.1.10

### Fixed

- Tag editor: the cursor no longer pushes the text apart (it showed as a gap). The letter under the cursor is highlighted instead.

## 0.1.9

### Fixed

- Tag editor (`e`): fields have a real cursor now. `←`/`→`, `home`/`end` move it; typing, `backspace` and `delete` work where it is. Before, you could only add or remove at the end.

## 0.1.8

### New

- Konsole 26.08 and newer get real album covers by themselves. Older Konsole: choose Settings → Appearance → Album art → Kitty images.

## 0.1.7

### Fixed

- **Kitty images** (Settings → Appearance → Album art) now also works inside tmux, whatever terminal tmux runs in, for example Konsole. Restart Musinator after choosing it.

## 0.1.6

### Fixed

- Real album covers inside tmux, when tmux runs in Kitty or Ghostty. Before, tmux always got half-blocks.

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
