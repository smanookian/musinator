# Launch copy

Everything here is checkable. If a number changes, change it here too.

**Paste as normal text.** The blocks below are ready to paste: one line per paragraph or list item. Don't use Reddit's code-block button, or it shows up in a grey monospace box.

**AI rules:** r/commandline, r/rust and Hacker News do not want AI-written post text. There, use these texts only as facts and write the post yourself, and say plainly that most of the code was written with an AI assistant.

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

## Status (2026-11-11)

Places, rules and flairs for every launch: `~/Projects/Stevinator/docs/LAUNCH_PLAYBOOK.md`.

| Place | Status |
|---|---|
| Blog post | ✓ posted |
| r/omarchy | ✓ posted (flair: I Made a Thing) |
| r/unixporn | ✓ posted (flair: Material) |
| r/CLI | ✓ posted |
| r/tui | ✓ crossposted (flair: AI assisted) |
| r/rust weekly thread | ✓ commented |
| Hacker News (Show HN) | ✓ posted |
| X | open: blocked as "automated" on first try; post 1 alone, then replies, without the curl line |
| Omarchy Discord | open: #omarchy-apps on https://discord.gg/tXFUdasqhY |
| r/commandline | open: **on or after 2026-11-23**, written by you |
| Mastodon | not used |

---

## Show HN (posted 2026-11-11)

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

Post when you can answer for a few hours. Spreading posts over days is only to keep your replies manageable; r/CLI has no rule against posting today.

### r/omarchy (posted 2026-11-11)

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

### r/unixporn (posted 2026-11-11)

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

### r/CLI (posted 2026-11-11)

Rules (checked 2026-11-11): be kind, stay on topic (CLI/terminal), no spam, no NSFW. No AI or project-age rules. No post flairs.

**Title:** `Musinator: a terminal music player in Rust with album covers, synced lyrics and gapless playback`

Attach the GIF, then:

```
So here is the thing. I wanted a terminal music player that is fast and gapless, and still shows the album cover and the lyrics. So I wrote one in Rust (ratatui, symphonia, native PipeWire).

- Real album covers in Ghostty/Kitty, half-block pictures in other terminals
- Synced lyrics from .lrc files, tags, or LRCLIB; full-screen with f
- Gapless in every format; plays at the file's own sample rate and says when the system resamples
- Internet radio, podcasts, and Navidrome/Jellyfin servers in the same screens and search
- 22 themes, 10-band EQ, mini mode, tag editing with MusicBrainz
- Near 0% CPU when idle; a small command line too (musinator status, toggle, next)

Similar programs: cmus (very fast, no covers or lyrics), ncmpcpp and rmpc (need MPD; rmpc shows covers), termusic (Rust, covers, podcasts), kew (local files). Musinator puts covers, synced lyrics, radio, podcasts and servers into one program without a daemon.

Most of the code was written with an AI coding assistant; I designed it, reviewed it and tested it on my machine.

Linux x86_64 only, no Spotify. MIT: https://github.com/smanookian/musinator

I hope to see you next time. Steve out. Tschau!
```

### r/tui (crossposted 2026-11-11)

Rules (checked 2026-11-11): AI-assisted code needs the flair **"AI assisted"** (required). Max one post per week per project. Crosspost from r/CLI is fine; body text optional.

### r/commandline (not before 2026-11-23)

Rules (checked 2026-11-11) that matter:
- **No projects newer than 30 days.** The repo was created 2026-10-24, so post on or after **2026-11-23**.
- **AI Code Policy:** post text and titles written by AI are not allowed, so **write this post yourself** (use the r/CLI text above only as a list of facts). Projects with more than a small part of AI code must say so, e.g. "This software's code is partially AI-generated." Ours: say it plainly (see the sentence in the r/CLI text).
- **List similar software and how yours is different** (required; facts in the r/CLI text).
- Flair: **Terminal User Interface**.

### r/rust (commented 2026-11-11, weekly thread only)

Rules (checked 2026-11-11): *"Applications who just happen to be written in Rust are not on-topic"*; progress on Rust projects goes in the stickied **"What's Everyone Working on this Week?"** thread. *"Submissions appearing to contain AI-generated content may be removed."* So: **no own post**, a short comment in that thread, in your own words. Facts for it:

- Terminal music player, ratatui + symphonia + native PipeWire (pipewire-rs).
- Sample-exact gapless across formats (MP4 priming, Opus pre-skip, LAME padding trimmed).
- Audio thread never locks or allocates; UI redraws only on change, near 0% idle CPU.
- Lyric clock includes PipeWire output latency.
- Most code written with an AI assistant, designed/reviewed/tested by you.
- https://github.com/smanookian/musinator

Where: https://www.reddit.com/r/rust/comments/1xrooh0/whats_everyone_working_on_this_week_462026/ (week 46/2026; a new thread starts every Monday). Comment there, no own post, no flair.

Draft comment for the weekly thread (a starting point; change it into your own words before posting, because of the AI rule above):

```
Musinator, a terminal music player: https://github.com/smanookian/musinator

ratatui for the UI, symphonia for decoding, and native PipeWire output through pipewire-rs. The part I spent the most time on was sample-exact gapless playback: MP4 priming, Opus pre-skip and LAME padding all get trimmed, so albums play without a click between songs. The audio thread never locks or allocates, and the UI only redraws when something changes, so it idles near 0% CPU. The lyric clock adds the PipeWire output latency, so lines switch when you hear them.

Most of the code was written with an AI coding assistant; I designed it, reviewed it and tested it on my machine.
```

A real r/rust post fits later only as a technical write-up (flair "🧠 educational"), e.g. "How I got sample-exact gapless playback in Rust", written by you.

---

## X thread

Post 1 with the video (`musinator-demo.mp4`) and `#Omarchy #Linux #Rust`. Then add 2–6 as replies. Leave out the `curl` line in post 6 (X flags it); the app page has it.

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
6/ Install with one line, no sudo, on the page:
https://stevinator.com/apps/musinator

I hope to see you next time. Steve out. Tschau!
```

---

## Omarchy Discord

Invite: https://discord.gg/tXFUdasqhY. Post in **#omarchy-apps** (server "Omacom"). Not #omarchy, announcements or help.

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
- **Don't post the same text everywhere at once.** Reddit's spam filter flags identical posts across many subreddits in a short time. Two or three a day with different texts is fine.
