# Zed Terminal 구현 분석 문서

## 개요
Zed 에디터의 터미널 구현을 분석하여 Rust로 동일한 기능을 복제하기 위한 기술 문서입니다.

## 1. 전체 아키텍처

### 1.1 핵심 구조
```
Terminal (Zed의 터미널 관리자)
├── alacritty_terminal::Term (실제 터미널 엔진)
├── alacritty_terminal::EventLoop (이벤트 처리)
├── alacritty_terminal::tty::Pty (PTY 인터페이스)
└── PtyProcessInfo (프로세스 정보 관리)
```

### 1.2 주요 컴포넌트
- **alacritty_terminal**: 터미널 에뮬레이션의 핵심 엔진
- **Term**: 터미널 상태 및 렌더링 관리
- **EventLoop**: PTY와 터미널 간 통신
- **Pty**: 플랫폼별 의사 터미널 구현

## 2. 터미널 생성 과정

### 2.1 TerminalBuilder::new() 과정
```rust
// 1. 환경 변수 설정
env.insert("ZED_TERM", "true");
env.insert("TERM_PROGRAM", "zed");
env.insert("TERM", "xterm-256color");

// 2. Shell 설정 (Windows의 경우)
Shell::System => {
    program: util::get_windows_system_shell(), // PowerShell 또는 cmd.exe
}

// 3. PTY 옵션 구성
let pty_options = alacritty_terminal::tty::Options {
    shell: alac_shell,
    working_directory: working_directory.or_else(|| Some(home_dir())),
    drain_on_exit: true,
    env: env.clone().into_iter().collect(),
};

// 4. 터미널 생성
let term = Term::new(config, &TerminalBounds::default(), ZedListener(events_tx));

// 5. PTY 생성
let pty = tty::new(&pty_options, TerminalBounds::default().into(), window_id)?;

// 6. EventLoop 연결
let event_loop = EventLoop::new(term, ZedListener(events_tx), pty, drain_on_exit, false)?;

// 7. IO 스레드 시작
let _io_thread = event_loop.spawn();
```

## 3. Windows 시스템 Shell 선택

### 3.1 Shell 우선순위
```rust
pub fn get_windows_system_shell() -> String {
    // 1. PowerShell 7+ 찾기 (Program Files)
    // 2. PowerShell 6+ 찾기 (Program Files x86)
    // 3. Windows PowerShell 찾기
    // 4. cmd.exe 사용 (fallback)
}
```

### 3.2 환경 변수 설정
- `LANG`: "en_US.UTF-8" (기본값)
- `ZED_TERM`: "true"
- `TERM_PROGRAM`: "zed"
- `TERM`: "xterm-256color"
- `TERM_PROGRAM_VERSION`: Zed 버전

## 4. PTY (Pseudo Terminal) 시스템

### 4.1 Windows PTY 구현 (pty_info.rs)
```rust
pub struct ProcessIdGetter {
    handle: i32,
    fallback_pid: u32,
}

// Windows에서 프로세스 ID 얻기
fn pid(&self) -> Option<Pid> {
    let pid = unsafe { GetProcessId(HANDLE(self.handle as _)) };
    if pid == 0 {
        Some(Pid::from_u32(self.fallback_pid))
    } else {
        Some(Pid::from_u32(pid))
    }
}
```

### 4.2 프로세스 정보 관리
- **PtyProcessInfo**: PTY 프로세스 모니터링
- **sysinfo**: 시스템 프로세스 정보 수집
- **프로세스 추적**: PID, 작업 디렉토리, 명령행 인수

## 5. 이벤트 시스템

### 5.1 이벤트 플로우
```
KeyInput -> Terminal -> EventLoop -> PTY -> Shell Process
                  ↓
             AlacTermEvent <- EventLoop <- PTY Output
                  ↓
              Terminal Display Update
```

### 5.2 주요 이벤트 타입
- `AlacTermEvent::Title`: 터미널 제목 변경
- `AlacTermEvent::Bell`: 벨 사운드
- `AlacTermEvent::ClipboardStore`: 클립보드 저장
- `AlacTermEvent::ClipboardLoad`: 클립보드 로드

## 6. 터미널 설정 (Config)

### 6.1 핵심 설정
```rust
let config = Config {
    scrolling_history: max_scroll_history_lines, // 스크롤 히스토리
    default_cursor_style: AlacCursorStyle::from(cursor_shape), // 커서 스타일
    ..Config::default()
};
```

### 6.2 스크롤 설정
- **기본 히스토리**: `DEFAULT_SCROLL_HISTORY_LINES`
- **최대 히스토리**: `MAX_SCROLL_HISTORY_LINES`
- **태스크 모드**: 최대 히스토리 사용 (대량 출력 대응)

## 7. 입출력 처리

### 7.1 입력 처리
- 키보드 입력 → mappings/keys.rs → PTY
- 마우스 입력 → mappings/mouse.rs → PTY
- 클립보드 → Terminal → PTY

### 7.2 출력 처리
- PTY 출력 → EventLoop → Terminal → 화면 렌더링
- VTE 파서를 통한 ANSI 이스케이프 시퀀스 처리

## 8. 구현시 핵심 포인트

### 8.1 필수 의존성
```toml
[dependencies]
alacritty_terminal = "0.24"  # 핵심 터미널 엔진
futures = "0.3"              # 비동기 처리
smol = "2.0"                 # 경량 async runtime
sysinfo = "0.30"             # 시스템 정보
dirs = "5.0"                 # 디렉토리 유틸리티

[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_System_Console",
    "Win32_System_Threading"
]}
```

### 8.2 Windows 특화 구현
- ConPTY API 사용
- Windows API를 통한 프로세스 관리
- PowerShell 우선 shell 선택

### 8.3 권한 관리
- 관리자 권한 없이 동작
- 사용자 수준 프로세스 생성
- 환경 변수를 통한 권한 제어

## 9. 복제 구현 계획

### 9.1 1단계: 기본 구조
- alacritty_terminal 의존성 추가
- TerminalBuilder 패턴 구현
- 기본 PTY 생성

### 9.2 2단계: Windows 통합
- get_windows_system_shell() 구현
- Windows PTY 처리
- 프로세스 정보 관리

### 9.3 3단계: 이벤트 시스템
- EventLoop 연결
- 입출력 처리
- 터미널 디스플레이

### 9.4 4단계: UI 통합
- Ratatui와 연결
- 키보드/마우스 이벤트
- 터미널 렌더링

## 10. 주요 차이점

### 10.1 Zed vs 구현 목표
- **Zed**: GPUI 기반 네이티브 UI
- **목표**: Ratatui 기반 터미널 UI

### 10.2 단순화 포인트
- UI 렌더링 시스템 변경
- 일부 고급 기능 제외 (하이퍼링크, 검색 등)
- 핵심 터미널 기능에 집중

이 분석을 바탕으로 Zed와 동일한 터미널 기능을 가진 Rust 애플리케이션을 구현할 수 있습니다.