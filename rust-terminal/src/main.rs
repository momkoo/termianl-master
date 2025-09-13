mod terminal;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal as RatatuiTerminal,
};
use std::{collections::HashMap, io};
use terminal::{Shell, Terminal, TerminalBuilder};

/// 메인 애플리케이션 구조체
struct App {
    terminal: Terminal,
    events_rx: futures::channel::mpsc::UnboundedReceiver<alacritty_terminal::event::Event>,
    should_quit: bool,
}

impl App {
    /// 새 애플리케이션 인스턴스 생성 (Zed 방식 사용)
    fn new() -> Result<Self> {
        println!("애플리케이션 초기화 시작");

        // Zed 문서에 따른 터미널 생성
        let working_directory = None; // 현재 디렉토리 사용
        let shell = Shell::System; // 시스템 기본 셸 사용
        let mut env = HashMap::new();

        // 기본 환경 변수들 추가
        for (key, value) in std::env::vars() {
            env.insert(key, value);
        }

        let window_id = 1; // 임의의 윈도우 ID

        println!("TerminalBuilder 생성 중...");
        let builder = TerminalBuilder::new(working_directory, shell, env, window_id)?;
        let (terminal, events_rx) = builder.build();

        println!("애플리케이션 초기화 완료");

        Ok(Self {
            terminal,
            events_rx,
            should_quit: false,
        })
    }

    /// 메인 실행 루프
    fn run<B: ratatui::backend::Backend>(&mut self, ratatui_terminal: &mut RatatuiTerminal<B>) -> Result<()> {
        println!("애플리케이션 실행 시작");

        loop {
            // 화면 그리기
            ratatui_terminal.draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(1)
                    .constraints([Constraint::Percentage(100)].as_ref())
                    .split(f.area());

                // 터미널 내용을 줄별로 가져오기
                let lines = match self.terminal.get_renderable_content() {
                    Ok(content_lines) => {
                        content_lines.into_iter()
                            .map(|line| Line::from(Span::styled(line, Style::default().fg(Color::White))))
                            .collect::<Vec<_>>()
                    },
                    Err(_) => vec![Line::from(Span::raw("터미널 내용 로딩 중..."))]
                };

                // 커서 위치 가져오기
                let (cursor_col, cursor_row) = self.terminal.get_cursor();

                let paragraph = Paragraph::new(lines)
                    .block(Block::default()
                        .title(format!("Zed-style Rust Terminal (커서: {},{}) - Ctrl+Q to quit", cursor_col, cursor_row))
                        .borders(Borders::ALL))
                    .style(Style::default().bg(Color::Black));

                f.render_widget(paragraph, chunks[0]);
            })?;

            // 알라크리티 터미널 이벤트 처리 (단순화)
            // StreamExt 없이 처리하므로 일단 건너뛰기

            // 키보드 이벤트 처리
            if event::poll(std::time::Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                self.should_quit = true;
                                println!("종료 요청 받음");
                            }
                            KeyCode::Char(c) => {
                                // UTF-8 문자를 바이트로 변환하여 터미널로 전송
                                let char_str = c.to_string();
                                let bytes = char_str.as_bytes();
                                if let Err(e) = self.terminal.input(bytes) {
                                    println!("입력 전송 실패: {}", e);
                                } else {
                                    println!("문자 입력: '{}' ({:?})", c, bytes);
                                }
                            }
                            KeyCode::Enter => {
                                // Enter 키 처리
                                if let Err(e) = self.terminal.input(b"\r") {
                                    println!("Enter 입력 전송 실패: {}", e);
                                }
                            }
                            KeyCode::Backspace => {
                                // 백스페이스 처리
                                if let Err(e) = self.terminal.input(b"\x7f") {
                                    println!("백스페이스 입력 전송 실패: {}", e);
                                }
                            }
                            KeyCode::Tab => {
                                // 탭 처리
                                if let Err(e) = self.terminal.input(b"\t") {
                                    println!("탭 입력 전송 실패: {}", e);
                                }
                            }
                            KeyCode::Esc => {
                                // ESC 처리
                                if let Err(e) = self.terminal.input(b"\x1b") {
                                    println!("ESC 입력 전송 실패: {}", e);
                                }
                            }
                            _ => {
                                // 다른 키들은 무시
                            }
                        }
                    }
                }
            }

            if self.should_quit {
                break;
            }
        }

        println!("애플리케이션 실행 종료");
        Ok(())
    }
}

fn main() -> Result<()> {
    println!("=== Zed-style Rust Terminal 시작 ===");

    // 터미널 설정
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut ratatui_terminal = RatatuiTerminal::new(backend)?;

    println!("Ratatui 터미널 설정 완료");

    // 애플리케이션 생성 및 실행
    let app_result = App::new().and_then(|mut app| {
        app.run(&mut ratatui_terminal)
    });

    // 터미널 복원
    disable_raw_mode()?;
    execute!(
        ratatui_terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    ratatui_terminal.show_cursor()?;

    // 결과 처리
    match app_result {
        Ok(_) => {
            println!("애플리케이션이 정상적으로 종료되었습니다.");
        }
        Err(err) => {
            println!("애플리케이션 실행 중 오류 발생: {}", err);
            return Err(err);
        }
    }

    println!("=== Zed-style Rust Terminal 종료 ===");
    Ok(())
}