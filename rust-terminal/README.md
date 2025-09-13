# Rust Terminal

Zed 에디터의 터미널 기능을 참조하여 Rust로 구현한 터미널 에뮬레이터입니다.

## 특징

- **Alacritty 기반**: Alacritty 터미널 엔진을 사용하여 높은 성능과 정확한 터미널 에뮬레이션
- **크로스 플랫폼**: Windows (ConPTY)와 Unix (PTY) 모두 지원
- **TUI 인터페이스**: Ratatui를 사용한 깔끔한 터미널 UI
- **실시간 처리**: 비동기 이벤트 처리로 반응성 있는 터미널 경험

## 구조

```
src/
├── main.rs           # 메인 애플리케이션과 TUI
├── terminal.rs       # 터미널 엔진 (Alacritty 기반)
├── pty.rs           # PTY 인터페이스
└── pty/
    ├── mod.rs       # PTY 모듈
    ├── windows_pty.rs # Windows ConPTY 구현
    └── unix_pty.rs   # Unix PTY 구현
```

## 빌드 및 실행

```bash
cd rust-terminal
cargo build --release
cargo run
```

## 사용법

- 일반적인 터미널 명령어 입력 가능
- `Ctrl+Q`: 프로그램 종료
- 키보드 입력이 바로 터미널로 전달됨

## 의존성

- `alacritty_terminal`: 터미널 엔진
- `ratatui`: TUI 프레임워크
- `crossterm`: 크로스플랫폼 터미널 API
- `tokio`: 비동기 런타임
- `windows` (Windows): Windows API 바인딩
- `nix` (Unix): Unix 시스템 호출

## Zed와의 차이점

이 구현은 Zed의 터미널 기능을 단순화한 버전입니다:
- UI 프레임워크: GPUI → Ratatui
- 복잡한 기능 제거: 하이퍼링크, 검색, VI 모드 등
- 핵심 터미널 기능에 집중

## 라이선스

MIT License