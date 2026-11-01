# Musinator Guide

This guide shows how to use Musinator, step by step. For a short overview and
install steps, see the [README](../README.md).

Every setting is inside the app. You never need to edit a file.

## Contents

- [First start](#first-start)
- [The screen](#the-screen)
- [Moving around](#moving-around)
- [Playing music](#playing-music)
- [Library](#library)
- [Now Playing](#now-playing)
- [Radio](#radio)
- [Podcasts](#podcasts)
- [Music servers](#music-servers)
- [Last.fm](#lastfm)
- [Audio](#audio)
- [Themes](#themes)
- [Settings](#settings)
- [Keys](#keys)
- [Desktop](#desktop)
- [Command line](#command-line)
- [Files](#files)
- [Problems and fixes](#problems-and-fixes)

## First start

Run:

```sh
musinator
```

What happens:

- Musinator scans `~/Music` in the background. The status bar shows
  `scanning 120/4000` while it works.
- After the scan, it measures the loudness of songs without ReplayGain tags.
  The status bar shows `measuring loudness 40%`.
- The **Albums** list opens.
- On Omarchy, the theme is **Match Omarchy**. Elsewhere it is **Catppuccin Mocha**.

Your music is somewhere else? Press `S`, go to **Library → Music folders**,
press `enter`, then:

- `a` adds a folder (`~/` works).
- `d` removes the selected folder.
- `esc` closes the window and scans again.

Musinator scans these files: `.flac`, `.mp3`, `.m4a`, `.mp4`, `.m4b`, `.aac`,
`.ogg`, `.oga`, `.opus`, `.wav`, `.wave`, `.aif`, `.aiff`.

New files show up by themselves while **Watch for changes** is on. To scan by
hand, press `ctrl+r`.

When you quit, Musinator saves the queue and the position. Next time it brings
them back, paused. Turn this off with **Playback → Resume where you left off**.

Only one Musinator can run at a time.

## The screen

Musinator has two screens. `z` switches between them. `esc` goes back.

**Library**

- Left: the **Library** pane, with all your sources.
- Right, top: the open list.
- Right, bottom: the **Now Playing** panel (cover, progress, buttons,
  audio line, visualizer). It hides when the window is short.

**Now Playing**

- Left: big cover, title, lyrics and visualizer.
- Right: **Up next**, the rest of the queue.

**Focus.** The pane with the bright border has focus. Keys like `j` and `k`
work in that pane. `tab` goes to the next pane, `shift+tab` to the one before.

**Hints in pane titles.** Each pane title shows the keys that work there, for
example `enter play  a queue  A next  / search`. If you change a key, the hint
changes too. You can click a hint to press that key.

**Status bar.** The bottom line shows:

- left: state, songs in the queue and volume, e.g.
  `♪ playing · 12 in queue · vol 70%`, plus scan progress, sleep timer and
  online lookups. Short messages (like `Shuffle on`) show here for a few seconds.
- right: `/ search · q queue · t themes · S settings · ? help`.

**Mouse.** Click the player buttons (⏮ ⏸ ⏭, shuffle, repeat), the progress
bar (jumps to that spot), any key hint, and rows in lists and windows. The
wheel scrolls.

**Help.** `?` lists every key. Type to filter the list.

## Moving around

| Key | What it does |
|-----|--------------|
| `j` / `k` (or `↓` / `↑`) | down / up |
| `g` / `G` (or `home` / `end`) | top / bottom |
| `pagedown` / `pageup` (or `ctrl+d` / `ctrl+u`) | one page down / up |
| `tab` / `shift+tab` | next / previous pane |
| `enter` | open the list, or play the song |
| `esc` / `backspace` | back to the list before (or click the `‹` title) |
| `<` / `>` | make the left pane narrower / wider |

In the Library pane, moving onto an entry opens its list right away.

## Playing music

| Key | What it does |
|-----|--------------|
| `enter` | play the selected song (the rest of the list follows) |
| `space` | play / pause |
| `n` / `p` | next / previous song |
| `←` / `→` | seek 5 s back / forward |
| `shift+←` / `shift+→` | seek 30 s back / forward |
| `+` (or `=`) / `-` | volume up / down by 5% |
| `s` | shuffle on / off |
| `r` | repeat: off, all, one |
| `ctrl+t` | sleep timer |

Notes:

- `p` restarts the song if it has played for more than 3 seconds.
- Seek steps can be changed in **Playback → Seek step** and **Big seek step**.
- Shuffle mixes only the songs after the current one. Turning it off brings back the old order.
- Sleep timer choices: 15, 30, 45, 60 or 90 minutes, end of this song, end of
  this album. The status bar shows `sleep 25m`. A timer pauses; "end of" stops.

### The queue

| Key | What it does |
|-----|--------------|
| `a` | add the selected song, album, artist or list to the end of the queue |
| `A` | play it next |
| `q` | show the queue |
| `J` / `K` | move the selected item down / up |
| `d` / `delete` | remove it |
| `X` | clear the queue |

`space` with an empty queue plays the selected item.

## Library

The Library pane holds:

- **Now Playing** and **Queue**
- **Library**: Artists, Albums, Tracks, Genres, Folders
- **Playlists**
- **Radio** (see [Radio](#radio))
- one section per music server (see [Music servers](#music-servers))
- **Podcasts** (see [Podcasts](#podcasts))
- **Last.fm**, when you are logged in (see [Last.fm](#lastfm))
- **Smart**: Most played, Never played, Added this week, Liked, Top rated

### Album grid

`o` shows albums as a grid of covers. Press it again for the list. `j` / `k`
move one row of covers. Click a cover to select it. The setting is
**Appearance → Albums as grid**.

### Search

`/` searches everything at once. Results come in groups: Artists, Albums,
Songs, Playlists, songs on your music servers (`On <server>`), and radio
Stations (from 3 letters).

| Key | What it does |
|-----|--------------|
| `↑` / `↓` (or `tab` / `shift+tab`) | move |
| `enter` | open or play |
| `ctrl+a` | add to the queue |
| `esc` | close |

### Filter

`ctrl+f` filters the open list. Type words in any order; all must match.

- `enter` opens the selected row.
- `esc` shows the whole list again.
- `ctrl+u` clears what you typed.

The queue and playlists can't be filtered.

### Likes and ratings

| Key | What it does |
|-----|--------------|
| `L` | like / unlike the song (♥) |
| `1` … `5` | rate 1 to 5 stars |
| `0` | clear the rating |

Liked songs are in **Smart → Liked**. Rated songs are in **Smart → Top rated**.
On a radio station, `L` adds it to Favorites. On a podcast, `L` subscribes.

### Playlists

| Key | What it does |
|-----|--------------|
| `N` | new playlist |
| `P` | add the selected item to a playlist (or a new one) |
| `R` | rename the playlist |
| `E` | export the playlist as `.m3u8` |
| `J` / `K` | move a song inside the playlist |
| `d` | remove a song from the playlist |
| `d` on a playlist in the Library pane | delete the playlist (asks first) |

Export suggests `~/Music/Playlists/<name>.m3u8`. You can change the path.

To import an `.m3u8` file, use **Settings → Library → Import playlist**. The
playlist gets the file's name.

### Edit tags

`e` edits the tags of the selected song. On an album or folder, it edits every
song in it. This works for your own files, not for servers or podcasts.

Fields: Title, Artist, Album, Album artist, Year, Track, Disc, Genre. For many
songs at once, only Artist, Album, Album artist, Year and Genre are shown.

| Key | What it does |
|-----|--------------|
| `ctrl+f` | fill tags from the file and folder name |
| `ctrl+b` | look the album up on MusicBrainz, then pick the release |
| `ctrl+x` | remove the cover (on the Cover row) |
| `enter` | save |
| `esc` | cancel |

**Cover:** type the path to a picture on the Cover row. It goes inside every
song.

MusicBrainz gets only the artist and album names, and only when you press
`ctrl+b`. It fills the tags and the front cover from the Cover Art Archive.
Nothing is saved until you press `enter`. Musinator writes a copy of the file
first, then replaces the original.

## Now Playing

Press `z` to open it.

| Key | What it does |
|-----|--------------|
| `l` | lyrics on / off |
| `v` | visualizer on / off |
| `f` | full-screen lyrics (`f` or `esc` closes) |
| `ctrl+l` | find lyrics online (LRCLIB) |
| `[` / `]` | lyrics 0.5 s earlier / later for this song |
| `m` | mini mode |
| `V` | watch the video in mpv |
| `tab` | jump to Up next, where `J` / `K` / `d` / `enter` work |

### Lyrics

Musinator looks for lyrics in this order:

1. an `.lrc` file next to the song (same name)
2. lyrics inside the song's tags
3. lyrics found online earlier

Synced lyrics follow the song. Lyrics with word times ("enhanced LRC") light up
word by word.

No lyrics? Press `ctrl+l`. Musinator asks [LRCLIB](https://lrclib.net) with
the artist, title, album and length. It only asks when you press the key.
Found lyrics are kept in Musinator's cache. Turn on
**Library → Save lyrics next to songs** to also write an `.lrc` file next to
the song.

Lyrics too early or too late? `[` and `]` move them by 0.5 s for this song.
Musinator remembers this per song.

**Appearance → Lyrics lead time** (default 0.3 s) shows the next line a little
before it is sung, so you can read it in time.

In full-screen lyrics on Kitty or Ghostty, the current line is drawn in big
text. Turn this off with **Appearance → Big current lyric line**.

### Visualizer

A spectrum from the real audio. **Appearance → Visualizer speed** sets the
frames per second (lower uses less CPU).

### Mini mode

`m` shows only three lines: title and artist, progress, buttons and time. The
cover shows too when there is room. Mini mode also turns on by itself when the
window is smaller than 60 columns or 12 rows.

### Video

`V` opens the selected (or playing) video file in [mpv](https://mpv.io), at
the same spot. Musinator pauses. You need mpv installed
(`sudo pacman -S mpv`). Musinator only scans `.mp4` among video types, so this
works for `.mp4` files in your music folders.

## Radio

Internet radio from [radio-browser.info](https://www.radio-browser.info). It is
free and needs no account. Musinator only asks it when you open these lists.

Under **Radio** in the Library pane:

- **Favorites**: your stations. `L` on a station adds or removes it.
- **Popular**, **Genres**, **Countries**
- **Find a station…**: search by name, genre or country.
- **Add stream address…**: your own station. The address must start with
  `http://` or `https://`. `.pls` and `.m3u` links work. The station goes into
  Favorites.

The main search (`/`) also finds stations.

Radio has no seeking. Pause stops the stream; play connects again, so you hear
what is on air now. The song on air shows when the station sends it.

## Podcasts

Under **Podcasts** in the Library pane:

- **New episodes**: unplayed episodes of your shows
- **Shows**: the shows you follow
- **Downloads**: downloaded episodes
- **Find a podcast…**: type a name to search the Apple Podcasts directory
  (free, no account), or paste a feed address

| Key | What it does |
|-----|--------------|
| `L` on a show or episode | subscribe / unsubscribe |
| `D` on an episode | download it; press again to delete the download |

- Episodes stream with seeking, or play from the download.
- Musinator remembers where you stopped in each episode and continues there.
- An episode counts as played in its last minute. It then shows `✓ played`.
- Shows are checked for new episodes at start and every 2 hours.
- **Library → Download new podcast episodes** keeps the newest episode of each
  show on disk. These are marked "auto" and deleted after you played them.

## Music servers

Navidrome, Jellyfin and other Subsonic servers (Gonic, Airsonic, Ampache, …).
Musinator finds out which kind it is.

Add one: **Settings → Library → Music servers**, then:

- `a` adds a server. Musinator asks for a name (empty = the address), the
  address (e.g. `http://192.168.1.5:4533`), your user name and password.
- `r` renames the selected server.
- `d` removes it.

Only a login token is saved, never your password.

Each server gets its own section in the Library pane: **Artists**, **Albums**,
**Recently added**, **Playlists**. The main search (`/`) finds songs on all
servers. Songs stream in their original quality, with seeking.

## Last.fm

Log in: **Settings → Desktop → Last.fm account**. Your browser opens. Click
"Allow" there within 3 minutes. Your password never goes through Musinator.
Press the same setting again to log out.

Then:

- Now Playing shows `last.fm`, then `last.fm ✓` once the song counts.
- A song counts when it is longer than 30 s and you heard half of it or
  4 minutes.
- Radio songs count after 30 s, when the station names them. Turn this off
  with **Scrobble radio songs**.
- Offline? Scrobbles wait and are sent later.
- `L` (like) also loves the song on Last.fm.

A **Last.fm** section shows up in the Library pane:

- **Recent**, **Loved**
- **Top artists**, **Top songs**: this week, this month, this year, all time
- **Open profile**: opens your profile in the browser

Songs and artists that are in your library play from there. Others show
`not in your library`.

## Audio

Musinator plays through PipeWire directly.

### The audio line

The Now Playing panel shows the real format, for example:

```
FLAC 24/96 → 48 kHz · gapless · RG −1.6 dB
```

| Part | Meaning |
|------|---------|
| `FLAC 24/96` | codec, bits, rate of the file |
| `→ 48 kHz` | PipeWire runs at another rate and resamples |
| `gapless` | the next song has the same rate and channels, so it flows on with no gap |
| `EQ Rock` | the equalizer is on, with this preset |
| `RG −1.6 dB` | ReplayGain from the file's tags |
| `R128 −1.6 dB` | loudness measured by Musinator |
| `bit-perfect` | rates match, volume is 100%, no gain, EQ off |

### ReplayGain

**Audio → ReplayGain** modes:

- **Auto**: album gain when you play an album, else track gain
- **Track**, **Album**, **Off**

Also in **Settings → Audio**: **ReplayGain preamp**, **Gain without info**
(for files with no loudness info) and **Prevent clipping**.

### Loudness measuring

Files without ReplayGain tags are measured (EBU R128) in the background after
a scan. Turn this off with **Library → Measure loudness**.

### Bit-perfect and rate switching

By default PipeWire runs at one rate (often 48 kHz) and resamples the rest.
Turn on **Audio → PipeWire rate switching** to let PipeWire switch to each
file's rate. Musinator writes
`~/.config/pipewire/pipewire.conf.d/50-musinator.conf` and applies it at once.
Turn it off to remove the file.

PipeWire only switches when no other app is playing at another rate.

### Equalizer

`ctrl+e` opens a 10-band equalizer (31 Hz to 16 kHz, ±12 dB) with a live curve.

| Key | What it does |
|-----|--------------|
| `←` / `→` | choose a band |
| `↑` / `↓` | ±1 dB |
| `0` | reset this band |
| `r` | flat |
| `p` / `P` | next / previous preset |
| `space` | on / off |
| `esc` | close |

Presets: Flat, Rock, Pop, Jazz, Classical, Electronic, Vocal, Bass boost,
Treble boost, Loudness. Change a band and it becomes "Custom". Boosts lower the
overall level, so nothing distorts.

## Themes

`t` opens the theme list. `↑` / `↓` preview the theme live. `enter` keeps it.
`esc` goes back to the old one.

Themes: Catppuccin (Mocha, Macchiato, Frappé, Latte), Tokyo Night (Night, Storm,
Day), Rosé Pine (Main, Moon, Dawn), Gruvbox (Dark, Light), Nord, Dracula,
One Dark, Solarized (Dark, Light), Kanagawa Wave, Everforest Dark, Ayu Mirage,
GitHub (Dark, Light).

**Match Omarchy** uses your current Omarchy theme. When you change the theme in
Omarchy, Musinator follows right away.

**Appearance → Match terminal colors** gives the terminal the theme's
background and text color while Musinator is open.

## Settings

Press `S`.

| Key | What it does |
|-----|--------------|
| `tab` | switch between sections (left) and settings (right) |
| `↑` / `↓` | move |
| `←` / `→` | change the value |
| `enter` | change, or open (for rows with `›`) |
| `esc` | close |

Changes apply at once and are saved.

### Appearance

| Setting | Default | What it does |
|---------|---------|--------------|
| Theme | Match Omarchy on Omarchy, else Catppuccin Mocha | Colors. Opens the theme list. |
| Density | Compact | Compact or Comfortable list spacing. |
| Pane corners | Round | Round or Square pane corners. |
| Dialog shadow | On | Soft shadow under floating windows. |
| Match terminal colors | On | Gives the terminal the theme's background and text color while open. |
| Album art | Auto | Auto: real pictures on Kitty/Ghostty, half-blocks elsewhere. Also Kitty images, Half-blocks, Off. |
| Albums as grid | Off | Show albums as a grid of covers. |
| Visualizer | On | Spectrum from the real audio. |
| Visualizer speed | 30 fps | 10 to 60 fps. Lower uses less CPU. |
| Big current lyric line | On | Big current line in full-screen lyrics. Kitty and Ghostty only. |
| Lyrics lead time | 0.3 s | 0 to 1.5 s. Shows the next line this much early. |
| Lyrics | On | Synced lyrics on Now Playing. |

### Playback

| Setting | Default | What it does |
|---------|---------|--------------|
| Confirm before quitting | On | Asks before Musinator closes. |
| Resume where you left off | On | Brings back the queue and position at start (paused). |
| Seek step | 5 s | 1 to 30 s, for `←` and `→`. |
| Big seek step | 30 s | 5 to 120 s, for `shift+←` and `shift+→`. |

### Audio

| Setting | Default | What it does |
|---------|---------|--------------|
| ReplayGain | Auto | Auto, Track, Album or Off. |
| ReplayGain preamp | +0.0 dB | −15 to +15 dB, added to every ReplayGain value. |
| Gain without info | +0.0 dB | −15 to 0 dB, for files with no loudness info. |
| Prevent clipping | On | Lowers the gain so peaks stay below 0 dBFS. |
| PipeWire rate switching | Off | Lets PipeWire run at the file's rate. Writes `~/.config/pipewire/pipewire.conf.d/50-musinator.conf`. |

### Library

| Setting | Default | What it does |
|---------|---------|--------------|
| Music folders | `~/Music` | Folders to scan. `a` add, `d` remove. |
| Watch for changes | On | Scans again when files change. |
| Measure loudness | On | Measures EBU R128 loudness for files without ReplayGain tags. |
| Save lyrics next to songs | Off | Lyrics found online also go into an `.lrc` file next to the song. |
| Rescan now | | Scans the music folders again. |
| Import playlist | | Imports an `.m3u8` file. |
| Music servers | None | Navidrome, Jellyfin or other Subsonic servers. |
| Download new podcast episodes | Off | Keeps the newest episode of each show on disk. |

### Desktop

| Setting | Default | What it does |
|---------|---------|--------------|
| Show in Omarchy bar | Off | Adds the playing song to the Omarchy bar. |
| Media keys (MPRIS) | On | Media keys, `playerctl` and the Omarchy media controls. |
| Notify on track change | Off | Desktop notification when a new song starts. |
| Last.fm account | Not connected | Log in once in the browser. Press again to log out. |
| Scrobble to Last.fm | On | Sends the songs you hear to your Last.fm profile. |
| Scrobble radio songs | On | Also sends radio songs, when the station names them. |

### Keys

| Setting | What it does |
|---------|--------------|
| Remap keys | Change the key for any action. See [Keys](#keys). |

## Keys

All defaults. Keys with "or" have two defaults.

### Playback

| Action | Key |
|--------|-----|
| Play / pause | `space` |
| Play selected | `enter` |
| Next track | `n` |
| Previous track | `p` |
| Seek back 5 s | `←` |
| Seek forward 5 s | `→` |
| Seek back 30 s | `shift+←` |
| Seek forward 30 s | `shift+→` |
| Volume up | `+` or `=` |
| Volume down | `-` |
| Shuffle on / off | `s` |
| Repeat: off, all, one | `r` |
| Sleep timer | `ctrl+t` |
| Equalizer (10 bands) | `ctrl+e` |

### Queue

| Action | Key |
|--------|-----|
| Add to queue | `a` |
| Play next | `A` |
| Move item down | `J` |
| Move item up | `K` |
| Remove item | `d` or `delete` |
| Show queue | `q` |
| Clear queue | `X` |

### Find

| Action | Key |
|--------|-----|
| Search everything | `/` |

### Library

| Action | Key |
|--------|-----|
| Like / unlike | `L` |
| Rate 1 to 5 stars | `1` `2` `3` `4` `5` |
| Clear rating | `0` |
| Add to playlist | `P` |
| Edit tags (song or album) | `e` |
| New playlist | `N` |
| Rename playlist | `R` |
| Export playlist (.m3u8) | `E` |
| Album grid on / off | `o` |
| Download podcast episode (again: delete it) | `D` |
| Filter this list | `ctrl+f` |
| Rescan music folders | `ctrl+r` |

### Screens

| Action | Key |
|--------|-----|
| Library ↔ Now Playing | `z` |
| Lyrics on / off | `l` |
| Visualizer on / off | `v` |
| Mini mode (3 lines) | `m` |
| Watch video in mpv | `V` |
| Full-screen lyrics | `f` |
| Find lyrics online (LRCLIB) | `ctrl+l` |
| Lyrics 0.5 s earlier | `[` |
| Lyrics 0.5 s later | `]` |
| Themes | `t` |
| Settings | `S` |
| Help | `?` |

### Move

| Action | Key |
|--------|-----|
| Next pane | `tab` |
| Previous pane | `shift+tab` |
| Down | `j` or `↓` |
| Up | `k` or `↑` |
| Top | `g` or `home` |
| Bottom | `G` or `end` |
| Page down | `pagedown` or `ctrl+d` |
| Page up | `pageup` or `ctrl+u` |
| Back / close | `esc` or `backspace` |
| Left pane narrower | `<` |
| Left pane wider | `>` |

### App

| Action | Key |
|--------|-----|
| Quit | `Q` or `ctrl+c` |

Quit asks first. Press `y` or `enter` to quit. Turn the question off with
**Playback → Confirm before quitting**.

### Change a key

1. `S`, then **Keys → Remap keys**.
2. Select the action and press `enter`.
3. Press the new key. `esc` cancels.

If the key is already used, Musinator tells you which action has it. Change
that one first. `r` resets the selected action to its default.

A remapped action gets exactly one key. To give an action more than one key,
add it to the `[keys]` table in `~/.config/musinator/config.toml`, using the
names from the table below:

```toml
[keys]
play_pause = ["space", "ctrl+p"]
next = ["n", "ctrl+right"]
```

Key names: letters and symbols as they are (`J` means shift+j), `space`,
`enter`, `esc`, `tab`, `backtab`, `backspace`, `delete`, `left`, `right`, `up`,
`down`, `home`, `end`, `pageup`, `pagedown`, `f1` … `f12`, with `ctrl+`,
`alt+` or `shift+` in front.

<details>
<summary>Action names for config.toml</summary>

| Name | Action |
|------|--------|
| `play_pause` | Play / pause |
| `play_selected` | Play selected |
| `next` | Next track |
| `prev` | Previous track |
| `seek_back` | Seek back 5 s |
| `seek_fwd` | Seek forward 5 s |
| `seek_back_big` | Seek back 30 s |
| `seek_fwd_big` | Seek forward 30 s |
| `vol_up` | Volume up |
| `vol_down` | Volume down |
| `shuffle` | Shuffle on / off |
| `repeat` | Repeat: off, all, one |
| `sleep_timer` | Sleep timer |
| `equalizer` | Equalizer |
| `add_queue` | Add to queue |
| `play_next` | Play next |
| `move_down` | Move item down |
| `move_up` | Move item up |
| `remove` | Remove item |
| `queue` | Show queue |
| `clear_queue` | Clear queue |
| `search` | Search everything |
| `like` | Like / unlike |
| `rate_1` … `rate_5` | Rate 1 to 5 stars |
| `rate_clear` | Clear rating |
| `add_to_playlist` | Add to playlist |
| `edit_tags` | Edit tags |
| `new_playlist` | New playlist |
| `rename_playlist` | Rename playlist |
| `export_playlist` | Export playlist |
| `grid` | Album grid on / off |
| `download` | Download podcast episode |
| `filter` | Filter this list |
| `rescan` | Rescan music folders |
| `toggle_screen` | Library ↔ Now Playing |
| `lyrics` | Lyrics on / off |
| `visualizer` | Visualizer on / off |
| `mini_mode` | Mini mode |
| `open_video` | Watch video in mpv |
| `full_lyrics` | Full-screen lyrics |
| `fetch_lyrics` | Find lyrics online |
| `lyrics_earlier` | Lyrics 0.5 s earlier |
| `lyrics_later` | Lyrics 0.5 s later |
| `themes` | Themes |
| `settings` | Settings |
| `help` | Help |
| `next_pane` | Next pane |
| `prev_pane` | Previous pane |
| `down` | Down |
| `up` | Up |
| `top` | Top |
| `bottom` | Bottom |
| `page_down` | Page down |
| `page_up` | Page up |
| `back` | Back / close |
| `pane_shrink` | Left pane narrower |
| `pane_grow` | Left pane wider |
| `quit` | Quit |

</details>

## Desktop

### Omarchy bar

Turn on **Settings → Desktop → Show in Omarchy bar**. Musinator adds a module
to `~/.config/omarchy/shell.json`, left of the tray. It saves a backup first,
in `shell.json.bak-musinator`. Clicking the module opens Musinator. Turn the
setting off to remove the module.

The module shows `♪ Title · Artist  2:11` and hides when nothing plays.

### Waybar

```jsonc
"custom/musinator": { "exec": "musinator --bar", "return-type": "json", "interval": 2 }
```

### Media keys and playerctl

With **Media keys (MPRIS)** on, Musinator shows up on D-Bus as `musinator`.
Media keys, the Omarchy media controls and `playerctl` work:

```sh
playerctl -p musinator play-pause
playerctl -p musinator next
playerctl -p musinator metadata
```

### Notifications

Turn on **Desktop → Notify on track change** to get a desktop notification
with the title, artist, album and cover when a new song starts.

## Command line

These talk to the running app.

```sh
musinator                 # open the player
musinator play            # resume playing
musinator pause           # pause
musinator toggle          # play / pause
musinator next            # next song
musinator prev            # previous song
musinator stop            # stop
musinator status          # what is playing
musinator status --json   # the same, as JSON
musinator --bar           # one line of JSON for the Omarchy bar or Waybar
musinator --version
musinator --help
```

Example:

```
$ musinator status
▶ Night Drive — Neon Harbor  2:11 / 5:37  (FLAC 24/96 → 48 kHz, vol 70%)
```

When Musinator is not running, `status --json` prints
`{"running":false,"state":"stopped"}` and `--bar` prints an empty line. The
other commands print `musinator: Musinator is not running`.

## Files

| Path | What is in it |
|------|---------------|
| `~/.config/musinator/config.toml` | settings, keys, volume, equalizer |
| `~/.config/musinator/servers.json` | music servers (login tokens, no passwords) |
| `~/.local/share/musinator/library.db` | library, playlists, likes, ratings, play counts, lyric timing |
| `~/.local/share/musinator/radio.db` | radio favorites and stations |
| `~/.local/share/musinator/podcasts.db` | shows, episodes, where you stopped |
| `~/.local/share/musinator/podcasts/` | downloaded episodes |
| `~/.local/state/musinator/session.json` | queue and position, for resume |
| `~/.local/state/musinator/lastfm.json` | Last.fm login and waiting scrobbles |
| `~/.cache/musinator/art/` | cover cache |
| `~/.cache/musinator/lyrics/` | lyrics found online |
| `$XDG_RUNTIME_DIR/musinator.sock` | control socket for the commands |

If you set `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME` or
`XDG_CACHE_HOME`, Musinator uses those folders instead.

Musinator changes files outside its own folders only when you turn on a
setting:

- **PipeWire rate switching**: `~/.config/pipewire/pipewire.conf.d/50-musinator.conf`
- **Show in Omarchy bar**: `~/.config/omarchy/shell.json` (backup:
  `shell.json.bak-musinator`)

## Problems and fixes

**"Musinator is already running in another window"**
Only one can run at a time. Use the open one, or close it first.

**`musinator: Musinator is not running`**
The commands need the app open. Start `musinator` first.

**No sound**
Musinator plays through PipeWire. Check that it runs:

```sh
systemctl --user status pipewire
```

**The audio line shows `→ 48 kHz`**
PipeWire resamples. Turn on **Settings → Audio → PipeWire rate switching**.
If it still resamples, another app is playing at another rate.

**No `bit-perfect`**
It needs: same rate, volume 100%, no ReplayGain, EQ off.

**Covers look blocky**
Real pictures need Kitty or Ghostty. Inside tmux or screen, Musinator always
uses half-blocks.

**"This stream can't seek"**
Radio can't seek.

**"HLS streams (.m3u8) are not supported"**
Find another address for the station, for example a `.pls` or `.m3u` link.

**"mpv is not installed"**
Install it: `sudo pacman -S mpv`.

**"Omarchy bar not found"**
The setting needs `~/.config/omarchy/shell.json`. On other bars, use the
[Waybar](#waybar) module.

**"the Last.fm login is not valid any more"**
Log in again in **Settings → Desktop → Last.fm account**.

**"No answer from Last.fm in 3 minutes"**
You did not click "Allow" in the browser in time. Try again.

**"The server is still reading your library"**
Your music server is still scanning. Try again in a few minutes.

**Lyrics are off**
Use `[` and `]` for this song, or change **Appearance → Lyrics lead time**.
