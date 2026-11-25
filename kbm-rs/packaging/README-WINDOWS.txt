MiniKMB Native @VERSION@ - Windows x64 preview

압축을 새 폴더에 풀고 minikmb-native.exe를 실행합니다. Electron v1.0을 교체하거나
설치 위치를 덮어쓰지 않는 별도 미리보기입니다. v1 기능과 REST/MCP 구현·기본 검증을
마친 뒤 직접 사용하며 개선하는 배포본이며 모든 실제 PC 환경을 검증했다는 뜻은 아닙니다.
이 ZIP은 서명되지 않았습니다. manifest.json과 .sha256은 무결성 기록이며 게시자 서명이 아닙니다.

필요 환경
- Windows 10/11 x64 대상 미리보기입니다. OS별 수용 검증은 진행 중입니다.
  DirectX 12를 지원하는 GPU/드라이버와 Windows Media Foundation이 필요합니다.
- Microsoft Visual C++ v14 x64 Runtime: 빌드 도구와 같거나 더 최신 버전.
  공식 안내: https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist
  런타임/Windows DLL을 이 ZIP에 복사해서 배포하지 않습니다.
- FAT32/exFAT/EXT4 새 이미지 생성/포맷은 기존 WSL mkfs/fsck 도구를 사용합니다.
  설치된 WSL과 해당 도구가 필요하며 물리 PC 디스크를 포맷하지 않습니다.
- Node.js, Electron, Python, Rust, Visual Studio는 앱 실행에 필요하지 않습니다.
  Python/Rust/MSVC는 소스에서 빌드·포장할 때만 필요합니다.

첫 실행
관리 USB로 찾은 MCU와 고정 허브 포트의 MS2130이 자동으로 채널로 묶입니다.
채널 목록에서 하나를 선택한 뒤 제어하기로 들어갑니다. 활성 채널은 전체에서 하나입니다.
키 입력은 실제 타깃에 전달됩니다. 영상에 포커스를 둔 상태에서 제어합니다.
특수 키 전송은 기본 OFF이며 ON일 때 앱 영상의 포커스 범위에서만 전달합니다.
특수 키용 tools/minikmb-keyboard.dll이 앱 내부 스레드에서 동작합니다. 별도 키보드
프로세스는 시작하지 않습니다. 동봉된 tools 폴더를 함께 유지하세요. 런타임 다운로드는 없습니다.
네트워크·REST·MCP는 기본 OFF입니다. 앱에서 명시적으로 켭니다.
REST/MCP API 문서·연동 안내는 서버를 켜지 않고도 읽을 수 있습니다.
MCP ON은 REST도 켜고 REST 단독 ON도 지원합니다. 키는 앱에서 생성·개별 폐기합니다.
큰 파일은 MCP가 알려주는 REST URL로 같은 키를 사용해 업로드합니다.

로컬 CLI
minikmb-cli.exe는 실행 중인 업데이트된 GUI와 Windows Named Pipe로 통신합니다.
REST/MCP를 켜거나 API 키를 만들 필요가 없습니다. 같은 Windows 사용자로 실행합니다.
  minikmb-cli.exe channels
  minikmb-cli.exe commands
  minikmb-cli.exe schema COMMAND
  minikmb-cli.exe call COMMAND --json-file request.json
전체 REST 기능·바이너리 업로드·영상 캡처·이벤트 구독을 지원합니다.
REST-API.md는 전체 HTTP 요청의 인자·필수값·허용 범위·예시를 설명하고 openapi.json은 정확한 스키마입니다.
REST-API.html 파일을 직접 열어 문서 조회와 API 호출을 모두 할 수 있습니다. 별도 웹서버는 필요 없습니다.
앱에서 REST를 켠 뒤 HTML에 실제 http://127.0.0.1:포트 주소와 API 키를 입력하세요.
앱의 /api/v1/reference 페이지는 선택 사항이며 브라우저 보안 설정 변경은 필요 없습니다.
JSON/파일 업로드·이미지 응답·SSE를 지원하며 키를 디스크에 저장하지 않습니다.
CLI.md / CLI.en.md에 사용법을 수록했습니다. AI 에이전트용 SKILL.md도 함께 제공합니다. 활성 채널은 GUI/REST/MCP와 하나를 공유합니다.

펌웨어 기록
상단 또는 왼쪽 펌웨어 메뉴에서 ST-Link/SWD의 STM32, RP2040 Writer의 MS2130 ROM을
기록할 수 있습니다. tools/의 exe와 firmware/의 STM32 FS/HID r63 및 MS2130 ROM을
동봉했습니다. 인터넷 다운로드나 개발 도구 설치는 앱에서 요구하지 않습니다.
ST-Link/Writer USB 드라이버와 실제 배선은 필요합니다. 채널 선택 없이 연결한 프로그래머와 파일을
고르고 기록 시작을 누릅니다. 관리 USB/MCU UID 대조나 별도 확인 창은 필요하지 않습니다. 플래시 용량이 파일보다 작으면 지우기 전에 오류로 중단합니다.
기록 시작 후에는 전원을 유지하고 검증 완료를 기다립니다. MS2130 ROM은 다음
보드 재기동부터 적용됩니다. 앱은 연결만으로 자동 플래싱하지 않습니다.

설정과 롤백
앱 데이터는 %LOCALAPPDATA%\MiniKMB\MiniKMB Native\data 아래에 저장됩니다.
영상/네트워크/포워딩/WOL 설정은 앱 설정 화면에서 기존 Electron JSON을 검토해 가져옵니다.
저장한 설정만으로 기능을 자동 시작하지 않습니다. API 키는 자동 이관하지 않습니다.
두 앱에서 같은 관리 USB를 동시에 열지 마세요. 한 앱을 종료한 뒤 다른 앱을 실행합니다.
롤백은 Rust 앱을 정상 종료하고 기존 Electron을 다시 실행합니다. 펌웨어를 자동 변경하지 않습니다.
앱을 지워도 사용자 설정/키/로그/SD 파일은 이 ZIP의 범위 밖이며 자동 삭제하지 않습니다.

라이선스와 소스
THIRD-PARTY.md / dependencies.json에 고정 버전·선언 라이선스·출처·고지문 위치를 기록합니다.
licenses/firmware/에 STM32·Pico 펌웨어의 원래 고지를 포함합니다.
licenses/에 크레이트·Rust 표준 라이브러리·글꼴/아이콘의 고지와 라이선스를 포함합니다.
MPL-2.0의 option-ext/serialport는 sources/의 변경하지 않은 .crate 원본 소스로 제공합니다.
원본의 MPL 라이선스와 고지는 각 소스에 있으며 source checksum은 Cargo.lock과 같습니다.
probe-rs(MIT OR Apache-2.0)와 SPI helper(GPL-2.0-or-later)는 별도 exe로 호출합니다.
licenses/firmware-tools/와 sources/minikmb-ms2130-helper-source.tar.gz에 고지/대응 소스와
빌드 절차를 포함합니다. 동봉 펌웨어는 사용자 제공 프로젝트 이미지입니다.
MiniKMB의 직접 작성한 앱 코드는 MIT입니다. LICENSE.txt를 참조하세요.
외부 코드·도구·글꼴과 MS2130 ROM에는 각자의 권리와 라이선스가 적용됩니다.
MS2130 ROM의 별도 공개 라이선스는 확인되지 않았으며 MIT 적용 대상이 아닙니다.
개발에는 OpenAI Codex의 GPT 6 Astra와 Sol을 활용했습니다.
소스와 References: https://github.com/jay94ks/minikmb
채널 카드를 드래그해 정렬할 수 있고 REST/MCP에서 클릭·더블클릭·입력 배열을 지원합니다.
프로젝트의 Electron 키보드 보조 프로그램을 이식한 소스와 빌드 스크립트는
sources/minikmb-keyboard.cpp와 sources/build-native-keyboard.py에 포함됩니다.
