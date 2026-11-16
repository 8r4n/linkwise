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
        if window.lines.len() == SCROLLBACK {
            window.lines.pop_front();
        }
        window.lines.push_back(line.into());
        window.unread |= self.selected != index;
        if window.scroll > 0 {
            window.scroll = (window.scroll + 1).min(window.lines.len() - 1);
        }
    }

    pub fn page(&mut self, up: bool) -> io::Result<()> {
        let step = usize::from(terminal::size()?.1.saturating_sub(5)).max(1);
        let window = &mut self.windows[self.selected];
        window.scroll = if up {
            (window.scroll + step).min(window.lines.len().saturating_sub(1))
        } else {
            window.scroll.saturating_sub(step)
        };
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
        let mut visible = VecDeque::new();
        for line in window
            .lines
            .iter()
            .take(window.lines.len().saturating_sub(window.scroll))
        {
            // Reserve two cells for non-ASCII characters to keep wide text inside the viewport.
            let mut row = String::new();
            let mut cells = 0;
            for ch in line.chars() {
                let size = if ch.is_ascii() { 1 } else { 2 };
                if cells + size > width {
                    visible.push_back(std::mem::take(&mut row));
                    cells = 0;
                }
                row.push(ch);
                cells += size;
            }
            visible.push_back(row);
            while visible.len() > rows {
                visible.pop_front();
            }
        }
        for (index, line) in visible.into_iter().enumerate() {
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
