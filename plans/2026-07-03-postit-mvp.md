# postit — 리눅스 데스크탑 포스트잇 MVP 기획서

작성: 2026-07-03 (기획: Fable / 구조 코딩: Sonnet / 단순 코딩: Haiku)

## 1. 개요

모니터 화면 위 원하는 위치에 붙이는 초소형 포스트잇 프로그램.
- 대상 환경(1차): Pop!_OS COSMIC / Wayland. 이후 Windows·Mac 확장 예정.
- 언어/스택: Rust + iced 0.13 계열 + **iced_layershell 0.18** (zwlr_layer_shell_v1).
  - Wayland은 일반 창의 절대 위치 지정을 금지하므로 layer-shell 서피스로 구현.
  - 향후 크로스플랫폼: UI(iced 위젯)는 공유, 윈도잉 백엔드만 교체(iced multiwindow + winit `set_outer_position`).

## 2. 핵심 UX

### 2.1 상단 위젯 바 (툴바)
- 화면 상단 중앙에 항상 떠 있는 작은 layer surface (anchor: Top, 크기 약 240×36, layer: Top).
- 내용: 컬러 스와치 5개. 스와치 클릭 → 해당 색 새 포스트잇 생성.

### 2.2 포스트잇 노트
- 크기: **가로 4cm × 세로 1cm ≈ 152×40 logical px** (96dpi 환산, 고정).
- 노트 하나 = layer surface 하나. anchor: Top|Left, margin(x, y)로 위치 표현.
- 생성 위치: 화면 좌상단 기준 (120, 120)에서 노트마다 +24px 계단식 오프셋.
- 구성(한 줄 레이아웃): `[⣿ 드래그 그립 12px] [텍스트 입력(한 줄)] [▾ 메뉴 버튼]`
- **드래그 → 고정**: 그립을 잡고 드래그하면 margin 실시간 갱신, 놓으면 그 위치에 고정 + 저장.
- **편집**: 텍스트 영역 클릭 → 바로 입력. 변경 즉시 저장.
- **▾ 메뉴**: 노트가 아래로 확장(40→76px)되어 인라인 메뉴 표시
  - 색 변경 스와치 5개 / 📌 "항상 표시" 토글 / 🗑 삭제
  - (layer surface에서 별도 팝업 서피스는 위험하므로 인라인 확장 방식)

### 2.3 가시성 규칙 — 앱 바인딩 (핵심 차별 기능)
- 노트 생성 시점의 **활성 프로그램 app_id를 노트에 기록**(`bound_app`).
- 기본 동작: 활성 프로그램이 `bound_app`과 다르면 노트 숨김, 돌아오면 다시 표시.
- 노트별 옵션 `always_visible = true`이면 화면 전환과 무관하게 항상 표시.
- 예외: 활성 앱이 postit 자신이면(노트 편집 중) 직전 판정 유지 — 편집하려고 클릭했는데 숨어버리면 안 됨.
- 구현: `zwlr_foreign_toplevel_manager_v1` 프로토콜로 activated 이벤트 구독(별도 스레드 + 채널 → iced subscription).
- **폴백**: 프로토콜 미지원 컴포지터에서는 경고 로그 후 전 노트 항상 표시로 동작(기능 자체는 죽지 않음).

### 2.4 저장
- `~/.local/share/postit/notes.json` (serde_json).
- 텍스트/색/위치/옵션 변경 시 즉시 저장(원자적: tmp 파일 → rename).
- 시작 시 로드하여 노트 전부 복원.

## 3. 데이터 모델 (인터페이스 확정 — 구현자는 이 시그니처 준수)

```rust
// src/note.rs
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Note {
    pub id: u64,               // 생성 시각 기반 유니크 id
    pub text: String,
    pub color: NoteColor,
    pub x: i32,                // 화면 좌상단 기준 margin
    pub y: i32,
    pub always_visible: bool,  // 기본 false
    pub bound_app: Option<String>, // 생성 시 활성 app_id
}

// src/colors.rs
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteColor { Yellow, Pink, Blue, Green, Orange }
impl NoteColor {
    pub const ALL: [NoteColor; 5];
    pub fn bg(&self) -> iced::Color;     // 배경
    pub fn border(&self) -> iced::Color; // 테두리(배경보다 진하게)
    pub fn text(&self) -> iced::Color;   // 글자색 (진회갈색 #3E2723 계열)
}
// 팔레트: Yellow #FFF176 / Pink #F48FB1 / Blue #81D4FA / Green #A5D6A7 / Orange #FFB74D

// src/storage.rs
pub fn load_notes() -> Vec<Note>;      // 파일 없으면 빈 벡터
pub fn save_notes(notes: &[Note]);     // 실패 시 eprintln만, panic 금지
```

## 4. 모듈 구조 및 담당

```
postit/
├── Cargo.toml            (Sonnet)
├── plans/2026-07-03-postit-mvp.md   ← 본 문서
├── postit.desktop        (Haiku)
├── README.md             (Haiku)
└── src/
    ├── main.rs           (Sonnet) iced_layershell daemon 부트스트랩
    ├── app.rs            (Sonnet) 전역 상태·Message·update/view 라우팅(서피스 id↔노트 매핑)
    ├── toolbar.rs        (Sonnet) 상단 바 뷰
    ├── note_view.rs      (Sonnet) 노트 뷰(그립/입력/인라인 메뉴) + 드래그 로직
    ├── focus.rs          (Sonnet) wlr-foreign-toplevel 활성 앱 트래킹(스레드+채널), 폴백 포함
    ├── note.rs           (Haiku) 모델
    ├── colors.rs         (Haiku) 팔레트
    └── storage.rs        (Haiku) JSON 로드/저장
```

## 5. 기술 메모 (Sonnet 참고)

- iced_layershell 0.18: `to_layer_message` 매크로 + daemon(멀티 서피스) 모드 사용.
  버전 궁합은 crates.io 문서·예제(waycrate/exwlshelleventloop 저장소의 examples) 기준으로 맞출 것.
- 노트 숨김/표시: 서피스 close/재생성 방식(id↔Note 매핑 유지). margin 오프스크린 이동은 입력 이벤트가 남으므로 금지.
- 드래그: 그립에서 ButtonPressed 시 시작점 기록 → CursorMoved 델타로 margin 변경 액션 발행 → ButtonReleased 시 저장.
- keyboard_interactivity: 노트는 `OnDemand` (텍스트 입력 필요), 바는 `None`.
- exclusive_zone: 모두 0 (다른 창 영역 안 밀어냄).

## 6. MVP 범위 밖 (다음 단계)

- 멀티 모니터 출력 선택, 노트 크기 조절, 여러 줄 텍스트, 알림/리마인더
- 트레이 아이콘·자동 시작, 바 자동 숨김
- Windows/Mac 백엔드 (iced multiwindow + winit)

## 7. 2026-07-03 추가: 트레이 전환·목록·종료

- 상단 패널 아이콘화: `ksni`(0.3.5, blocking feature) StatusNotifierItem 트레이(`src/tray.rs`) 추가. 클릭=노랑 새 노트, 메뉴=색상별 새 포스트잇/목록/종료. 아이콘은 `icon_pixmap()`로 직접 그린 22×22 ARGB32(노란 사각형+접힌 모서리), 테마 아이콘 미의존.
- daemon `StartMode::Background`로 전환해 초기 서피스 자체를 만들지 않음(`size: None`) — 트레이 등록 성공 시 플로팅 툴바가 아예 생성되지 않고, 실패 시(`TrayMessage::Unavailable`)에만 그때 가서 `NewLayerShell`로 만듦. 기존 방식(초기 서피스를 만들었다가 RemoveWindow)보다 단순.
- 폴백 툴바(`toolbar.rs`)에 ☰(목록)·✕(종료) 버튼 추가, 폭 240→300.
- 새 layer surface 기반 "포스트잇 목록" 패널(`src/list_view.rs`, anchor Top·320×300·margin-top 48) 추가: 숨김 노트 포함 전체 목록, 색 칩+텍스트+[가져오기]/[삭제]. 가져오기는 (160,160)+계단식 오프셋으로 이동 후 서피스 없으면 생성.
- 종료는 `iced::exit()` 사용(iced_layershell의 `Action::Exit` → `should_exit`가 이벤트 루프를 정상 종료시키는 것을 소스로 확인) + 직전 `save()`.

## 8. 2026-07-04 추가: 트레이 패닉 수정 · 멀티 모니터

### 8.1 트레이 아이콘 미표시의 진짜 원인 (수정 완료)
- 증상: 패널에 아이콘이 안 뜸. 실제로는 **앱이 시작 직후 패닉으로 즉사**하고 있었음:
  `there is no reactor running, must be called from the context of a Tokio 1.x runtime` (zbus executor)
- 원인: ksni의 default feature가 `tokio` → `zbus/tokio` 활성화. 같은 프로세스에서 iced_layershell의
  테마 감지(mundy)가 `zbus/async-io`로 zbus를 호출하는데, zbus는 tokio feature가 켜져 있으면
  무조건 tokio 리액터를 요구 → 메인 스레드 패닉. (tray_probe는 mundy가 없어서 정상이었음 —
  이것으로 등록/렌더링 문제가 아님을 분리 검증)
- 수정: `ksni = { version = "0.3", default-features = false, features = ["async-io", "blocking"] }`
- 교훈: **zbus를 쓰는 크레이트를 2개 이상 넣을 때는 feature를 async-io로 통일할 것.**

### 8.2 활성 앱 추적 프로토콜 (미해결, 다음 과제)
- COSMIC은 `zwlr_foreign_toplevel_manager_v1`을 **구현하지 않음** (레지스트리 직접 확인).
  대신 `zcosmic_toplevel_info_v1`(+`ext_foreign_toplevel_list_v1`) 제공.
- 따라서 현재 focus.rs는 항상 폴백(전체 항상 표시)으로 동작 → 핵심 기능인 앱 바인딩 숨김이 죽어 있음.
- 다음 단계: focus.rs에 `zcosmic_toplevel_info_v1` 백엔드 추가 (cosmic-protocols 크레이트,
  activated 상태 이벤트 + app_id). wlr 백엔드는 다른 컴포지터용으로 유지.

### 8.3 멀티 모니터 (이번 구현 범위)
- 문제: layer surface는 생성 시 한 wl_output에 고정. margin을 키워도 그 모니터 안에서 잘릴 뿐
  옆 모니터로 못 넘어감. `OutputOption::None`이면 컴포지터가 임의 출력에 배치해서
  사용자가 보는 모니터가 아닌 곳에 노트가 생기는 혼란도 발생(실사용 확인).
- 설계 (예측 가능성 우선):
  1. **src/outputs.rs (신규, Haiku 수준 아님 — Sonnet)**: 별도 Wayland 연결로 출력 열거.
     `zxdg_output_manager_v1`로 이름(logical name: "DP-1" 등)·논리 위치·논리 크기 수집.
     ```rust
     #[derive(Clone, Debug)]
     pub struct OutputInfo { pub name: String, pub x: i32, pub y: i32, pub width: i32, pub height: i32 }
     pub fn list_outputs() -> Vec<OutputInfo> // 논리 x 기준 좌→우 정렬, 실패 시 빈 벡터
     ```
  2. **Note에 `#[serde(default)] pub output: Option<String>` 추가 (Haiku)**. None = 미지정(레거시).
  3. **서피스 생성 시 항상 명시적 출력**: `OutputOption::OutputName(name)`.
     새 노트·가져오기 노트는 outputs[0](가장 왼쪽)에 생성. 레거시 None은 outputs[0]으로 간주하고 기록.
     outputs가 비면(열거 실패) 기존처럼 `OutputOption::None` 폴백.
  4. **모니터 간 이동 2가지 경로**:
     - 드래그 릴리즈 시 노트가 출력 경계에 붙어 있으면(왼쪽: x ≤ 2, 오른쪽: x + NOTE_W ≥ width − 2)
       인접 출력으로 hop: 서피스 close → 새 출력에 NewLayerShell 재생성.
       오른쪽 hop 후 x = 8, 왼쪽 hop 후 x = 새 출력 width − NOTE_W − 8, y는 유지(새 출력 height로 클램프).
       (드래그 중 실시간 hop은 그랩이 끊겨 UX가 더 나쁨 — 릴리즈 시점만 판정)
     - 노트 인라인 메뉴(▾)에 🖥 버튼: 다음 출력으로 순환 이동(위치는 (160,160)으로 리셋).
  5. 마이그레이션: notes.json에 output 없는 기존 데이터는 serde default로 None 로드 → 3번 규칙 적용.
- 구현 후 발견·수정: xdg_output v3부터 `zxdg_output_v1.done`이 폐기되어 COSMIC은 안 보냄 —
  `wl_output.done`에서 스냅샷 확정하도록 수정 (outputs.rs). 실측: 열거 정상 동작.

## 9. 2026-07-04 추가: 한글 IME 버그 2건 (라이브러리 패치) · 노트 폭 조절

### 9.1 한글 IME 버그 — 원인 확정, vendored 패치로 해결
- **버그 A: 마지막 글자 유실.** layershellev 0.18.1의 `zwp_text_input_v3` `Leave` 핸들러가
  조합 중(pending preedit) 글자를 커밋하지 않고 disable → 마지막 음절이 프로토콜 레벨에서 소실.
  surface를 먼저 None으로 지워서 IME가 deactivate 시점에 보내는 커밋도 드롭됨.
  패치: Leave에서 pending preedit을 `Ime::Commit`으로 flush 후 disable (GTK의 focus-out 커밋과 동일 동작).
- **버그 B: 노트 간 이동 시 한글 불가.** iced_layershell 0.18.1의 IME 허용 상태(`set_ime_allowed`)는
  **전역 1개**인데, 모든 창이 redraw 때마다 자기 IME 요청을 전역에 적용(handle_ui_state, update_ime=true).
  창별 상태머신(`ime_state`)은 전이 시 1회만 발화하므로, 비포커스 창 A의 Disabled가
  포커스 창 B의 Allowed **뒤에** 적용되면 전역 IME가 꺼진 채 고착(B는 재요청 안 함).
  패치: `ev.current_surface_id() == window.id`(키보드 포커스 서피스)일 때만 전역 IME 상태 반영,
  비포커스 창은 로컬 상태머신만 리셋.
- 상류(crates.io 0.18.1 최신, git master)에 수정 없음을 확인 → `vendor/iced_layershell`,
  `vendor/layershellev`로 복사 후 패치, `[patch.crates-io]`로 연결. 추후 업스트림 PR 후보.

### 9.1.1 후속 수정: live_preedit (마지막 글자 유실 재수정)
- 1차 패치(pending_preedit flush)는 no-op였음 — pending_preedit는 `done`에서 전달되는 순간 비워지는
  임시 버퍼라 Leave 시점엔 항상 None. `TextInputDataInner.live_preedit`(클라이언트가 현재 표시 중인
  조합 문자열)를 추가로 추적해 Leave에서 그것을 Commit. 사용자 검증 완료(한글 정상).

### 9.3 설정(트레이 전역): 크기 프리셋 · 투명도 (2026-07-04)
- `src/settings.rs`: AppSettings { size_preset: default|small, opacity: 100|90|80|70|60 },
  `~/.local/share/postit/settings.json` 저장. 트레이 메뉴 "크기"/"투명도" 서브메뉴로 전환, 전체 노트 일괄 적용.
- Small: 높이 30/펼침 60, 기본 폭 120, 글자 11. 최소 폭 = 인라인 메뉴 아이콘 줄 실측(Default 240 / Small 200),
  메뉴 펼침 동안만 `max(width, min)`로 임시 확장. 투명도는 배경·테두리 알파에만 적용(글자 알파 0.85 하한).
- 목록 패널 글자색: 컨테이너가 시스템 테마 글자색(어두운 테마=밝음)을 상속해 안 보이던 것 →
  container style `text_color`를 #3E2723으로 고정.
- 기타 UX: 생성 시 text_input 자동 포커스, 인라인 메뉴 버튼 클릭 시 자동 접힘, 🔗 재바인딩 버튼
  (bound_app을 마지막 활성 프로그램으로 재설정 — 추적 기능 이전에 만든 노트 구제),
  모니터 목록은 트레이 "모니터 새로읽기"로 명시 갱신(자동 재스캔 제거, 사용자 선호).

### 9.2 노트 폭 조절 (우측 가장자리 리사이즈)
- Note에 `#[serde(default = "default_width")] pub width: i32` (기본 152 = NOTE_COLLAPSED.0).
- 노트 뷰 맨 우측에 폭 10px 리사이즈 핸들(세로 전체). 잡고 드래그 → `SizeChange`로 실시간 폭 변경.
- 드래그와 동일한 절대 좌표 방식: `그랩 시점 폭 + (현재 커서 x − press x)`, 8ms 게이트, 범위 100..=800.
- 릴리즈 시 저장. 인라인 메뉴 확장(76px)·접힘(40px) 높이는 기존 유지, 폭만 가변.
