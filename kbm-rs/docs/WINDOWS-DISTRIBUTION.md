# Windows 미리보기 배포

2026-11-21 · Electron v1.0을 유지하는 별도 x64 ZIP입니다. 기본 구현·회귀를 마친 별도 배포이며 기본
앱을 자동 교체하지 않습니다. 현재 빌드 버전은0.1.0이며 코드 서명 없는 preview입니다.

## 빌드와 구성

고정 Rust1.95.0/MSVC와 현재 Cargo.lock을 사용합니다. 사용하지 않던 utoipa를
제거했으며 OpenAPI3.1 생성은 기존 공통 Serialize/Deserialize 스키마를 사용합니다.
검증 후 다음 명령으로 최적화 빌드와 배포 구성을 재생성합니다.

```powershell
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
python -m unittest discover -s scripts -p test_release_support.py -v
python firmware-tools/package-tools.py --frwr-root <로컬-frwr-원본>
python scripts/package-windows.py
```

개발 단계에서 firmware-tools의 fetch/build/stage-images로 고정 입력을 준비한 뒤
package-tools로 로컬 exe/이미지/소스를 구성합니다. 앱 런타임 다운로드는 없습니다.
도구는 Python3.11 이상과 고정 Rust/MSVC가 필요합니다. 앱 실행에는 Python·Node·
Electron·Rust/Visual Studio가 필요하지 않습니다. 패키징은 매번 `cargo build
--release --locked --offline`을 실행해 오래된 실행 파일을 그대로 포장하지 않습니다.
필요한 Cargo 원본 패키지가 캐시에 있어야 하며 패키징 중 임의 네트워크 요청이나
외부 발행·설치·관리자 설정 변경을 하지 않습니다. Git에 `dist/target/__pycache__`
산출물을 넣지 않습니다.

`dist/MiniKMB-Native-<version>-windows-x64-preview-<exe SHA 앞12자리>.zip`과
`.sha256`을 만듭니다. ZIP에는 다음 파일만 넣습니다.

- 최적화한 `minikmb-native.exe`와 시작·설정 이관·롤백·선행 조건을 안내하는 README.
- 실제 Cargo.lock과 Windows 의존성 목록/선언 라이선스·소스 체크섬·고지문 위치.
- 크레이트·Rust 표준 라이브러리·번들 글꼴/아이콘의 라이선스·저작권 고지.
- 변경하지 않은 option-ext0.2.0/serialport4.10.1의 MPL-2.0 원본 `.crate` 소스.
- 동봉 probe-rs·MS2130 helper exe, STM32 r61 ELF·MS2130 ROM과 해시 목록.
- 도구 고지문과 GPL helper의 전체 대응 소스/패치/빌드 절차.
- 직접/지연 PE DLL import 기록, rustc 버전, UI 자산 목록, 파일별 SHA256/크기 manifest.

설정·API 키·로그·사용자 파일·장치 캡처·Electron/Node·컴파일 PDB는 넣지 않습니다.
임의 작업 디렉터리 전체를 복사하는 방식이 아니며 고정된 공개 소스/필요한 문서와
실행 파일로만 구성합니다. ZIP은 새 임시 파일로 만든 뒤 전체 검증을 통과하면
프로젝트 `dist`의 정해진 이름으로 교체합니다.

## 라이선스와 버전 근거

[고정 Windows 의존성 목록](../licenses/windows-dependencies.json)은 Cargo의
Windows 필터 그래프에서 normal/build 경로로 닿는275개 레지스트리 패키지를
포함하며 dev-only 패키지는 제외합니다. 일부 빌드 도구까지 보수적으로 포함하므로
275개 라이브러리가 모두 실행 파일에 링크됐다는 뜻은 아닙니다. 형식은
[Cargo metadata 공식 문서](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html)를 따릅니다.

캐시 `.crate` 전체 SHA256을 Cargo.lock과 대조한 뒤 원본의 README/라이선스/
NOTICE/저작권과 egui 기본 글꼴의 추가 고지문을 수집합니다. 배포 크레이트에서
생략된 egui·AccessKit·clipboard-win·profiling의 문서는 `.cargo_vcs_info.json`과
같은 원본 커밋의 고정 파일로 보완합니다. hexf-parse의 CC0 전문도 포함하며,
추가 원본12개의 URL·commit·길이·해시를 `licenses/upstream.json`에 고정했습니다.
MPL 두 패키지는 로컬 소스가 원본과 같은지 검사하고 정확한 원본 아카이브를 제공합니다.
소스 제공 방식의 기준은 [Mozilla MPL 안내](https://www.mozilla.org/en-US/MPL/2.0/FAQ/#q8-i-want-to-distribute-outside-my-organization-executable-programs-or-libraries-that-i-have-compiled-from-someone-elses-unchanged-mpl-licensed-source-code-either-standalone-or-part-of-a-larger-work-what-do-i-have-to-do)입니다.

`--notices-only`는 실행 파일을 빌드하지 않고 같은 목록/고지문 인덱스를 갱신합니다.
원본/해시/고지가 빠지거나 MPL 소스가 로컬에서 바뀌면 생성을 중단합니다. 선언된
SPDX 식은 AND/OR와 추가 글꼴 조건까지 그대로 보존합니다. 자동 수집을 전체 법적
검토 승인이라고 표시하거나 MiniKMB 자체 코드에 새 라이선스를 임의 부여하지 않습니다.

## 실행 환경과 지원 범위

대상은 Windows10/11 x64, DX12 GPU/드라이버와 Media Foundation입니다. 현재
개발 PC Windows NT10.0 build26300에서 압축 해제한 릴리스의 실제 egui 렌더와
종료 코드0을 확인했습니다. 이 한 대의 결과를 모든 OS/GPU 지원 인증으로 넓히지
않습니다. Windows10·다른 GPU/깨끗한 PC의 설치/기능 수용 검증은 남아 있습니다.

PE에서 x64 GUI subsystem2와 직접 DLL32개/지연 import0개를 확인했습니다.
Windows API/Media Foundation DLL과 `vcruntime140.dll`을 사용합니다. 외부 앱
DLL이나 Node/Electron 런타임은 직접 import에 없습니다. DX12 등 동적 로딩까지
이 목록만으로 증명하지 않으며 실제 렌더링을 따로 확인합니다.

MSVC Runtime은 빌드 도구와 같거나 더 최신인 x64 v14가 필요합니다. Windows/
Microsoft DLL을 사용자 시스템에서 복사해 ZIP에 넣지 않으며 필요 시
[공식 Visual C++ Runtime 설치 안내](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist)를 사용합니다.
이미지 생성/포맷은 기존 WSL mkfs/fsck 도구를 필요로 하며, 앱이 물리 PC 디스크를
포맷하거나 NIC·route·방화벽·ICS 설정을 임의 변경하지 않습니다.
