# 앱 내 REST 문서·MCP 연동 안내


## 앱에서 읽기

왼쪽 **REST API 문서**에서 명령 이름·HTTP 메서드·경로·한글 설명을 검색하고 기능별로
좁힐 수 있습니다. 선택한 명령의 요청/응답/오류/사용 흐름 탭은 같은 공통 명령 목록과
Rust 스키마를 사용합니다. 서버 OFF에서도 읽을 수 있으며 예제 복사는 요청을 실행하지
않습니다. 예제의 키는 항상 `<API_KEY>`입니다.

PowerShell7용 요청 예제에는 현재 REST bound 주소를 넣습니다. OFF에서는 기본
loopback 주소가 예시임을 표시합니다. **OpenAPI 복사** 또는 **JSON 내보내기**로
동일한 문서를 얻습니다. 내보내기는 Windows 저장 대화상자의 파일 선택·덮어쓰기
확인 뒤 해당 JSON만 기록합니다. 원격 API에 호스트 파일 경로를 받는 기능은 아닙니다.

MCP 연동 안내는 연결/키/서버, 채널·영상·픽셀 마우스, 키보드·드래그/해제, 시리얼,
파일/작업, SD/디스크/광학/폴더, NCM/포워딩/스캔, WOL/로그/오류 해결을 포함합니다.
도구와 리소스를 볼 수 있고 현재 주소의 연결 설정/전체 안내를 복사합니다.
클라이언트별 설정 파일 형식을 하나로 가정하거나 OAuth를 지원한다고 표시하지 않습니다.

현재 두 서버는 `127.0.0.1`에만 바인딩합니다. UI 기본 REST18180/MCP18181이며 실제
bound 포트를 문서에 사용합니다. 다른 PC에서 이 loopback 주소에 직접 접속할 수 있다는
의미는 아닙니다. MCP ON은 REST ON을 포함하고 REST 단독 ON은 가능합니다.
일반 제어는 UID 없이 단일 활성 채널을 사용하고 선택만 UID 하나를 받습니다.

## 스키마·바이너리·작업

입력은 실제 Deserialize 타입의 JSON Schema, 응답은 Serialize 계약의 JSON Schema로
생성합니다. 모든 공개 명령에 응답 스키마를 지정하고 MCP tools/list의 outputSchema에도
같은 문서를 제공합니다. URI/장치 상태 같은 실제 사용자 값이나 API 키를 문서 생성에
넣지 않습니다. `api_docs.rs`의 응답 envelope와 원래 직렬화 타입을 함께 사용합니다.

OpenAPI3.1 문서에 여러 스키마를 넣을 때 각 문서의 `#/$defs/...`를 명령별 component로
이동하고 모든 참조를 바꿉니다. 서로 다른 Status 타입을 덮어쓰거나 문서 루트에서
해결되지 않는 참조를 남기지 않습니다. REST `GET /api/v1/openapi.json`과 앱 내 복사/
내보내기는 같은 생성기를 사용합니다. 형식 기준은 [OpenAPI3.1 공식 명세](https://spec.openapis.org/oas/v3.1.0.html)입니다.

`video_snapshot`은 REST에서 JPEG/PNG 바이너리와 `x-minikmb-*` 메타데이터 헤더를,
MCP에서는 image content와 FrameInfo structuredContent를 반환합니다. OpenAPI는
이 차이와 query format·no-store를 구분합니다. 요청 좌표는 영상 픽셀이며 HID
0..32767을 외부 좌표로 사용하지 않습니다.

저장소 action은 job_id와 진행/종료/error/cleanup_error/can_cancel을 반환합니다.
job.result는 action/phase에 따라 달라지는 JSON 값이며 단일 고정 결과 구조라고
표시하지 않습니다. 전체 카드 포맷의 수락 뒤 취소 제한과 수신 완료/SD 완료의 차이를
별도로 안내합니다. MCU 수락·HID 보고서 완료와 타깃 OS 동작도 구분합니다.

큰 파일은 `storage_upload_prepare`의 `url`, `content_length`, `content_type`을 사용해
같은 Bearer 키로 원문을 POST합니다. MIME은 application/octet-stream이며 정확한
Content-Length가 필요합니다. 주소는10분 유효/단회이고 진행 중 idle30초를 구분합니다.
MCP 본문에 파일을 넣거나, 다른 키·query·chunked 전송을 사용하는 예제를 만들지
않습니다. POST 응답 뒤 `storage_job_get`으로 SD 검증 완료를 확인합니다.

## MCP 리소스

| URI | 반환 내용 |
|---|---|
| `minikmb://events/snapshot` | 상태 이벤트와 같은 캐시 메타데이터 전체 |
| `minikmb://app/state` | UI와 같은 앱·전체 채널·활성/서버 상태 |
| `minikmb://channels` | 현재 수동 채널 목록·허브 묶음·UID |
| `minikmb://active/channel` | 현재 활성 하나; 없으면 NO_ACTIVE_CHANNEL |
| `minikmb://active/diagnostics` | 캐시된 MCU/호스트 상태·신선도·카운터 |
| `minikmb://reference/openapi` | 현재 REST 주소의 OpenAPI JSON |
| `minikmb://reference/guide` | 같은 앱 안내·전체 도구/리소스의 Markdown |

resources capability와 상태 리소스 구독을 알립니다. URI는 정확히 고정된 목록이며
UID·query·호스트 경로를 붙이지 않습니다. 읽기는 같은 인증/세션을 사용하고 활성 채널을
바꾸거나 HID·영상 열기·SD/네트워크 기능을 시작하지 않습니다. API 키 폐기는 이미 연
세션에도 적용됩니다. 변경 알림 뒤 현재 상태를 재조회합니다. 정적 목록의 listChanged는 광고하지 않습니다. SDK에 TTL0/private를 설정하고, 이전 프로토콜에서는 해당 필드가
생략될 수 있으므로 클라이언트가 현재 상태를 필요할 때 다시 읽도록 안내합니다.
리소스 형식과 기능 광고는 [MCP 공식 리소스 명세](https://modelcontextprotocol.io/specification/2025-11-25/server/resources)를 따릅니다.

## 확인한 범위와 남은 일

신규 시험은59개 요청 예제/응답 스키마 생성, OpenAPI 참조/바이너리 계약, 실제 공통
서비스의 모의 응답, 검색/안내 전체 도구 대응, 실제 axum/rmcp 인증 경유의 리소스/
outputSchema·현재값/없는 active/잘못된 URI/키 폐기, 서버 OFF의 egui 문서/안내 렌더를
검증합니다. 실제 HID·SD·NIC·사용자 키 입력을 사용하지 않습니다. JSON Schema
검증기는 테스트 전용 jsonschema0.61.2이며 HTTP/파일 참조 기능을 끕니다.

REST 업무 오류와 인증/Host/Origin·JSON/Content-Type·본문 한도·경로/메서드 거부는
같은 JSON 오류 봉투를 사용합니다. `request_id`는 HTTP `x-minikmb-request-id`와 같고,
성공 응답에도 해당 헤더와 no-store를 제공합니다. JSON-RPC 도구 오류는 같은 봉투를
structuredContent, 리소스 업무 오류는 error.data에 넣습니다. SDK 자체의 JSON-RPC
프로토콜 오류 형식은 유지하며 HTTP 요청 ID 헤더로 대조합니다.

```json
{
  "code": "HID_RELEASE_UNCONFIRMED",
  "message": "원래 MCU의 입력 해제 완료를 확인하지 못했습니다.",
  "uncertain": true,
  "request_id": "<server-generated-request-id>",
  "channel_uid": "0x010101010101010101010101",
  "generation": 7
}
```

실제 작업이 고정한 UID/세대가 있으면 접수 시 관찰한 active보다 우선합니다. 입력 큐/
오래된 lease·영상/픽셀 검증·정리 오류가 새 채널로 바뀌지 않으며, 아직 대상을 고정하기
전 오류는 접수 당시 관찰한 활성 채널 또는 null을 사용합니다. 소유권 확인을 마친
작업의 거부에는 원래 job_id를 붙일 수 있습니다. 잘못된/다른 키의 job ID를 그대로
반사하지 않습니다. 내부 오류의 origin은 직렬화/스키마/로그에 노출하지 않습니다.

인증 실패에는 channel_uid/generation이 null이고 job_id를 넣지 않습니다. 401은
고정 Bearer challenge,405는 Allow 헤더를 유지합니다. 일반 파싱 진단·입력 본문/
사용자가 보낸 요청 ID·Host/Origin 값은 반사하지 않습니다. 입력/세션 한도는429,
취소 불가/소유권·세션·전환 상태 충돌은409로 구분합니다.

키가 처리 도중 폐기되면 새401 응답은 uncertain=true로 이미 수락한 동작의 결과를
확인하도록 알립니다. 새 요청의 잘못된 키 거부와 구분합니다. 폐기 전에 만든 성공/
오류·장기 GET/SSE 본문도 토큰을 재검사해 끊습니다. 이미 전달 중인 응답은 완전한
JSON 대신 연결 중단으로 관찰될 수 있으므로 DOWN/쓰기를 자동 재시도하지 않습니다.

성공한 job 조회 안의 error/cleanup_error는 내부 장치 오류 형식을 유지합니다.
그 경우 작업의 uid/generation/job_id와 조회 HTTP 헤더를 사용합니다. 성공 응답을
오류 봉투로 바꾸거나 과거 job에 현재 요청 ID를 저장하지 않습니다.

요청 ID/원래 채널·작업 문맥과 키 폐기 일관화, REST SSE와 MCP 상태 리소스
구독을 구현했습니다. 상세 계약은 [상태 이벤트](STATE-EVENTS.md)를 따릅니다. 실제 저장 대화상자의 선택/취소/덮어쓰기, 문서/안내의 DPI·
키보드 탐색·클립보드 및 실제 클라이언트 연동 수용 시험도 남아 있습니다.

## 펌웨어 명령 추가

공통59명령에는 firmware_images/programmers/inspect/upload_prepare/start/job_get/
job_cancel/get_status가 포함됩니다. 요청·응답·MCP outputSchema와 앱 내 안내도 같은
레지스트리에서 제공합니다. 원본 파일은 준비한 REST 바이너리 URL에 POST해 source_id를
받고 별도 confirmed=true start를 호출합니다. 용량 부족은 지우기 전에 오류이며
기록 전 취소와 기록 수락 후 검증 대기를 구분합니다. 상세는 [FIRMWARE.md](FIRMWARE.md)입니다.
