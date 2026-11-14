# 예정 사항

## Claude 구독 한도 직접 조회

ChatGPT처럼 채팅 없이 Claude(Pro/Max) 구독의 남은 양을 묻는다. 아직 실제 계정으로 확인하지 못했다(이 PC의 pi에 Claude 구독 로그인이 없음).

**알려진 방법** (Claude Code의 `/usage`가 쓰는 것으로 알려짐, 확인 필요)
- `GET https://api.anthropic.com/api/oauth/usage`
- 헤더: `Authorization: Bearer <구독 OAuth 토큰>`, `anthropic-beta: oauth-2025-04-20`
- 응답에 5시간(`five_hour`)·7일(`seven_day`) 창의 사용률(`utilization`)과 초기화 시각(`resets_at`)이 있을 것으로 본다. 모델별(예: Opus) 창이 따로 올 수 있다.

**만들 때**
1. Claude 구독으로 로그인한 pi에서 `pi auth print-bearer-token --provider anthropic`으로 토큰을 받아 위 주소를 한 번 불러 응답 형태를 확인한다(토큰은 출력하지 않는다).
2. `src/usage.rs`에 `anthropic` 조회를 추가한다. 토큰은 지금처럼 curl 표준입력으로만 넘기고 저장하지 않는다. 결과는 `limits.rs`의 같은 저장소(`provider: "anthropic"`)로 넣는다.
3. 응답 헤더(`anthropic-ratelimit-unified-*`) 이름도 실제 응답으로 확인해 `ext/oven-limits.ts`를 맞춘다.
4. Claude의 한도 오류 문구를 확인해 `resume.rs`의 `is_usage_limit`과 시간 읽기(`minutes_in`)를 맞춘다. 주간 한도의 초기화 규칙(정해진 요일·시각)은 계산하지 않고 응답의 시각을 그대로 쓴다.
5. 설정의 "ChatGPT에서 직접 확인"을 제공자 공통 이름으로 바꾼다.

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
