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
    fn rows(&self, width: usize) -> Vec<&str> {
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

    fn visible_rows(&self, width: usize, height: usize) -> Vec<&str> {
        let rows = self.rows(width);
        let offset = self.scroll.min(rows.len().saturating_sub(height));
        let end = rows.len() - offset;
        rows[end.saturating_sub(height)..end].to_vec()
    }
}

fn wrap_rows(line: &str, width: usize) -> Vec<&str> {
    let mut rows = Vec::new();
    let mut start = 0;
    let mut cells = 0;
    for (index, ch) in line.char_indices() {
        // Reserve two cells for non-ASCII characters to keep wide text inside the viewport.
        let size = if ch.is_ascii() { 1 } else { 2 };
        if cells + size > width {
            rows.push(&line[start..index]);
            start = index;
            cells = 0;
        }
        cells += size;
    }
    rows.push(&line[start..]);
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
            window.scroll += wrap_rows(&line, usize::from(width.saturating_sub(1)).max(2)).len();
        }
        if window.lines.len() == SCROLLBACK {
            window.lines.pop_front();
        }
        window.lines.push_back(line);
        window.unread |= self.selected != index;
    }

    pub fn page(&mut self, up: bool) -> io::Result<()> {
        let (width, height) = terminal::size()?;
        if width >= 16 && height >= 5 {
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
        if width < 16 || height < 5 {
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
        for (index, line) in window.visible_rows(width, rows).into_iter().enumerate() {
            queue!(out, MoveTo(0, index as u16 + 1), Print(line))?;
        }
        let status = format!("{local} | {status} | {} | /help", window.peer);
        queue!(
            out,
            MoveTo(0, height - 3),
            Print(status.chars().take(width).collect::<String>()),
            MoveTo(0, height - 2),
            Print("> "),
        )?;
        let tail: String = window
            .draft
            .chars()
            .rev()
            .take((width - 2) / 2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        queue!(out, Print(tail))?;
        out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_up_reaches_the_start_of_a_long_wrapped_record() {
        let mut window = Window {
            lines: [format!("START{}END", "x".repeat(1_016))].into(),
            ..Window::default()
        };
        assert!(window.visible_rows(39, 6).last().unwrap().ends_with("END"));
        for _ in 0..10 {
            window.scroll_by_page(true, 39, 6);
        }
        assert_eq!(window.visible_rows(39, 6).len(), 6);
        assert!(window.visible_rows(39, 6)[0].starts_with("START"));
        for _ in 0..10 {
            window.scroll_by_page(false, 39, 6);
        }
        assert!(window.visible_rows(39, 6).last().unwrap().ends_with("END"));
    }
}
