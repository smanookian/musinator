//! Invented names: artists, albums, tracks, lyrics. Some use accents, CJK and other
//! scripts, and some are very long, to test layout.

use crate::rng::Rng;

pub const GENRES: [&str; 15] = [
    "Ambient",
    "Electronic",
    "Synthwave",
    "Rock",
    "Indie",
    "Jazz",
    "Classical",
    "Hip-Hop",
    "Folk",
    "Metal",
    "Pop",
    "Soul",
    "Techno",
    "Post-Rock",
    "Downtempo",
];

const ADJ: &[&str] = &[
    "Neon",
    "Silent",
    "Golden",
    "Hollow",
    "Velvet",
    "Electric",
    "Distant",
    "Paper",
    "Crystal",
    "Midnight",
    "Faded",
    "Burning",
    "Quiet",
    "Wild",
    "Northern",
    "Broken",
    "Glass",
    "Static",
    "Lunar",
    "Copper",
    "Endless",
    "Restless",
    "Hidden",
    "Frozen",
    "Amber",
    "Silver",
    "Crimson",
    "Pale",
    "Analog",
    "Digital",
    "Slow",
    "Bright",
    "Lost",
    "Secret",
    "Open",
    "Low",
    "High",
    "Last",
    "First",
    "Little",
    "Heavy",
    "Gentle",
    "Empty",
    "Sudden",
    "Vivid",
    "Cold",
    "Warm",
    "Blue",
    "Violet",
    "Ivory",
    "Rusty",
    "Floating",
    "Sleeping",
    "Wandering",
    "Shining",
    "Falling",
    "Rising",
    "Hazy",
    "Coastal",
    "Urban",
];

const NOUN: &[&str] = &[
    "Harbor",
    "Signal",
    "Garden",
    "River",
    "Echo",
    "Mirror",
    "Lantern",
    "Horizon",
    "Tide",
    "Engine",
    "Satellite",
    "Forest",
    "Window",
    "Highway",
    "Static",
    "Orbit",
    "Station",
    "Shadow",
    "Canyon",
    "Avenue",
    "Comet",
    "Meadow",
    "Circuit",
    "Lighthouse",
    "Desert",
    "Island",
    "Thunder",
    "Ember",
    "Cathedral",
    "Machine",
    "Valley",
    "Ocean",
    "Paper",
    "Glacier",
    "Pulse",
    "Atlas",
    "Arcade",
    "Feather",
    "Compass",
    "Bridge",
    "Tower",
    "Season",
    "Morning",
    "Evening",
    "Winter",
    "Summer",
    "Harvest",
    "Fever",
    "Parade",
    "Postcard",
    "Radio",
    "Wire",
    "Spiral",
    "Prism",
    "Cloud",
    "Rain",
    "Ghost",
    "Signal Fire",
    "Night",
    "Dream",
];

const FIRST: &[&str] = &[
    "Ada", "Milo", "Zoë", "Søren", "Mélanie", "José", "Björn", "Ines", "Kai", "Lena", "Tomás", "Noor", "Elif", "Jonas", "Chloé", "Mateo",
    "Freya", "Anaïs", "Pål", "Ana", "Theo", "Mira", "Ravi", "Hana", "Oskar", "Liv", "Nina", "Émile", "Dario", "Sasha", "Yara", "Felix",
    "Iris", "Bruno", "Alma", "Luca",
];

const LAST: &[&str] = &[
    "Lindqvist",
    "Okafor",
    "Dubois",
    "Moreau",
    "Kowalski",
    "Haugen",
    "Novak",
    "Ferreira",
    "Åberg",
    "Müller",
    "Castillo",
    "Nakamura",
    "Rossi",
    "Byrne",
    "Andersen",
    "Petrov",
    "Silva",
    "Horvat",
    "Laurent",
    "Østergaard",
    "Keane",
    "Varga",
    "Ortega",
    "Sato",
    "Fischer",
    "Quinn",
    "Delacroix",
    "Brandt",
    "Szabó",
    "Navarro",
];

const CJK_ARTISTS: &[&str] = &[
    "坂本 光",
    "山田 美雨",
    "青木 透",
    "林 夏海",
    "藤原 空",
    "王 小明",
    "陈 静",
    "李 雨桐",
    "김 하늘",
    "박 서준",
    "夜行列車",
    "星屑通信",
    "月光楽団",
    "푸른 새벽",
    "東京信号",
];

const OTHER_ARTISTS: &[&str] = &[
    "Ночной Сигнал",
    "Θάλασσα",
    "Les Lumières",
    "Café Électrique",
    "Señal Perdida",
    "Über Grün",
    "Fjärilseffekten",
    "Mañana Club",
    "Ølhallen",
    "Nuit Blanche",
];

const CJK_TITLES: &[&str] = &[
    "夜の信号",
    "海の灯り",
    "青い港",
    "静かな雨",
    "星空",
    "遠い街",
    "月光",
    "雨の音",
    "光の中で",
    "東京の夜",
    "夏の終わり",
    "白い朝",
    "城市之光",
    "无声的河",
    "夜空",
    "밤의 신호",
    "푸른 바다",
    "새벽 세 시",
];

const OTHER_TITLES: &[&str] = &[
    "Été Indien",
    "Café de Nuit",
    "Señales",
    "Über den Wolken",
    "Fjäril",
    "Mañana Lejos",
    "Ночной Город",
    "Ωκεανός",
    "Déjà Vu",
    "Rêverie",
    "Smörgåsbord Blues",
    "São Paulo à Noite",
    "Naïve Mélodie",
    "Ålesund",
];

const SUFFIX: &[&str] = &[" (Remix)", " (Live)", " (Interlude)", " (Reprise)", " (Demo)", " (Radio Edit)", " (Acoustic)"];

fn adj_noun(rng: &mut Rng) -> String {
    format!("{} {}", rng.pick(ADJ), rng.pick(NOUN))
}

fn long_title(rng: &mut Rng) -> String {
    format!(
        "{} {} and the {} {} of the {} {} (Extended Version) [Remastered {}]",
        rng.pick(ADJ),
        rng.pick(NOUN),
        rng.pick(ADJ),
        rng.pick(NOUN),
        rng.pick(ADJ),
        rng.pick(NOUN),
        rng.range(1995, 2024)
    )
}

pub fn artist_name(rng: &mut Rng) -> String {
    match rng.weighted(&[30, 18, 25, 10, 6, 4, 3, 4]) {
        0 => adj_noun(rng),
        1 => format!("The {}s", rng.pick(NOUN)),
        2 => format!("{} {}", rng.pick(FIRST), rng.pick(LAST)),
        3 => format!("{} {}", rng.pick(NOUN), rng.pick(NOUN)),
        4 => rng.pick(CJK_ARTISTS).to_string(),
        5 => rng.pick(OTHER_ARTISTS).to_string(),
        6 => format!("{} & The {}s", rng.pick(FIRST), rng.pick(NOUN)),
        _ => format!("{} {} Orchestra", rng.pick(ADJ), rng.pick(NOUN)),
    }
}

pub fn album_title(rng: &mut Rng) -> String {
    match rng.weighted(&[35, 15, 12, 10, 8, 6, 5, 4, 5]) {
        0 => adj_noun(rng),
        1 => format!("{} Signals", rng.pick(ADJ)),
        2 => format!("{} of {}", rng.pick(NOUN), rng.pick(NOUN)),
        3 => format!("The {} {}", rng.pick(ADJ), rng.pick(NOUN)),
        4 => rng.pick(NOUN).to_string(),
        5 => rng.pick(CJK_TITLES).to_string(),
        6 => rng.pick(OTHER_TITLES).to_string(),
        7 => long_title(rng),
        _ => format!("{} {} Vol. {}", rng.pick(ADJ), rng.pick(NOUN), rng.range(1, 4)),
    }
}

pub fn track_title(rng: &mut Rng) -> String {
    let base = match rng.weighted(&[40, 14, 12, 10, 8, 5, 4, 3, 4]) {
        0 => adj_noun(rng),
        1 => rng.pick(NOUN).to_string(),
        2 => format!("{} in the {}", rng.pick(NOUN), rng.pick(NOUN)),
        3 => format!("The {} {}", rng.pick(ADJ), rng.pick(NOUN)),
        4 => format!("{} {}", rng.pick(NOUN), rng.pick(NOUN)),
        5 => rng.pick(CJK_TITLES).to_string(),
        6 => rng.pick(OTHER_TITLES).to_string(),
        7 => long_title(rng),
        _ => format!("{} ({} Mix)", adj_noun(rng), rng.pick(ADJ)),
    };
    if rng.chance(0.06) { format!("{base}{}", rng.pick(SUFFIX)) } else { base }
}

const LYRIC_SUBJ: &[&str] =
    &["We", "You", "I", "They", "The streets", "Our voices", "The waves", "Old radios", "The city lights", "Old friends"];
const LYRIC_VERB: &[&str] = &["keep", "follow", "carry", "forget", "hear", "light up", "count", "chase", "hold", "remember", "wait for"];
const LYRIC_OBJ: &[&str] = &[
    "the signal",
    "a thousand lights",
    "the harbor glow",
    "every quiet road",
    "the static",
    "the morning rain",
    "our names",
    "the last train home",
    "a paper moon",
    "the open sky",
    "the slow parade",
    "the northern wind",
];
const LYRIC_TAIL: &[&str] = &["", " tonight", " again", " down the line", " until the dawn", " in the dark", " for a while"];

/// One invented lyric line.
pub fn lyric_line(rng: &mut Rng) -> String {
    format!("{} {} {}{}", rng.pick(LYRIC_SUBJ), rng.pick(LYRIC_VERB), rng.pick(LYRIC_OBJ), rng.pick(LYRIC_TAIL))
}

/// A synced LRC text with lines spread over `len_s` seconds.
pub fn lrc(rng: &mut Rng, title: &str, artist: &str, album: &str, len_s: f64, gap: (f64, f64)) -> String {
    let mut s = format!("[ti:{title}]\n[ar:{artist}]\n[al:{album}]\n[length:{:02}:{:02}]\n", (len_s as u64) / 60, (len_s as u64) % 60);
    let mut t = rng.range_f(0.2, 0.8);
    let end = (len_s - 0.3).max(0.5);
    while t < end {
        let cs = (t * 100.0).round() as u64;
        let text = if rng.chance(0.08) { "(instrumental)".to_string() } else { lyric_line(rng) };
        s.push_str(&format!("[{:02}:{:02}.{:02}]{text}\n", cs / 6000, (cs / 100) % 60, cs % 100));
        t += rng.range_f(gap.0, gap.1);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_lines_stay_inside_the_track() {
        let mut rng = Rng::new(3);
        for len in [3.0, 7.5, 12.0] {
            let text = lrc(&mut rng, "t", "a", "b", len, (1.0, 2.2));
            let times: Vec<f64> = text
                .lines()
                .filter_map(|l| {
                    let (m, rest) = l.strip_prefix('[')?.split_once(':')?;
                    let s = rest.split_once(']')?.0;
                    Some(m.parse::<f64>().ok()? * 60.0 + s.parse::<f64>().ok()?)
                })
                .collect();
            assert!(!times.is_empty());
            assert!(times.iter().all(|&t| t >= 0.0 && t < len));
        }
    }
}
