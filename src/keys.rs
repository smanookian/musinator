//! Actions and their keys. Every action is remappable in Settings → Keys.

use std::collections::HashMap;
use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    PlayPause,
    PlaySelected,
    Next,
    Prev,
    SeekBack,
    SeekFwd,
    SeekBackBig,
    SeekFwdBig,
    VolUp,
    VolDown,
    Shuffle,
    Repeat,
    AddQueue,
    PlayNext,
    Search,
    ToggleScreen,
    Lyrics,
    Visualizer,
    Queue,
    Themes,
    Settings,
    Help,
    NextPane,
    PrevPane,
    Down,
    Up,
    Top,
    Bottom,
    PageDown,
    PageUp,
    Back,
    MoveDown,
    MoveUp,
    Remove,
    Like,
    AddToPlaylist,
    NewPlaylist,
    GridView,
    PaneShrink,
    PaneGrow,
    Rescan,
    RenamePlaylist,
    ExportPlaylist,
    ClearQueue,
    Quit,
}

pub struct ActionInfo {
    pub action: Action,
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub keys: &'static [&'static str],
}

macro_rules! actions {
    ($($a:ident $id:literal $group:literal $label:literal [$($k:literal),*];)*) => {
        pub const ACTIONS: &[ActionInfo] = &[$(ActionInfo { action: Action::$a, id: $id, label: $label, group: $group, keys: &[$($k),*] }),*];
    };
}

actions! {
    PlayPause "play_pause" "Playback" "Play / pause" ["space"];
    PlaySelected "play_selected" "Playback" "Play selected" ["enter"];
    Next "next" "Playback" "Next track" ["n"];
    Prev "prev" "Playback" "Previous track" ["p"];
    SeekBack "seek_back" "Playback" "Seek back 5 s" ["left"];
    SeekFwd "seek_fwd" "Playback" "Seek forward 5 s" ["right"];
    SeekBackBig "seek_back_big" "Playback" "Seek back 30 s" ["shift+left"];
    SeekFwdBig "seek_fwd_big" "Playback" "Seek forward 30 s" ["shift+right"];
    VolUp "vol_up" "Playback" "Volume up" ["+", "="];
    VolDown "vol_down" "Playback" "Volume down" ["-"];
    Shuffle "shuffle" "Playback" "Shuffle on / off" ["s"];
    Repeat "repeat" "Playback" "Repeat: off, all, one" ["r"];
    AddQueue "add_queue" "Queue" "Add to queue" ["a"];
    PlayNext "play_next" "Queue" "Play next" ["A"];
    MoveDown "move_down" "Queue" "Move item down" ["J"];
    MoveUp "move_up" "Queue" "Move item up" ["K"];
    Remove "remove" "Queue" "Remove item" ["d", "delete"];
    Queue "queue" "Queue" "Show queue" ["q"];
    ClearQueue "clear_queue" "Queue" "Clear queue" ["X"];
    Search "search" "Find" "Search everything" ["/"];
    Like "like" "Library" "Like / unlike" ["L"];
    AddToPlaylist "add_to_playlist" "Library" "Add to playlist" ["P"];
    NewPlaylist "new_playlist" "Library" "New playlist" ["N"];
    RenamePlaylist "rename_playlist" "Library" "Rename playlist" ["R"];
    ExportPlaylist "export_playlist" "Library" "Export playlist (.m3u8)" ["E"];
    GridView "grid" "Library" "Album grid on / off" ["o"];
    Rescan "rescan" "Library" "Rescan music folders" ["ctrl+r"];
    ToggleScreen "toggle_screen" "Screens" "Library ↔ Now Playing" ["z"];
    Lyrics "lyrics" "Screens" "Lyrics on / off" ["l"];
    Visualizer "visualizer" "Screens" "Visualizer on / off" ["v"];
    Themes "themes" "Screens" "Themes" ["t"];
    Settings "settings" "Screens" "Settings" ["S"];
    Help "help" "Screens" "Help" ["?"];
    NextPane "next_pane" "Move" "Next pane" ["tab"];
    PrevPane "prev_pane" "Move" "Previous pane" ["backtab"];
    Down "down" "Move" "Down" ["j", "down"];
    Up "up" "Move" "Up" ["k", "up"];
    Top "top" "Move" "Top" ["g", "home"];
    Bottom "bottom" "Move" "Bottom" ["G", "end"];
    PageDown "page_down" "Move" "Page down" ["pagedown", "ctrl+d"];
    PageUp "page_up" "Move" "Page up" ["pageup", "ctrl+u"];
    Back "back" "Move" "Back / close" ["esc", "backspace"];
    PaneShrink "pane_shrink" "Move" "Left pane narrower" ["<"];
    PaneGrow "pane_grow" "Move" "Left pane wider" [">"];
    Quit "quit" "App" "Quit" ["Q", "ctrl+c"];
}

pub fn info(a: Action) -> &'static ActionInfo {
    ACTIONS.iter().find(|i| i.action == a).expect("action info")
}

/// A key with modifiers, normalised so `shift+j` and `J` compare equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Chord {
    pub fn from_event(e: &KeyEvent) -> Self {
        let mut mods = e.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        let code = match e.code {
            KeyCode::Char(c) => {
                // Shift is already in the character.
                mods.remove(KeyModifiers::SHIFT);
                KeyCode::Char(c)
            }
            KeyCode::BackTab => {
                mods.remove(KeyModifiers::SHIFT);
                KeyCode::BackTab
            }
            c => c,
        };
        Self { code, mods }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let mut mods = KeyModifiers::NONE;
        let mut parts: Vec<&str> = s.split('+').collect();
        // "+" alone, or "ctrl++".
        let key = if s.ends_with("++") || s == "+" {
            parts.retain(|p| !p.is_empty());
            "+"
        } else {
            parts.pop()?
        };
        for m in parts {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" => mods |= KeyModifiers::CONTROL,
                "alt" => mods |= KeyModifiers::ALT,
                "shift" => mods |= KeyModifiers::SHIFT,
                "" => {}
                _ => return None,
            }
        }
        let code = match key.to_ascii_lowercase().as_str() {
            "space" => KeyCode::Char(' '),
            "enter" => KeyCode::Enter,
            "esc" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "backtab" => KeyCode::BackTab,
            "backspace" => KeyCode::Backspace,
            "delete" => KeyCode::Delete,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            f if f.len() > 1 && f.starts_with('f') && f[1..].parse::<u8>().is_ok() => KeyCode::F(f[1..].parse().ok()?),
            _ if key.chars().count() == 1 => {
                let c = key.chars().next()?;
                if mods.contains(KeyModifiers::SHIFT) && c.is_ascii_alphabetic() {
                    mods.remove(KeyModifiers::SHIFT);
                    KeyCode::Char(c.to_ascii_uppercase())
                } else {
                    KeyCode::Char(c)
                }
            }
            _ => return None,
        };
        if matches!(code, KeyCode::Char(c) if c.is_ascii_alphabetic())
            && mods.contains(KeyModifiers::CONTROL)
            && let KeyCode::Char(c) = code
        {
            return Some(Self { code: KeyCode::Char(c.to_ascii_lowercase()), mods });
        }
        Some(Self { code, mods })
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mods.contains(KeyModifiers::CONTROL) {
            f.write_str("ctrl+")?;
        }
        if self.mods.contains(KeyModifiers::ALT) {
            f.write_str("alt+")?;
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            f.write_str("shift+")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("space"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::Enter => f.write_str("enter"),
            KeyCode::Esc => f.write_str("esc"),
            KeyCode::Tab => f.write_str("tab"),
            KeyCode::BackTab => f.write_str("shift+tab"),
            KeyCode::Backspace => f.write_str("backspace"),
            KeyCode::Delete => f.write_str("delete"),
            KeyCode::Left => f.write_str("←"),
            KeyCode::Right => f.write_str("→"),
            KeyCode::Up => f.write_str("↑"),
            KeyCode::Down => f.write_str("↓"),
            KeyCode::Home => f.write_str("home"),
            KeyCode::End => f.write_str("end"),
            KeyCode::PageUp => f.write_str("pageup"),
            KeyCode::PageDown => f.write_str("pagedown"),
            KeyCode::F(n) => write!(f, "f{n}"),
            _ => f.write_str("?"),
        }
    }
}

/// Config spelling of a chord (ASCII, parseable).
pub fn chord_id(c: &Chord) -> String {
    let s = c.to_string();
    s.replace('←', "left").replace('→', "right").replace('↑', "up").replace('↓', "down").replace("shift+tab", "backtab")
}

#[derive(Clone, Debug)]
pub struct Keymap {
    by_chord: HashMap<Chord, Action>,
    by_action: HashMap<Action, Vec<Chord>>,
}

impl Keymap {
    /// Defaults, with user overrides (`action id -> list of keys`).
    pub fn new(overrides: &HashMap<String, Vec<String>>) -> Self {
        let mut by_action: HashMap<Action, Vec<Chord>> = HashMap::new();
        for i in ACTIONS {
            let keys: Vec<Chord> = match overrides.get(i.id) {
                Some(v) => v.iter().filter_map(|k| Chord::parse(k)).collect(),
                None => i.keys.iter().filter_map(|k| Chord::parse(k)).collect(),
            };
            by_action.insert(i.action, keys);
        }
        let mut by_chord = HashMap::new();
        for i in ACTIONS {
            for c in &by_action[&i.action] {
                by_chord.entry(*c).or_insert(i.action);
            }
        }
        Self { by_chord, by_action }
    }

    pub fn action(&self, e: &KeyEvent) -> Option<Action> {
        self.by_chord.get(&Chord::from_event(e)).copied()
    }

    pub fn keys(&self, a: Action) -> &[Chord] {
        self.by_action.get(&a).map(Vec::as_slice).unwrap_or(&[])
    }

    /// First key as text, for hints ("space", "/").
    pub fn hint(&self, a: Action) -> String {
        self.keys(a).first().map(|c| c.to_string()).unwrap_or_else(|| "—".into())
    }

    /// Another action that already uses this chord.
    pub fn conflict(&self, c: &Chord, not: Action) -> Option<Action> {
        ACTIONS.iter().map(|i| i.action).find(|a| *a != not && self.keys(*a).contains(c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent { code, modifiers: mods, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::NONE }
    }

    #[test]
    fn defaults_and_remap() {
        let k = Keymap::new(&HashMap::new());
        assert_eq!(k.action(&ev(KeyCode::Char(' '), KeyModifiers::NONE)), Some(Action::PlayPause));
        // Terminals send uppercase J with SHIFT set.
        assert_eq!(k.action(&ev(KeyCode::Char('J'), KeyModifiers::SHIFT)), Some(Action::MoveDown));
        assert_eq!(k.action(&ev(KeyCode::Left, KeyModifiers::SHIFT)), Some(Action::SeekBackBig));
        assert_eq!(k.action(&ev(KeyCode::Char('c'), KeyModifiers::CONTROL)), Some(Action::Quit));
        let mut o = HashMap::new();
        o.insert("play_pause".to_string(), vec!["ctrl+p".to_string()]);
        let k = Keymap::new(&o);
        assert_eq!(k.action(&ev(KeyCode::Char(' '), KeyModifiers::NONE)), None);
        assert_eq!(k.action(&ev(KeyCode::Char('p'), KeyModifiers::CONTROL)), Some(Action::PlayPause));
        for i in ACTIONS {
            for key in i.keys {
                let c = Chord::parse(key).unwrap_or_else(|| panic!("bad default key {key}"));
                assert_eq!(Chord::parse(&chord_id(&c)), Some(c), "{key}");
            }
        }
    }

    #[test]
    fn no_default_conflicts() {
        let k = Keymap::new(&HashMap::new());
        for i in ACTIONS {
            for c in k.keys(i.action) {
                assert_eq!(k.conflict(c, i.action), None, "{} clashes on {c}", i.id);
            }
        }
    }
}
