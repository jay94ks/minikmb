# MiniKMB 로컬 CLI

[English](CLI.en.md)

`minikmb-cli.exe`는 실행 중인 **업데이트된 Rust GUI**의 REST 처리기를 Windows
Named Pipe로 호출합니다. 별도의 MCU 연결을 열지 않습니다. REST/MCP가 꺼져 있어도
사용할 수 있고 API 키는 필요 없습니다. 기존 REST 인증·키 정책은 유지됩니다.

## 시작

GUI `minikmb-native.exe`를 먼저 실행하고 같은 Windows 사용자로 다음을 실행합니다.
기본 프로필이 같으면 파이프 이름을 지정할 필요가 없습니다.

```powershell
.\minikmb-cli.exe channels
.\minikmb-cli.exe state
.\minikmb-cli.exe activate 0x0123456789ABCDEF01234567
.\minikmb-cli.exe capture --format png --output screen.png
.\minikmb-cli.exe deactivate
```

`activate`의 UID는 `channels`에서 조회한 실제 값으로 바꿉니다. 일반 명령에는 UID를
넣지 않습니다. GUI·REST·MCP·CLI 전체에서 활성 채널은 하나이며 다른 경로의 채널
변경도 즉시 반영됩니다. 활성 채널이 필요한 명령은 채널이 없으면 같은 API 오류를
반환합니다. `capture`는 현재 열린 영상의 신선한 프레임을 저장하므로 먼저
`video_open`을 호출하거나 GUI에서 영상을 엽니다. 임의 카메라로 대체하지 않습니다.

사용자 지정 프로필은 GUI와 CLI 모두 `--profile C:\path\profile`을 지정합니다.
`endpoint`는 그 프로필의 파이프 이름을 출력합니다. `--pipe NAME`으로 명시할 수도
있으며 원격 컴퓨터 파이프와 TCP URL은 받지 않습니다.

## 기본 입력·출력과 JSON 모드

기본 출력은 표와 텍스트입니다. JSON 요청을 작성하지 않고 개별 옵션을 쓸 수 있습니다.

```powershell
.\minikmb-cli.exe hid_key_down --usage 4
.\minikmb-cli.exe hid_key_up --usage 4
.\minikmb-cli.exe hid_key_sequence --keys a --keys b --keys c
.\minikmb-cli.exe hid_click --x 100 --y 200 --button 1
.\minikmb-cli.exe hid_key_down --help
.\minikmb-cli.exe channels --json
```

옵션은 앱의 실제 입력 스키마를 사용해 숫자·문자열·불리언으로 변환합니다.
`input_session`과 `--input-session`처럼 밑줄/하이픈 이름을 사용할 수 있습니다.
불리언만 값을 생략해 `--confirmed`처럼 쓸 수 있고 `--confirmed false`도 가능합니다.
정수는 `--usage 0x04` 형식도 받습니다. 배열은 같은 옵션을 반복하고, 중첩 필드는
`--rules.0.protocol tcp --rules.0.target-ip 192.168.127.100`처럼 점으로 구분합니다.
예약된 CLI 옵션과 같은 필드는 `--arg output=VALUE`처럼 지정합니다.
알 수 없는 필드·중복 스칼라·빠진 값은 명령을 제출하기 전에 오류로 처리합니다.

`--json` / `-j`는 출력 모드를 JSON으로 바꿉니다. 기존 `--json '{...}'`와
`--json '[...]'` 요청 문법도 유지하며, 명확한 입력 옵션은 `--args-json JSON`입니다.
`--json-file`과 `--pretty`도 JSON 출력을 선택합니다. 개별 필드 옵션과 JSON 요청
본문은 함께 쓰지 않습니다. JPEG/PNG는 바이너리, `openapi`는 JSON 문서로 유지합니다.
`--raw`는 가공하지 않은 REST 응답 바이트를 보존합니다.

## 전체 기능과 스키마

```powershell
.\minikmb-cli.exe commands --pretty
.\minikmb-cli.exe schema hid_key_sequence
.\minikmb-cli.exe call hid_key_sequence --json '["a","b","c"]'
.\minikmb-cli.exe call hid_click --json '{"x":100,"y":100,"button":1}'
.\minikmb-cli.exe network_get_status --pretty
.\minikmb-cli.exe call tty_get_status --pretty
.\minikmb-cli.exe call storage_get_status --pretty
.\minikmb-cli.exe openapi --output openapi.json
.\minikmb-cli.exe request GET /api/v1/active/network --pretty
```

실행 중인 GUI가 명령 목록·HTTP 메서드·경로·입력 JSON 스키마를 제공합니다.
CLI가 별도 명령 목록을 복제하지 않으며 `request METHOD PATH`로 모든 REST 경로에
직접 접근할 수도 있습니다. 키보드·픽셀 기준 절대 마우스·클릭/더블클릭·시퀀스,
TTY, NCM/호스트 공유/포워딩/스캔, MSC/광학/SD/작업, WOL, 진단/로그, 펌웨어 API의
기존 인자·사전 조건·결과를 그대로 사용합니다. 정확한 이름과 필드는 `commands`,
`schema`, 앱 내 REST API 문서를 기준으로 합니다. 위 입력 예제는 실제 타깃을
제어하므로 원하는 채널이 활성화되어 있을 때 실행합니다.

GET/DELETE 명령의 JSON 객체는 URL query로 변환합니다. POST/PUT은 JSON 본문을
보냅니다. 배열은 변형 없이 전달합니다. 복잡한 JSON은 UTF-8 파일에 저장해
`--json-file request.json`을 사용합니다. `--json-file -`는 stdin을 읽습니다.

여러 CLI 호출은 같은 로컬 입력 소유자를 사용합니다. DOWN/UP을 나눠 호출할 때는
`input_open`에서 받은 `input_session`을 두 요청에 넣을 수 있습니다. 기존 30초 lease와
heartbeat/close 규칙을 따르며 채널 변경·앱 종료 뒤에는 재사용할 수 없습니다.
단발 키나 연속 키에는 시퀀스 명령을 사용하면 호출 사이 간격을 줄일 수 있습니다.
TCP REST를 끄는 것만으로 로컬 IPC 입력 세션이 해제되지는 않습니다.

## 파일과 스트림

업로드는 해당 `*_upload_prepare` 명령의 스키마에 맞춰 준비한 뒤, 응답의 `url`과
원본 파일을 전달합니다. 로컬 응답 URL은 `ipc:/api/v1/...` 형식입니다.

```powershell
.\minikmb-cli.exe upload 'ipc:/api/v1/active/storage/uploads/RETURNED_ID' 'C:\images\boot.iso'
.\minikmb-cli.exe events
.\minikmb-cli.exe request GET '/api/v1/logs?limit=20' --pretty
```

업로드는 파일 크기를 Content-Length로 넣고 스트리밍하며 Base64로 변환하지 않습니다.
prepare와 upload는 같은 실행 중인 GUI를 사용해야 합니다. 펌웨어 파일 업로드만으로
플래싱하지 않습니다. 기존 start 명령으로 실행합니다. 진행 상황은 기존 job 조회나
이벤트 스트림으로 확인합니다.

JSON 응답은 기본적으로 표·텍스트로 표시합니다. `events`도 읽기 쉬운 이벤트를 출력하며,
`events --json`은 이벤트별 NDJSON, `events --raw`는 원본 SSE를 출력합니다.
바이너리/PNG/JPEG는 stdout에 원본 바이트를 씁니다. 바이너리는
`--output FILE`로 직접 저장하고 `--pretty`를 함께 쓰지 마세요. 파일 출력은 성공 시
임시 파일을 원자 교체하며 실패한 API 응답으로 기존 파일을 덮어쓰지 않습니다.
`events`는 Ctrl+C까지 실행합니다. `--headers`는 응답 상태·헤더를 stderr에
텍스트로 표시하며 JSON 모드에서는 JSON으로 표시합니다. 직접 요청은 `--data-binary FILE`, 추가 헤더는 반복 가능한
`--header NAME:VALUE`를 지원합니다. 프레이밍·인증 헤더는 전송 계층이 관리합니다.

종료 코드는 성공 **0**, 인자/IPC/I/O 오류 **2**, API 실패 응답 **3**입니다.
오류는 stderr로 출력합니다. 연결 후 명령을 자동 재전송하지 않습니다.
`--connect-timeout-ms`는 바쁜 파이프 연결 대기만 제한하며 실행 시간 제한이 아닙니다.
연결이 끊겼을 때 작업이 실행됐을 수 있으므로 상태/작업을 조회한 뒤 판단합니다.

## AI 에이전트

배포 폴더의 `SKILL.md`는 AI 에이전트가 이 CLI를 사용할 때 읽는 안내입니다.
JSON 출력·명령 스키마 조회·한 활성 채널·영상 좌표·입력 세션·파일/작업 확인을
설명합니다. 원본은 `packaging/SKILL.md`이며 패키징 시 실행 파일과 같은 위치에
자동 포함됩니다. 전역 스킬 설치나 앱 설정 변경은 수행하지 않습니다.

## REST API 문서와 실행 콘솔

배포 폴더에 `REST-API.md`, `openapi.json`, `REST-API.html`을 함께 제공합니다.
`REST-API.md`는 66개 HTTP 동작의 메서드·경로·인자·필수 여부·허용값·범위·요청 예시를
정리하며 `openapi.json`은 응답과 공통 타입까지 포함한 정확한 OpenAPI 3.1 계약입니다.
세 파일은 앱의 실제 스키마에서 생성되며 배포 시 자동 갱신됩니다.

`REST-API.html` 파일을 직접 열어 사용할 수 있습니다. 별도 웹서버나 설치는 필요 없습니다.
업데이트된 앱에서 REST를 켜고, HTML에 표시된 실제 `http://127.0.0.1:포트` 주소와
앱에서 만든 API 키를 입력하면 파일 페이지에서 API를 직접 호출합니다.
앱이 제공하는 `/api/v1/reference` 페이지는 선택적으로 사용할 수 있습니다. 요청을 선택하고 인자를
입력한 뒤 **Send request**를 누르면 상태·헤더·JSON/이미지 응답을 확인할 수 있습니다.
바이너리 파일 업로드와 SSE 구독/중지도 지원합니다. 키는 페이지 메모리에만 유지됩니다.
앱은 로컬 파일의 CORS 사전 요청을 처리하므로 브라우저 보안 설정을 바꿀 필요가 없습니다. CLI의 키 없는 IPC와 달리
HTTP 제어 요청에는 Bearer 키가 필요합니다.

## 구현과 검증

`src/ipc/`는 현재 사용자 SID 전용 DACL, 원격 클라이언트 거부, 서버 사용자 확인,
프로필별 주소, 32연결 제한을 적용합니다. 같은 Windows 사용자로 실행하는 로컬 앱은
이 제어 권한을 가집니다. `src/remote/ipc.rs`가 기존 `rest_router`를 감싸며 REST의
요청 검증·한 채널 제약·작업 소유권과 바이너리/SSE 처리기를 공유합니다.

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release --bins
```

빌드 결과는 `target/release/minikmb-cli.exe`와 `minikmb-native.exe`입니다.
테스트는 실제 Windows Named Pipe 전송, 명령 목록, 파일 수신, 이미지 응답, SSE,
재시작·단일 활성 채널·입력 소유권을 확인하며 테스트 중 실제 MCU/HID/SD/NIC/플래시를
조작하지 않습니다. 장시간 운용 시험은 포함하지 않습니다.
