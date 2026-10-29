<p align="center"><img src="web/public/logo.svg" width="96" alt="pi-oven"></p>

# pi-oven

[pi](https://pi.dev) 코딩 에이전트를 브라우저와 데스크톱·안드로이드 앱에서 쓰는 웹 UI입니다. Rust(axum) 서버가 세션마다 `pi --mode rpc` 프로세스를 띄워 WebSocket으로 중계하고, 화면은 Svelte 5 SPA입니다. 같은 네트워크의 다른 기기(폰 포함)에서도 접속할 수 있습니다.

따뜻한 차콜 바탕에 주황 하나가 기본 테마입니다. 에이전트가 실제로 일하는 동안에만 앰버색이 켜집니다. UI 문구는 한국어입니다.

## 할 수 있는 것

- **대화**: 스트리밍, 도구 호출 표시, 입력 중 방향 전환(steer)·이어서 보내기, 이미지 첨부, 모델·추론 강도 선택, 자동/수동 권한.
- **세션 관리**: 세션 목록과 검색, 이름 바꾸기, 세션 트리에서 분기·다시 생성·여기부터 지우기(항상 새 세션에서 이어짐), 폴더별 묶음.
- **백그라운드 작업**: `bg_run` 작업이 칩으로 올라오고, 끝나면 알림 말풍선으로 들어갑니다.
- **할 일**: `@juicesharp/rpiv-todo`가 있으면 입력창 위에 진행 상황 트레이가 나옵니다(애드온 방식).
- **우측 패널**: 정보 · 파일(탐색기) · Diff · Git(스테이지·커밋) · Pin(대화에서 찾은 주소·토큰·고정한 메시지).
- **편집기**: 파일을 끌어 놓거나 컨텍스트 메뉴로 열면 CodeMirror 편집기가 대화 옆에 열립니다. 에이전트가 파일을 고치면 입력한 내용을 지키며 반영합니다.
- **모델 추가**: 엔드포인트 주소·API 키·모델 ID를 적으면 pi의 `models.json`에 저장합니다.
- **쇼케이스 모드**: 화면 공유·스크린샷용으로 폴더 경로, 사용자 이름, 토큰, 서버 주소 등을 항목별로 가립니다(흐리게 또는 예시 글자).
- **Pin 감지 규칙**: 직접 쓴 정규식으로 Pin에 올릴 값을 더합니다.
- 응답 아래의 tok/s·첫 글자까지의 시간(TTFT), 파일 종류별 아이콘, 라이트/다크 테마, 폰 화면 지원.

## 데모 영상

백그라운드 작업(`bg_run`)이 칩으로 올라와 실행 시간이 올라가다가, 끝나면 알림 말풍선으로 들어가는 모습입니다. 둘 다 설정의 **쇼케이스 모드**로 폴더 이름과 주소를 예시 글자로 바꿔 촬영했습니다.

**폰**

<video src="docs/media/background-tasks-phone.mp4" poster="docs/media/background-tasks-phone.jpg" controls muted playsinline width="280"></video>

[폰 영상 (mp4)](docs/media/background-tasks-phone.mp4)

**데스크톱**

<video src="docs/media/background-tasks-desktop.webm" poster="docs/media/background-tasks-desktop.jpg" controls muted playsinline width="720"></video>

[데스크톱 영상 (webm)](docs/media/background-tasks-desktop.webm)

## 실행

필요한 것: [pi](https://pi.dev)(`pi --mode rpc`), 최근 stable Rust, Node.js(22에서 확인).

```sh
cd web && npm install && npm run build && cd ..
cargo build --release
./target/release/pi-oven       # http://127.0.0.1:4141
```

프론트를 고쳤다면 `npm run build` 뒤에 `touch src/web.rs`를 하고 다시 빌드해야 바이너리에 들어갑니다(`web/dist`를 내장합니다). 개발 중에는 `cargo run`과 `cd web && npm run dev`(5173, `/api` 프록시)를 같이 띄웁니다. 타입·접근성 검사는 `cd web && npm run check`입니다.

### 다른 기기에서 쓰기

```sh
./target/release/pi-oven --lan                         # 토큰을 만들어 출력
./target/release/pi-oven --lan --password-file ~/pw    # 비밀번호로 접속
./target/release/pi-oven --lan --token-file ~/token    # 토큰을 정해서 접속
```

주소 끝에 `#token=…`을 붙여 접속하거나 접속 화면에 입력합니다. 틀린 입력이 반복되면 접속한 주소별로 잠깁니다. 옵션 전체는 `--help`와 [docs/USAGE.md](docs/USAGE.md)에 있습니다.

> pi는 이 PC에서 셸 명령을 실행합니다. 토큰이 곧 이 PC의 권한이므로 인터넷에 직접 열지 말고 VPN이나 HTTPS 리버스 프록시 뒤에서 쓰세요.

## 데스크톱·안드로이드 앱

`desktop/`는 서버에 연결하는 얇은 [Tauri](https://tauri.app) 창입니다. 서버 주소(와 토큰)를 한 번 입력하면 저장되고, 폰에서는 앱을 열 때 저장된 서버에 자동으로 연결됩니다. 화면은 서버가 내려주는 웹 UI 그대로라서 서버는 따로 실행해 두어야 합니다.

```sh
cargo install tauri-cli --version "^2"
cd desktop
cargo tauri build                                  # Linux: deb, AppImage (webkit2gtk-4.1 개발 패키지 필요)
cargo tauri android build --apk --target aarch64   # Android: SDK·NDK와 `rustup target add aarch64-linux-android` 필요, 서명은 직접
```

Linux만 직접 돌려 확인했고, 안드로이드 APK는 빌드와 서명까지만 확인했습니다. Windows와 macOS는 빌드해 보지 않았습니다.

## 의존하는 pi 패키지

pi-oven 자체는 `pi --mode rpc`만 있으면 동작합니다. 아래 pi 패키지는 있을 때만 해당 화면이 켜집니다(없으면 그 부분만 나오지 않습니다).

| 패키지 | 켜지는 것 |
|---|---|
| [`pi-background-tasks`](https://www.npmjs.com/package/pi-background-tasks) | `bg_run` 백그라운드 작업 칩, 실행 시간, 완료 알림 말풍선, 작업 목록 |
| [`@juicesharp/rpiv-todo`](https://www.npmjs.com/package/@juicesharp/rpiv-todo) | 입력창 위의 할 일 트레이 |
| [`pi-llama-infra`](https://www.npmjs.com/package/pi-llama-infra) | 답변 아래 tok/s, 첫 글자 전 대기 중 "프리필 중" 표시 |

권한(자동/수동) 확장은 이 저장소의 `ext/`에 있어서 따로 설치할 것이 없습니다.

## 구조

| 경로 | 내용 |
|---|---|
| `src/` | Rust 서버: pi 프로세스·WebSocket 중계, 세션 파일(분기·이름), 파일·Git API, 모델 설정, 인증 |
| `web/src/` | Svelte 5 프론트: `lib/`(상태·순수 로직), `components/`, `addons/`(패키지별 애드온) |
| `ext/` | 서버에 내장되어 pi에 `--extension`으로 로드되는 권한(자동/수동) 확장 |
| `desktop/` | Tauri 앱(데스크톱 + Android) |

## 라이선스와 에셋

- 로고와 아이콘은 이 프로젝트를 위해 그린 것입니다. pi 프로젝트의 로고는 쓰지 않습니다.
- 글꼴 JetBrains Mono는 SIL OFL 1.1입니다. 메이플스토리 서체는 넥슨의 이용 조건(무료, 배포·번들 가능, 수정 금지)에 따라 그대로 포함했고 조건 전문은 `web/src/assets/fonts/MAPLESTORY-LICENSE.txt`에 있습니다. 이 프로젝트의 화면에는 메이플스토리가 제공한 메이플스토리 서체가 적용되어 있습니다.
- 의존성의 라이선스와 고지는 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), 에셋은 [ASSETS_LICENSE.md](ASSETS_LICENSE.md).
- 이 프로젝트는 [Apache License 2.0](LICENSE)입니다. 위에 적은 글꼴과 의존성은 각자의 조건을 따릅니다.
