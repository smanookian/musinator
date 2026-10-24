# Musinator

A modern-looking music player for the terminal. Linux/Omarchy first.
Status: planned (2026-10-24). Idea list: `~/Projects/ideas/IDEAS.md`.

## 1. What it is
- TUI music player. Keyboard first, mouse optional.
- Looks modern: framed panes, live themes, real album art, a visualizer.
- Free, MIT (price can change later).
- Sources, in this order:
  1. **v1: local music files** (FLAC, MP3, AAC/M4A, Ogg/Opus, WAV, ALAC).
  2. v2: URLs and servers: internet radio / HTTP streams, Subsonic/Navidrome, maybe MPD.
  3. v3: Spotify (needs Premium; via librespot or similar).
  4. Later: more (YouTube Music, Jellyfin, ...).
  Design a `Source` interface from day one so v2/v3 plug in without rewrites. Build only v1 now.

## 2. Design inspiration
Borrow ideas (not code) from Allison's Tide apps: https://github.com/allisonhere/tide , https://github.com/allisonhere/tidemail , https://github.com/allisonhere/tideui
- Panes with full 4-sided borders. Focused pane = accent border. Selected row always visible, also in unfocused panes (`→` marker).
- Each pane has a title bar with its own key hints on the right: `enter play  a queue  / search`.
- One quiet status bar at the bottom: state on the left, `/ search · q queue · t themes · S settings · ? help` on the right.
- Floating dialogs (settings, theme picker, search, help) with a soft drop shadow.
- Theme picker that previews live while you move; `esc` reverts.
- Many built-in themes (Catppuccin, Tokyo Night, Rosé Pine, Gruvbox, Nord, Dracula, ...) + `match-omarchy` that follows the Omarchy theme live.
- Contrast checked: text 4.5:1, borders/selection 3:1, in every theme (automated test).
- **No config file editing needed.** Every setting is in the app (config file still exists for power users).
- Compact / comfortable density.
- Images: real album art on Kitty/Ghostty (kitty graphics), half-block fallback elsewhere, text placeholder if off.
- Help (`?`) inside the app, searchable.
Do not copy her code. If any of her MIT code is ever reused, keep her license notice and credit her in the README.

## 3. Screens
Mockups: `design/library.svg` (A), `design/now-playing.svg` (B + mini + bar).

### A. Library (main screen)
- Left pane: Now Playing, Queue (count), Library (Artists, Albums, Tracks, Genres, Folders), Playlists (with counts), Smart lists (Most played, Never played, Added this week). Radio appears here in v2.
- Upper right: the list for what is selected (albums, tracks of an album, playlist, search results).
- Lower right: now-playing panel: cover art, title, artist · album · year, progress bar with times, controls state (shuffle/repeat), audio info (`FLAC 24/96 · gapless · RG −6.2 dB`), small spectrum visualizer.
- Status bar.
- Album grid view (covers as a grid) as an option for Albums.

### B. Now Playing (full screen)
- Big cover art, title/artist/album/track number.
- Synced lyrics (`.lrc` next to the file or embedded); current line highlighted. Hidden if none.
- Big progress bar, controls, volume.
- Full-width visualizer (`v` toggles).
- Right side: Up next (queue), reorder with `J/K`, remove with `d`.

### Switching
- **`z` toggles A ↔ B.** `esc` in B goes back to A.
- Panes in A resizable with `shift+←/→`.

### Mini mode (v1.1) and bar
- Mini mode: 3 lines (title/artist, progress, controls + time) for a small terminal window.
- `musinator --bar`: Waybar/Omarchy-bar JSON (current song + time). Settings toggle "Show in Omarchy bar", like Tide.

## 4. Keys (defaults, all remappable in settings)
| Key | Action |
|-----|--------|
| `space` | play / pause |
| `enter` | play selected |
| `n` / `p` | next / previous |
| `←` / `→` | seek −5 s / +5 s (`shift` = 30 s) |
| `+` / `-` | volume |
| `s` / `r` | shuffle / repeat (off, all, one) |
| `a` / `A` | add to queue / play next |
| `/` | search everything (artists, albums, tracks, playlists) |
| `z` | Library ↔ Now Playing |
| `l` / `v` | lyrics / visualizer |
| `q` | queue |
| `t` | themes |
| `S` | settings |
| `?` | help |
| `tab` | next pane; `j/k` move; `g/G` top/bottom |
Quit: `ctrl+c` or `Q` (with no confirm while music plays? decide).

## 5. Audio (the difference; Steve is an acoustics engineer)
- Gapless playback.
- ReplayGain (track/album) from tags; fallback: measure loudness (EBU R128) during scan and store it.
- Show real format: codec, sample rate, bit depth, channels. Show when the output resamples.
- Bit-perfect mode where the system allows (exclusive output / no resampling). [Check what PipeWire allows.]
- 10-band EQ with a visible curve (v1.1). Presets + custom.
- Visualizer from the real audio stream (FFT), smooth, low CPU.

## 6. Library
- Pick music folders in settings (default `~/Music`). Scan in background, watch for changes.
- Read tags (incl. embedded cover, `cover.jpg`/`folder.jpg`, lyrics).
- Local SQLite database: tracks, play counts, last played, ratings/likes.
- Playlists: create/edit in app; import/export `.m3u8`.
- Fast: 50,000 tracks scan in the background; UI never blocks.

## 7. Desktop integration
- MPRIS (D-Bus) so media keys, `playerctl` and the Omarchy media controls work.
- Desktop notification on track change (optional).
- `--bar` for the Omarchy bar / Waybar.
- Headless commands: `musinator play|pause|next|prev|status --json`.

## 8. Tech (proposal, builder can push back)
- Rust + ratatui (UI), symphonia (decode), cpal or PipeWire (output), lofty (tags), ratatui-image (kitty graphics + half-blocks), zbus/mpris-server (MPRIS), rusqlite (DB).
- Reason: good audio decoding and low-latency output in Rust; ratatui is mature.
- Alternative: Go + Bubble Tea + Lipgloss (same stack as Tide). Weaker audio side.
- One binary. AUR package (`musinator-bin`) + install script.

## 9. Not in v1
Servers, radio, Spotify (v2/v3). EQ, mini mode and tag editing (v1.1, see below). Last.fm scrobbling. Podcasts. Cloud sync.

### v1.1 plan (decided 2026-10-24, build in this order)
1. Full-screen lyrics view.
2. Word-by-word karaoke (enhanced LRC with word times).
3. Mini mode (3 lines).
4. Tag editing, easy to hard: fill tags from the file name; edit one song (title, artist, album, album artist, year, track, disc, genre); edit a whole album; change/add cover art; look up tags online (MusicBrainz). Saves safely (write a copy, then replace).
5. 10-band EQ with a visible curve, presets + custom.

Already added after v1: terminal color sync, sleep timer, lyrics offset, resume on start, star ratings, online lyrics on a key (LRCLIB, cached; optional `.lrc` next to songs).

## 10. Done when (v1)
1. Install from AUR or one script on Omarchy; `musinator` opens Library.
2. Scans `~/Music` with 10,000+ test files without freezing the UI.
3. Plays FLAC, MP3, M4A, Ogg/Opus, WAV gapless; seek works.
4. `z` switches A ↔ B; both match the mockups in look and layout.
5. Album art shows as real images on Kitty/Ghostty, half-blocks in other terminals.
6. Theme picker previews live; `match-omarchy` follows an Omarchy theme change live.
7. Contrast test passes for every built-in theme.
8. Media keys and `playerctl` work (MPRIS). `--bar` prints valid JSON.
9. Every setting can be changed in the app; every key can be remapped.
10. ReplayGain works; audio info line shows the real format and when it resamples.
11. Idle CPU near 0%; visualizer on stays light (target < 5% of one core).

## 11. Existing players (checked 2026-10-24)
| Player | ★ | Lang | Note |
|--------|---|------|------|
| cmus https://github.com/cmus/cmus | 6.3k | C | classic, fast, plain look |
| musikcube https://github.com/clangen/musikcube | 4.9k | C++ | player + server |
| rmpc https://github.com/mierak/rmpc | 3.4k | Rust | modern MPD client, album art |
| kew https://github.com/ravachol/kew | 3.2k | C | "immersive", local files |
| ncmpcpp https://github.com/ncmpcpp/ncmpcpp | 2.5k | C++ | MPD client |
| termusic https://github.com/tramhao/termusic | 2.2k | Rust | local + podcasts |
| spotify-player / ncspot | 7.3k / 6.8k | Rust | Spotify only |
Crowded. Musinator must win on look (Tide-style polish, themes, Omarchy fit) **and** audio quality (section 5). One source app for local → servers → Spotify is also rare.

## 12. Open questions
- Name: "Musinator". GitHub: 3 repos, all 0 stars (2022, 2022, 2026). AUR: none. App stores not checked.
- Rust or Go (section 8).
- Paid later? (e.g. Pro: EQ, Spotify, servers). Free for now.
