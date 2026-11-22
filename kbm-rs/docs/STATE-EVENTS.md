# 상태 이벤트

2026-11-21 · UI/REST/MCP가 공유하는 캐시 메타데이터입니다.

REST `GET /api/v1/events`는 다른 API와 같은 Bearer 인증 SSE입니다. query/UID를
받지 않으며 활성 채널이 없어도 구독할 수 있습니다. 구독으로 장치 검색·영상 열기·
HID·TTY 수신·SD 작업·NCM을 시작하지 않습니다. 서버·키는 앱에서 명시적으로 켭니다.

```powershell
curl.exe -N -H 'Authorization: Bearer <API_KEY>' http://127.0.0.1:18180/api/v1/events
```

실제로 표시된 bound 주소를 사용합니다. 서버 OFF에서는 위 주소가 예시입니다.

## 데이터와 재조회

SSE `id`는 앱 수명의 `epoch:sequence`, `event`는 `state` 또는 `resync`이고
`data`는 JSON입니다. JSON의 `epoch`, `sequence`, `observed_at`(Unix ms)은 전송
봉투입니다. `state`의 `snapshot`에는 다음 내용을 넣습니다.

- UI와 같은 앱 스냅샷: 채널 목록·연결·단일 활성 UID/세대·영상 상태·앱 소유 자원.
- 현재 REST/MCP 서버 상태와 구독 수/한도.
- 같은 UID/세대의 캐시된 HID 상태/눌림 개수, TTY 설정/카운터, SD 메타데이터,
  현재 작업의 ID·종류·단계·진행량·속도/ETA·오류/정리 오류.

영상 이미지·키 usage/문자·TTY 바이트·파일/디스크 본문·API 키 원문·`job.result`는
전달하지 않습니다. 작업 결과가 필요하면 원래 `job_id`로 `storage_job_get`을
호출합니다. `active_metadata.binding`은 그 메타데이터의 원래 UID/세대입니다.
수집 중 채널이 바뀌면 메타데이터를 생략하고 다음 표본에서 새 세대를 보냅니다.

연결 직후 최신 전체 상태를 보냅니다. 이후 변경은 공유 수집기 하나가 약250ms
간격으로 비교하고 최신값 하나로 병합합니다. 클라이언트마다 변경 이력을 쌓지
않으므로 sequence가 건너뛸 수 있으며 짧은 작업의 모든 중간 단계를 보장하지
않습니다. 완료 판단은 작업 조회를 사용합니다. 15초 keepalive는 데이터 이벤트가
아닌 SSE 주석입니다. 신규 구독에 필요한 초기 상태는 주기와 별개로 갱신합니다.

재접속은 다시 최신 전체 상태로 시작합니다. `Last-Event-ID`로 과거 상태를
재생하지 않습니다. sequence/epoch는 손실 감지용이며 명령 재시도나 인증 토큰이
아닙니다. UID 배열이나 다중 활성 제어를 제공하지 않습니다.

직렬화한 보고서가1MiB를 넘으면 목록을 자르지 않고 `resync`를 보냅니다.
`reason`은 `payload_limit`, `snapshot_url`은 `/api/v1/events/snapshot`입니다.
이 주소 또는 MCP `events_get_snapshot`/`minikmb://events/snapshot`을 읽으면
전체 보고서를 받습니다. 큰 상태가 계속되는 동안 같은 경고를 반복하지 않으므로
클라이언트가 큰 목록 갱신을 계속 확인하려면 제한된 간격으로 재조회합니다.
이벤트 JSON은 보고서 한도에 작은 봉투 크기가 추가됩니다.

## MCP

`resources.subscribe=true`를 알립니다. 정적 리소스 목록에 대한 `listChanged`는
알리지 않습니다. 다음 고정 URI만 변경 구독할 수 있습니다.

- `minikmb://events/snapshot`
- `minikmb://app/state`
- `minikmb://channels`
- `minikmb://active/channel`
- `minikmb://active/diagnostics`

알림은 URI만 담은 `notifications/resources/updated`입니다. 세부 변경 목록이나
원문 본문이 아니므로 알림 뒤 `resources/read`를 호출합니다. 더 넓은 앱 메타데이터가
변하면 같은 URI에 알림이 올 수 있습니다. 현재 활성 채널이 없을 때도 구독은
유지되며 active 리소스를 읽으면 기존 `NO_ACTIVE_CHANNEL` 오류를 받습니다.
참조용 OpenAPI/guide와 임의 `file://`·UID/query URI는 구독하지 않습니다.

2025-11-25 연결은 `resources/subscribe`와 `resources/unsubscribe`를 사용합니다.
같은 세션/URI의 중복 구독은 하나이며 해제는 해당 세션에만 적용됩니다.
2026-07-28은 initialize/세션 없이 요청별 버전 협상으로 연결하며,
`MCP-Protocol-Version: 2026-07-28`, `Mcp-Method: subscriptions/listen` 헤더와
`params._meta`의 `io.modelcontextprotocol/protocolVersion`,
`io.modelcontextprotocol/clientCapabilities`를 사용합니다.
`subscriptions/listen`의 `notifications.resourceSubscriptions`로
지원하는 URI 부분집합을 받고 SSE 연결을 닫아 요청을 취소합니다. 요청하지 않은 알림 범주와
임의 URI는 허용하지 않습니다. 현재 SDK의 버전별 API를 함께 구현합니다.
구형 리소스 계약은 [공식 MCP 명세](https://modelcontextprotocol.io/specification/2025-11-25/server/resources),
현재 버전은 [공식 Streamable HTTP 명세](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)와
고정된 `rmcp 3.11.0` 소스의 `SubscriptionContext`/필터 계약을 따릅니다.

## 수명과 한도

REST와 MCP를 합쳐 전체32개, API 키별8개 구독입니다. 초과 REST 요청은
`429 EVENT_SUBSCRIPTION_LIMIT`, MCP는 같은 code의 구조화된 오류를 반환합니다.
구형 MCP는 URI별 하나, 현재 MCP listen은 URI 묶음별 하나로 셉니다. 앱의 서버·키
화면에 같은 구독 수를 표시합니다. MCP 전송 계층도 SDK의 유한 채널을 사용하며,
알림 전송이1초 안에 진행되지 않으면 해당 구독을 끝냅니다.

REST 연결 종료·구독 해제/취소·키 폐기·MCP 세션 종료·서버 OFF·앱 종료는 구독을
정리합니다. 유휴 수집기는 표본을 만들지 않고 깨어날 때까지 대기합니다. 수집기는
앱을 weak 참조하며 종료 시 join하므로 구독이 앱 전체를 계속 붙잡지 않습니다.
키 폐기 후 본문을 더 전달하지 않는 공통 인증 계층도 그대로 적용됩니다.

## 검증 범위

시험은 모의 MCU와 실제 axum/rmcp 라우터를 사용합니다. 상태 계약·최신값 병합·
채널 세대·상한/RAII·폐기/종료·큰 상태의 resync와 원문 결과 제외를 확인합니다.
이 상태 구독 시험에는 실제 HID·SD 쓰기·NIC 변경을 사용하지 않습니다. 사용자 지정으로
장시간 재접속/부하 시험은 제외했습니다. 나머지 실제 클라이언트/DPI 등 실사용 확인은
사용 인계 (development reference omitted)에 따라 사용자가 직접 진행한 뒤 수정을 요청합니다.
