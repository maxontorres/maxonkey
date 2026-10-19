use std::io::{self, IsTerminal};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph, Wrap},
};

use crate::info;

const BACKGROUND: Color = Color::Rgb(0x08, 0x0E, 0x1C);
const PANEL: Color = Color::Rgb(0x15, 0x24, 0x3A);
const TEXT: Color = Color::Rgb(0xE8, 0xED, 0xF8);
const BLUE: Color = Color::Rgb(0x62, 0x8D, 0xFF);
const ICE: Color = Color::Rgb(0xB7, 0xD7, 0xFF);
const VIOLET: Color = Color::Rgb(0x67, 0x5D, 0xFF);
const AMBER: Color = Color::Rgb(0xFF, 0xD1, 0x66);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Screen {
    #[default]
    Dashboard,
    Help,
    About,
}

impl Screen {
    const ALL: [Self; 3] = [Self::Dashboard, Self::Help, Self::About];

    fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Help => "Help",
            Self::About => "About",
        }
    }

    fn cycle(self, forward: bool) -> Self {
        match (self, forward) {
            (Self::Dashboard, true) | (Self::About, false) => Self::Help,
            (Self::Help, true) | (Self::Dashboard, false) => Self::About,
            (Self::About, true) | (Self::Help, false) => Self::Dashboard,
        }
    }
}

#[derive(Default)]
struct App {
    screen: Screen,
    quit: bool,
}

impl App {
    fn handle_event(&mut self, event: Event) {
        let Event::Key(key) = event else { return };
        if key.kind != KeyEventKind::Press {
            return;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        // Shift+Tab is represented by BackTab, not Tab with a shift modifier.
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Tab | KeyCode::Right | KeyCode::Down => self.screen = self.screen.cycle(true),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Up => {
                self.screen = self.screen.cycle(false)
            }
            KeyCode::Char('1') => self.screen = Screen::Dashboard,
            KeyCode::Char('2' | '?') => self.screen = Screen::Help,
            KeyCode::Char('3') => self.screen = Screen::About,
            _ => {}
        }
    }
}

// Construct before initialization so partial setup failures also trigger cleanup.
struct TerminalSession {
    active: bool,
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.active {
            ratatui::restore();
        }
    }
}

pub fn run() -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "the TUI requires an interactive terminal; use maxonkey --help or maxonkey status",
        ));
    }
    let mut session = TerminalSession { active: true };
    let mut terminal = ratatui::try_init()?;
    let mut app = App::default();
    while !app.quit {
        terminal.draw(|frame| render(frame, &app))?;
        app.handle_event(event::read()?);
    }
    // Explicitly return restoration errors. The guard retries cleanup on error/unwind.
    terminal.show_cursor()?;
    ratatui::try_restore()?;
    session.active = false;
    Ok(())
}

fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    frame.render_widget(
        Block::new().style(Style::default().bg(BACKGROUND).fg(TEXT)),
        area,
    );
    if area.width < 64 || area.height < 22 {
        let text = format!(
            "MaxonKey\nResize to 64x22\nq/Esc Exit\n{}",
            app.screen.label()
        );
        frame.render_widget(Paragraph::new(text).style(Style::default().fg(TEXT)), area);
        return;
    }
    let [header, navigation, content, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(12),
        Constraint::Length(2),
    ])
    .margin(1)
    .areas(area);

    let title = Line::from(vec![
        Span::styled(
            " MAXONKEY ",
            Style::default().fg(ICE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" // {}  FOUNDATION PROTOTYPE", env!("CARGO_PKG_VERSION")),
            Style::default().fg(TEXT),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(title)
            .block(Block::bordered().border_style(Style::default().fg(VIOLET)))
            .style(Style::default().bg(PANEL)),
        header,
    );

    let tabs = Layout::horizontal([Constraint::Ratio(1, 3); 3]).split(navigation);
    for (index, screen) in Screen::ALL.into_iter().enumerate() {
        let selected = screen == app.screen;
        let label = format!(
            "{} {} [{}]",
            if selected { ">" } else { " " },
            screen.label(),
            index + 1
        );
        let block = Block::bordered()
            .border_type(if selected {
                BorderType::Double
            } else {
                BorderType::Plain
            })
            .border_style(Style::default().fg(if selected { BLUE } else { ICE }));
        frame.render_widget(
            Paragraph::new(label)
                .block(block)
                .style(Style::default().fg(TEXT).bg(PANEL)),
            tabs[index],
        );
    }

    let lines: Vec<Line<'_>> = match app.screen {
        Screen::Dashboard => std::iter::once(Line::from(
            "Foundation only. No secrets are read or stored.",
        ))
        .chain(std::iter::once(Line::from("")))
        .chain(info::UNAVAILABLE.into_iter().map(|label| {
            Line::from(vec![
                Span::styled("[!] ", Style::default().fg(AMBER)),
                Span::raw(format!("{label}: NOT IMPLEMENTED")),
            ])
        }))
        .collect(),
        Screen::Help => vec![
            Line::from("Tab / Right / Down     Next screen"),
            Line::from("Shift+Tab / Left / Up   Previous screen"),
            Line::from("1 / 2 / 3              Dashboard / Help / About"),
            Line::from("?                      Open Help"),
            Line::from("q / Esc / Ctrl+C       Exit and restore terminal"),
            Line::from(""),
            Line::from("CLI: help, version, about, status"),
            Line::from("No animations. No secret operations available."),
        ],
        Screen::About => info::ABOUT.lines().map(Line::from).collect(),
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::bordered()
                    .title(format!(" {} ", app.screen.label()))
                    .border_style(Style::default().fg(ICE)),
            )
            .style(Style::default().fg(TEXT).bg(PANEL))
            .wrap(Wrap { trim: false }),
        content,
    );
    frame.render_widget(
        Paragraph::new("Tab/Arrows Navigate | 1-3 Select | ? Help\nq/Esc/Ctrl+C Exit")
            .style(Style::default().fg(ICE)),
        footer,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyEventKind};
    use ratatui::{Terminal, backend::TestBackend};

    fn key(app: &mut App, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    fn rendered(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn navigation_wraps_and_supports_shortcuts() {
        let mut app = App::default();
        assert_eq!(app.screen, Screen::Dashboard);
        for code in [KeyCode::Tab, KeyCode::Right, KeyCode::Down] {
            for expected in [Screen::Help, Screen::About, Screen::Dashboard] {
                key(&mut app, code);
                assert_eq!(app.screen, expected);
            }
        }
        for code in [KeyCode::BackTab, KeyCode::Left, KeyCode::Up] {
            for expected in [Screen::About, Screen::Help, Screen::Dashboard] {
                key(&mut app, code);
                assert_eq!(app.screen, expected);
            }
        }
        for (code, screen) in [
            ('3', Screen::About),
            ('1', Screen::Dashboard),
            ('2', Screen::Help),
            ('?', Screen::Help),
        ] {
            key(&mut app, KeyCode::Char(code));
            assert_eq!(app.screen, screen);
        }
        assert!(!app.quit);
    }

    #[test]
    fn all_exit_keys_request_normal_shutdown() {
        for code in [KeyCode::Char('q'), KeyCode::Esc] {
            let mut app = App::default();
            key(&mut app, code);
            assert!(app.quit);
        }
        let mut app = App::default();
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert!(app.quit);
    }

    #[test]
    fn ignores_releases_repeats_and_unrelated_events() {
        let mut app = App::default();
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            app.handle_event(Event::Key(KeyEvent::new_with_kind(
                KeyCode::Tab,
                KeyModifiers::NONE,
                kind,
            )));
        }
        app.handle_event(Event::Resize(80, 24));
        key(&mut app, KeyCode::Char('x'));
        assert_eq!(app.screen, Screen::Dashboard);
        assert!(!app.quit);
    }

    #[test]
    fn normal_layouts_show_truthful_content_and_navigation() {
        for (width, height) in [(120, 32), (80, 24), (64, 22)] {
            for (screen, expected) in [
                (Screen::Dashboard, "NOT IMPLEMENTED"),
                (Screen::Help, "Shift+Tab"),
                (Screen::About, "Maxon Torres Software"),
            ] {
                let app = App {
                    screen,
                    ..App::default()
                };
                let text = rendered(&app, width, height);
                assert!(text.contains(expected), "{width}x{height}: {text}");
                assert!(text.contains(&format!("> {}", screen.label())));
                assert!(text.contains("q/Esc/Ctrl+C Exit"));
                assert!(!text.contains("vault unlocked"));
                if screen == Screen::Dashboard {
                    for label in info::UNAVAILABLE {
                        assert!(text.contains(&format!("{label}: NOT IMPLEMENTED")));
                    }
                }
            }
        }
    }

    #[test]
    fn tiny_layouts_are_safe_and_offer_resize_guidance() {
        for (width, height) in [(40, 10), (20, 5), (1, 1), (0, 0)] {
            let text = rendered(&App::default(), width, height);
            if width >= 20 && height >= 5 {
                assert!(text.contains("Resize to 64x22"));
                assert!(text.contains("q/Esc Exit"));
            }
        }
    }
}
