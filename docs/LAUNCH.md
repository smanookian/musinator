# Launch copy

Everything here is checkable. If a number changes, change it here too.

**Paste as normal text.** The blocks below are ready to paste: one line per paragraph or list item. Don't use Reddit's code-block button, or it shows up in a grey monospace box.

**Voice** (same as the Omarchy course, Superkey, Markinator and Lazynator posts on stevinator.com):
- Open with "So here is the thing …" (or "So here is why …").
- Short, plain sentences. First person. Say what was missing, then what it does.
- Be honest about what broke and what is not there yet.
- Close with: **I hope to see you next time. Steve out. Tschau!**

Blog post (publish first, link it everywhere): https://stevinator.com/blog/programming/musinator-a-music-player-for-the-terminal

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

**URL:** `https://github.com/smanookian/musinator`

**Text:**

```
So here is the thing about music in the terminal. The players are fast, but most of them look like 1998.

I live in the terminal on Omarchy (Arch + Hyprland), and I wanted my music there too. cmus is fast. ncmpcpp is powerful, but you set up MPD first. None of them showed me the album cover, the lyrics, or colors that match my desktop. So I built the player I wanted, in Rust.

What it does:

- Real album covers (Kitty graphics in Ghostty/Kitty, half-blocks elsewhere).
- Synced lyrics from .lrc files, tags, or LRCLIB on a key press. Full-screen lyrics too.
- 22 themes that change live, or "match Omarchy", which follows your desktop theme.
- Gapless in every format: FLAC, MP3, AAC, ALAC, Vorbis, Opus, WAV, AIFF.
- Native PipeWire output at the file's own rate. If the system resamples, it says so.
- Internet radio, podcasts with downloads and resume, and Navidrome / Jellyfin / Subsonic servers, in the same screens and the same search.
- Tag editing with MusicBrainz, a 10-band EQ, mini mode, mouse, MPRIS, Last.fm.

Near 0% CPU when idle. No account, no telemetry.

What broke along the way, in case it helps someone building the same kind of thing:

- Lyrics switched a little late. The sound needs about 40 ms from the program to your ears; the lyric clock now counts in the PipeWire latency.
- Album covers piled up in the terminal's memory while scrolling. Every new picture got a new Kitty image id. Now each place on the screen reuses its id.
- In tmux the first key press got lost. Musinator asked the terminal about pictures at startup and tmux never answered. Now it does not ask terminals that cannot answer.

Not there yet: Spotify (their API only allows remote control for small apps), an AUR package (an install script works), and anything besides Linux x86_64.

MIT, 70 tests.
curl -fsSL https://raw.githubusercontent.com/smanookian/musinator/main/install.sh | sh

I hope to see you next time. Steve out. Tschau!
```

**Answers to comments you will get** (have them ready; don't put them in the post):

- *"Why not cmus / ncmpcpp / rmpc?"* — They are great, and faster to learn if you live in
  MPD. Musinator is one program without a daemon, with covers, synced lyrics, radio,
  podcasts and servers built in, and themes that follow Omarchy.
- *"Why not MPD?"* — One less service to set up. It still has a command line
  (`musinator status --json`, play, pause, next) and MPRIS, so bars and media keys work.
- *"Kitty graphics are a hack."* — They are the only way to show real pictures in a
  terminal. Other terminals get half-block pictures, and pictures can be turned off.
- *"Bit-perfect in a terminal player?"* — Only when the file rate equals the device rate
  and volume and ReplayGain are neutral. The info line shows "→ 48 kHz" in orange when
  PipeWire resamples, and there is a setting that lets PipeWire switch rates.
- *"Spotify?"* — Spotify allows 5 users per app and remote control only. Playing inside
  Musinator would need librespot, which is against their terms. Later, maybe.

---

## Reddit

Post when you can answer for a few hours. One subreddit per day, not all at once.

### r/omarchy (day 1)

**Title:** `I built a terminal music player for Omarchy: covers, synced lyrics, and it follows your theme`

Attach the GIF, then:

```
So here is the thing. I wanted my music in the terminal, but with the album cover, the lyrics, and colors that match my Omarchy theme.

So I built Musinator.

- "Match Omarchy" theme: switch your Omarchy theme and Musinator follows right away
- Album covers as real pictures in Ghostty
- Media keys work, and it can show the song in the Omarchy bar (it backs up shell.json first)
- Synced lyrics, full-screen with f
- Internet radio, podcasts, and your Navidrome or Jellyfin servers
- 10-band EQ, mini mode with m, tag editing with MusicBrainz

One line to install, no sudo:
curl -fsSL https://raw.githubusercontent.com/smanookian/musinator/main/install.sh | sh

Page with screenshots: https://stevinator.com/apps/musinator
How I built it: https://stevinator.com/blog/programming/musinator-a-music-player-for-the-terminal

Free, MIT. If a key feels wrong or something looks off on your machine, tell me. I would rather hear about it than not.

I hope to see you next time. Steve out. Tschau!
```

### r/unixporn (day 4)

**Title:** `[OC] Musinator – a terminal music player with real album covers, synced lyrics and live theme switching (Omarchy, Ghostty)`

Upload the GIF. Then **right away** the setup as the first comment (r/unixporn removes posts
without it):

```
So here is the setup.

- App: Musinator, a terminal music player I wrote in Rust
- Terminal: Ghostty (covers are Kitty graphics; other terminals get half-blocks)
- WM: Hyprland via Omarchy
- Themes: 22 built in, previewed live with t, or "match Omarchy", which follows your Omarchy theme
- Lyrics: synced from .lrc files, tags, or LRCLIB; full-screen with f
- Also: radio, podcasts, Navidrome/Jellyfin, 10-band EQ, mini mode with m

MIT: https://github.com/smanookian/musinator

I hope to see you next time. Steve out. Tschau!
```

### r/commandline (day 5) and r/rust (day 6)

**Title:** `Musinator: a terminal music player in Rust with gapless playback, native PipeWire output and synced lyrics`

```
So here is the thing. I wanted a terminal music player that is fast, gapless, and still shows the album cover and the lyrics. So I wrote one in Rust (ratatui, symphonia, native PipeWire).

Things that might interest you:

- Gapless in every format: MP4 priming, Opus pre-skip and LAME padding are trimmed.
- The audio thread never locks or allocates. The UI only redraws when something changes, so idle CPU is near zero.
- The lyric clock counts in the PipeWire output latency, so lines switch when you hear them, not when they are decoded.
- One source interface for local files, radio, podcasts and Subsonic/Jellyfin servers.

To be honest, the hardest bug was a lost first key press in tmux: a startup query that tmux never answers. Now Musinator skips it there.

70 tests, MIT: https://github.com/smanookian/musinator

I hope to see you next time. Steve out. Tschau!
```

---

## X / Mastodon thread

Post 1 with the video (`musinator-demo.mp4`), then reply to yourself with 2–6.

```
1/ So here is the thing about music in the terminal: fast, but it looks like 1998.

So I built Musinator. Real album covers, lyrics in time with the song, 22 themes that switch live. Free and open source, for Omarchy and Linux.

[video]
```

```
2/ Gapless in every format: FLAC, MP3, AAC, ALAC, Vorbis, Opus, WAV, AIFF.

It plays at the file's own sample rate. If the sound system changes it, the info line says so.
```

```
3/ Lyrics from .lrc files, the song itself, or LRCLIB with one key.

f opens full-screen lyrics with the current line in big letters.
```

```
4/ Not only your files: internet radio, podcasts with download and resume, and your Navidrome or Jellyfin servers. Same screens, same search.
```

```
5/ Near 0% CPU when idle. No account, no telemetry. It only goes online for the things you use.
```

```
6/ One line, no sudo:
curl -fsSL https://raw.githubusercontent.com/smanookian/musinator/main/install.sh | sh

https://stevinator.com/apps/musinator

I hope to see you next time. Steve out. Tschau!
```

---

## Omarchy Discord (day 2)

```
So here is a new one for Omarchy: Musinator, a terminal music player. It follows your Omarchy theme live, works with the media keys, can show the song in the bar, and shows album covers as real pictures in Ghostty. Also synced lyrics, radio, podcasts and Navidrome/Jellyfin.

https://stevinator.com/apps/musinator (GIF on the page)

I hope to see you next time. Steve out. Tschau!
```

---

## Rules for posting

- **Blog post first.** Publish the stevinator.com post, then link it from every post.
- **Lead with what you see.** The GIF first; covers, lyrics and themes are the hook.
- **Numbers over adjectives.** "Gapless in every format, near 0% CPU idle" beats "blazing fast".
- **Name the limits first:** Linux x86_64 only, no Spotify, no AUR package yet.
- **Don't trash other players.** cmus, ncmpcpp, rmpc, kew and termusic are good; say what is
  different, not what is worse.
- **Post when you can stay for a few hours.** The first hour of comments brings the real bug
  reports; fix fast and say so in the thread.
- **One place per day.** Reddit removes posts that look like spam across subreddits.
