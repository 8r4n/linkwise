use std::{
    collections::VecDeque,
    io::{self, Write},
};

use crossterm::{
    cursor::MoveTo,
    execute, queue,
    style::Print,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use linkwise::modem::setup::MAX_LINK_CALLSIGN_LEN;

const SCROLLBACK: usize = 200;
const WINDOWS: usize = 10;

pub struct Terminal;

impl Terminal {
    pub fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let terminal = Self;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(terminal)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Default)]
pub struct Window {
    pub peer: String,
    pub draft: String,
    lines: VecDeque<String>,
    unread: bool,
    scroll: usize,
}

impl Window {
    #[cfg(test)]
    pub(super) fn scrollback(&self) -> &VecDeque<String> {
        &self.lines
    }

    fn rows(&self, width: usize) -> Vec<Row<'_>> {
        self.lines
            .iter()
            .flat_map(|line| wrap_rows(line, width))
            .collect()
    }

    fn scroll_by_page(&mut self, up: bool, width: usize, height: usize) {
        let max = self.rows(width).len().saturating_sub(height);
        let step = height.saturating_sub(1).max(1);
        let offset = self.scroll.min(max);
        self.scroll = if up {
            (offset + step).min(max)
        } else {
            offset.saturating_sub(step)
        };
    }

    fn visible_rows(&self, width: usize, height: usize) -> Vec<Row<'_>> {
        let rows = self.rows(width);
        let offset = self.scroll.min(rows.len().saturating_sub(height));
        let end = rows.len() - offset;
        rows[end.saturating_sub(height)..end].to_vec()
    }
}

pub(super) enum MessageProtection {
    Plain,
    #[cfg(feature = "secure-transfer")]
    Signed,
    #[cfg(feature = "secure-transfer")]
    Encrypted,
    #[cfg(feature = "secure-transfer")]
    VerificationFailed,
    #[cfg(feature = "secure-transfer")]
    EncryptionFailed,
}

pub(super) fn format_message(callsign: &str, protection: MessageProtection, text: &str) -> String {
    let indicator = match protection {
        MessageProtection::Plain => "  ",
        #[cfg(feature = "secure-transfer")]
        MessageProtection::Signed => "|V",
        #[cfg(feature = "secure-transfer")]
        MessageProtection::Encrypted => "|E",
        #[cfg(feature = "secure-transfer")]
        MessageProtection::VerificationFailed => "|X",
        #[cfg(feature = "secure-transfer")]
        MessageProtection::EncryptionFailed => "|F",
    };
    format!("<{callsign:MAX_LINK_CALLSIGN_LEN$} {indicator}> {text}")
}

#[derive(Clone, Copy)]
struct Row<'a> {
    text: &'a str,
    indent: usize,
}

fn break_space(ch: char) -> bool {
    ch.is_whitespace() && !matches!(ch, '\u{00a0}' | '\u{2007}' | '\u{202f}')
}

fn wrap_at(text: &str, width: usize) -> (&str, &str) {
    let mut cells = 0;
    let mut last_break = None;
    let mut in_word = false;
    for (index, grapheme) in text.grapheme_indices(true) {
        let space = grapheme.chars().all(break_space);
        if space && in_word {
            last_break = Some(index);
        }
        let size = grapheme.width();
        if cells + size > width && index > 0 {
            let (row, rest) = text.split_at(last_break.unwrap_or(index));
            return (row, rest.trim_start_matches(break_space));
        }
        in_word = !space;
        cells += size;
    }
    (text, "")
}

fn wrap_rows(line: &str, width: usize) -> Vec<Row<'_>> {
    let mut rows = Vec::new();
    let mut rest = line;
    let mut indent = 0;
    // Keep the fixed-width callsign and status together where they fit.
    let header_len = MAX_LINK_CALLSIGN_LEN + 6;
    if let Some(header) = line.get(..header_len)
        && header.starts_with('<')
        && header.ends_with("> ")
        && width >= header_len - 1
    {
        let message = &line[header_len..];
        let remaining = width.saturating_sub(header_len);
        // On narrow terminals, continuation rows use the full width.
        indent = if remaining >= 24 { header_len } else { 0 };
        let first_word = message.split(break_space).find(|word| !word.is_empty());
        let (first, tail) = if remaining < 3
            || first_word
                .is_some_and(|word| word.width() > remaining && word.width() <= width - indent)
        {
            ("", message)
        } else {
            wrap_at(message, remaining)
        };
        rows.push(Row {
            text: line[..header_len + first.len()].trim_end_matches(break_space),
            indent: 0,
        });
        rest = tail;
    }
    while !rest.is_empty() {
        let (text, tail) = wrap_at(rest, width - indent);
        rows.push(Row { text, indent });
        rest = tail;
    }
    if rows.is_empty() {
        rows.push(Row {
            text: "",
            indent: 0,
        });
    }
    rows
}

pub struct View {
    pub windows: Vec<Window>,
    pub selected: usize,
}

impl View {
    pub fn new() -> Self {
        Self {
            windows: vec![Window {
                peer: "Status".into(),
                ..Window::default()
            }],
            selected: 0,
        }
    }

    pub fn peer(&mut self, peer: &str) -> usize {
        if let Some(index) = self.windows.iter().position(|window| window.peer == peer) {
            return index;
        }
        if self.windows.len() == WINDOWS {
            self.windows.remove(1);
        }
        self.windows.push(Window {
            peer: peer.into(),
            ..Window::default()
        });
        self.windows.len() - 1
    }

    pub fn select(&mut self, index: usize) {
        if let Some(window) = self.windows.get_mut(index) {
            self.selected = index;
            window.unread = false;
        }
    }

    pub fn log(&mut self, index: usize, line: impl Into<String>) {
        let window = &mut self.windows[index];
        let line = line.into();
        if window.scroll > 0
            && let Ok((width, _)) = terminal::size()
        {
            window.scroll += wrap_rows(&line, usize::from(width.saturating_sub(1)).max(3)).len();
        }
        if window.lines.len() == SCROLLBACK {
            window.lines.pop_front();
        }
        window.lines.push_back(line);
        window.unread |= self.selected != index;
    }

    pub fn page(&mut self, up: bool) -> io::Result<()> {
        let (width, height) = terminal::size()?;
        if width >= 4 && height >= 5 {
            self.windows[self.selected].scroll_by_page(
                up,
                usize::from(width - 1),
                usize::from(height - 4),
            );
        }
        Ok(())
    }

    pub fn draw(&self, local: &str, status: &str) -> io::Result<()> {
        let (width, height) = terminal::size()?;
        let mut out = io::stdout().lock();
        queue!(out, MoveTo(0, 0), Clear(ClearType::All))?;
        if width < 4 || height < 5 {
            return out.flush();
        }
        let width = usize::from(width - 1);
        let window = &self.windows[self.selected];
        let tabs = self
            .windows
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let label = format!("{index}:{}{}", tab.peer, if tab.unread { "*" } else { "" });
                if index == self.selected {
                    format!("[{label}]")
                } else {
                    label
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let above = self.windows[..self.selected]
            .iter()
            .map(|w| w.peer.chars().count() + 4)
            .sum::<usize>();
        let start = above.saturating_sub(width / 2);
        queue!(
            out,
            Print(tabs.chars().skip(start).take(width).collect::<String>())
        )?;

        let rows = usize::from(height - 4);
        for (index, row) in window.visible_rows(width, rows).into_iter().enumerate() {
            queue!(
                out,
                MoveTo(row.indent as u16, index as u16 + 1),
                Print(row.text)
            )?;
        }
        let status = format!("{local} | {status} | {} | /help", window.peer);
        queue!(
            out,
            MoveTo(0, height - 3),
            Print(status.chars().take(width).collect::<String>()),
            MoveTo(0, height - 2),
            Print("> "),
        )?;
        let mut available = width - 2;
        let mut start = window.draft.len();
        for (index, grapheme) in window.draft.grapheme_indices(true).rev() {
            let cells = grapheme.width();
            if cells > available {
                break;
            }
            available -= cells;
            start = index;
        }
        queue!(out, Print(&window.draft[start..]))?;
        out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "secure-transfer")]
    #[test]
    fn protection_codes_and_message_use_fixed_columns() {
        let plain = format_message("BB2BB", MessageProtection::Plain, "Hello");
        let signed = format_message("BB2BB", MessageProtection::Signed, "Hello");
        let encrypted = format_message("KN6MQS-15", MessageProtection::Encrypted, "Hello");
        let bad_signature = format_message("BB2BB", MessageProtection::VerificationFailed, "Hello");
        let bad_encryption =
            format_message("KN6MQS-15", MessageProtection::EncryptionFailed, "Hello");
        for (line, code) in [
            (&plain, "  "),
            (&signed, "|V"),
            (&encrypted, "|E"),
            (&bad_signature, "|X"),
            (&bad_encryption, "|F"),
        ] {
            assert_eq!(&line[11..13], code);
            assert_eq!(wrap_rows(line, 15)[1].text, "Hello");
        }
    }

    #[test]
    fn page_up_reaches_the_start_of_a_long_wrapped_record() {
        let mut window = Window {
            lines: [format!("START{}END", "x".repeat(1_016))].into(),
            ..Window::default()
        };
        assert!(
            window
                .visible_rows(39, 6)
                .last()
                .unwrap()
                .text
                .ends_with("END")
        );
        for _ in 0..10 {
            window.scroll_by_page(true, 39, 6);
        }
        assert_eq!(window.visible_rows(39, 6).len(), 6);
        assert!(window.visible_rows(39, 6)[0].text.starts_with("START"));
        for _ in 0..10 {
            window.scroll_by_page(false, 39, 6);
        }
        assert!(
            window
                .visible_rows(39, 6)
                .last()
                .unwrap()
                .text
                .ends_with("END")
        );
    }

    #[test]
    fn wraps_at_words_and_preserves_spacing_inside_rows() {
        let rows = wrap_rows("one  two three four", 9);
        let text = rows.iter().map(|row| row.text).collect::<Vec<_>>();
        assert_eq!(text, ["one  two", "three", "four"]);
    }

    #[test]
    fn message_keeps_its_header_and_uses_more_space_on_narrow_terminals() {
        let line = format_message(
            "BB2BB",
            MessageProtection::Plain,
            "A longer reply from my station.",
        );
        let narrow = wrap_rows(&line, 24);
        assert!(narrow[0].text.ends_with("> A longer"));
        assert_eq!(narrow[1].text, "reply from my station.");
        assert_eq!(narrow[1].indent, 0);
        let wide = wrap_rows(&line, 40);
        assert!(wide[0].text.ends_with("> A longer reply from my"));
        assert_eq!(wide[1].text, "station.");
        assert_eq!(wide[1].indent, 15);
    }

    #[test]
    fn all_supported_widths_keep_words_and_graphemes_readable() {
        let word = "https://example.invalid/".repeat(30);
        let rendered = format!("{word} café e\u{301} 🇺🇸 👩‍💻 日本語");
        let line = format_message("KN6MQS-15", MessageProtection::Plain, &rendered);
        let without_spaces = |text: &str| {
            text.chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>()
        };
        for width in 3..=240 {
            let rows = wrap_rows(&line, width);
            let contents = rows.iter().map(|row| row.text).collect::<String>();
            assert_eq!(
                without_spaces(&contents),
                without_spaces(&line),
                "width {width}"
            );
            for row in &rows {
                assert!(
                    row.indent + row.text.width() <= width,
                    "width {width}: {}",
                    row.text
                );
                assert!(
                    !row.text.starts_with(['\u{301}', '\u{200d}']),
                    "width {width}: {}",
                    row.text
                );
                assert!(
                    !row.text.ends_with('\u{200d}'),
                    "width {width}: {}",
                    row.text
                );
            }
        }
    }
}
