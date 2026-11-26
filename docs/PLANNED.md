# 예정 사항

## 보류: 주소에 실리는 접속 비밀값 (2026-11-26)

웹소켓·이미지·`/api/events`는 헤더를 못 붙여 `?token=`으로 인증한다. 이 값은 세션 토큰이 아니라 서버 접속 비밀값 그 자체다(`--password`면 비밀번호 원문). 프록시·접근 로그에 남을 수 있다. 고치려면 로그인 때 서버가 짧게 쓰는 세션 토큰(또는 HttpOnly 쿠키)을 내주고, 주소에는 그것만 싣는다. 모바일 앱(다른 주소에서 접속)에서 쿠키가 되는지 먼저 확인한다.

## 보류: 터미널 pi가 만든 변화 (2026-11-26)

`/api/events`는 pi-oven을 거친 변화만 안다. 터미널의 `pi`로 만든 세션, `pi auth login`으로 바뀐 계정은 페이지가 다른 계기로 다시 읽을 때까지 안 보인다. 세션 폴더·`auth.json`을 감시하면 잡을 수 있다(`watch.rs` 방식).

## Claude 구독 한도 직접 조회 — 만듦 (2026-11-16)

`src/usage/claude.rs`(`GET https://api.anthropic.com/api/oauth/usage`, `anthropic-beta: oauth-2025-04-20`, `five_hour`·`seven_day`의 `utilization`·`resets_at`). 남은 것:
- 응답의 `limits[]`(모델별 주간 한도, 예: 특정 모델 전용)는 아직 읽지 않는다.
- 한도가 차면 Claude는 "usage limit" 문구 없이 `HTTP 429 rate_limit_error`("would exceed your account's rate limit")로 답한다. 짧은 속도 제한과 같은 모양이라, 바로 사용량을 다시 물어 95% 이상인 창이 있을 때만 한도 도달로 본다(`resume::Limit::Maybe`). pi가 재시도를 포기한 실패(`auto_retry_end`)는 그 판정이 끝날 때까지(최대 30초) 100h 모드에 넘기지 않는다: 한도면 Zero-Waste만, 아니면 100h(`Runtime::settle_held_failure`). 실제 한도 도달 응답의 헤더(`anthropic-ratelimit-unified-status: rejected` 등)로 바로 알 수 있는지는 아직 확인하지 않았다.

## Claude 캐시 수명 1시간으로 — 만듦 (2026-11-17)

설정 → 자동 진행 → Claude 캐시(`src/cache_ttl.rs`, 기본 켬). 켜면 pi를 띄울 때 `PI_CACHE_RETENTION=long`을 넘긴다. 서버 환경에 값이 있으면 그것을 따르고 스위치는 잠긴다. 새로 여는 세션부터 적용된다.
- 근거: Claude Code는 같은 구독에서 1시간 캐시만 쓴다. pi 기본은 5분.
- `pi-cache-optimizer`(2.10.6)도 이 값을 켠다고 하지만, 실행 중에 바꾸는 방식이라 실제 요청에는 반영되지 않았다(2026-11-16 세션 응답 21건 모두 `cacheWrite1h` 0). 시작할 때 주면 반영된다(짧은 요청으로 확인, `cacheWrite1h` = 전체).
- 남은 것: 같은 작업에서 구독 한도 소모가 실제로 줄었는지 비교. 요금 비율로 추정하면 쉬는 시간 없는 10분 세션은 약 21% 더 들고, 중간에 20분 쉬는 시간이 한 번이면 비슷하다.

## ChatGPT 한도 초기화권(리셋권) 쓰기

지금은 개수만 보여 준다(`wham/usage`의 `rate_limit_reset_credits.available_count`). 쓰는 기능은 아직 없다.

**알려진 방법** (공개 API가 아님, 제3자 도구와 이슈에서 확인된 내용이라 직접 확인 필요)
- 목록: `GET https://chatgpt.com/backend-api/wham/rate-limit-reset-credits`
- 쓰기: `POST https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume`
- 인증은 `wham/usage`와 같은 헤더(`Authorization: Bearer`, `chatgpt-account-id`)로 된다는 보고와, 그것만으로는 403이 난다는 보고가 함께 있다. 목록 주소가 429를 내는 문제도 보고되어 있다(openai/codex #37934).
- OpenAI 도움말: 리셋권은 저장해 두었다가 쓸 수 있고(받은 뒤 30일 안), 쓰면 바로 초기화된다. 유료 리셋은 주간 한도가 바닥났을 때만 앱에서 제안된다.

**만들 때**
1. 리셋권이 1개 이상인 계정에서 목록 주소를 불러 형태를 확인한다(쓰지는 않는다).
2. 정보 탭 카드의 "한도 초기화권 N개" 옆에 "쓰기" 버튼. 계정에 되돌릴 수 없는 동작이라 `ConfirmDialog`로 한 번 묻고(취소에 포커스), 쓴 뒤 바로 사용량을 다시 묻는다.
3. Zero-Waste가 기다리는 중에는 띠에 "리셋권 쓰기"를 둘지 정한다(자동으로 쓰지는 않는다).

## 구독 계정 여러 개 (예: Anthropic 둘) — 만듦 (2026-11-18)

설정 → 계정, Baton Touch(`src/accounts.rs`). 보강(2026-11-18): 모두 한도면 먼저 풀리는 계정으로 그 시각에, 다른 제공자에서 원래 모델로 복귀, 세션 기록에 남는 알림, 다른 세션에도 알림, macOS `script` 인자와 `script`가 없을 때 안내, 고르는 규칙 테스트. 남은 것(가짜 pi로만 확인함): 실제 Claude 로그인을 화면에서 끝까지 해 보기, 실제 pi에서 다른 제공자로 넘기고 돌아오기, 실제 토큰으로 쓰지 않는 계정의 사용량 조회, 원격 서버에 `script`가 있는지. 아래는 처음 세운 계획.

pi는 `auth.json`에 제공자 id마다 자격 증명을 하나만 둔다(`anthropic`, `openai-codex`). 확장은 `registerProvider("anthropic-b", { api: "anthropic-messages", oauth })`로 다른 id를 만들 수 있지만, 내장 Anthropic 로그인 흐름은 공개되어 있지 않아(`@earendil-works/pi-ai/oauth`는 타입만) 그 흐름을 따로 구현해야 한다. 그래서 pi-oven 쪽에서 계정 묶음으로 하는 쪽이 낫다.

- 계정 묶음: `pi auth login anthropic`으로 로그인할 때마다 그 항목을 pi-oven 폴더에 이름 붙여 저장(권한 600, 응답에 토큰을 내보내지 않음). 바꿀 때는 지금 `auth.json`의 항목을 먼저 그 계정 쪽에 되돌려 저장한 뒤(pi가 토큰을 갱신하면 refresh 토큰이 바뀐다) 고른 계정의 항목을 써 넣는다(`.bak`, 원자적 저장, 모르는 항목 유지).
- 같은 `auth.json`을 모든 세션이 같이 쓰므로 바꾸면 모든 세션에 적용된다(다음 요청부터). 실행 중인 요청이 있으면 끝난 뒤에.
- Zero-Waste와 잇기: 한도에 걸리면 초기화를 기다리는 대신 다른 계정으로 바꾸고 이어서 진행(설정에서 고름). 한도 조회(`usage/claude.rs`)도 계정별로.
- 정보 탭 구독 카드에 계정마다 한 장.
