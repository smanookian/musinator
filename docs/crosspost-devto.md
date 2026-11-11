---
title: "Musinator: A Music Player for the Terminal That Looks Good"
published: true
description: "A free terminal music player for Omarchy and Linux. Real album covers, lyrics in time with the song, 22 themes, internet radio, podcasts, and your Navidrome or Jellyfin servers."
tags: rust, linux, opensource, showdev
cover_image: https://stevinator.com/apps/musinator/og.png
canonical_url: https://stevinator.com/blog/programming/musinator-a-music-player-for-the-terminal
---

So here is the thing about music in the terminal. The players are fast, but most of them look like 1998.

I live in the terminal on Omarchy, and I wanted my music there too. cmus is fast. ncmpcpp is powerful, but you set up MPD first. None of them show me the album cover, the lyrics, or colors that match my desktop. So I built the player I wanted.

## What it does

![Musinator in action: album covers, Now Playing with synced lyrics and a visualizer, live theme switching, and mini mode](https://stevinator.com/apps/musinator/demo.gif)

Two screens. **Library** on the left, your music on the right. **Now Playing** with a big cover, the lyrics and a visualizer. `z` switches between them.

**Real album covers.** In Ghostty, which Omarchy uses, the covers are real pictures. In other terminals they are block pictures. There is also an album grid, if you like to browse by cover.

**Lyrics in time.** From an `.lrc` file next to the song, from the song itself, or from LRCLIB when you press `ctrl+l`. `f` opens full-screen lyrics with the current line in big letters.

**22 themes.** Press `t` and they change live while you move through the list. Or pick "Match Omarchy", and Musinator follows when you switch your Omarchy theme.

![Now Playing: the Highway To Hell cover, the lyrics with the current line marked, and the visualizer](https://stevinator.com/apps/musinator/nowplaying.webp)

## Not only your files

This started as a player for my own music. Then it grew:

- **Internet radio** from radio-browser.info: popular stations, genres, countries, search, and favorites. It shows the song that is on air.
- **Podcasts.** Find a show, subscribe, stream or download. It remembers where you stopped in every episode.
- **Music servers.** Navidrome, Jellyfin and other Subsonic servers. As many as you like, each in its own part of the sidebar.
- **Last.fm.** Scrobbles, now playing, and your recent and top lists.

All in the same screens, and one search (`/`) finds songs, albums, stations and server songs together.

## The details I cared about

- **Gapless.** No gap between songs, in every format: FLAC, MP3, AAC, ALAC, Vorbis, Opus, WAV, AIFF.
- **Honest audio.** It plays at the file's own sample rate. If the sound system changes it, the info line says so in orange.
- **Volume leveling.** ReplayGain from the tags, or measured by Musinator for songs without it.
- **Tag editing.** One song or a whole album. Fill tags from the file name, add a cover, or look the album up on MusicBrainz.
- **Light.** Near 0% CPU when idle. The visualizer stays under 5% of one core.
- **Private.** No account, no telemetry. It only goes online for the things you use.

Plus a 10-band equalizer, a sleep timer, mini mode with `m`, mouse support, and the Omarchy media keys.

![The 10-band equalizer with its curve and the Pop preset](https://stevinator.com/apps/musinator/eq.webp)

## What broke along the way

To be honest, a lot of it only showed up with real use:

- The lyrics switched a little late. The sound needs a moment from the program to your ears, about 40 ms on my machine. Now the lyric clock counts that in, so the line changes when you hear it.
- Album covers piled up in the terminal's memory while scrolling. Every new picture got a new number. Now each place on the screen reuses its number.
- In tmux the first key press after starting got lost. Musinator asked the terminal a question at startup, and tmux never answered. Now it does not ask terminals that cannot answer.
- With a new Navidrome, the artist list stayed empty and the server said "Library not found". It was still reading the music for the first time. Now Musinator tells you that, and how far it is.

Seventy automatic tests run on every change now.

## Install

One line, no sudo:

```sh
curl -fsSL https://raw.githubusercontent.com/smanookian/musinator/main/install.sh | sh
```

It needs PipeWire and libopus. On Omarchy you have both. Then run `musinator` and press `?` for all keys. An AUR package is coming.

## Where to find it

- App page, keys, and FAQ: [stevinator.com/apps/musinator](https://stevinator.com/apps/musinator)
- Source: [github.com/smanookian/musinator](https://github.com/smanookian/musinator)

Rust, MIT license. If something sounds wrong or looks wrong on your machine, open an issue. I would rather hear about it than not.

Most of the code was written with an AI coding assistant; I designed it, reviewed it and tested it on my machine.

*First published on [stevinator.com](https://stevinator.com/blog/programming/musinator-a-music-player-for-the-terminal).*

I hope to see you next time. Steve out. Tschau!
