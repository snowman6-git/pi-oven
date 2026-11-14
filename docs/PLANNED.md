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
