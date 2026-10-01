use fieldide_core::{AppState, SafetyMode};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub fn render(frame: &mut Frame, state: &AppState, content: &str) {
    let area = frame.area();
    if area.width < 80 || area.height < 24 {
        frame.render_widget(
            Paragraph::new("FieldIDE에는 최소 80×24 터미널이 필요합니다.").block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("화면이 너무 작음"),
            ),
            area,
        );
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    render_header(frame, rows[0], state);
    frame.render_widget(
        Paragraph::new(content).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Editor / 편집기"),
        ),
        rows[1],
    );
    let notice = state
        .notice
        .as_deref()
        .unwrap_or("q 종료  ·  Observe 모드에서는 제어 명령이 실행되지 않습니다");
    frame.render_widget(
        Paragraph::new(notice).block(Block::default().borders(Borders::ALL).title("Status")),
        rows[2],
    );
}

fn render_header(frame: &mut Frame, area: Rect, state: &AppState) {
    let (mode, color) = match state.mode {
        SafetyMode::Observe => ("OBSERVE", Color::Green),
        SafetyMode::Armed => ("ARMED", Color::Yellow),
        SafetyMode::Control => ("CONTROL", Color::Red),
    };
    let title = Line::from(vec![
        " FieldIDE  ".into(),
        ratatui::text::Span::styled(
            mode,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        format!("  {:?} ", state.panel).into(),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn screen(width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| render(frame, &AppState::default(), "한글과 emoji 🚀\r\n둘째 줄"))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_single_panel_at_80_by_24() {
        let output = screen(80, 24);
        let compact = output.replace(' ', "");
        assert!(output.contains("FieldIDE"));
        assert!(output.contains("OBSERVE"));
        assert!(compact.contains("한글과emoji"));
        assert!(compact.contains("Editor/편집기"));
    }

    #[test]
    fn warns_below_minimum_size() {
        assert!(screen(79, 23).replace(' ', "").contains("화면이너무작음"));
    }
}
