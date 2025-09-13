use alacritty_terminal::{
    Term,
    event::{Event as AlacTermEvent, EventListener},
    event_loop::{EventLoop, Notifier},
    grid::Dimensions,
    term::Config,
    tty::{self, Options as PtyOptions, Shell as AlacShell},
};
use anyhow::{Result, bail};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
};
use alacritty_terminal::sync::FairMutex;

/// 터미널 크기와 경계 정보
#[derive(Clone, Debug)]
pub struct TerminalBounds {
    pub num_lines: usize,
    pub num_cols: usize,
}

impl Default for TerminalBounds {
    fn default() -> Self {
        Self {
            num_lines: 24,
            num_cols: 80,
        }
    }
}

impl Dimensions for TerminalBounds {
    fn total_lines(&self) -> usize {
        self.num_lines
    }

    fn screen_lines(&self) -> usize {
        self.num_lines
    }

    fn columns(&self) -> usize {
        self.num_cols
    }
}

impl From<TerminalBounds> for alacritty_terminal::event::WindowSize {
    fn from(bounds: TerminalBounds) -> Self {
        Self {
            num_lines: bounds.num_lines as u16,
            num_cols: bounds.num_cols as u16,
            cell_width: 8,  // 기본값
            cell_height: 16, // 기본값
        }
    }
}

/// Zed의 ZedListener와 동일한 역할
pub struct TerminalListener(pub UnboundedSender<AlacTermEvent>);

impl EventListener for TerminalListener {
    fn send_event(&self, event: AlacTermEvent) {
        let _ = self.0.unbounded_send(event);
    }
}

/// Shell 타입 정의 (현재는 System만 사용)
#[derive(Clone, Debug)]
pub enum Shell {
    System,
    #[allow(dead_code)]
    Program(String),
    #[allow(dead_code)]
    WithArguments {
        program: String,
        args: Vec<String>,
    },
}

/// 터미널 빌더 (Zed TerminalBuilder와 동일 구조)
pub struct TerminalBuilder {
    terminal: Terminal,
    events_rx: UnboundedReceiver<AlacTermEvent>,
}

/// 메인 터미널 구조체 (Zed Terminal과 동일 구조)
pub struct Terminal {
    pty_tx: Notifier,
    term: Arc<FairMutex<Term<TerminalListener>>>,
    #[allow(dead_code)]
    events_rx: Option<UnboundedReceiver<AlacTermEvent>>,
}

impl TerminalBuilder {
    /// Zed의 TerminalBuilder::new()와 동일한 시그니처로 구현
    pub fn new(
        working_directory: Option<PathBuf>,
        shell: Shell,
        mut env: HashMap<String, String>,
        window_id: u64,
    ) -> Result<TerminalBuilder> {
        // 1. Zed와 동일한 환경 변수 설정
        if std::env::var("LANG").is_err() {
            env.entry("LANG".to_string())
                .or_insert_with(|| "en_US.UTF-8".to_string());
        }

        env.insert("ZED_TERM".to_string(), "true".to_string());
        env.insert("TERM_PROGRAM".to_string(), "zed".to_string());
        env.insert("TERM".to_string(), "xterm-256color".to_string());
        env.insert("TERM_PROGRAM_VERSION".to_string(), "1.0.0".to_string());

        // 2. Shell 파라미터 설정 (Zed와 동일한 로직)
        let shell_program = match shell.clone() {
            Shell::System => {
                #[cfg(target_os = "windows")]
                {
                    get_windows_system_shell()
                }
                #[cfg(not(target_os = "windows"))]
                {
                    std::env::var("SHELL").unwrap_or("/bin/sh".to_string())
                }
            }
            Shell::Program(program) => program,
            Shell::WithArguments { program, .. } => program,
        };

        let shell_args = match shell {
            Shell::WithArguments { args, .. } => Some(args),
            _ => None,
        };

        // 3. PTY 옵션 구성 (Zed와 동일)
        let alac_shell = AlacShell::new(shell_program, shell_args.unwrap_or_default());
        let working_dir = working_directory
            .or_else(|| dirs::home_dir());

        let pty_options = PtyOptions {
            shell: Some(alac_shell),
            working_directory: working_dir,
            env: env.into_iter().collect(),
            hold: false,
        };

        // 4. 이벤트 채널 생성
        let (events_tx, events_rx) = unbounded();

        // 5. 터미널 생성 (Zed와 동일)
        let config = Config::default();
        let bounds = TerminalBounds::default();
        let term = Term::new(
            config,
            &bounds,
            TerminalListener(events_tx.clone()),
        );

        let term = Arc::new(FairMutex::new(term));

        // 6. PTY 생성 (Zed와 동일)
        let pty = match tty::new(&pty_options, bounds.into(), window_id) {
            Ok(pty) => pty,
            Err(error) => {
                bail!("PTY 생성 실패: {}", error);
            }
        };

        // 7. EventLoop 연결 (Zed와 동일)
        let event_loop = EventLoop::new(
            term.clone(),
            TerminalListener(events_tx),
            pty,
            true, // drain_on_exit
            false, // hold
        )?;

        // 8. IO 스레드 시작 (Zed와 동일)
        let pty_tx = event_loop.channel();
        let _io_thread = event_loop.spawn();

        let terminal = Terminal {
            pty_tx: Notifier(pty_tx),
            term,
            events_rx: None, // events_rx는 따로 관리
        };

        Ok(TerminalBuilder {
            terminal,
            events_rx,
        })
    }

    /// 터미널 빌더에서 완성된 터미널 반환
    pub fn build(self) -> (Terminal, UnboundedReceiver<AlacTermEvent>) {
        (self.terminal, self.events_rx)
    }
}

impl Terminal {
    /// 터미널에 입력 전송
    pub fn input(&mut self, data: &[u8]) -> Result<()> {
        let data_vec = data.to_vec();
        self.pty_tx.0.send(alacritty_terminal::event_loop::Msg::Input(data_vec.into()))?;
        Ok(())
    }

    /// 터미널 내용을 렌더링 가능한 형태로 가져오기 (한글 지원 개선)
    pub fn get_renderable_content(&self) -> Result<Vec<String>> {
        let term = self.term.lock();
        let grid = term.grid();
        let mut lines = Vec::new();

        for line_index in 0..grid.screen_lines() {
            let line_idx = alacritty_terminal::index::Line(line_index as i32);
            let line = &grid[line_idx];

            let mut line_content = String::new();
            let mut col_index = 0;

            while col_index < line.len() {
                let cell = &line[alacritty_terminal::index::Column(col_index)];
                let ch = cell.c;

                // 실제 문자만 추가 (null character와 wide char spacer 제외)
                if ch != '\0' && ch != ' ' || !cell.flags.contains(alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER) {
                    line_content.push(ch);
                }

                // wide character인 경우 다음 셀은 spacer이므로 건너뛰기
                if cell.flags.contains(alacritty_terminal::term::cell::Flags::WIDE_CHAR) {
                    col_index += 2; // wide char는 2개 셀을 차지
                } else {
                    col_index += 1;
                }
            }

            // 줄 끝의 공백 유지
            lines.push(line_content);
        }

        Ok(lines)
    }

    /// 커서 위치 가져오기 (마우스 커서 위치 - 디버그용)
    pub fn get_cursor(&self) -> (u16, u16) {
        let term = self.term.lock();
        let grid = term.grid();
        let cursor_pos = grid.cursor.point;
        (cursor_pos.column.0 as u16, cursor_pos.line.0 as u16)
    }

    /// 깜빡이는 터미널 커서 위치 가져오기 (RenderableCursor)
    pub fn get_renderable_cursor(&self) -> (u16, u16, char) {
        let term = self.term.lock();
        let content = term.renderable_content();
        let cursor_char = term.grid()[content.cursor.point].c;
        (
            content.cursor.point.column.0 as u16,
            content.cursor.point.line.0 as u16,
            cursor_char
        )
    }

    /// 터미널이 마우스 모드를 지원하는지 확인
    pub fn is_mouse_mode_enabled(&self) -> bool {
        let term = self.term.lock();
        let mode = term.mode();
        // MOUSE_REPORT_CLICK, MOUSE_DRAG, MOUSE_MOTION 등 마우스 모드 확인
        mode.intersects(
            alacritty_terminal::term::TermMode::MOUSE_REPORT_CLICK
            | alacritty_terminal::term::TermMode::MOUSE_DRAG
            | alacritty_terminal::term::TermMode::MOUSE_MOTION
        )
    }

    /// 터미널이 대체 화면 모드인지 확인
    #[allow(dead_code)]
    pub fn is_alternate_screen(&self) -> bool {
        let term = self.term.lock();
        term.mode().contains(alacritty_terminal::term::TermMode::ALT_SCREEN)
    }
}

/// Windows 시스템 Shell 찾기 (Zed와 동일한 로직)
#[cfg(target_os = "windows")]
fn get_windows_system_shell() -> String {

    // 1. PowerShell 7+ 찾기
    if let Some(pwsh) = find_powershell(false, false) {
        return pwsh.to_string_lossy().to_string();
    }

    // 2. PowerShell 6+ 찾기
    if let Some(pwsh) = find_powershell(true, false) {
        return pwsh.to_string_lossy().to_string();
    }

    // 3. Windows PowerShell 찾기
    if let Some(powershell) = find_windows_powershell() {
        return powershell.to_string_lossy().to_string();
    }

    // 4. cmd.exe fallback
    std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
}

#[cfg(target_os = "windows")]
fn find_powershell(find_alternate: bool, find_preview: bool) -> Option<PathBuf> {
    let env_var = if find_alternate {
        "ProgramFiles(x86)"
    } else {
        "ProgramFiles"
    };

    let install_base_dir = PathBuf::from(std::env::var_os(env_var)?)
        .join("PowerShell");

    let entries = install_base_dir.read_dir().ok()?;
    let mut versions = Vec::new();

    for entry in entries.flatten() {
        if entry.file_type().ok()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with('7') || (!find_preview && name.starts_with('6')) {
                    versions.push(entry.path());
                }
            }
        }
    }

    versions.sort();
    versions.reverse();

    versions.into_iter()
        .find_map(|path| {
            let exe_path = path.join("pwsh.exe");
            if exe_path.exists() {
                Some(exe_path)
            } else {
                None
            }
        })
}

#[cfg(target_os = "windows")]
fn find_windows_powershell() -> Option<PathBuf> {
    let system32 = PathBuf::from(std::env::var_os("SystemRoot")?)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");

    if system32.exists() {
        Some(system32)
    } else {
        None
    }
}