
mod terminal;
// mod hangul; // 현재 사용하지 않음

use anyhow::Result;
use log::{info, debug, error};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind, MouseButton},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Modifier},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal as RatatuiTerminal,
};
use std::{collections::HashMap, io, sync::Arc, sync::atomic::{AtomicBool, Ordering}};
use terminal::{Shell, Terminal, TerminalBuilder};
// 한글 처리 모듈은 현재 사용하지 않음
// use hangul::HangulComposer;

/// 텍스트 선택 영역
#[derive(Debug, Clone, Default)]
struct TextSelection {
    start_row: u16,
    start_col: u16,
    end_row: u16,
    end_col: u16,
    is_active: bool,
}

/// 커서 모양 정의 (Zed 방식)
#[derive(Debug, Clone, Copy)]
enum CursorShape {
    Block,
    Underline,
    Beam,
    Hollow,
}

/// 커서 상태 정보
#[derive(Debug, Clone)]
struct CursorState {
    position: (u16, u16), // (col, row)
    shape: CursorShape,
    visible: bool,
    blink_state: bool,
    last_blink: std::time::Instant,
    character: char,
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            position: (0, 0),
            shape: CursorShape::Block,
            visible: true,
            blink_state: true,
            last_blink: std::time::Instant::now(),
            character: ' ',
        }
    }
}

/// 메인 애플리케이션 구조체
struct App {
    terminal: Terminal,
    should_quit: bool,
    text_selection: TextSelection,
    is_dragging: bool,
    shutdown_signal: Arc<AtomicBool>,
    cursor_state: CursorState,
    terminal_area: Rect, // 실제 터미널 컨텐츠 영역
    scroll_offset: u16,  // 스크롤 오프셋 (위로 스크롤된 줄 수)
    total_lines: usize,  // 전체 터미널 출력 라인 수
    quit_confirm_count: u8, // Ctrl+Z 종료 확인 카운터
}

impl App {
    /// 새 애플리케이션 인스턴스 생성 (Zed 방식 사용)
    fn new(shutdown_signal: Arc<AtomicBool>) -> Result<Self> {
        // Zed 문서에 따른 터미널 생성
        let working_directory = None; // 현재 디렉토리 사용
        let shell = Shell::System; // 시스템 기본 셸 사용
        let mut env = HashMap::new();

        // 기본 환경 변수들 추가
        for (key, value) in std::env::vars() {
            env.insert(key, value);
        }

        let window_id = 1; // 임의의 윈도우 ID

        let builder = TerminalBuilder::new(working_directory, shell, env, window_id)?;
        let (terminal, _events_rx) = builder.build();

        Ok(Self {
            terminal,
            should_quit: false,
            text_selection: TextSelection::default(),
            is_dragging: false,
            shutdown_signal,
            cursor_state: CursorState::default(),
            terminal_area: Rect::default(),
            scroll_offset: 0,
            total_lines: 0,
            quit_confirm_count: 0,
        })
    }

    /// 메인 실행 루프
    fn run<B: ratatui::backend::Backend>(&mut self, ratatui_terminal: &mut RatatuiTerminal<B>) -> Result<()> {

        loop {
            // 화면 그리기
            ratatui_terminal.draw(|f| {
                // 전체 영역을 터미널과 스크롤바로 분할
                let main_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .margin(1)
                    .constraints([Constraint::Min(10), Constraint::Length(1)].as_ref()) // 터미널 영역 + 스크롤바 1칸
                    .split(f.area());

                // 실제 터미널 컨텐츠 영역 저장 (스크롤바 제외)
                self.terminal_area = main_chunks[0];
                let scrollbar_area = main_chunks[1];

                // 터미널 커서 위치 가져오기 및 상태 업데이트
                let (cursor_col, cursor_row, cursor_char) = self.terminal.get_renderable_cursor();
                self.cursor_state.position = (cursor_col, cursor_row);
                self.cursor_state.character = cursor_char;

                // 터미널 내용을 줄별로 가져오기 (선택 영역 하이라이트 포함)
                let all_lines = match self.terminal.get_renderable_content() {
                    Ok(content_lines) => {
                        // 전체 라인 수 업데이트
                        self.total_lines = content_lines.len();

                        // 전체 라인들을 스크롤 오프셋과 함께 렌더링
                        let all_lines_with_selection = content_lines.into_iter()
                            .enumerate()
                            .map(|(row_idx, line)| self.render_line_with_selection(line, row_idx as u16))
                            .collect::<Vec<_>>();
                        all_lines_with_selection
                    },
                    Err(_) => vec![Line::from(Span::raw("터미널 내용 로딩 중..."))]
                };

                // 스크롤 오프셋을 적용하여 보여줄 라인들만 선택
                let visible_height = self.terminal_area.height.saturating_sub(2) as usize;
                let start_idx = self.scroll_offset as usize;
                let end_idx = (start_idx + visible_height).min(all_lines.len());

                let lines = if start_idx < all_lines.len() {
                    all_lines[start_idx..end_idx].to_vec()
                } else {
                    vec![]
                };

                // 선택 영역 상태 표시 추가
                let selection_info = if self.text_selection.is_active {
                    format!(" [선택: {}]", if self.is_dragging { "진행중" } else { "완료" })
                } else {
                    String::new()
                };

                // 스크롤 위치 정보 (항상 표시)
                let scroll_info = {
                    let scroll_percentage = if self.total_lines > visible_height && self.total_lines > 0 {
                        (self.scroll_offset as f32 / (self.total_lines.saturating_sub(visible_height)) as f32 * 100.0) as u16
                    } else {
                        0
                    };
                    format!(" [라인:{} 표시:{} 오프셋:{} ({}%)]",
                        self.total_lines, visible_height, self.scroll_offset, scroll_percentage)
                };

                // 커서 디버그 정보
                let cursor_debug = format!(" [커서:{}x{} 절대:{} 상대:{}]",
                    cursor_col, cursor_row,
                    if self.total_lines > visible_height { (self.total_lines - visible_height) as u16 + cursor_row } else { cursor_row },
                    if self.total_lines > visible_height {
                        let abs_row = (self.total_lines - visible_height) as u16 + cursor_row;
                        if abs_row >= self.scroll_offset { abs_row - self.scroll_offset } else { 0 }
                    } else { cursor_row }
                );

                // 종료 상태 메시지
                let quit_status = if self.quit_confirm_count > 0 {
                    " [Ctrl+Z로 다시 누르면 종료됩니다]"
                } else {
                    ""
                };

                let paragraph = Paragraph::new(lines)
                    .block(Block::default()
                        .title(format!("Rust Terminal{}{}{}{} - 마우스휠/PageUp/Down: 스크롤, Ctrl+Z: 종료",
                            selection_info, scroll_info, cursor_debug, quit_status))
                        .borders(Borders::ALL))
                        .style(Style::default().bg(Color::Black));

                f.render_widget(paragraph, main_chunks[0]);

                // 스크롤바 렌더링
                self.render_scrollbar(f, scrollbar_area);

                // 실제 터미널 커서 위치로 이동
                self.set_terminal_cursor_position(f);
            })?;

            // 터미널 이벤트 처리 (alacritty events)
            // 향후 터미널 출력 변경사항을 처리할 수 있음

            // 키보드 및 마우스 이벤트 처리
            if event::poll(std::time::Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        self.handle_key_event(key)?;
                    }
                    Event::Mouse(mouse) => {
                        self.handle_mouse_event(mouse)?;
                    }
                    _ => {}
                }
            }

            // 종료 신호 확인
            if self.should_quit || self.shutdown_signal.load(Ordering::Relaxed) {
                break;
            }
        }

        Ok(())
    }

    /// 키 이벤트 처리
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+Z 안전 종료 - 첫 번째 누름 시 경고, 두 번째 누름 시 종료
                if self.quit_confirm_count == 0 {
                    self.quit_confirm_count = 1;
                    debug!("First Ctrl+Z pressed - showing quit confirmation");
                } else {
                    self.should_quit = true;
                    debug!("Second Ctrl+Z pressed - exiting application");
                }
            }
            KeyCode::Char(c) => {
                self.handle_char_input(c)?;
            }
            KeyCode::Enter => {
                let _ = self.terminal.input(b"\r");
            }
            KeyCode::Backspace => {
                let _ = self.terminal.input(b"\x7f");
            }
            KeyCode::Tab => {
                let _ = self.terminal.input(b"\t");
            }
            KeyCode::Esc => {
                let _ = self.terminal.input(b"\x1b");
            }
            KeyCode::Up => {
                let _ = self.terminal.input(b"\x1b[A");
            }
            KeyCode::Down => {
                let _ = self.terminal.input(b"\x1b[B");
            }
            KeyCode::Right => {
                let _ = self.terminal.input(b"\x1b[C");
            }
            KeyCode::Left => {
                let _ = self.terminal.input(b"\x1b[D");
            }
            KeyCode::PageUp => {
                // Page Up - 한 페이지 위로 스크롤
                let page_size = self.terminal_area.height.saturating_sub(2) as u16;
                self.scroll_offset = self.scroll_offset.saturating_sub(page_size);
                debug!("Page up to offset: {}", self.scroll_offset);
            }
            KeyCode::PageDown => {
                // Page Down - 한 페이지 아래로 스크롤
                let page_size = self.terminal_area.height.saturating_sub(2) as u16;
                let visible_lines = self.terminal_area.height.saturating_sub(2) as usize;
                if self.total_lines > visible_lines {
                    let max_scroll = self.total_lines.saturating_sub(visible_lines) as u16;
                    self.scroll_offset = (self.scroll_offset + page_size).min(max_scroll);
                    debug!("Page down to offset: {} / max: {}", self.scroll_offset, max_scroll);
                }
            }
            KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+Home - 맨 위로
                self.scroll_offset = 0;
                debug!("Scrolled to top");
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+End - 맨 아래로
                let visible_lines = self.terminal_area.height.saturating_sub(2) as usize;
                if self.total_lines > visible_lines {
                    let max_scroll = self.total_lines.saturating_sub(visible_lines) as u16;
                    self.scroll_offset = max_scroll;
                    debug!("Scrolled to bottom: offset={}", self.scroll_offset);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// 문자 입력 처리 (한글 조합 포함)
    fn handle_char_input(&mut self, c: char) -> Result<()> {
        debug!("Character input: '{}' (U+{:04X})", c, c as u32);

        // UTF-8 바이트로 인코딩하여 터미널에 전송
        let mut buffer = [0; 4];
        let utf8_str = c.encode_utf8(&mut buffer);

        debug!("Sending UTF-8 bytes: {:?}", utf8_str.as_bytes());
        let _ = self.terminal.input(utf8_str.as_bytes());
        Ok(())
    }

    /// 마우스 이벤트 처리
    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<()> {
        debug!("Mouse event: {:?} [Terminal Area: {}x{} at ({},{})]",
            mouse, self.terminal_area.width, self.terminal_area.height,
            self.terminal_area.x, self.terminal_area.y);

        // 마우스 이벤트를 터미널로 전달 (xterm mouse protocol)
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                debug!("Mouse left click at ({}, {})", mouse.column, mouse.row);

                // 텍스트 선택 시작
                self.start_text_selection(mouse.column, mouse.row)?;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                debug!("Mouse left release at ({}, {}), is_dragging: {}, selection_active: {}",
                    mouse.column, mouse.row, self.is_dragging, self.text_selection.is_active);

                if self.is_dragging {
                    // 드래그 종료 - 텍스트 복사
                    self.finish_text_selection(mouse.column, mouse.row)?;
                    debug!("Final selection state: {:?}", self.text_selection);
                    self.copy_selected_text()?;
                    debug!("Text selection copied to clipboard");
                } else if self.text_selection.is_active {
                    // 단순 클릭으로 선택 완료 - 텍스트 복사
                    self.finish_text_selection(mouse.column, mouse.row)?;
                    debug!("Single click final selection: {:?}", self.text_selection);
                    self.copy_selected_text()?;
                    debug!("Single click selection copied to clipboard");
                } else {
                    // 단순 클릭 - 커서 이동
                    if let Some((terminal_col, terminal_row)) = self.mouse_to_terminal_coords(mouse.column, mouse.row) {
                        // 커서 이동 escape sequence 전송
                        let escape_seq = format!("\x1b[{};{}H", terminal_row + 1, terminal_col + 1);
                        let _ = self.terminal.input(escape_seq.as_bytes());
                        debug!("Cursor moved to ({}, {})", terminal_col, terminal_row);
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                debug!("Mouse right click at ({}, {}) - ignored (no terminal forwarding)", mouse.column, mouse.row);
                // 오른쪽 클릭은 터미널로 전달하지 않음 (이상한 문자 출력 방지)
            }
            MouseEventKind::Up(MouseButton::Right) => {
                debug!("Mouse right release at ({}, {}) - ignored", mouse.column, mouse.row);
                // 오른쪽 클릭 릴리스 무시
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                debug!("Mouse left drag to ({}, {})", mouse.column, mouse.row);

                // 드래그 시작 확인
                if !self.is_dragging {
                    self.is_dragging = true;
                }

                // 텍스트 선택 영역 업데이트
                self.update_text_selection(mouse.column, mouse.row)?;
            }
            MouseEventKind::Moved => {
                // 드래그 중일 때만 처리
                if self.is_dragging && self.text_selection.is_active {
                    debug!("Mouse moved while dragging to ({}, {})", mouse.column, mouse.row);
                    self.update_text_selection(mouse.column, mouse.row)?;
                }
            }
            MouseEventKind::ScrollDown => {
                debug!("Mouse scroll down at ({}, {})", mouse.column, mouse.row);
                let visible_lines = self.terminal_area.height.saturating_sub(2) as usize; // 테두리 제외
                debug!("Scroll check: total_lines={}, visible_lines={}, current_offset={}",
                    self.total_lines, visible_lines, self.scroll_offset);

                if self.total_lines > visible_lines {
                    let max_scroll = self.total_lines.saturating_sub(visible_lines) as u16;
                    if self.scroll_offset < max_scroll {
                        let old_offset = self.scroll_offset;
                        self.scroll_offset = (self.scroll_offset + 3).min(max_scroll); // 3줄씩 스크롤
                        debug!("Scrolled down: {} -> {} (max: {})", old_offset, self.scroll_offset, max_scroll);
                    } else {
                        debug!("Already at max scroll: offset={}, max={}", self.scroll_offset, max_scroll);
                    }
                } else {
                    debug!("No scrolling possible: total_lines={} <= visible_lines={}", self.total_lines, visible_lines);
                }
            }
            MouseEventKind::ScrollUp => {
                debug!("Mouse scroll up at ({}, {})", mouse.column, mouse.row);
                let visible_lines = self.terminal_area.height.saturating_sub(2) as usize;
                debug!("Scroll up check: total_lines={}, visible_lines={}, current_offset={}",
                    self.total_lines, visible_lines, self.scroll_offset);

                if self.scroll_offset > 0 {
                    let old_offset = self.scroll_offset;
                    self.scroll_offset = self.scroll_offset.saturating_sub(3); // 3줄씩 스크롤
                    debug!("Scrolled up: {} -> {}", old_offset, self.scroll_offset);
                } else {
                    debug!("Already at top: offset=0");
                }
            }
            _ => {
                debug!("Other mouse event: {:?}", mouse.kind);
            }
        }
        Ok(())
    }

    /// 마우스 이벤트를 xterm mouse protocol로 터미널에 전달
    fn send_mouse_event(&mut self, button: u8, col: u16, row: u16) -> Result<()> {
        // xterm mouse reporting: ESC[M<button><col+32><row+32>
        let button_char = (button + 32) as char;
        let col_char = (col.saturating_add(32).min(255)) as u8 as char;
        let row_char = (row.saturating_add(32).min(255)) as u8 as char;

        let mouse_sequence = format!("\x1b[M{}{}{}", button_char, col_char, row_char);
        debug!("Sending mouse sequence: {:?}", mouse_sequence.as_bytes());

        let _ = self.terminal.input(mouse_sequence.as_bytes());
        Ok(())
    }

    /// 텍스트 선택 시작 (Zed 방식 좌표 변환 사용)
    fn start_text_selection(&mut self, col: u16, row: u16) -> Result<()> {
        if let Some((terminal_col, terminal_row)) = self.mouse_to_terminal_coords(col, row) {
            debug!("Starting text selection at terminal coords: ({}, {})", terminal_col, terminal_row);
            self.text_selection = TextSelection {
                start_row: terminal_row,
                start_col: terminal_col,
                end_row: terminal_row,
                end_col: terminal_col,
                is_active: true,
            };
            self.is_dragging = false; // 드래그는 실제 드래그 이벤트에서 시작
            debug!("Text selection state: {:?}", self.text_selection);
        } else {
            debug!("Failed to convert mouse coords ({}, {}) to terminal coords", col, row);
        }
        Ok(())
    }

    /// 텍스트 선택 영역 업데이트 (Zed 방식 좌표 변환 사용)
    fn update_text_selection(&mut self, col: u16, row: u16) -> Result<()> {
        if self.text_selection.is_active {
            if let Some((terminal_col, terminal_row)) = self.mouse_to_terminal_coords(col, row) {
                debug!("Updating selection to: ({}, {})", terminal_col, terminal_row);
                self.text_selection.end_row = terminal_row;
                self.text_selection.end_col = terminal_col;
                debug!("Updated text selection state: {:?}", self.text_selection);
            } else {
                debug!("Failed to convert mouse coords ({}, {}) during update", col, row);
            }
        } else {
            debug!("Ignoring selection update - no active selection");
        }
        Ok(())
    }

    /// 텍스트 선택 완료 (Zed 방식 좌표 변환 사용)
    fn finish_text_selection(&mut self, col: u16, row: u16) -> Result<()> {
        if self.is_dragging {
            if let Some((terminal_col, terminal_row)) = self.mouse_to_terminal_coords(col, row) {
                self.text_selection.end_row = terminal_row;
                self.text_selection.end_col = terminal_col;
            }
            self.is_dragging = false;
        }
        Ok(())
    }

    /// 선택된 텍스트를 클립보드에 복사
    fn copy_selected_text(&mut self) -> Result<()> {
        if !self.text_selection.is_active {
            return Ok(());
        }

        // 터미널 내용 가져오기
        let lines = match self.terminal.get_renderable_content() {
            Ok(lines) => lines,
            Err(_) => return Ok(()),
        };

        let mut selected_text = String::new();
        let (start_row, start_col, end_row, end_col) = self.normalize_selection();

        for row in start_row..=end_row {
            if let Some(line) = lines.get(row as usize) {
                let line_chars: Vec<char> = line.chars().collect();

                let start_pos = if row == start_row { start_col as usize } else { 0 };
                let end_pos = if row == end_row {
                    std::cmp::min(end_col as usize + 1, line_chars.len())
                } else {
                    line_chars.len()
                };

                if start_pos < line_chars.len() {
                    let selected_part: String = line_chars[start_pos..end_pos].iter().collect();
                    selected_text.push_str(&selected_part);
                }

                // 줄 바꿈 추가 (마지막 줄 제외)
                if row < end_row {
                    selected_text.push('\n');
                }
            }
        }

        // 클립보드에 복사
        if !selected_text.trim().is_empty() {
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                let _ = clipboard.set_text(selected_text);
            }
        }

        // 선택 해제
        self.text_selection.is_active = false;
        Ok(())
    }

    /// 선택 영역 정규화 (시작점이 끝점보다 뒤에 있을 경우 교환)
    fn normalize_selection(&self) -> (u16, u16, u16, u16) {
        let mut start_row = self.text_selection.start_row;
        let mut start_col = self.text_selection.start_col;
        let mut end_row = self.text_selection.end_row;
        let mut end_col = self.text_selection.end_col;

        // 시작점이 끝점보다 뒤에 있으면 교환
        if start_row > end_row || (start_row == end_row && start_col > end_col) {
            std::mem::swap(&mut start_row, &mut end_row);
            std::mem::swap(&mut start_col, &mut end_col);
        }

        (start_row, start_col, end_row, end_col)
    }

    /// 커서 상태 업데이트 (Zed 방식 - 깜빡임 처리)
    fn update_cursor_state(&mut self) {
        let now = std::time::Instant::now();

        // 500ms마다 깜빡임
        if now.duration_since(self.cursor_state.last_blink).as_millis() > 500 {
            self.cursor_state.blink_state = !self.cursor_state.blink_state;
            self.cursor_state.last_blink = now;
        }
    }

    /// 스크롤바 렌더링
    fn render_scrollbar(&self, f: &mut ratatui::Frame, scrollbar_area: Rect) {
        if scrollbar_area.height < 3 {
            return; // 너무 작으면 스크롤바를 그리지 않음
        }

        let visible_lines = self.terminal_area.height.saturating_sub(2) as usize;

        // 스크롤 가능한 경우에만 스크롤바 표시
        if self.total_lines > visible_lines {
            let scrollbar_height = scrollbar_area.height as usize;
            let max_scroll = self.total_lines.saturating_sub(visible_lines) as f32;

            // 스크롤바 썸(thumb) 크기 계산 - 보이는 영역 비율에 따라
            let thumb_size = ((visible_lines as f32 / self.total_lines as f32) * scrollbar_height as f32).max(1.0) as usize;

            // 스크롤바 썸 위치 계산
            let scroll_ratio = if max_scroll > 0.0 {
                self.scroll_offset as f32 / max_scroll
            } else {
                0.0
            };
            let thumb_position = (scroll_ratio * (scrollbar_height - thumb_size) as f32) as usize;

            // 스크롤바 그리기
            for y in 0..scrollbar_height {
                let is_thumb = y >= thumb_position && y < thumb_position + thumb_size;
                let char = if is_thumb { '█' } else { '│' };
                let style = if is_thumb {
                    Style::default().fg(Color::White).bg(Color::Blue)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                if scrollbar_area.y + (y as u16) < scrollbar_area.y + scrollbar_area.height {
                    let span = Span::styled(char.to_string(), style);
                    let line = Line::from(span);
                    let para = Paragraph::new(line);

                    let cell_area = Rect {
                        x: scrollbar_area.x,
                        y: scrollbar_area.y + y as u16,
                        width: 1,
                        height: 1,
                    };
                    f.render_widget(para, cell_area);
                }
            }
        }
    }

    /// 실제 터미널 커서 위치 설정 (스크롤 오프셋 고려)
    fn set_terminal_cursor_position(&self, f: &mut ratatui::Frame) {
        let (cursor_col, cursor_row) = self.cursor_state.position;

        // 커서가 현재 보이는 영역에 있는지 확인
        if cursor_row >= self.scroll_offset {
            let relative_cursor_row = cursor_row - self.scroll_offset;
            let visible_height = self.terminal_area.height.saturating_sub(2);

            // 커서가 보이는 영역 내에 있으면 표시
            if relative_cursor_row < visible_height {
                let cursor_x = self.terminal_area.x + 1 + cursor_col;
                let cursor_y = self.terminal_area.y + 1 + relative_cursor_row;
                f.set_cursor_position((cursor_x, cursor_y));
                return;
            }
        }

        // 커서가 보이지 않는 영역에 있으면 숨김
        f.set_cursor_position((0, 0));
    }

    /// Zed 방식 커서 렌더링 (사용하지 않음)
    fn _render_cursor(&self, f: &mut ratatui::Frame, terminal_area: Rect) {
        if !self.cursor_state.visible || !self.cursor_state.blink_state {
            return;
        }

        let (cursor_col, cursor_row) = self.cursor_state.position;

        // 터미널 영역 내부 좌표 계산 (테두리 제외)
        let inner_area = Rect {
            x: terminal_area.x + 1,
            y: terminal_area.y + 1,
            width: terminal_area.width.saturating_sub(2),
            height: terminal_area.height.saturating_sub(2),
        };

        // 커서 위치가 터미널 영역을 벗어나지 않는지 확인
        if cursor_row >= inner_area.height || cursor_col >= inner_area.width {
            return;
        }

        // 커서 렌더링 위치 계산
        let cursor_x = inner_area.x + cursor_col;
        let cursor_y = inner_area.y + cursor_row;

        // Zed 방식 커서 모양에 따른 렌더링
        let cursor_area = Rect {
            x: cursor_x,
            y: cursor_y,
            width: 1,
            height: 1,
        };

        match self.cursor_state.shape {
            CursorShape::Block => {
                // 블록 커서 - 문자 반전
                let cursor_char = if self.cursor_state.character == ' ' || self.cursor_state.character == '\0' {
                    ' '
                } else {
                    self.cursor_state.character
                };

                let cursor_span = Span::styled(
                    cursor_char.to_string(),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD)
                );

                let cursor_paragraph = Paragraph::new(Line::from(cursor_span));
                f.render_widget(cursor_paragraph, cursor_area);
            }
            CursorShape::Underline => {
                // 언더라인 커서
                let cursor_span = Span::styled(
                    "_",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                );

                let cursor_paragraph = Paragraph::new(Line::from(cursor_span));
                f.render_widget(cursor_paragraph, cursor_area);
            }
            CursorShape::Beam => {
                // 빔 커서 (세로선)
                let cursor_span = Span::styled(
                    "|",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                );

                let cursor_paragraph = Paragraph::new(Line::from(cursor_span));
                f.render_widget(cursor_paragraph, cursor_area);
            }
            CursorShape::Hollow => {
                // 비어있는 블록 커서
                let cursor_span = Span::styled(
                    "□",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                );

                let cursor_paragraph = Paragraph::new(Line::from(cursor_span));
                f.render_widget(cursor_paragraph, cursor_area);
            }
        }
    }

    /// 선택 영역이 있는 줄을 하이라이트하여 렌더링
    fn render_line_with_selection(&self, line: String, row_idx: u16) -> Line<'_> {
        if !self.text_selection.is_active {
            return Line::from(Span::styled(line, Style::default().fg(Color::White)));
        }

        let (start_row, start_col, end_row, end_col) = self.normalize_selection();

        // 디버그용 로깅 (첫 번째와 마지막 줄만 로그)
        if row_idx == 0 || (row_idx < 5 && self.text_selection.is_active) {
            debug!("Rendering line {} with selection: start=({},{}), end=({},{}), active={}",
                row_idx, start_row, start_col, end_row, end_col, self.text_selection.is_active);
        }

        // 현재 줄이 선택 영역에 포함되는지 확인
        if row_idx < start_row || row_idx > end_row {
            return Line::from(Span::styled(line, Style::default().fg(Color::White)));
        }

        let line_chars: Vec<char> = line.chars().collect();
        let mut spans = Vec::new();

        for (col_idx, &ch) in line_chars.iter().enumerate() {
            let is_selected = if row_idx == start_row && row_idx == end_row {
                // 단일 줄 선택
                col_idx >= start_col as usize && col_idx <= end_col as usize
            } else if row_idx == start_row {
                // 시작 줄
                col_idx >= start_col as usize
            } else if row_idx == end_row {
                // 끝 줄
                col_idx <= end_col as usize
            } else {
                // 중간 줄 (전체 선택)
                true
            };

            let style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::White) // 선택된 텍스트는 반전
            } else {
                Style::default().fg(Color::White)
            };

            spans.push(Span::styled(ch.to_string(), style));
        }

        Line::from(spans)
    }

    /// 마우스 좌표를 터미널 좌표로 변환 (Zed 방식)
    fn mouse_to_terminal_coords(&self, mouse_col: u16, mouse_row: u16) -> Option<(u16, u16)> {
        // 터미널 영역의 경계 계산 (테두리 포함)
        let area_left = self.terminal_area.x;
        let area_top = self.terminal_area.y;
        let area_right = self.terminal_area.x + self.terminal_area.width;
        let area_bottom = self.terminal_area.y + self.terminal_area.height;

        // 마우스 좌표가 터미널 영역 내부인지 확인
        if mouse_col <= area_left || mouse_col >= area_right - 1 ||
           mouse_row <= area_top || mouse_row >= area_bottom - 1 {
            debug!("Mouse outside terminal area: ({}, {}) vs area bounds: ({},{}) to ({},{})",
                   mouse_col, mouse_row, area_left, area_top, area_right, area_bottom);
            return None;
        }

        // 터미널 영역 상대 좌표로 변환 (테두리 제외)
        let terminal_col = mouse_col.saturating_sub(area_left + 1);
        let relative_terminal_row = mouse_row.saturating_sub(area_top + 1);

        // 스크롤 오프셋을 고려하여 전체 버퍼에서의 절대 위치 계산
        let terminal_row = relative_terminal_row + self.scroll_offset;

        // 터미널 영역 내부 크기 확인
        let inner_width = self.terminal_area.width.saturating_sub(2);
        let inner_height = self.terminal_area.height.saturating_sub(2);

        if terminal_col < inner_width && relative_terminal_row < inner_height {
            debug!("Converted mouse coords: ({}, {}) -> terminal ({}, {}) [relative: {}]",
                   mouse_col, mouse_row, terminal_col, terminal_row, relative_terminal_row);
            Some((terminal_col, terminal_row))
        } else {
            debug!("Mouse outside inner area: ({}, {}) vs inner size: ({}x{})",
                   terminal_col, relative_terminal_row, inner_width, inner_height);
            None
        }
    }

    /// 정상 종료 처리
    fn cleanup(&mut self) -> Result<()> {
        // 터미널은 자동으로 정리됩니다 (Drop trait 구현)
        // 현재 alacritty_terminal은 kill 메서드가 없으므로
        // 자동 정리에 맡깁니다
        Ok(())
    }
}

/// 신호 핸들러 설정
fn setup_signal_handlers() -> Result<Arc<AtomicBool>> {
    let shutdown_signal = Arc::new(AtomicBool::new(false));

    #[cfg(unix)]
    {
        use signal_hook::{consts::SIGINT, iterator::Signals};
        let signals = Signals::new(&[SIGINT])?;
        let shutdown_clone = shutdown_signal.clone();

        std::thread::spawn(move || {
            for _ in signals.forever() {
                shutdown_clone.store(true, Ordering::Relaxed);
                break;
            }
        });
    }

    #[cfg(windows)]
    {
        use ctrlc;
        let shutdown_clone = shutdown_signal.clone();

        ctrlc::set_handler(move || {
            shutdown_clone.store(true, Ordering::Relaxed);
        })?;
    }

    Ok(shutdown_signal)
}

fn main() -> Result<()> {
    // 로깅 초기화 - 로그를 파일에 저장
    std::env::set_var("RUST_LOG", "debug");
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Pipe(Box::new(std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("terminal_app.log")?)))
        .init();

    info!("터미널 앱 시작");

    // 신호 핸들러 설정
    let shutdown_signal = setup_signal_handlers()?;

    // 터미널 설정
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, SetTitle("Rust Terminal App"), EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut ratatui_terminal = RatatuiTerminal::new(backend)?;

    // 애플리케이션 생성 및 실행
    let app_result = match App::new(shutdown_signal.clone()) {
        Ok(mut app) => {
            info!("앱 실행 시작");
            let result = app.run(&mut ratatui_terminal);
            info!("앱 실행 완료");
            app.cleanup().ok(); // 정리 작업 수행
            result
        }
        Err(e) => {
            error!("앱 생성 실패: {:?}", e);
            Err(e)
        },
    };

    // 터미널 복원
    let restore_result = (|| -> Result<()> {
        disable_raw_mode()?;
        execute!(
            ratatui_terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        ratatui_terminal.show_cursor()?;
        Ok(())
    })();

    // 복원 오류가 있어도 앱 결과를 우선 반환
    if let Err(restore_err) = restore_result {
        error!("터미널 복원 중 오류: {:?}", restore_err);
    }

    // 결과 처리
    app_result
}