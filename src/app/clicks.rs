//! Mouse targets recorded while drawing: whatever draws a button or a key hint
//! records where it is and what a click means. Cleared at the start of each frame;
//! later entries (dialogs, drawn on top) win.

use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect};
use unicode_width::UnicodeWidthStr;

use crate::keys::{Action, Chord};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Click {
    /// Run this action (player buttons).
    Act(Action),
    /// Same as pressing this key (key hints like "a queue").
    Key(KeyEvent),
    /// A row in a dialog list.
    Row(usize),
}

thread_local! {
    static HITS: RefCell<Vec<(Rect, Click)>> = const { RefCell::new(Vec::new()) };
}

pub fn clear() {
    HITS.with(|h| h.borrow_mut().clear());
}

pub fn add(r: Rect, c: Click) {
    if r.width > 0 && r.height > 0 {
        HITS.with(|h| h.borrow_mut().push((r, c)));
    }
}

pub fn at(p: Position) -> Option<Click> {
    HITS.with(|h| h.borrow().iter().rev().find(|(r, _)| r.contains(p)).map(|(_, c)| *c))
}

/// The key a hint starts with: "a", "ctrl+l", "enter", "/", "←/→" (first one), "J/K".
fn key_of(word: &str) -> Option<KeyEvent> {
    let w = if word.len() > 1 && word.contains('/') { word.split('/').next()? } else { word };
    let code = match w {
        "←" => Some(KeyCode::Left),
        "→" => Some(KeyCode::Right),
        "↑" => Some(KeyCode::Up),
        "↓" => Some(KeyCode::Down),
        _ => None,
    };
    if let Some(code) = code {
        return Some(KeyEvent::new(code, KeyModifiers::NONE));
    }
    // Only short words are keys ("type", "Filled" are not).
    let c = Chord::parse(w)?;
    // A word like "to" is not a key, even if it parses as one.
    if matches!(c.code, KeyCode::Char(_)) && w.chars().count() > 1 && !w.contains('+') {
        return None;
    }
    Some(KeyEvent::new(c.code, c.mods))
}

/// Make each "key label" part of a hint line clickable. Parts are split by
/// " · " or two spaces, as all hint lines are written.
pub fn hint_text(x: u16, y: u16, text: &str) {
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut parts = Vec::new();
    while i <= text.len() {
        let sep = if text[i..].starts_with(" · ") {
            " · ".len()
        } else if i + 1 < text.len() && bytes[i] == b' ' && bytes[i + 1] == b' ' {
            2
        } else if i == text.len() {
            0
        } else {
            i += text[i..].chars().next().map_or(1, char::len_utf8);
            continue;
        };
        parts.push((start, i));
        if sep == 0 {
            break;
        }
        i += sep;
        while text[i..].starts_with(' ') {
            i += 1;
        }
        start = i;
    }
    for (s, e) in parts {
        let part = &text[s..e];
        let Some(key) = part.split_whitespace().next().and_then(key_of) else { continue };
        let px = x + text[..s].width() as u16;
        add(Rect { x: px, y, width: part.width() as u16, height: 1 }, Click::Key(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(text: &str) -> Vec<(u16, u16, KeyCode)> {
        clear();
        hint_text(10, 0, text);
        HITS.with(|h| {
            h.borrow()
                .iter()
                .map(|(r, c)| match c {
                    Click::Key(k) => (r.x, r.width, k.code),
                    _ => unreachable!(),
                })
                .collect()
        })
    }

    #[test]
    fn hint_parts_become_keys_at_their_place() {
        assert_eq!(
            keys("enter open  a queue  / search"),
            vec![(10, 10, KeyCode::Enter), (22, 7, KeyCode::Char('a')), (31, 8, KeyCode::Char('/'))]
        );
        assert_eq!(keys("y quit · esc cancel"), vec![(10, 6, KeyCode::Char('y')), (19, 10, KeyCode::Esc)]);
        assert_eq!(keys("←/→ band · ctrl+x remove")[0].2, KeyCode::Left);
        // Plain sentences are not keys.
        assert!(keys("type to filter · Filled 3 from the file name").is_empty());
    }
}
