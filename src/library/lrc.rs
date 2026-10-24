//! Parse synced lyrics (.lrc): `[mm:ss.xx] line`, several stamps per line allowed,
//! `[offset:+/-ms]` supported. Text without stamps becomes plain lyrics.
//! Enhanced LRC word times (`[00:12.00] <00:12.00> Hold <00:12.40> on`) give
//! word-by-word karaoke.

use crate::source::Lyrics;

/// (start ms, word) pairs of one line.
type Words = Vec<(u64, String)>;

pub fn parse(text: &str) -> Lyrics {
    let mut synced: Vec<(u64, String, Words)> = Vec::new();
    let mut plain = Vec::new();
    let mut offset: i64 = 0;
    for line in text.lines() {
        let mut rest = line.trim();
        let mut stamps = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            let tag = &stripped[..end];
            if let Some(v) = tag.strip_prefix("offset:") {
                offset = v.trim().parse().unwrap_or(0);
            } else if let Some(ms) = stamp(tag) {
                stamps.push(ms);
            }
            rest = stripped[end + 1..].trim_start();
        }
        if stamps.is_empty() {
            if !line.trim_start().starts_with('[') {
                plain.push(line.to_string());
            }
            continue;
        }
        let (text, words) = split_words(rest);
        for s in stamps {
            let shift = |t: u64| (t as i64 - offset).max(0) as u64;
            let words = words.iter().map(|(t, w)| (shift(*t), w.clone())).collect();
            synced.push((shift(s), text.clone(), words));
        }
    }
    synced.sort_by_key(|l| l.0);
    let plain = plain.join("\n");
    Lyrics {
        words: synced.iter().map(|l| l.2.clone()).collect(),
        synced: synced.into_iter().map(|l| (l.0, l.1)).collect(),
        plain: if plain.trim().is_empty() { None } else { Some(plain) },
    }
}

/// Split `<mm:ss.xx> word <mm:ss.xx> word` into plain text and timed words.
fn split_words(s: &str) -> (String, Vec<(u64, String)>) {
    if !s.contains('<') {
        return (s.to_string(), Vec::new());
    }
    let mut words: Vec<(u64, String)> = Vec::new();
    let mut rest = s;
    let mut cur: Option<u64> = None;
    loop {
        match rest.find('<') {
            Some(i) => {
                let before = rest[..i].trim();
                if let (Some(t), false) = (cur, before.is_empty()) {
                    words.push((t, before.to_string()));
                }
                let Some(j) = rest[i..].find('>') else { break };
                cur = stamp(&rest[i + 1..i + j]).or(cur);
                rest = &rest[i + j + 1..];
            }
            None => {
                let last = rest.trim();
                if let (Some(t), false) = (cur, last.is_empty()) {
                    words.push((t, last.to_string()));
                }
                break;
            }
        }
    }
    let text = words.iter().map(|w| w.1.as_str()).collect::<Vec<_>>().join(" ");
    (text, words)
}

fn stamp(t: &str) -> Option<u64> {
    let (m, s) = t.split_once(':')?;
    let m: u64 = m.trim().parse().ok()?;
    let s: f64 = s.trim().replace(',', ".").parse().ok()?;
    Some(m * 60_000 + (s * 1000.0).round() as u64)
}

/// Index of the line playing at `ms`.
pub fn current(l: &Lyrics, ms: u64) -> Option<usize> {
    match l.synced.partition_point(|x| x.0 <= ms) {
        0 => None,
        n => Some(n - 1),
    }
}

/// How many words of line `i` have been sung at `ms` (None if the line has no word times).
pub fn words_sung(l: &Lyrics, i: usize, ms: u64) -> Option<usize> {
    let w = l.words.get(i).filter(|w| !w.is_empty())?;
    Some(w.partition_point(|x| x.0 <= ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stamps_offset_and_repeats() {
        let l = parse("[ar:x]\n[offset:500]\n[00:01.00][00:05.50]Hold on\n[00:03.20] Every street\nno stamp");
        assert_eq!(l.synced, vec![(500, "Hold on".into()), (2700, "Every street".into()), (5000, "Hold on".into())]);
        assert_eq!(l.plain.as_deref(), Some("no stamp"));
        assert_eq!(current(&l, 100), None);
        assert_eq!(current(&l, 2700), Some(1));
        assert_eq!(current(&l, 99_000), Some(2));
        assert!(l.words.iter().all(|w| w.is_empty()));
    }

    #[test]
    fn parses_word_times() {
        let l = parse("[00:12.00]<00:12.00> Hold <00:12.40> on, <00:13.10> the morning\n[00:15.00] plain line");
        assert_eq!(l.synced[0], (12_000, "Hold on, the morning".into()));
        assert_eq!(l.words[0], vec![(12_000, "Hold".into()), (12_400, "on,".into()), (13_100, "the morning".into())]);
        assert_eq!(words_sung(&l, 0, 11_999), Some(0));
        assert_eq!(words_sung(&l, 0, 12_400), Some(2));
        assert_eq!(words_sung(&l, 0, 20_000), Some(3));
        assert_eq!(words_sung(&l, 1, 15_500), None);
    }
}
