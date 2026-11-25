# 테스트와 공개판 검증

[한국어](TESTING.md) · [English](TESTING.en.md)

일반 테스트는 호스트에서 실행하며 실제 HID 입력, SD 포맷, 보드 리셋이나 호스트 NIC 변경을 수행하지 않습니다. 실기 테스트는 명시적으로 실행하며 기본적으로 제외됩니다. 모의 테스트와 실제 신호 무결성·타깃 OS 검증을 구분합니다.

## Rust 데스크톱 앱

`kbm-rs/`에서 고정된 Windows 도구체인을 사용합니다.

```powershell
$env:CARGO_INCREMENTAL = '0'
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
python -m unittest discover -s scripts -p test_release_support.py
```

v1.0.0 최초 공개 시점의 체크아웃에서 라이브러리 268개와 바이너리/UI 15개, 총 283개 테스트가 통과했습니다. 하드웨어 테스트 5개는 제외했습니다. 패키징 테스트 8개도 통과했습니다. 앱 ZIP의 manifest와 직접 사용 확인을 마친 실행 파일의 SHA-256을 대조했습니다.

## 로컬 IPC CLI

최신 CLI 소스는 `cargo test --all-targets --locked`로 라이브러리283개, GUI15개,
CLI 별도 프로세스1개, 예제2개 등 **301개**가 통과했고 명시적 환경 시험5개는
제외했습니다. Clippy all-targets 경고0개, 패키징 시험9개와 API 문서 생성 시험3개도 통과했습니다. 배포 문서 시험은 SKILL.md·한영 CLI 안내·REST 문서·OpenAPI·HTML의 포함·해시 검증 및 스킬 원본 누락 시 실패를 확인합니다.
기본 표·텍스트와 JSON 호환, 스키마 기반 개별 인자·배열·중첩 필드, 실제 Windows
Named Pipe의 JSON/바이너리/이미지/SSE, 입력 소유권·채널 변경·SD 작업 흐름을
테스트 백엔드로 검증합니다. 실제 HID·SD 쓰기·NIC 변경·플래싱은 수행하지 않습니다.
[CLI 사용법](../kbm-rs/docs/CLI.md)에 입력·출력 계약을 정리했습니다.
REST 콘솔은 장치 탐색 없는 임시 프로필 서버에서 실제 브라우저로 GET 상태 조회와 JSON POST의 HTTP 200 응답, 업로드 주소 전환을 확인했습니다. 잘못된 Host/Origin 거부와 제어 API 인증도 회귀 테스트합니다.
기존 v1.0.0 ZIP의 사용 확인과 최신 소스의 자동 검증은 별도 기록입니다.

## RP2040 펌웨어

저장소 루트의 WSL 셸에서 `PICO_SDK_PATH`를 지정합니다.

```sh
cmake -S common/tests -B build/host-tests -DCMAKE_BUILD_TYPE=Debug -DPICO_SDK_PATH="$PICO_SDK_PATH"
cmake --build build/host-tests --parallel
ctest --test-dir build/host-tests --output-on-failure
```

프로토콜·큐·라우팅·DMA 커서·깨우기·HID·부팅 세션을 다루는 호스트 테스트 9개가 통과했습니다. 공개 소스로 master와 slave를 다시 빌드하고 UF2를 생성했습니다. 이 절차는 보드를 플래싱하지 않습니다.

## STM32 펌웨어

`tools/build.py`로 소스 93개의 ARM 빌드를 확인했습니다. WSL/Linux에서는 다음 호스트 회귀 테스트를 실행할 수 있습니다.

```sh
python3 firmware/stm32h7/tools/test_target_hid_usb.py
python3 firmware/stm32h7/tools/test_management_hid.py
```

FS/HS 어댑터 구성, 키보드·마우스의 독립 처리, 관리 HID wire 테스트가 통과했습니다. HS 테스트는 엔드포인트 모의 환경이므로 실제 보드의 HS 송신 문제가 해결됐다는 뜻은 아닙니다.

추가 저장소·광학 매체·프로토콜 C/Python 테스트는 `firmware/stm32h7/tools/`에 있습니다. 실행 전 스크립트를 확인하십시오. `probe_*`와 업로드 도구는 명시적으로 장치를 진단할 때 사용합니다.

## Electron

`kbm/`에서 `npm run typecheck`와 `npm run build`를 실행합니다. 공개 체크아웃에서 두 절차와 `work/*-tests.cjs` 회귀 스크립트 17개의 통과를 확인했습니다. 해당 스크립트는 모의 전송 계층을 사용하며 `kbm/node_modules`와 저장소 구조를 요구합니다.

## 실제 사용 확인의 범위

Release 실행 파일을 구동하고 채널 카드 드래그 정렬을 직접 확인한 뒤 포장했습니다. 개발 중 다른 기능의 실기도 수행했으며 남은 제약과 설계자의 HS 원인 판단은 [알려진 제한](KNOWN-ISSUES.md)에 정리했습니다. 장시간 운용, 두 MCU 동시 NCM 검증, 모든 OS·케이블·허브 조합의 인증을 주장하지 않습니다.

이번 문서 언어 분리와 설계 회고 추가는 문서 변경입니다. 이를 새로운 하드웨어 시험이나 HS 복구 결과로 기록하지 않습니다.
