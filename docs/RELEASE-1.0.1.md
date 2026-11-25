# MiniKMB v1.0.1 배포 안내

[한국어](RELEASE-1.0.1.md) · [English](RELEASE-1.0.1.en.md)

이번 릴리스는 Windows Rust 앱에 로컬 IPC CLI와 AI 에이전트 안내, 전체 REST API 문서와 독립 HTML 테스트 콘솔을 추가합니다. Native 앱의 내부 Cargo 버전은 0.1.0 preview이며 릴리스 묶음 버전은 v1.0.1입니다. 펌웨어와 보드 프로토콜은 변경하지 않았습니다.

## 다운로드와 실행

`MiniKMB-Native-v1.0.1-windows-x64.zip`을 모두 풀고 `minikmb-native.exe`를 실행합니다. 기존 앱은 먼저 종료하고 같은 MCU를 두 앱에서 동시에 열지 마세요. 도구·펌웨어·ROM·라이선스·대응 소스가 동봉되어 있으며 앱 실행에 Python·Node·별도 웹서버가 필요하지 않습니다. 코드 서명되지 않은 Windows x64 미리보기입니다.

`minikmb-cli.exe`는 같은 Windows 사용자와 프로필로 실행 중인 새 앱의 Named Pipe에 연결합니다. TCP REST/MCP와 API 키 없이 사용할 수 있습니다. 기본 입력·출력은 개별 옵션과 표·텍스트이고 `--json`으로 JSON을 선택합니다. 구버전 GUI는 이 CLI의 IPC를 제공하지 않으므로 함께 업데이트합니다.

## 동봉 문서

- `SKILL.md`: AI 에이전트용 CLI 사용 지침.
- `CLI.md`, `CLI.en.md`: 한영 사용자 안내.
- `REST-API.md`: 66개 HTTP 동작의 경로·인자·필수값·허용 범위·예시.
- `openapi.json`: 정확한 OpenAPI 3.1 요청·응답 계약.
- `REST-API.html`: 파일을 직접 열어 쓰는 테스트 콘솔. 별도 웹서버나 CDN 없이 동작합니다.

HTML에서 실제 API를 호출하려면 앱에서 키를 만들고 REST를 켠 뒤, HTML에 앱의 실제 `http://127.0.0.1:포트` 주소와 키를 입력합니다. 브라우저 보안 설정 변경은 필요 없습니다. API 키는 페이지 메모리에만 유지되며 파일·URL·브라우저 저장소에 저장하지 않습니다. `/api/v1/reference`의 앱 제공 페이지도 선택적으로 사용할 수 있습니다. 앱 REST 서버가 꺼져 있으면 문서는 읽을 수 있지만 실제 API 호출은 연결되지 않습니다.

GUI·CLI·REST·MCP는 활성 채널 하나를 공유합니다. 일반 제어 명령에는 UID를 전달하지 않으며 선택에만 UID를 사용합니다. 키보드·마우스·저장소·네트워크 제어는 활성 타깃에 실제로 적용됩니다.

## 검증과 라이선스

Rust 303개, 패키징·문서 생성 12개 테스트와 Clippy를 통과했습니다. 하드웨어 환경 시험 5개와 장시간 운용은 제외했습니다. 파일 출처의 인증·사전 요청·JSON/바이너리 업로드·키 폐기·호스트/출처 거부를 검증했습니다. 새 하드웨어 실기나 HS 복구를 주장하지 않으며 타깃 USB는 기존 FS 구성입니다.

`SHA256SUMS`, `release-manifest.json`과 ZIP 내부 `manifest.json`으로 파일을 검증할 수 있습니다. 직접 작성한 코드는 MIT이며 외부 도구·라이브러리·글꼴·ROM의 원래 조건은 `NOTICE.md`, `THIRD-PARTY.md`, `licenses/`, `sources/`를 따릅니다. MS2130 ROM은 MIT 적용 대상이 아닙니다. OpenAI Codex의 GPT 6 Astra와 Sol을 활용해 개발했습니다.
