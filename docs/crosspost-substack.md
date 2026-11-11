# Substack article (Stevinator Notes)

Same shape as "A Free Omarchy Course: From Zero to Hero": short intro, what's in it, what comes with it, one link, ask for feedback, sign-off.

New post → paste title and subtitle → paste the body → put the GIF (`docs/demo/musinator-demo.gif`) under the first paragraph → "Make the headings": the lines starting with `##` become headings.

**Title:**
```
Musinator: A Music Player for the Terminal That Looks Good
```

**Subtitle:**
```
Album covers, lyrics in time with the song, and 22 themes, free for Omarchy and Linux. Plus what broke along the way.
```

**Body:**

```
So here is the thing about music in the terminal. The players are fast, but most of them look like 1998.

I live in the terminal on Omarchy, and I wanted my music there too. None of the players I tried showed me the album cover, the lyrics, or colors that match my desktop. So I built the one I wanted. It's free, and it stays free.

## What it does

Two screens: your library, and Now Playing with a big cover, the lyrics and a visualizer.

- Real album covers in Ghostty, the terminal Omarchy uses.
- Lyrics in time with the song, from a file, the song itself, or LRCLIB with one key.
- 22 themes that change live, or "Match Omarchy", which follows your Omarchy theme.
- Gapless playback in every format, at the file's own sample rate.
- Internet radio, podcasts with downloads, and your Navidrome or Jellyfin servers, all in the same screens and one search.
- Tag editing with MusicBrainz, a 10-band equalizer, mini mode, mouse support, and the Omarchy media keys.

It uses almost no CPU when idle. No account, no telemetry.

## What broke along the way

- The lyrics switched a little late. Sound needs about 40 ms from the program to your ears, so the lyric clock now counts that in.
- Album covers piled up in the terminal's memory while scrolling. Now each place on the screen reuses its picture.
- In tmux, the first key press got lost because tmux never answered a question Musinator asked at startup. Now it doesn't ask.

Most of the code was written with an AI coding assistant; I designed it, reviewed it and tested it on my machine.

## Try it

One line, no sudo:

curl -fsSL https://raw.githubusercontent.com/smanookian/musinator/main/install.sh | sh

Screenshots, keys and FAQ: https://stevinator.com/apps/musinator
Source (MIT): https://github.com/smanookian/musinator
The longer story: https://stevinator.com/blog/programming/musinator-a-music-player-for-the-terminal

If you try it, I'd appreciate honest feedback, especially anything that sounds wrong or looks off on your machine.

Steve out. Tschau!
```

Format the `curl` line as code in Substack (select it, then the code button). Leave everything else as normal text.
