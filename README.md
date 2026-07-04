# Postit

리눅스 데스크탑 화면에 붙이는 초소형 포스트잇 프로그램입니다.

![postit icon](assets/postit.png)

## 소개

Postit은 데스크탑 화면의 원하는 위치에 작은 포스트잇을 고정하여 메모할 수 있는 도구입니다.

**주요 특징:**
- 시스템 트레이(패널 status area)에서 생성·관리
- 6가지 색상: 노랑, 분홍, 파랑, 초록, 주황, 회색
- **앱 바인딩**: 포스트잇을 만들 때 사용 중이던 프로그램에서만 표시 — 다른 프로그램으로 전환하면 자동으로 숨고, 돌아오면 다시 나타남 (노트별 "항상 표시" 옵션 제공)
- 드래그로 자유 배치, 우측 가장자리 드래그로 폭 조절 (100~800px)
- 멀티 모니터: 모니터 경계로 드래그하면 옆 모니터로 이동, 메뉴의 🖥 버튼으로도 이동
- 전역 설정(트레이 메뉴): 크기 프리셋(기본/스몰), 투명도(100~60%)
- 저장: `~/.local/share/postit/notes.json`, 설정: `~/.local/share/postit/settings.json`

## 요구사항

- Wayland 컴포지터 + `zwlr_layer_shell_v1` (Pop!_OS COSMIC, Sway, Hyprland 등)
- 활성 앱 추적: `zwlr_foreign_toplevel_manager_v1`(wlroots 계열) 또는 `zcosmic_toplevel_info_v1`(COSMIC). 둘 다 없으면 모든 노트 항상 표시로 동작
- 트레이 아이콘: StatusNotifierItem 지원 패널 (없으면 플로팅 툴바로 폴백)
- Rust 1.75+ / cargo

## 빌드·설치

```bash
cargo build --release
cp target/release/postit ~/.local/bin/
cp postit.desktop ~/.local/share/applications/   # Exec 경로를 환경에 맞게 조정
cp assets/postit.png ~/.local/share/icons/hicolor/128x128/apps/
```

## 사용법

- **새 포스트잇**: 트레이 아이콘 클릭(노랑) 또는 우클릭 → 새 포스트잇 → 색상 선택
- **이동**: 왼쪽 그립(⣿) 드래그, 모니터 경계까지 밀면 옆 모니터로
- **폭 조절**: 오른쪽 가장자리 세로 바 드래그
- **▾ 메뉴**: 색 변경 / 📌 항상 표시 / 🖥 다른 모니터로 / 🔗 현재 프로그램에 재바인딩 / 🗑 삭제
- **포스트잇 목록**: 트레이 우클릭 → 포스트잇 목록 — 숨겨진 노트 포함 전체 목록, [가져오기]로 화면 밖 노트 구조. 그립으로 드래그 이동 가능
- **모니터 새로읽기**: 모니터를 연결/분리했을 때 트레이 메뉴에서 실행

## 구현 메모

- `vendor/`의 `iced_layershell`·`layershellev`(0.18.1, MIT — `vendor/LICENSE-exwlshelleventloop`)는 한글 등 조합형 IME 버그 2건을 로컬 패치한 사본입니다:
  1. 포커스 아웃 시 조합 중 글자를 커밋 (마지막 글자 유실 방지)
  2. 전역 IME 상태를 키보드 포커스 서피스만 변경 가능하게 (창 간 이동 시 IME 죽는 문제)
- 설계 문서와 진행 로그는 `plans/2026-07-03-postit-mvp.md` 참고.
