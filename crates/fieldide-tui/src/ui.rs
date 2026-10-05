use fieldide_core::{AppState, SafetyMode};
use fieldide_editor::TextBuffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn render(frame: &mut Frame, state: &AppState, content: &str) {
    let buffer = TextBuffer::from_text(content);
    render_editor(frame, state, &buffer, "Editor / 편집기", None);
}

pub fn render_editor(
    frame: &mut Frame,
    state: &AppState,
    buffer: &TextBuffer,
    title: &str,
    status_override: Option<&str>,
) {
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
    let body_height = rows[1].height.saturating_sub(2) as usize;
    let scroll = buffer
        .cursor()
        .line
        .saturating_sub(body_height.saturating_sub(1));
    let cursor_column = cursor_column(buffer);
    let body_width = rows[1].width.saturating_sub(2) as usize;
    let horizontal_scroll = cursor_column.saturating_sub(body_width.saturating_sub(1));
    frame.render_widget(
        Paragraph::new(styled_lines(buffer))
            .scroll((
                u16::try_from(scroll).unwrap_or(u16::MAX),
                u16::try_from(horizontal_scroll).unwrap_or(u16::MAX),
            ))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title.to_owned()),
            ),
        rows[1],
    );

    let default_status = if buffer.is_dirty() {
        "수정됨 · Ctrl+S 저장 · Ctrl+Q/C 종료 · Ctrl+F 검색 · Tab 문서 전환"
    } else {
        "Ctrl+S 저장 · Ctrl+Q/C 종료 · Ctrl+F 검색 · Tab 문서 전환"
    };
    let notice = status_override
        .or(state.notice.as_deref())
        .unwrap_or(default_status);
    frame.render_widget(
        Paragraph::new(notice).block(Block::default().borders(Borders::ALL).title("Status")),
        rows[2],
    );

    render_cursor(frame, rows[1], buffer, scroll, horizontal_scroll);
}

pub fn render_dashboard(frame: &mut Frame, state: &AppState, status_override: Option<&str>) {
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

    let mut lines = vec![Line::styled(
        format!("Nodes ({})", state.ros.nodes.len()),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    if state.ros.nodes.is_empty() {
        lines.push(Line::from("  아직 수집된 node가 없습니다"));
    } else {
        lines.extend(
            state
                .ros
                .nodes
                .iter()
                .map(|node| Line::from(format!("  {}", node.name))),
        );
    }
    lines.push(Line::default());
    lines.push(Line::styled(
        format!("Topics ({})", state.ros.topics.len()),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    if state.ros.topics.is_empty() {
        lines.push(Line::from("  아직 수집된 topic이 없습니다"));
    } else {
        lines.extend(state.ros.topics.iter().map(|topic| {
            let types = if topic.types.is_empty() {
                "Unknown".to_owned()
            } else {
                topic.types.join(", ")
            };
            Line::from(format!(
                "  {}  pub:{} sub:{}  {}",
                topic.name, topic.publishers, topic.subscribers, types
            ))
        }));
    }
    if !state.ros.warnings.is_empty() {
        lines.push(Line::default());
        lines.push(Line::styled(
            format!("Warnings ({})", state.ros.warnings.len()),
            Style::default().fg(Color::Yellow),
        ));
        lines.extend(
            state
                .ros
                .warnings
                .iter()
                .take(3)
                .map(|warning| Line::from(format!("  {warning}"))),
        );
    }
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title("ROS Graph / Dashboard"),
        ),
        rows[1],
    );
    let default_status = if state.ros_refreshing {
        "갱신 중… · Esc 취소/편집기 · F1 편집기 · Ctrl+Q/C 종료"
    } else {
        "r/F5 갱신 · Esc/F1 편집기 · Ctrl+Q/C 종료"
    };
    let notice = status_override
        .or(state.notice.as_deref())
        .unwrap_or(default_status);
    frame.render_widget(
        Paragraph::new(notice).block(Block::default().borders(Borders::ALL).title("Status")),
        rows[2],
    );
}

fn styled_lines(buffer: &TextBuffer) -> Vec<Line<'static>> {
    let text = buffer.text();
    let selection = buffer.selection();
    let mut char_offset = 0;
    let mut lines = Vec::new();
    for raw_line in text.split_inclusive('\n') {
        let visible = raw_line.trim_end_matches(['\r', '\n']);
        let mut spans = Vec::new();
        for grapheme in visible.graphemes(true) {
            let start = char_offset;
            let end = start + grapheme.chars().count();
            let selected = selection.is_some_and(|range| start < range.end && end > range.start);
            let style = if selected {
                Style::default().bg(Color::Blue).fg(Color::White)
            } else {
                Style::default()
            };
            spans.push(Span::styled(grapheme.to_owned(), style));
            char_offset = end;
        }
        char_offset += raw_line[visible.len()..].chars().count();
        lines.push(Line::from(spans));
    }
    if text.is_empty() || text.ends_with('\n') {
        lines.push(Line::default());
    }
    lines
}

fn cursor_column(buffer: &TextBuffer) -> usize {
    let cursor = buffer.cursor();
    buffer
        .text()
        .lines()
        .nth(cursor.line)
        .unwrap_or_default()
        .graphemes(true)
        .take(cursor.grapheme)
        .map(UnicodeWidthStr::width)
        .sum()
}

fn render_cursor(
    frame: &mut Frame,
    area: Rect,
    buffer: &TextBuffer,
    scroll: usize,
    horizontal_scroll: usize,
) {
    let cursor = buffer.cursor();
    let relative_line = cursor.line.saturating_sub(scroll);
    if relative_line >= area.height.saturating_sub(2) as usize {
        return;
    }
    let column =
        u16::try_from(cursor_column(buffer).saturating_sub(horizontal_scroll)).unwrap_or(u16::MAX);
    let x = area.x.saturating_add(1).saturating_add(column);
    let y = area
        .y
        .saturating_add(1)
        .saturating_add(u16::try_from(relative_line).unwrap_or(u16::MAX));
    if x < area.right().saturating_sub(1) && y < area.bottom().saturating_sub(1) {
        frame.set_cursor_position((x, y));
    }
}

fn render_header(frame: &mut Frame, area: Rect, state: &AppState) {
    let (mode, color) = match state.mode {
        SafetyMode::Observe => ("OBSERVE", Color::Green),
        SafetyMode::Armed => ("ARMED", Color::Yellow),
        SafetyMode::Control => ("CONTROL", Color::Red),
    };
    let dirty = if state.dirty { " ●" } else { "" };
    let title = Line::from(vec![
        " FieldIDE  ".into(),
        Span::styled(
            mode,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        format!("  {:?}{dirty} ", state.panel).into(),
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

    #[test]
    fn selected_text_uses_highlight_style() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut buffer = TextBuffer::from_text("한글 test");
        buffer.move_right_with_selection(true);
        terminal
            .draw(|frame| {
                render_editor(frame, &AppState::default(), &buffer, "Editor", None);
            })
            .expect("draw");
        assert_eq!(terminal.backend().buffer()[(1, 4)].bg, Color::Blue);
    }

    #[test]
    fn dashboard_renders_nodes_topics_and_counts() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut state = AppState {
            panel: fieldide_core::Panel::Dashboard,
            ..AppState::default()
        };
        state.ros.nodes.push(fieldide_core::RosNode {
            name: "/motor".to_owned(),
        });
        state.ros.topics.push(fieldide_core::RosTopic {
            name: "/cmd_vel".to_owned(),
            types: vec!["geometry_msgs/msg/Twist".to_owned()],
            publishers: 1,
            subscribers: 2,
        });
        terminal
            .draw(|frame| render_dashboard(frame, &state, None))
            .expect("draw");
        let output = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .replace(' ', "");
        assert!(output.contains("/motor"));
        assert!(output.contains("/cmd_velpub:1sub:2"));
    }
}
