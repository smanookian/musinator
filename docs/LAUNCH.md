# Launch copy

Everything here is checkable. If a number changes, change it here too.

Facts used below (checked 2026-11-09):
- Rust, about 17,000 lines, MIT, 70 automatic tests.
- Plays FLAC, MP3, AAC/ALAC, Ogg Vorbis, Opus, WAV, AIFF; sample-exact gapless in all of them.
- Native PipeWire output; asks for the file's own sample rate and shows when the system resamples.
- 22 built-in themes plus "match Omarchy" (follows theme changes live).
- Idle CPU near 0%; the visualizer stays under 5% of one core.
- Online only when you use it: LRCLIB (lyrics), radio-browser.info, podcast feeds, MusicBrainz, your servers, Last.fm.
- Not there yet: Spotify, AUR package (install script works), Linux x86_64 only.

Links:
- Repo: https://github.com/smanookian/musinator
- Page: https://stevinator.com/apps/musinator
- Install: `curl -fsSL https://raw.githubusercontent.com/smanookian/musinator/main/install.sh | sh`
- Demo GIF: `docs/demo/musinator-demo.gif` · video: `docs/demo/musinator-demo.mp4`

---

## Show HN

**Title** (80 char limit; this is 77):

```
Show HN: Musinator – a terminal music player with album art and synced lyrics
```

**Body:**

```
Musinator is a music player for the terminal, written in Rust. I wanted cmus-level speed with
a nicer face: real album covers (Kitty graphics in Ghostty/Kitty, half-blocks elsewhere),
lyrics that follow the song, and themes that match my desktop.

What it does:

- Local files: FLAC, MP3, AAC/ALAC, Vorbis, Opus, WAV, AIFF. Sample-exact gapless in all of
  them (MP4 priming, Opus pre-skip and LAME padding are trimmed), ReplayGain, or EBU R128
  measured for files without tags.
- Native PipeWire output. It asks for the file's own rate and the info line says when the
  system resamples instead, so "bit-perfect" is only shown when it is true.
- Synced lyrics from .lrc files, tags, or LRCLIB on a key press. Full-screen lyrics, word by
  word when the lyrics have word times. The lyric clock includes the PipeWire latency, so
  lines switch when you hear them, not when they are decoded.
- Internet radio (radio-browser.info), podcasts with downloads and resume, and Navidrome /
  Jellyfin / Subsonic servers, all in the same screens and the same search.
- Tag editing with MusicBrainz lookup and cover art; writes a copy, then replaces the file.
- 10-band EQ, sleep timer, mini mode, mouse support, MPRIS, Last.fm.

Idle CPU is near zero: it only redraws when something changes. The visualizer stays under
5% of one core.

Limits, so you don't have to find them: Linux x86_64 only, no Spotify (their API only allows
remote control for small apps), no AUR package yet (an install script works).

MIT. curl -fsSL https://raw.githubusercontent.com/smanookian/musinator/main/install.sh | sh
https://github.com/smanookian/musinator
```

**Answers to comments you will get** (have them ready; don't put them in the post):

- *"Why not cmus / ncmpcpp / rmpc?"* — They are great and faster to learn if you live in MPD.
  Musinator is one program without a daemon, with covers, synced lyrics, radio, podcasts and
  servers built in, and themes that follow Omarchy.
- *"Why not MPD?"* — One less service to set up. Musinator still has a command line
  (`musinator status --json`, play/pause/next) and MPRIS, so bars and media keys work.
- *"Kitty graphics are a hack."* — They are the only way to show real pictures in a terminal.
  Other terminals get half-block pictures, and pictures can be turned off.
- *"Bit-perfect in a terminal player?"* — Only when the file rate equals the device rate and
  volume and ReplayGain are neutral. The info line shows "→ 48 kHz" in orange when PipeWire
  resamples, and there is a setting that lets PipeWire switch rates.
- *"Spotify?"* — Spotify's Development Mode allows 5 users per app and remote control only.
  Playing inside Musinator would need librespot, which is against their terms. Later, maybe.

---

## Reddit

Post when you can answer for a few hours. One subreddit per day, not all at once.

### r/unixporn

**Title:** `[OC] Musinator – a terminal music player with real album covers, synced lyrics and live theme switching (Omarchy, Ghostty)`

Attach the GIF. Then comment with details (r/unixporn wants the setup in a comment):

```
Musinator, a terminal music player I wrote in Rust.

- Terminal: Ghostty (album covers are Kitty graphics; other terminals get half-blocks)
- WM: Hyprland via Omarchy
- Themes: 22 built in, previewed live with t, or "match Omarchy", which follows your Omarchy
  theme when you switch it
- Lyrics: synced from .lrc files, tags, or LRCLIB; full-screen with f
- Also: radio, podcasts, Navidrome/Jellyfin, 10-band EQ, mini mode with m

MIT, install script: https://github.com/smanookian/musinator
```

### r/omarchy

**Title:** `Musinator: a music player that follows your Omarchy theme, shows the song in the bar, and works with the media keys`

```
I built a terminal music player for Omarchy.

- "Match Omarchy" theme: switch your Omarchy theme and Musinator follows right away
- Optional song in the Omarchy bar (it backs up shell.json first)
- Media keys work through MPRIS, so the Omarchy media controls just work
- Album covers as real pictures in Ghostty

It also does synced lyrics, internet radio, podcasts, Navidrome/Jellyfin, an EQ and tag
editing with MusicBrainz.

One line to install, no sudo:
curl -fsSL https://raw.githubusercontent.com/smanookian/musinator/main/install.sh | sh

Page with screenshots: https://stevinator.com/apps/musinator
Feedback welcome, especially on keys that feel wrong.
```

### r/commandline and r/rust

**Title:** `Musinator: a terminal music player in Rust with gapless playback, native PipeWire output and synced lyrics`

```
Musinator is a TUI music player in Rust (ratatui, symphonia, native PipeWire).

Things that might interest you:
- Sample-exact gapless across FLAC, MP3, AAC/ALAC, Vorbis, Opus, WAV, AIFF: MP4 priming,
  Opus pre-skip and LAME padding are trimmed.
- The audio thread never locks or allocates; the UI redraws only on change, so idle CPU is
  near zero.
- Lyric timing adds the PipeWire output latency, so lines switch when you hear them.
- One source interface for local files, radio, podcasts and Subsonic/Jellyfin servers.
- 70 tests, MIT.

https://github.com/smanookian/musinator
```

---

## X / Mastodon thread

```
1/ Musinator: a music player for the terminal.

Real album covers, lyrics in time with the song, 22 themes that switch live.
Free and open source, for Omarchy and Linux.

[GIF]
```

```
2/ It plays your files gapless in every format: FLAC, MP3, AAC/ALAC, Vorbis, Opus, WAV, AIFF.

Native PipeWire output, at the file's own sample rate. If the system resamples, the info line
says so.
```

```
3/ Lyrics from .lrc files, tags, or LRCLIB with one key.

f opens full-screen lyrics with the current line in big letters.
```

```
4/ Not just your files: internet radio, podcasts (download + resume), and your Navidrome or
Jellyfin servers. Same screens, same search.
```

```
5/ Near 0% CPU when idle. No account, no telemetry.

It only goes online for the things you use: radio, podcasts, lyrics, MusicBrainz, your
servers, Last.fm.
```

```
6/ One line, no sudo:
curl -fsSL https://raw.githubusercontent.com/smanookian/musinator/main/install.sh | sh

https://github.com/smanookian/musinator
https://stevinator.com/apps/musinator
```

---

## Omarchy Discord (#show-and-tell or similar)

```
Made a terminal music player for Omarchy: Musinator. It follows your Omarchy theme live,
works with the media keys, can show the song in the bar, and shows album covers as real
pictures in Ghostty. Also synced lyrics, radio, podcasts and Navidrome/Jellyfin.
https://stevinator.com/apps/musinator (GIF on the page)
```

---

## Rules for posting

- **Lead with what you see.** The GIF first; covers, lyrics and themes are the hook.
- **Numbers over adjectives.** "Gapless in 7 formats, near 0% CPU idle" beats "blazing fast".
- **Name the limits first:** Linux x86_64 only, no Spotify, no AUR package yet.
- **Don't trash other players.** cmus, ncmpcpp, rmpc, kew and termusic are good; say what is
  different, not what is worse.
- **Post when you can stay for a few hours.** The first hour of comments brings the real bug
  reports; fix fast and say so in the thread.
- **One place per day.** Reddit removes posts that look like spam across subreddits.
