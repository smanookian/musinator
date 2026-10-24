//! Parse synced lyrics (.lrc): `[mm:ss.xx] line`, several stamps per line allowed,
//! `[offset:+/-ms]` supported. Text without stamps becomes plain lyrics.

use crate::source::Lyrics;

pub fn parse(text: &str) -> Lyrics {
    let mut synced = Vec::new();
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
        } else {
            for s in stamps {
                synced.push(((s - offset).max(0) as u64, rest.to_string()));
            }
        }
    }
    synced.sort_by_key(|l| l.0);
    let plain = plain.join("\n");
    Lyrics { synced, plain: if plain.trim().is_empty() { None } else { Some(plain) } }
}

fn stamp(t: &str) -> Option<i64> {
    let (m, s) = t.split_once(':')?;
    let m: i64 = m.trim().parse().ok()?;
    let s: f64 = s.trim().replace(',', ".").parse().ok()?;
    Some(m * 60_000 + (s * 1000.0).round() as i64)
}

/// Index of the line playing at `ms`.
pub fn current(l: &Lyrics, ms: u64) -> Option<usize> {
    match l.synced.partition_point(|x| x.0 <= ms) {
        0 => None,
        n => Some(n - 1),
    }
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
    }
}
