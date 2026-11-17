# 예정 사항

## Claude 구독 한도 직접 조회 — 만듦 (2026-11-16)

`src/usage/claude.rs`(`GET https://api.anthropic.com/api/oauth/usage`, `anthropic-beta: oauth-2025-04-20`, `five_hour`·`seven_day`의 `utilization`·`resets_at`). 남은 것:
- 응답의 `limits[]`(모델별 주간 한도, 예: 특정 모델 전용)는 아직 읽지 않는다.
- 한도가 차면 Claude는 "usage limit" 문구 없이 `HTTP 429 rate_limit_error`("would exceed your account's rate limit")로 답한다. 짧은 속도 제한과 같은 모양이라, 바로 사용량을 다시 물어 95% 이상인 창이 있을 때만 한도 도달로 본다(`resume::Limit::Maybe`). 실제 한도 도달 응답의 헤더(`anthropic-ratelimit-unified-status: rejected` 등)로 바로 알 수 있는지는 아직 확인하지 않았다.

## Claude 캐시 수명 1시간으로 (`PI_CACHE_RETENTION=long`)

pi는 Claude 요청의 캐시를 기본 5분(`short`)으로 쓴다. Claude Code는 같은 구독에서 1시간짜리만 쓴다(최근 세션 4개: 캐시 쓰기 100%가 1시간, 새로 쓰는 비율 1~3%). pi 세션은 13%였다. 5분이 지나 다시 말을 걸면 대화 전체를 다시 쓰고(읽기의 약 10배 비용), 그만큼 구독 한도가 빨리 닳는다.

- pi는 환경변수 `PI_CACHE_RETENTION=long`이면 1시간을 쓴다(`pi-ai`의 `resolveCacheRetention`). 설정 파일 항목은 없다.
- 할 일: pi-oven이 pi를 띄울 때 이 값을 넘기는 설정(설정 → 자동 진행 또는 모델, 기본 켬?)을 둔다. 이미 환경에 값이 있으면 그대로 둔다. 1시간 캐시는 쓸 때 5분보다 비싸므로(2배 대 1.25배) 짧게 끝나는 세션이 많으면 오히려 손해일 수 있다 — 기본값은 실제 사용량으로 비교해 보고 정한다.
- 확인: 켠 세션의 응답별 `cacheWrite`가 쉬었다 돌아온 뒤에도 작게 유지되는지, 같은 작업에서 구독 한도 소모가 줄어드는지.

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
