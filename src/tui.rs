use std::io::{self, Stdout};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Gauge, List, ListItem, Paragraph, Row, Table, Wrap,
};
use ratatui::Terminal;

use crate::voter::VoteStatus;

// ── Shared state between async voter logic and TUI render loop ──────────────

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct FrEntry {
    pub id: String,
    pub url: String,
    pub status: FrUiStatus,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FrUiStatus {
    Pending,
    InProgress,
    Voted,
    AlreadyVoted,
    DryRunOk,
    Failed,
}

impl FrUiStatus {
    pub fn color(&self) -> Color {
        match self {
            FrUiStatus::Pending => Color::DarkGray,
            FrUiStatus::InProgress => Color::Yellow,
            FrUiStatus::Voted => Color::Green,
            FrUiStatus::AlreadyVoted => Color::Cyan,
            FrUiStatus::DryRunOk => Color::Blue,
            FrUiStatus::Failed => Color::Red,
        }
    }

    pub fn icon(&self) -> &str {
        match self {
            FrUiStatus::Pending => "  ",
            FrUiStatus::InProgress => ">>",
            FrUiStatus::Voted => "OK",
            FrUiStatus::AlreadyVoted => "--",
            FrUiStatus::DryRunOk => "~~",
            FrUiStatus::Failed => "!!",
        }
    }

    pub fn label(&self) -> &str {
        match self {
            FrUiStatus::Pending => "PENDING",
            FrUiStatus::InProgress => "WORKING",
            FrUiStatus::Voted => "VOTED",
            FrUiStatus::AlreadyVoted => "ALREADY",
            FrUiStatus::DryRunOk => "DRY-OK",
            FrUiStatus::Failed => "FAILED",
        }
    }
}

impl From<&VoteStatus> for FrUiStatus {
    fn from(s: &VoteStatus) -> Self {
        match s {
            VoteStatus::Voted => FrUiStatus::Voted,
            VoteStatus::AlreadyVoted => FrUiStatus::AlreadyVoted,
            VoteStatus::DryRunOk => FrUiStatus::DryRunOk,
            VoteStatus::ProxyAdded => FrUiStatus::Voted,
            VoteStatus::Failed(_) => FrUiStatus::Failed,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppPhase {
    Disclaimer,
    Auth,
    Voting,
    Done,
}

pub struct AppState {
    pub phase: AppPhase,
    pub frs: Vec<FrEntry>,
    pub logs: Vec<String>,
    pub modal: Option<String>,
    pub dry_run: bool,
    pub mode_label: String,
    pub company: Option<String>,
    pub start_time: Instant,
    pub should_quit: bool,
    /// Signals the voter coroutine that the human confirmed a modal
    pub modal_confirmed: bool,
}

impl AppState {
    pub fn new(fr_ids: &[String], dry_run: bool, mode_label: &str, company: Option<String>) -> Self {
        let frs = fr_ids
            .iter()
            .map(|id| FrEntry {
                id: id.clone(),
                url: crate::config::Config::fr_url(id),
                status: FrUiStatus::Pending,
                message: String::new(),
            })
            .collect();
        AppState {
            phase: AppPhase::Disclaimer,
            frs,
            logs: vec!["VCF Voting Assistant initialized.".into()],
            modal: None,
            dry_run,
            mode_label: mode_label.to_string(),
            company,
            start_time: Instant::now(),
            should_quit: false,
            modal_confirmed: false,
        }
    }

    pub fn log(&mut self, msg: impl Into<String>) {
        let elapsed = self.start_time.elapsed().as_secs();
        let m = elapsed / 60;
        let s = elapsed % 60;
        self.logs.push(format!("[{:02}:{:02}] {}", m, s, msg.into()));
        // Keep log buffer bounded
        if self.logs.len() > 200 {
            self.logs.drain(..50);
        }
    }

    pub fn progress(&self) -> (usize, usize) {
        let done = self
            .frs
            .iter()
            .filter(|f| !matches!(f.status, FrUiStatus::Pending | FrUiStatus::InProgress))
            .count();
        (done, self.frs.len())
    }
}

pub type SharedState = Arc<Mutex<AppState>>;

// ── Terminal setup / teardown ────────────────────────────────────────────────

pub fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

pub fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) {
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
}

// ── Main TUI event loop (runs on main thread, driven by tokio) ──────────────

pub async fn run_tui_loop(state: SharedState) -> io::Result<()> {
    let mut terminal = setup_terminal()?;

    loop {
        // Draw
        {
            let st = state.lock().unwrap();
            terminal.draw(|f| draw_ui(f, &st))?;
            if st.should_quit {
                break;
            }
        }

        // Poll for keyboard events (non-blocking, 50ms tick)
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                let mut st = state.lock().unwrap();

                // Global quit: Ctrl+C or q (only when no modal is shown and not in voting)
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
                {
                    st.should_quit = true;
                    break;
                }

                match st.phase {
                    AppPhase::Disclaimer => {
                        if key.code == KeyCode::Enter {
                            st.phase = AppPhase::Auth;
                            st.log("Disclaimer accepted. Launching browser...");
                            st.modal_confirmed = true;
                        }
                        if key.code == KeyCode::Char('q') {
                            st.should_quit = true;
                        }
                    }
                    AppPhase::Auth | AppPhase::Voting => {
                        if st.modal.is_some() && key.code == KeyCode::Enter {
                            st.modal = None;
                            st.modal_confirmed = true;
                        }
                    }
                    AppPhase::Done => {
                        if key.code == KeyCode::Char('q') || key.code == KeyCode::Enter {
                            st.should_quit = true;
                        }
                    }
                }
            }
        }
    }

    restore_terminal(&mut terminal);
    Ok(())
}

// ── Drawing ─────────────────────────────────────────────────────────────────

fn draw_ui(f: &mut ratatui::Frame, state: &AppState) {
    let size = f.area();

    match state.phase {
        AppPhase::Disclaimer => draw_disclaimer(f, size, state),
        AppPhase::Auth => draw_main_layout(f, size, state),
        AppPhase::Voting => draw_main_layout(f, size, state),
        AppPhase::Done => draw_main_layout(f, size, state),
    }

    // Modal overlay
    if let Some(ref msg) = state.modal {
        draw_modal(f, size, msg);
    }
}

fn draw_disclaimer(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let tick = state.start_time.elapsed().as_millis() / 300;
    let border_color = match tick % 4 {
        0 => Color::Cyan,
        1 => Color::Blue,
        2 => Color::Magenta,
        _ => Color::Cyan,
    };

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "   VCF  VOTING  ASSISTANT",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "   Human-in-the-Loop Browser Automation",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "   for Broadcom Aha! Ideas Portal",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "   By proceeding you confirm:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "    1. You have permission to automate navigation",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "    2. You have read the portal Terms of Use",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "    3. You will perform login/2FA/CAPTCHA manually",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "    4. This tool does NOT store credentials",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "   Configuration:  ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("Mode={}", state.mode_label),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw("  "),
            Span::styled(
                if state.dry_run {
                    "DRY RUN"
                } else {
                    "LIVE"
                },
                Style::default().fg(if state.dry_run {
                    Color::Blue
                } else {
                    Color::Red
                }).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("FRs={}", state.frs.len()),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "          [ ENTER ] Accept & Continue       [ q ] Quit",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            " vcf-vote v0.1.0 ",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}

fn draw_main_layout(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    // Top bar (1) | FR list + logs (middle) | progress bar (1) | status bar (1)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header
            Constraint::Min(8),    // body
            Constraint::Length(3), // progress
            Constraint::Length(1), // status bar
        ])
        .split(area);

    // ── Header ──
    draw_header(f, outer[0], state);

    // ── Body: FR table (left) + Logs (right) ──
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(outer[1]);

    draw_fr_table(f, body[0], state);
    draw_logs(f, body[1], state);

    // ── Progress bar ──
    draw_progress(f, outer[2], state);

    // ── Status bar ──
    draw_status_bar(f, outer[3], state);
}

fn draw_header(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let phase_label = match state.phase {
        AppPhase::Disclaimer => "DISCLAIMER",
        AppPhase::Auth => "AUTHENTICATION",
        AppPhase::Voting => "VOTING",
        AppPhase::Done => "COMPLETE",
    };

    let phase_color = match state.phase {
        AppPhase::Disclaimer => Color::Yellow,
        AppPhase::Auth => Color::Magenta,
        AppPhase::Voting => Color::Cyan,
        AppPhase::Done => Color::Green,
    };

    let elapsed = state.start_time.elapsed().as_secs();
    let m = elapsed / 60;
    let s = elapsed % 60;

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " VCF VOTING ASSISTANT ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!(" {} ", phase_label),
            Style::default()
                .fg(Color::Black)
                .bg(phase_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            if state.dry_run { " DRY RUN " } else { " LIVE " },
            Style::default()
                .fg(Color::Black)
                .bg(if state.dry_run { Color::Blue } else { Color::Red })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        if let Some(ref co) = state.company {
            Span::styled(
                format!(" {} ", co),
                Style::default().fg(Color::Black).bg(Color::Yellow),
            )
        } else {
            Span::raw("")
        },
        Span::styled(
            format!("  {:02}:{:02} ", m, s),
            Style::default().fg(Color::DarkGray),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(header, area);
}

fn draw_fr_table(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let header = Row::new(vec!["", "FR ID", "STATUS", "MESSAGE"])
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .bottom_margin(1);

    let rows: Vec<Row> = state
        .frs
        .iter()
        .map(|fr| {
            let icon = fr.status.icon();
            let label = fr.status.label();
            let color = fr.status.color();
            let msg = if fr.message.len() > 35 {
                format!("{}...", &fr.message[..32])
            } else {
                fr.message.clone()
            };

            Row::new(vec![
                icon.to_string(),
                fr.id.clone(),
                label.to_string(),
                msg,
            ])
            .style(Style::default().fg(color))
        })
        .collect();

    let widths = [
        Constraint::Length(2),
        Constraint::Length(14),
        Constraint::Length(8),
        Constraint::Fill(1),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(Span::styled(
                    " Feature Requests ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

    f.render_widget(table, area);
}

fn draw_logs(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    // Show last N log lines that fit
    let inner_height = area.height.saturating_sub(2) as usize;
    let start = if state.logs.len() > inner_height {
        state.logs.len() - inner_height
    } else {
        0
    };

    let items: Vec<ListItem> = state.logs[start..]
        .iter()
        .map(|line| {
            let color = if line.contains("ERROR") || line.contains("FAIL") {
                Color::Red
            } else if line.contains("WARN") {
                Color::Yellow
            } else if line.contains("OK") || line.contains("voted") || line.contains("passed") {
                Color::Green
            } else {
                Color::DarkGray
            };
            ListItem::new(Line::from(Span::styled(line.as_str(), Style::default().fg(color))))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(Span::styled(
                " Activity Log ",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(list, area);
}

fn draw_progress(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let (done, total) = state.progress();
    let ratio = if total > 0 {
        done as f64 / total as f64
    } else {
        0.0
    };

    let label = format!("{}/{} feature requests processed", done, total);

    let color = if done == total && total > 0 {
        Color::Green
    } else {
        Color::Cyan
    };

    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Span::styled(
                    " Progress ",
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )),
        )
        .gauge_style(Style::default().fg(color).bg(Color::Black))
        .ratio(ratio)
        .label(Span::styled(label, Style::default().fg(Color::White)));

    f.render_widget(gauge, area);
}

fn draw_status_bar(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let help = match state.phase {
        AppPhase::Done => " [ENTER/q] Quit  |  Report saved ",
        _ => " [Ctrl+C] Quit  |  [ENTER] Confirm action ",
    };

    let bar = Paragraph::new(Line::from(vec![
        Span::styled(
            help,
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    f.render_widget(bar, area);
}

fn draw_modal(f: &mut ratatui::Frame, area: Rect, message: &str) {
    // Center a modal box
    let w = (area.width * 70 / 100).max(40).min(area.width.saturating_sub(4));
    let lines: Vec<&str> = message.lines().collect();
    let h = (lines.len() as u16 + 6).min(area.height.saturating_sub(4));

    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let modal_area = Rect::new(x, y, w, h);

    // Clear background
    f.render_widget(Clear, modal_area);

    let mut text_lines: Vec<Line> = Vec::new();
    text_lines.push(Line::from(""));
    for l in &lines {
        text_lines.push(Line::from(Span::styled(
            format!("  {}", l),
            Style::default().fg(Color::White),
        )));
    }
    text_lines.push(Line::from(""));
    text_lines.push(Line::from(""));
    text_lines.push(Line::from(Span::styled(
        "          [ ENTER ] Continue",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));

    let block = Block::default()
        .title(Span::styled(
            " ACTION REQUIRED ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Yellow));

    let paragraph = Paragraph::new(text_lines).block(block).wrap(Wrap { trim: false });
    f.render_widget(paragraph, modal_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voter::VoteStatus;

    #[test]
    fn test_fr_ui_status_icon() {
        assert_eq!(FrUiStatus::Pending.icon(), "  ");
        assert_eq!(FrUiStatus::InProgress.icon(), ">>");
        assert_eq!(FrUiStatus::Voted.icon(), "OK");
        assert_eq!(FrUiStatus::AlreadyVoted.icon(), "--");
        assert_eq!(FrUiStatus::DryRunOk.icon(), "~~");
        assert_eq!(FrUiStatus::Failed.icon(), "!!");
    }

    #[test]
    fn test_fr_ui_status_label() {
        assert_eq!(FrUiStatus::Pending.label(), "PENDING");
        assert_eq!(FrUiStatus::InProgress.label(), "WORKING");
        assert_eq!(FrUiStatus::Voted.label(), "VOTED");
        assert_eq!(FrUiStatus::AlreadyVoted.label(), "ALREADY");
        assert_eq!(FrUiStatus::DryRunOk.label(), "DRY-OK");
        assert_eq!(FrUiStatus::Failed.label(), "FAILED");
    }

    #[test]
    fn test_vote_status_to_fr_ui_status() {
        assert_eq!(FrUiStatus::from(&VoteStatus::Voted), FrUiStatus::Voted);
        assert_eq!(FrUiStatus::from(&VoteStatus::AlreadyVoted), FrUiStatus::AlreadyVoted);
        assert_eq!(FrUiStatus::from(&VoteStatus::DryRunOk), FrUiStatus::DryRunOk);
        assert_eq!(FrUiStatus::from(&VoteStatus::ProxyAdded), FrUiStatus::Voted);
        assert_eq!(FrUiStatus::from(&VoteStatus::Failed("err".into())), FrUiStatus::Failed);
    }

    #[test]
    fn test_app_state_new() {
        let fr_ids = vec!["VCF-I-1".into(), "VCF-I-2".into()];
        let state = AppState::new(&fr_ids, true, "vote_only", None);

        assert_eq!(state.frs.len(), 2);
        assert_eq!(state.phase, AppPhase::Disclaimer);
        assert_eq!(state.dry_run, true);
        assert_eq!(state.modal, None);
        assert_eq!(state.should_quit, false);
        assert_eq!(state.frs[0].id, "VCF-I-1");
        assert_eq!(state.frs[0].status, FrUiStatus::Pending);
    }

    #[test]
    fn test_app_state_log() {
        let fr_ids = vec!["VCF-I-1".into()];
        let mut state = AppState::new(&fr_ids, false, "vote_only", None);

        state.log("test message");
        assert_eq!(state.logs.len(), 2); // initial + new
        assert!(state.logs.last().unwrap().contains("test message"));
    }

    #[test]
    fn test_app_state_progress() {
        let fr_ids = vec!["VCF-I-1".into(), "VCF-I-2".into(), "VCF-I-3".into()];
        let mut state = AppState::new(&fr_ids, false, "vote_only", None);

        assert_eq!(state.progress(), (0, 3));

        state.frs[0].status = FrUiStatus::Voted;
        assert_eq!(state.progress(), (1, 3));

        state.frs[1].status = FrUiStatus::Failed;
        assert_eq!(state.progress(), (2, 3));
    }

    #[test]
    fn test_app_state_log_buffer_bounded() {
        let fr_ids = vec!["VCF-I-1".into()];
        let mut state = AppState::new(&fr_ids, false, "vote_only", None);

        for i in 0..250 {
            state.log(format!("log entry {}", i));
        }
        assert!(state.logs.len() < 210);
    }
}
