# MiniKMB REST API reference

Generated from the application's OpenAPI contract; do not edit the field tables manually.
OpenAPI JSON: `openapi.json`. Interactive tester: `REST-API.html`.
Contract SHA-256 (canonical JSON): `582ec4fe6d388da004de0353fb4548ee8135ee83654251dd911bdbbfca2362f7`.

## Connection and interpretation

Enable REST in the GUI and use its actual loopback address. HTTP controls require `Authorization: Bearer API_KEY` from the app. The offline server URL `/` is relative, not a listening address. Local CLI IPC does not require HTTP or an API key.

GUI, CLI, REST and MCP share one active channel. Only selection takes a UID. Mouse x/y are capture pixels. Submitting a job or completing a USB report does not prove target application/media completion.

Required fields apply within their chosen parent/variant. oneOf means exactly one alternative, anyOf one or more, allOf all constraints. Nested rows and [] describe child/item fields. Unknown fields are rejected where additionalProperties=false. Scalar query constraints are taken from the app's x-input-schema when no standard parameter entry is present.

These are structural schema limits. Endpoint descriptions and current channel/device/resource state add runtime conditions; satisfying a type range alone does not make every value valid. Read errors and status rather than replaying uncertain mutations.

Open `REST-API.html` directly as a local file: no separate web server, installation or CDN is required. Enable REST in MiniKMB, enter its actual http://127.0.0.1:PORT address and paste an app-generated key to send requests directly. The app handles file-origin CORS preflights while requiring Bearer authentication for controls. The served `/api/v1/reference` page remains optional. Keys stay in page memory only.

## Operations

| Operation | Method | Path | Active channel |
|---|---|---|---|
| [api_reference_page](#api_reference_page) | GET | `/api/v1/reference` | see description |
| [app_get_state](#app_get_state) | GET | `/api/v1/state` | see description |
| [channel_activate](#channel_activate) | PUT | `/api/v1/active-channel` | see description |
| [channel_deactivate](#channel_deactivate) | DELETE | `/api/v1/active-channel` | see description |
| [channel_get_active](#channel_get_active) | GET | `/api/v1/active` | yes |
| [channels_list](#channels_list) | GET | `/api/v1/channels` | see description |
| [diagnostics_get](#diagnostics_get) | GET | `/api/v1/active/diagnostics` | yes |
| [events_get_snapshot](#events_get_snapshot) | GET | `/api/v1/events/snapshot` | see description |
| [events_subscribe](#events_subscribe) | GET | `/api/v1/events` | see description |
| [firmware_get_status](#firmware_get_status) | POST | `/api/v1/firmware/status` | see description |
| [firmware_images](#firmware_images) | POST | `/api/v1/firmware/images` | see description |
| [firmware_inspect](#firmware_inspect) | POST | `/api/v1/firmware/inspect` | see description |
| [firmware_job_cancel](#firmware_job_cancel) | POST | `/api/v1/firmware/cancel` | see description |
| [firmware_job_get](#firmware_job_get) | POST | `/api/v1/firmware/job` | see description |
| [firmware_programmers](#firmware_programmers) | POST | `/api/v1/firmware/programmers` | see description |
| [firmware_start](#firmware_start) | POST | `/api/v1/firmware/start` | see description |
| [firmware_upload_bytes](#firmware_upload_bytes) | POST | `/api/v1/firmware/uploads/{upload_id}` | see description |
| [firmware_upload_prepare](#firmware_upload_prepare) | POST | `/api/v1/firmware/upload-prepare` | see description |
| [hid_button_down](#hid_button_down) | POST | `/api/v1/active/hid/buttons/down` | yes |
| [hid_button_up](#hid_button_up) | POST | `/api/v1/active/hid/buttons/up` | yes |
| [hid_chord](#hid_chord) | POST | `/api/v1/active/hid/chord` | yes |
| [hid_click](#hid_click) | POST | `/api/v1/active/hid/click` | yes |
| [hid_double_click](#hid_double_click) | POST | `/api/v1/active/hid/double-click` | yes |
| [hid_get_status](#hid_get_status) | GET | `/api/v1/active/hid` | yes |
| [hid_key_down](#hid_key_down) | POST | `/api/v1/active/hid/keys/down` | yes |
| [hid_key_sequence](#hid_key_sequence) | POST | `/api/v1/active/hid/keys/sequence` | yes |
| [hid_key_up](#hid_key_up) | POST | `/api/v1/active/hid/keys/up` | yes |
| [hid_mouse_move](#hid_mouse_move) | POST | `/api/v1/active/hid/mouse/move` | yes |
| [hid_recover](#hid_recover) | POST | `/api/v1/active/hid/recover` | yes |
| [hid_release](#hid_release) | POST | `/api/v1/active/hid/release` | yes |
| [hid_wheel](#hid_wheel) | POST | `/api/v1/active/hid/wheel` | yes |
| [input_close](#input_close) | POST | `/api/v1/input-sessions/close` | see description |
| [input_heartbeat](#input_heartbeat) | POST | `/api/v1/input-sessions/heartbeat` | yes |
| [input_open](#input_open) | POST | `/api/v1/input-sessions` | yes |
| [logs_configure](#logs_configure) | POST | `/api/v1/logs/configure` | see description |
| [logs_flush](#logs_flush) | POST | `/api/v1/logs/flush` | see description |
| [logs_get_state](#logs_get_state) | GET | `/api/v1/logs` | see description |
| [logs_open_folder](#logs_open_folder) | POST | `/api/v1/logs/open-folder` | see description |
| [network_configure](#network_configure) | POST | `/api/v1/active/network/configure` | yes |
| [network_forward_configure](#network_forward_configure) | POST | `/api/v1/active/network/forward/configure` | yes |
| [network_forward_start](#network_forward_start) | POST | `/api/v1/active/network/forward/start` | yes |
| [network_forward_stop](#network_forward_stop) | POST | `/api/v1/active/network/forward/stop` | yes |
| [network_get_status](#network_get_status) | GET | `/api/v1/active/network` | yes |
| [network_scan_start](#network_scan_start) | POST | `/api/v1/active/network/scan/start` | yes |
| [network_scan_stop](#network_scan_stop) | POST | `/api/v1/active/network/scan/stop` | yes |
| [network_set_mode](#network_set_mode) | POST | `/api/v1/active/network/mode` | yes |
| [openapi_get](#openapi_get) | GET | `/api/v1/openapi.json` | see description |
| [storage_action](#storage_action) | POST | `/api/v1/active/storage/actions` | yes |
| [storage_get_status](#storage_get_status) | GET | `/api/v1/active/storage` | yes |
| [storage_job_cancel](#storage_job_cancel) | POST | `/api/v1/storage/jobs/cancel` | see description |
| [storage_job_get](#storage_job_get) | POST | `/api/v1/storage/jobs/get` | see description |
| [storage_upload_bytes](#storage_upload_bytes) | POST | `/api/v1/active/storage/uploads/{upload_id}` | see description |
| [storage_upload_prepare](#storage_upload_prepare) | POST | `/api/v1/active/storage/uploads/prepare` | yes |
| [tty_close](#tty_close) | POST | `/api/v1/active/tty/close` | yes |
| [tty_get_status](#tty_get_status) | GET | `/api/v1/active/tty` | yes |
| [tty_open](#tty_open) | POST | `/api/v1/active/tty/open` | yes |
| [tty_read](#tty_read) | POST | `/api/v1/active/tty/read` | yes |
| [tty_write](#tty_write) | POST | `/api/v1/active/tty/write` | yes |
| [video_close](#video_close) | DELETE | `/api/v1/active/video` | yes |
| [video_get_status](#video_get_status) | GET | `/api/v1/active/video` | yes |
| [video_open](#video_open) | POST | `/api/v1/active/video` | yes |
| [video_snapshot](#video_snapshot) | GET | `/api/v1/active/video/snapshot` | yes |
| [wake_configure](#wake_configure) | POST | `/api/v1/active/wake/configure` | yes |
| [wake_get_status](#wake_get_status) | GET | `/api/v1/active/wake` | yes |
| [wake_list_adapters](#wake_list_adapters) | GET | `/api/v1/active/wake/adapters` | yes |
| [wake_send](#wake_send) | POST | `/api/v1/active/wake/send` | yes |

## api_reference_page

**GET `/api/v1/reference`** — REST API 테스트 페이지

같은 REST 서버에서 제공하는 자체 포함 HTML입니다. 페이지 열기에는 키가 필요 없지만 실제 API 요청에는 기존 Bearer 인증이 필요합니다. Host/Origin 제한을 유지하고 키를 저장하지 않습니다.

No request arguments are declared.

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | text/html | API 설명·인자·허용값·요청 편집·응답 보기 |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## app_get_state

**GET `/api/v1/state`** — 앱 상태

UI와 같은 채널 목록·연결 상태·활성 채널을 조회합니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## channel_activate

**PUT `/api/v1/active-channel`** — 채널 선택

선택한 UID 하나를 활성화합니다. 여러 대상을 지정할 수 없습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.uid | yes | string | - | 0x + 24 hex digits |

Request example (not executed automatically):

```json
{
  "uid": "0x010101010101010101010101"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## channel_deactivate

**DELETE `/api/v1/active-channel`** — 채널 해제

현재 활성 채널을 해제합니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## channel_get_active

**GET `/api/v1/active`** — 활성 채널

현재 활성 채널 하나를 조회합니다. UID를 입력하지 않습니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## channels_list

**GET `/api/v1/channels`** — 채널 목록

발견된 모든 채널의 상태 스냅샷을 조회합니다. 제어는 실행하지 않습니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## diagnostics_get

**GET `/api/v1/active/diagnostics`** — 채널 진단

활성 채널의 캐시된 MCU STATUS64·신선도·호스트 transport/영상/HID/TTY·NCM·저장소 작업 카운터를 조회합니다. 조회가 기능을 켜거나 키/파일/패킷 본문을 읽지 않습니다. 아직 수신하지 않은 MCU 상태는 null이며 정상으로 추정하지 않습니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## events_get_snapshot

**GET `/api/v1/events/snapshot`** — 이벤트 상태 재조회

이벤트와 같은 캐시 메타데이터 전체를 읽습니다. 장치 기능을 시작하지 않습니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## events_subscribe

**GET `/api/v1/events`** — 캐시 상태 변경 구독

Bearer 인증 SSE. 첫 이벤트는 최신 전체 상태이며 이후 최대 4Hz로 병합합니다. 재접속/Last-Event-ID는 이력을 재생하지 않습니다. state/resync를 처리하고 resync에서는 events_get_snapshot을 재조회합니다. 15초 keepalive; 앱 전체32/키별8 구독. 키 폐기·서버 OFF·종료 시 닫힙니다.

No request arguments are declared.

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | text/event-stream | SSE id=epoch:sequence, event=state/resync, data=Envelope JSON. 영상/TTY/파일 본문과 작업 result 제외. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## firmware_get_status

**POST `/api/v1/firmware/status`** — 펌웨어 진행 상태

앱의 진행 중인 펌웨어 작업 메타데이터를 조회합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |

Request example (not executed automatically):

```json
{}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## firmware_images

**POST `/api/v1/firmware/images`** — 동봉 펌웨어

바로 사용할 동봉 STM32/ROM의 크기와 SHA256을 조회합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |

Request example (not executed automatically):

```json
{}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## firmware_inspect

**POST `/api/v1/firmware/inspect`** — 기록 대상 확인

선택한 ST-Link/Writer의 플래시 용량을 읽습니다. 채널 선택이나 MCU UID 대조는 필요하지 않습니다. 기록하지 않습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.programmer_id | yes | string | - |  |
| body.target | yes | string | enum=["stm32","ms2130"] |  |

Request example (not executed automatically):

```json
{
  "programmer_id": "0483:3748",
  "target": "stm32"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## firmware_job_cancel

**POST `/api/v1/firmware/cancel`** — 기록 전 취소

지우기·기록 전이면 취소합니다. 기록 시작 후에는 can_cancel=false이며 검증 완료까지 기다립니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.job_id | yes | string | - |  |

Request example (not executed automatically):

```json
{
  "job_id": "<JOB_ID>"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## firmware_job_get

**POST `/api/v1/firmware/job`** — 펌웨어 작업 조회

같은 API 키의 원래 채널에 고정된 작업과 검증 결과를 조회합니다. 활성 채널이 없어도 됩니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.job_id | yes | string | - |  |

Request example (not executed automatically):

```json
{
  "job_id": "<JOB_ID>"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## firmware_programmers

**POST `/api/v1/firmware/programmers`** — 프로그래머 검색

stm32의 ST-Link 또는 ms2130의 RP2040 Writer를 검색합니다. 기록하지 않습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.target | yes | string | enum=["stm32","ms2130"] |  |

Request example (not executed automatically):

```json
{
  "target": "stm32"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## firmware_start

**POST `/api/v1/firmware/start`** — 펌웨어 기록

image_id 또는 업로드 source_id 하나와 confirmed=true로 기록합니다. 용량 부족이면 지우기 전에 오류. 채널 자원을 정리하고 기록·검증하며, 기록 시작 후에는 강제 취소하지 않습니다. STM32는 리셋, MS2130은 다음 보드 재기동에 적용됩니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.confirmed | yes | boolean | - |  |
| body.image_id | no | string/null | - |  |
| body.programmer_id | yes | string | - |  |
| body.source_id | no | string/null | - |  |
| body.target | yes | string | enum=["stm32","ms2130"] |  |

Request example (not executed automatically):

```json
{
  "confirmed": true,
  "image_id": "minikmb-fs-hid-r63",
  "programmer_id": "0483:3748",
  "target": "stm32"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## firmware_upload_bytes

**POST `/api/v1/firmware/uploads/{upload_id}`** — 준비한 펌웨어 업로드

firmware_upload_prepare의 단회 URL에 같은 Bearer 키로 파일 바이트를 POST합니다. Content-Length 일치, query/chunked 없음. 응답 source_id로 firmware_start를 별도 호출해야 기록됩니다.

### Path, query and header parameters

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| path.upload_id | yes | string | - |  |
| header.Content-Length | yes | integer | minimum=1; maximum=67108864 |  |

### Request body: `application/octet-stream`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | string | format="binary" |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 수신/이미지 검증 완료. 아직 기록하지 않았습니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |
| default | application/json | 인증·주소·크기·형식 오류 |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## firmware_upload_prepare

**POST `/api/v1/firmware/upload-prepare`** — 펌웨어 업로드 준비

같은 API 키로 원본 파일을 POST할 단회 REST 주소를 발급합니다. 수신만으로 기록하지 않습니다. 반환된 source_id로 firmware_start를 호출하세요.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.name | yes | string | - |  |
| body.size | yes | integer | minimum=0; format="uint64" |  |
| body.target | yes | string | enum=["stm32","ms2130"] |  |

Request example (not executed automatically):

```json
{
  "name": "firmware.bin",
  "size": 64468,
  "target": "ms2130"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## hid_button_down

**POST `/api/v1/active/hid/buttons/down`** — 마우스 버튼 누르기

버튼1~5를 누릅니다. 드래그는 같은 입력 세션에서 이동·UP으로 조합합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.button | yes | integer | minimum=0; maximum=255; format="uint8" |  |
| body.input_session | no | string/null | - |  |

Request example (not executed automatically):

```json
{
  "button": 1
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## hid_button_up

**POST `/api/v1/active/hid/buttons/up`** — 마우스 버튼 떼기

해당 소유자의 마우스 버튼을 해제합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.button | yes | integer | minimum=0; maximum=255; format="uint8" |  |
| body.input_session | no | string/null | - |  |

Request example (not executed automatically):

```json
{
  "button": 1
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## hid_chord

**POST `/api/v1/active/hid/chord`** — 키 조합

usage 목록을 순서대로 누르고 역순으로 뗍니다. USB report 완료와 타깃 앱 처리는 다릅니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.hold_ms | no | integer | minimum=0; format="uint64"; default=30 |  |
| body.input_session | no | string/null | - |  |
| body.usages | yes | array<integer> | - |  |
| body.usages[] | no | integer | minimum=0; maximum=65535; format="uint16" |  |

Request example (not executed automatically):

```json
{
  "hold_ms": 30,
  "usages": [
    224,
    4
  ]
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## hid_click

**POST `/api/v1/active/hid/click`** — 클릭·더블클릭

영상 픽셀 좌표로 이동 후 같은 입력 트랜잭션에서 클릭합니다. count는1~2입니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.button | no | integer | minimum=0; maximum=255; format="uint8"; default=1 |  |
| body.count | no | integer | minimum=0; maximum=255; format="uint8"; default=1 |  |
| body.frame_id | no | string/null | - |  |
| body.hold_ms | no | integer | minimum=0; format="uint64"; default=30 |  |
| body.input_session | no | string/null | - |  |
| body.x | yes | number | format="double" |  |
| body.y | yes | number | format="double" |  |

Request example (not executed automatically):

```json
{
  "button": 1,
  "count": 1,
  "hold_ms": 30,
  "x": 0,
  "y": 0
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## hid_double_click

**POST `/api/v1/active/hid/double-click`** — 더블클릭

활성 캡처 영상 픽셀 x/y에서 DOWN/UP 두 쌍을 한 요청으로 실행합니다. 기존 hid_click/count 동작은 유지합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.button | no | integer | minimum=0; maximum=255; format="uint8"; default=1 |  |
| body.frame_id | no | string/null | - |  |
| body.hold_ms | no | integer | minimum=0; format="uint64"; default=30 |  |
| body.input_session | no | string/null | - |  |
| body.x | yes | number | format="double" |  |
| body.y | yes | number | format="double" |  |

Request example (not executed automatically):

```json
{
  "button": 1,
  "hold_ms": 30,
  "x": 0,
  "y": 0
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## hid_get_status

**GET `/api/v1/active/hid`** — 입력 상태

눌림 개수와 미확인 해제 상태를 조회합니다. 키 내용은 제공하지 않습니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## hid_key_down

**POST `/api/v1/active/hid/keys/down`** — 키 누르기

USB HID usage를 누릅니다. 기본 lease 또는 지정한 입력 세션을 사용합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.input_session | no | string/null | - |  |
| body.usage | yes | integer | minimum=0; maximum=65535; format="uint16" |  |

Request example (not executed automatically):

```json
{
  "usage": 4
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## hid_key_sequence

**POST `/api/v1/active/hid/keys/sequence`** — 키 배열 순차 입력

keys:["a","b","c"]를 a DOWN/UP→b DOWN/UP→c DOWN/UP으로 처리합니다. 1~128개 키 이름/ASCII 문자, US 물리 키 매핑. REST는 문자열 배열 본문도 지원합니다. 한 활성 채널/세션에 고정하고 짝지은 UP을 미리 큐에 넣습니다. 펌웨어 변경 및 실시간 간격 보장은 없습니다. 부분 실패 시 전체를 재시도하지 마세요.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object / array<string> | - |  |
| body / oneOf[1] | yes | object | unknown fields forbidden |  |
| body / oneOf[1].input_session | no | string/null | - |  |
| body / oneOf[1].keys | yes | array<string> | minItems=1; maxItems=128 |  |
| body / oneOf[1].keys[] | no | string | - |  |
| body / oneOf[2] | yes | array<string> | minItems=1; maxItems=128 |  |
| body / oneOf[2][] | no | string | - |  |

Request example (not executed automatically):

```json
{
  "keys": [
    "a",
    "b",
    "c"
  ]
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## hid_key_up

**POST `/api/v1/active/hid/keys/up`** — 키 떼기

해당 소유자가 누른 usage를 해제합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.input_session | no | string/null | - |  |
| body.usage | yes | integer | minimum=0; maximum=65535; format="uint16" |  |

Request example (not executed automatically):

```json
{
  "usage": 4
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## hid_mouse_move

**POST `/api/v1/active/hid/mouse/move`** — 마우스 이동

캡처 영상의 픽셀 x/y를 내부 절대좌표로 변환합니다. frame_id로 오래된 geometry를 거부할 수 있습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.frame_id | no | string/null | - |  |
| body.input_session | no | string/null | - |  |
| body.x | yes | number | format="double" |  |
| body.y | yes | number | format="double" |  |

Request example (not executed automatically):

```json
{
  "x": 0,
  "y": 0
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## hid_recover

**POST `/api/v1/active/hid/recover`** — 입력 해제 복구

활성 MCU에 RELEASE_ALL을 보내 미확인 해제를 복구합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |

Request example (not executed automatically):

```json
{}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## hid_release

**POST `/api/v1/active/hid/release`** — 소유 입력 해제

기본 lease 또는 지정한 입력 세션 소유의 키·버튼만 해제합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.input_session | no | string/null | - |  |

Request example (not executed automatically):

```json
{}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## hid_wheel

**POST `/api/v1/active/hid/wheel`** — 마우스 휠

수직·수평 휠을 각축 -127~127 범위로 전송합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.horizontal | no | integer | minimum=-32768; maximum=32767; format="int16"; default=0 |  |
| body.input_session | no | string/null | - |  |
| body.vertical | yes | integer | minimum=-32768; maximum=32767; format="int16" |  |

Request example (not executed automatically):

```json
{
  "horizontal": 0,
  "vertical": 1
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## input_close

**POST `/api/v1/input-sessions/close`** — 입력 세션 닫기

해당 소유자의 입력을 원래 MCU에서 해제하고 세션을 닫습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.input_session | yes | string | - |  |

Request example (not executed automatically):

```json
{
  "input_session": "<INPUT_SESSION>"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## input_heartbeat

**POST `/api/v1/input-sessions/heartbeat`** — 입력 세션 유지

원래 키·클라이언트의 입력 세션만 갱신합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.input_session | yes | string | - |  |

Request example (not executed automatically):

```json
{
  "input_session": "<INPUT_SESSION>"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## input_open

**POST `/api/v1/input-sessions`** — 입력 세션 열기

현재 활성 채널에 30초 입력 lease를 엽니다. UID 인자는 없습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |

Request example (not executed automatically):

```json
{}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## logs_configure

**POST `/api/v1/logs/configure`** — 로그 설정

enabled/verbose 불리언으로 이번 실행의 기록·상세 메타데이터 기록을 설정하고 파일을 flush합니다. 기본ON/상세OFF, JSONL 4MiB×최대5개, 최근300건·대기1024건입니다. 이 설정으로 장치 기능을 켜지 않습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.enabled | yes | boolean | - |  |
| body.verbose | yes | boolean | - |  |

Request example (not executed automatically):

```json
{
  "enabled": true,
  "verbose": false
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## logs_flush

**POST `/api/v1/logs/flush`** — 대기 로그 기록

빈 인자로 대기 중인 메타데이터를 앱 로그에 기록합니다. 임의 경로를 받지 않으며 파일 기록 실패가 USB 제어를 중지시키지 않습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |

Request example (not executed automatically):

```json
{}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## logs_get_state

**GET `/api/v1/logs`** — 동작 로그 조회

현재 실행의 기록 설정·최근300건·유한 큐/누락·기록 오류를 조회합니다. 앱 전역 정보이며 active가 없어도 조회합니다. 키 입력값·파일 본문·키 원문·호스트 경로는 반환하지 않습니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## logs_open_folder

**POST `/api/v1/logs/open-folder`** — 로그 폴더 열기

앱에 고정된 로그 폴더를 호스트의 기본 파일 관리자로 여는 명시적 요청입니다. 빈 인자만 받으며 임의 경로/URL은 받지 않고 호스트 경로를 응답에 노출하지 않습니다. requested는 OS 요청 수락이며 실제 창 표시를 확인한 결과가 아닙니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |

Request example (not executed automatically):

```json
{}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## network_configure

**POST `/api/v1/active/network/configure`** — 네트워크 설정

NCM OFF 상태에서 사설 .1 서버 IPv4·DHCP 범위·임대 시간을 설정합니다. 현재 PC의 NIC/라우트/방화벽은 변경하지 않습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.lease_seconds | yes | integer | minimum=0; format="uint32" |  |
| body.pool_end | yes | integer | minimum=0; maximum=255; format="uint8" |  |
| body.pool_start | yes | integer | minimum=0; maximum=255; format="uint8" |  |
| body.server_ip | yes | string | - |  |

Request example (not executed automatically):

```json
{
  "lease_seconds": 3600,
  "pool_end": 199,
  "pool_start": 100,
  "server_ip": "192.168.127.1"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## network_forward_configure

**POST `/api/v1/active/network/forward/configure`** — 포워딩 규칙 저장

현재 채널의 중지된 포워딩 규칙을 설정합니다. rules 각 항목은 id/protocol(tcp/udp)/listen_port/target_ip/target_port입니다. 로컬 수신은127.0.0.1로 고정되며 타깃은 NCM /24입니다. 빈 배열은 목록을 지우며 저장만으로 포트를 열지 않습니다. UID별 설정 파일에 원자 저장하며 앱 재실행 시 자동으로 시작하지 않습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.rules | yes | array<object> | - |  |
| body.rules[] | no | object | unknown fields forbidden |  |
| body.rules[].id | yes | string | - |  |
| body.rules[].listen_port | yes | integer | minimum=0; maximum=65535; format="uint16" |  |
| body.rules[].protocol | yes | string | enum=["tcp","udp"] |  |
| body.rules[].target_ip | yes | string | - |  |
| body.rules[].target_port | yes | integer | minimum=0; maximum=65535; format="uint16" |  |

Request example (not executed automatically):

```json
{
  "rules": [
    {
      "id": "example",
      "listen_port": 18080,
      "protocol": "tcp",
      "target_ip": "192.168.127.100",
      "target_port": 80
    }
  ]
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## network_forward_start

**POST `/api/v1/active/network/forward/start`** — 포워딩 시작

rules1..32개로 새 loopback TCP/UDP 수신 포트를 열어 현재 NCM 타깃으로 연결합니다. UID/인터페이스를 받지 않으며 호스트 공유와 합쳐 최대32연결입니다. 유효한 시작 요청은 기존 포워딩을 중지한 뒤 준비하고 실패 시 새 포트를 모두 닫습니다. 성공한 자원은 창/클라이언트 종료 후에도 유지합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.rules | yes | array<object> | - |  |
| body.rules[] | no | object | unknown fields forbidden |  |
| body.rules[].id | yes | string | - |  |
| body.rules[].listen_port | yes | integer | minimum=0; maximum=65535; format="uint16" |  |
| body.rules[].protocol | yes | string | enum=["tcp","udp"] |  |
| body.rules[].target_ip | yes | string | - |  |
| body.rules[].target_port | yes | integer | minimum=0; maximum=65535; format="uint16" |  |

Request example (not executed automatically):

```json
{
  "rules": [
    {
      "id": "example",
      "listen_port": 18080,
      "protocol": "tcp",
      "target_ip": "192.168.127.100",
      "target_port": 80
    }
  ]
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## network_forward_stop

**POST `/api/v1/active/network/forward/stop`** — 포워딩 중지

현재 채널의 새 loopback 수신 포트와 프록시 연결을 닫고 대기 전송을 취소합니다. 소켓 정리 후 응답하며 NCM/호스트 공유는 유지합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |

Request example (not executed automatically):

```json
{}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## network_get_status

**GET `/api/v1/active/network`** — 네트워크 상태

현재 채널의 NCM 모드·연결 상태·통계·DHCP 임대·정리 필요 여부를 조회합니다. UID나 query 인자는 없습니다. 완료된 네트워크 자원은 제어 창/클라이언트 종료 후에도 유지됩니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## network_scan_start

**POST `/api/v1/active/network/scan/start`** — 타깃 포트 스캔

NCM이 연결된 현재 채널의 /24 타깃 하나를 명시적으로 SYN 스캔합니다. target/start/end/timeout_ms를 지정하고 network_get_status.scan에서 진행·열림/닫힘/응답 없음을 확인합니다. 최대8개 동시 프로브이며 키/클라이언트 종료·채널 전환·NCM OFF에서 취소합니다. PC 네트워크는 스캔하지 않습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.end | yes | integer | minimum=0; maximum=65535; format="uint16" |  |
| body.start | yes | integer | minimum=0; maximum=65535; format="uint16" |  |
| body.target | yes | string | - |  |
| body.timeout_ms | yes | integer | minimum=0; format="uint32" |  |

Request example (not executed automatically):

```json
{
  "end": 80,
  "start": 80,
  "target": "192.168.127.100",
  "timeout_ms": 1000
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## network_scan_stop

**POST `/api/v1/active/network/scan/stop`** — 포트 스캔 중지

현재 채널의 스캔을 중지하고 대기 프로브를 취소합니다. 이미 전송된 패킷은 되돌리지 않으며 남은 포트를 응답 없음으로 합산하지 않습니다. NCM 링크와 호스트 공유는 유지합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |

Request example (not executed automatically):

```json
{}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## network_set_mode

**POST `/api/v1/active/network/mode`** — NCM 모드 변경

off/emulation/host 중 하나를 지정합니다. host는 현재 PC 경로로 TCP/UDP·DNS를 중계합니다. USB 재연결 전에 원래 HID를 해제하며 수락된 작업은 연결 종료 후에도 정리까지 진행합니다. 결과 불명 시 원래 채널에서 off로 복구하세요.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.mode | yes | string | enum=["off","emulation","host"] |  |

Request example (not executed automatically):

```json
{
  "mode": "off"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## openapi_get

**GET `/api/v1/openapi.json`** — 현재 API 계약



No request arguments are declared.

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | OpenAPI 3.1 JSON 문서 |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## storage_action

**POST `/api/v1/active/storage/actions`** — SD 저장소 작업

SD 목록·메타데이터·광학/raw 매체·섹터 I/O·이미지 생성/포맷·전체 카드 포맷・폴더 공유를 관리합니다. disk_create는64..32768MiB, disk_format은 기존 용량을 보존합니다. card_format은FAT32/exFAT·수락 후 취소 불가이며 결과 불명 시 card_format_status로 확인합니다. folder_share는 앱에서 선택한 source_id로 새 RO 스냅샷을 만들어 타깃에 연결하며, folder_forget은 선택 목록에서만 해제합니다. 호스트 경로·UID는 받지 않습니다. confirmed:true와 expected_card_uuid/expected_size/expected_lease로 고정하고 job_id로 완료를 확인하세요. 섹터 쓰기는512바이트입니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object / object / object / object / object / object / object / object / object / object / object / object / object / object / object / object / object / object / object | - |  |
| body / oneOf[1] | yes | object | unknown fields forbidden |  |
| body / oneOf[1].action | yes | string | const="refresh" |  |
| body / oneOf[2] | yes | object | unknown fields forbidden |  |
| body / oneOf[2].action | yes | string | const="card_info" |  |
| body / oneOf[3] | yes | object | unknown fields forbidden |  |
| body / oneOf[3].action | yes | string | const="card_format_status" |  |
| body / oneOf[4] | yes | object | unknown fields forbidden |  |
| body / oneOf[4].action | yes | string | const="card_format" |  |
| body / oneOf[4].confirmed | yes | boolean | - |  |
| body / oneOf[4].expected_card_uuid | no | string/null | - |  |
| body / oneOf[4].format | yes | integer | minimum=0; maximum=255; format="uint8" |  |
| body / oneOf[5] | yes | object | unknown fields forbidden |  |
| body / oneOf[5].action | yes | string | const="name_get" |  |
| body / oneOf[6] | yes | object | unknown fields forbidden |  |
| body / oneOf[6].action | yes | string | const="name_set" |  |
| body / oneOf[6].name | yes | string | - |  |
| body / oneOf[7] | yes | object | unknown fields forbidden |  |
| body / oneOf[7].action | yes | string | const="user_read" |  |
| body / oneOf[8] | yes | object | unknown fields forbidden |  |
| body / oneOf[8].action | yes | string | const="user_write" |  |
| body / oneOf[8].bytes | yes | array<integer> | maxItems=4096 |  |
| body / oneOf[8].bytes[] | no | integer | minimum=0; maximum=255; format="uint8" |  |
| body / oneOf[9] | yes | object | unknown fields forbidden |  |
| body / oneOf[9].action | yes | string | const="optical_set" |  |
| body / oneOf[9].enabled | yes | boolean | - |  |
| body / oneOf[10] | yes | object | unknown fields forbidden |  |
| body / oneOf[10].action | yes | string | const="optical_mount" |  |
| body / oneOf[10].expected_card_uuid | no | string/null | - |  |
| body / oneOf[10].id | yes | integer | minimum=0; format="uint32" |  |
| body / oneOf[10].mounted | yes | boolean | - |  |
| body / oneOf[11] | yes | object | unknown fields forbidden |  |
| body / oneOf[11].action | yes | string | const="disk_open" |  |
| body / oneOf[11].expected_card_uuid | no | string/null | - |  |
| body / oneOf[11].id | yes | integer | minimum=0; format="uint32" |  |
| body / oneOf[11].path | yes | string | enum=["direct","target","management"] |  |
| body / oneOf[11].writable | yes | boolean | - |  |
| body / oneOf[12] | yes | object | unknown fields forbidden |  |
| body / oneOf[12].action | yes | string | const="disk_close" |  |
| body / oneOf[12].id | yes | integer | minimum=0; format="uint32" |  |
| body / oneOf[13] | yes | object | unknown fields forbidden |  |
| body / oneOf[13].action | yes | string | const="disk_sync" |  |
| body / oneOf[13].id | yes | integer | minimum=0; format="uint32" |  |
| body / oneOf[14] | yes | object | unknown fields forbidden |  |
| body / oneOf[14].action | yes | string | const="disk_create" |  |
| body / oneOf[14].confirmed | yes | boolean | - |  |
| body / oneOf[14].expected_card_uuid | no | string/null | - |  |
| body / oneOf[14].format | yes | integer | minimum=0; maximum=255; format="uint8" |  |
| body / oneOf[14].size_mib | yes | integer | minimum=0; format="uint32" |  |
| body / oneOf[15] | yes | object | unknown fields forbidden |  |
| body / oneOf[15].action | yes | string | const="folder_share" |  |
| body / oneOf[15].confirmed | yes | boolean | - |  |
| body / oneOf[15].expected_card_uuid | no | string/null | - |  |
| body / oneOf[15].source_id | yes | string | - |  |
| body / oneOf[16] | yes | object | unknown fields forbidden |  |
| body / oneOf[16].action | yes | string | const="folder_forget" |  |
| body / oneOf[16].source_id | yes | string | - |  |
| body / oneOf[17] | yes | object | unknown fields forbidden |  |
| body / oneOf[17].action | yes | string | const="disk_format" |  |
| body / oneOf[17].confirmed | yes | boolean | - |  |
| body / oneOf[17].expected_card_uuid | no | string/null | - |  |
| body / oneOf[17].expected_size | no | integer/null | minimum=0; format="uint64" |  |
| body / oneOf[17].format | yes | integer | minimum=0; maximum=255; format="uint8" |  |
| body / oneOf[17].id | yes | integer | minimum=0; format="uint32" |  |
| body / oneOf[18] | yes | object | unknown fields forbidden |  |
| body / oneOf[18].action | yes | string | const="disk_read" |  |
| body / oneOf[18].expected_card_uuid | no | string/null | - |  |
| body / oneOf[18].id | yes | integer | minimum=0; format="uint32" |  |
| body / oneOf[18].lba | yes | integer | minimum=0; format="uint32" |  |
| body / oneOf[19] | yes | object | unknown fields forbidden |  |
| body / oneOf[19].action | yes | string | const="disk_write" |  |
| body / oneOf[19].bytes | yes | array<integer> | minItems=512; maxItems=512 |  |
| body / oneOf[19].bytes[] | no | integer | minimum=0; maximum=255; format="uint8" |  |
| body / oneOf[19].confirmed | yes | boolean | - |  |
| body / oneOf[19].expected_card_uuid | no | string/null | - |  |
| body / oneOf[19].expected_lease | no | integer/null | minimum=0; format="uint32" |  |
| body / oneOf[19].id | yes | integer | minimum=0; format="uint32" |  |
| body / oneOf[19].lba | yes | integer | minimum=0; format="uint32" |  |

Request example (not executed automatically):

```json
{
  "action": "refresh"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## storage_get_status

**GET `/api/v1/active/storage`** — SD 저장소 상태

공통 스냅샷·작업·소유 매체·정리 오류·card_format 보류·앱에서 선택한 folder_sources를 반환합니다. 폴더에는 source_id/이름/만료 시각만 포함되며 경로는 공개하지 않습니다. stale이면 목록을 갱신하세요. active가 없으면 앱 상태의 storage_resources/storage_cleanup_error/storage_card_format을 조회하세요.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## storage_job_cancel

**POST `/api/v1/storage/jobs/cancel`** — SD 작업 취소

같은 키의 원래 MCU 작업을 취소합니다. 취소 요청 수락과 MCU 종료 확인은 다르므로 결과를 다시 조회하세요.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.job_id | yes | string | - |  |

Request example (not executed automatically):

```json
{
  "job_id": "<JOB_ID>"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## storage_job_get

**POST `/api/v1/storage/jobs/get`** — SD 작업 결과

같은 API 키가 시작한 job의 진행률·완료·검증 결과를 조회합니다. 원래 채널 정보가 응답에 포함됩니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.job_id | yes | string | - |  |

Request example (not executed automatically):

```json
{
  "job_id": "<JOB_ID>"
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## storage_upload_bytes

**POST `/api/v1/active/storage/uploads/{upload_id}`** — 준비한 이미지 파일 스트리밍

storage_upload_prepare가 반환한 url에 같은 API 키로 파일 원문을 한 번 POST합니다. Content-Length는 content_length와 같아야 합니다. chunked/query/inline MCP 본문은 지원하지 않습니다. 수신 완료 뒤 job_id로 SD 검증 완료를 조회하세요.

### Path, query and header parameters

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| path.upload_id | yes | string | - |  |
| header.Content-Length | yes | integer | minimum=1 |  |

### Request body: `application/octet-stream`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | string | format="binary" |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 앱의 SD 작업 수락. 완료는 job_id로 조회합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |
| default | application/json | 인증·단회 주소·크기/형식·활성 연결 또는 한도 오류 |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## storage_upload_prepare

**POST `/api/v1/active/storage/uploads/prepare`** — 이미지 업로드 주소

광학 또는 디스크 파일의 이름·크기·확인 여부로10분 유효한 단회 REST 업로드 URL을 발급합니다. 같은 키의 Bearer 인증과 application/octet-stream 파일 본문을 사용하세요. MCP에 파일을 넣지 마세요. 수신 후 job_id로 SD 검증 완료를 확인합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object / object | - |  |
| body / oneOf[1] | yes | object | unknown fields forbidden |  |
| body / oneOf[1].confirmed | yes | boolean | - |  |
| body / oneOf[1].expected_card_uuid | no | string/null | - |  |
| body / oneOf[1].kind | yes | string | const="optical" |  |
| body / oneOf[1].name | yes | string | - |  |
| body / oneOf[1].size | yes | integer | minimum=0; format="uint64" |  |
| body / oneOf[2] | yes | object | unknown fields forbidden |  |
| body / oneOf[2].confirmed | yes | boolean | - |  |
| body / oneOf[2].expected_card_uuid | no | string/null | - |  |
| body / oneOf[2].format | yes | integer | minimum=0; maximum=255; format="uint8" |  |
| body / oneOf[2].kind | yes | string | const="disk" |  |
| body / oneOf[2].replace_id | no | integer/null | minimum=0; format="uint32" |  |
| body / oneOf[2].size | yes | integer | minimum=0; format="uint64" |  |

Request example (not executed automatically):

```json
{
  "confirmed": true,
  "kind": "optical",
  "name": "boot.iso",
  "size": 81920
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## tty_close

**POST `/api/v1/active/tty/close`** — 시리얼 수신 중지

추가 수신 credit을 중지합니다. 이미 허용한 최대32KiB는 폐기될 수 있습니다. 타깃 CDC 자체를 닫지는 않습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |

Request example (not executed automatically):

```json
{}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## tty_get_status

**GET `/api/v1/active/tty`** — 시리얼 상태

타깃 line coding, TX/RX 큐·폐기 카운터와 앱 수신 상태를 조회합니다. 관리 USB baud를 변경하지 않습니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## tty_open

**POST `/api/v1/active/tty/open`** — 시리얼 수신 시작

현재 타깃 CDC의 수신을 시작합니다. 64KiB 유한 버퍼와 누적 credit을 사용합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |

Request example (not executed automatically):

```json
{}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## tty_read

**POST `/api/v1/active/tty/read`** — 시리얼 수신 읽기

최대4096바이트와 cursor/stream_id를 반환합니다. 버퍼 덮어쓰기는 skipped로 알리며 다른 클라이언트의 cursor를 소비하지 않습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.cursor | no | integer/null | minimum=0; format="uint64" |  |
| body.max_bytes | no | integer | minimum=1; maximum=4096; format="uint"; default=4096 |  |
| body.stream_id | no | string/null | - |  |

Request example (not executed automatically):

```json
{
  "max_bytes": 4096
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## tty_write

**POST `/api/v1/active/tty/write`** — 시리얼 송신

1~65536바이트를 순서대로256바이트씩 전송합니다. complete와 accepted를 확인하세요. 오류/불확실 수락은 재시도하지 않습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.bytes | yes | array<integer> | minItems=1; maxItems=65536 |  |
| body.bytes[] | no | integer | minimum=0; maximum=255; format="uint8" |  |

Request example (not executed automatically):

```json
{
  "bytes": [
    72,
    101,
    108,
    108,
    111,
    13,
    10
  ]
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## video_close

**DELETE `/api/v1/active/video`** — 영상 닫기

활성 채널의 캡처를 닫습니다. 다른 스트림으로 대체하지 않습니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## video_get_status

**GET `/api/v1/active/video`** — 영상 상태

UI와 동일한 캡처 상태·협상된 크기·주사율·프레임 시각을 조회합니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## video_open

**POST `/api/v1/active/video`** — 영상 열기

활성 채널의 캡처를 엽니다. 빈 객체는 자동 모드, 수동은 width/height/fps입니다. 첫 프레임 준비는 영상 상태로 확인합니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.fps | no | number | format="double"; default=0.0 |  |
| body.height | no | integer | minimum=0; format="uint32"; default=0 |  |
| body.width | no | integer | minimum=0; format="uint32"; default=0 |  |

Request example (not executed automatically):

```json
{}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## video_snapshot

**GET `/api/v1/active/video/snapshot`** — 현재 장면 캡처

활성 HDMI의 최근 1초 이내 프레임을 JPEG 또는 PNG로 반환합니다. format은 jpeg/png, 기본 jpeg입니다.

### Path, query and header parameters

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query.format | no | string | enum=["jpeg","png"]; default="jpeg" |  |

### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |
| query.format | no | string | enum=["jpeg","png"]; default="jpeg" |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | image/jpeg, image/png | 최근1초 이내의 실제 캡처 이미지. MCP는 image content와 FrameInfo structuredContent를 함께 반환합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## wake_configure

**POST `/api/v1/active/wake/configure`** — WOL 설정 저장

enabled/mac/local_address를 현재 활성 채널의 UID에 저장합니다. MAC은 유니캐스트 주소로 정규화하고 실제 발신 때 현재 LAN 주소를 다시 확인합니다. 저장은 패킷을 발신하지 않습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |
| body.enabled | yes | boolean | - |  |
| body.local_address | yes | string | - |  |
| body.mac | yes | string | - |  |

Request example (not executed automatically):

```json
{
  "enabled": false,
  "local_address": "",
  "mac": ""
}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## wake_get_status

**GET `/api/v1/active/wake`** — PC 깨우기 설정

현재 활성 채널의 저장된 WOL 설정과 USB wake/System Control 지원 여부를 조회합니다. 조회나 enabled 저장은 패킷을 발신하지 않습니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## wake_list_adapters

**GET `/api/v1/active/wake/adapters`** — WOL LAN 어댑터

현재 PC에서 연결된 브로드캐스트 가능 IPv4 LAN 주소와 broadcast를 조회합니다. USB/NCM이나 NIC·route 설정을 변경하지 않습니다.


### Query arguments

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| query | no | object | unknown fields forbidden |  |

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.

## wake_send

**POST `/api/v1/active/wake/send`** — WOL 발신

빈 인자로 현재 채널에 저장된 WOL 설정을 사용해 명시적 발신합니다. 선택한 PC LAN IPv4에 bind하고 해당 subnet의 broadcast UDP9로 102B Magic Packet을 한 번 보냅니다. code128은 발신/기상 미확인,129는 비활성,130은 미지원입니다. UID/호스트 경로/임의 목적지·포트는 받지 않습니다.


### Request body: `application/json`

| Field | Required | Type | Allowed values / constraints | Description |
|---|---|---|---|---|
| body | yes | object | unknown fields forbidden |  |

Request example (not executed automatically):

```json
{}
```

### Responses

| HTTP | Content type | Description |
|---|---|---|
| 200 | application/json | 성공 응답. USB/작업 수락과 실제 타깃 동작/SD 완료는 구분합니다. |
| 400 | application/json | 인자·JSON/Content-Type·장치 요청 오류. 메시지보다 code/status/uncertain을 확인하세요. |
| 401 | application/json | API 키가 없거나 폐기됐습니다. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 403 | application/json | 다른 키의 세션/작업 또는 Host/Origin 거부. 요청 ID가 있는 JSON 오류를 반환합니다. |
| 404 | application/json | 채널·작업·단회 업로드 주소를 찾을 수 없습니다. |
| 405 | application/json | 지원하지 않는 HTTP 메서드입니다. Allow 헤더를 확인하세요. |
| 409 | application/json | 활성 채널·원래 세션·소유권 또는 해제/매체 상태가 충돌합니다. |
| 413 | application/json | 허용한 요청 본문 크기를 넘었습니다. |
| 415 | application/json | Content-Type이 올바르지 않습니다. |
| 429 | application/json | 요청 큐·입력 세션/상태·이벤트 구독 한도입니다. 이미 수락한 DOWN/쓰기의 재시도와 구분하세요. |
| 501 | application/json | 현재 앱/펌웨어가 지원하지 않습니다. |
| 503 | application/json | 필요한 서버가 꺼져 있거나 종료 중입니다. |

Exact response objects, headers and component schemas are in `openapi.json` under this operation.
