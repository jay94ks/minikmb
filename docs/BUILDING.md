# MiniKMB 빌드 안내

[한국어](BUILDING.md) · [English](BUILDING.en.md)

빌드 도구는 보드를 자동으로 플래싱하지 않습니다. 실행 설정과 API 키는 저장소 밖에 보관하십시오. 공개 소스에는 캐시·장치 로그·개인 개발 기록을 포함하지 않습니다.

## Windows 네이티브 앱

Windows x64, MSVC/Windows SDK, Python 3.11 이상과 `kbm-rs/rust-toolchain.toml`에 고정된 Rust 1.95.0을 사용합니다. `kbm-rs/`에서 실행합니다.

```powershell
$env:CARGO_INCREMENTAL = '0'
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
python scripts/ui-assets.py
python scripts/build-native-keyboard.py
cargo build --release --locked
```

`ui-assets.py`는 명시적으로 다운로드를 요청하지 않으면 동봉한 글꼴과 라이선스를 오프라인으로 검증합니다. Rust 테스트는 이웃 Electron 디렉터리의 키보드 매핑 소스도 사용하므로 저장소 구조를 유지하십시오. 특수 키 전송에는 실행 파일과 함께 배치한 키보드 DLL이 필요합니다.

### 도구·펌웨어·ZIP

배포 ZIP에는 실행에 필요한 프로그래밍 도구와 펌웨어가 이미 포함되어 있습니다. 앱 실행 중 도구를 다운로드하지 않습니다. 개발자는 `firmware-tools/fetch-probe-rs.py`로 고정 버전의 오픈소스 도구를 받거나, 배포 ZIP에서 검증된 도구를 가져올 수 있습니다. `tools.lock.json`과 `images.lock.json`이 정확한 입력 파일을 지정합니다.

GPL helper의 전체 대응 소스와 재빌드 안내는 `vendor/flashrom/minikmb-ms2130-helper-source.tar.gz`에 있습니다. 소스에서 도구를 준비하는 순서는 `stage-images.py` → `package-tools.py` → `scripts/package-windows.py`입니다. `package-tools.py --frwr-root <writer>`는 대응 소스에 설명된 Writer/flashrom 빌드 입력을 요구합니다.

이미 공개한 도구를 재사용하려면 배포 ZIP의 `tools/`, `firmware/`, `licenses/`, `sources/`를 `kbm-rs/target/firmware-tools/package/` 아래에 원래 구조로 풀고 manifest를 보존하십시오. 개발용 실행에는 같은 ZIP의 `tools/`와 `firmware/`를 `target/release/`의 앱 실행 파일 옆에도 복사합니다.

`python scripts/package-windows.py`로 포장한 뒤 `python scripts/package-windows.py --verify <zip>`으로 확인합니다. 포장기는 도구·이미지·고지의 해시를 검사하고 MIT 및 GPL/MPL 고지·대응 소스를 포함합니다. 새 실행 파일은 직접 시험 구동한 뒤 ZIP으로 배포합니다. 현재 공개한 ZIP에는 사용 확인을 마친 실행 파일을 사용했습니다.

## Electron v1.0

`kbm/`에서 `npm ci`, `npm ci --prefix src-electron`, `npm run typecheck`, `npm run build`를 차례로 실행합니다. Node 버전은 package.json 조건을 따릅니다. 네이티브 키보드 도구 빌드에는 Windows/MSVC가 필요하며 prebuild 스크립트는 `../work/build-minikmb-native.cjs`입니다.

두 lock 파일과 원본 패키지 아카이브를 보존했습니다. Electron 플랫폼 런타임은 일반 패키지 설치 절차로 설치됩니다. 라이브러리 소스 아카이브가 Windows 도구체인이나 Electron 런타임을 대신하지는 않습니다.

## RP2040 master/slave

WSL에서 CMake·Ninja·GNU Arm Embedded를 사용합니다. [vendor 안내](../vendor/README.md)에 따라 Pico SDK 2.2.0을 풀거나 동일 리비전의 SDK를 사용합니다. 저장소 루트에서 실행합니다.

```sh
export PICO_SDK_PATH=/absolute/path/to/pico-sdk
cmake -S master -B master/build-wsl -G Ninja -DCMAKE_BUILD_TYPE=Release -DPICO_SDK_PATH="$PICO_SDK_PATH"
cmake --build master/build-wsl
cmake -S slave/kbm-slave -B slave/kbm-slave/build-wsl -G Ninja -DCMAKE_BUILD_TYPE=Release -DPICO_SDK_PATH="$PICO_SDK_PATH"
cmake --build slave/kbm-slave/build-wsl
```

picotool이 있으면 Pico SDK가 UF2를 생성할 수 있습니다. `-DPICO_NO_PICOTOOL=1`은 picotool 다운로드 없이 ELF/BIN을 빌드합니다. 필요한 경우 `picotool uf2 convert --quiet <elf> <uf2> --family rp2040`으로 변환합니다. 보드에 기록하지 않습니다. PowerShell의 `build-wsl.ps1` 래퍼에서는 `PICO_SDK_PATH`를 Windows 환경 경로로 지정합니다.

## STM32H7

`firmware/stm32h7/`를 기존 STM32CubeIDE 프로젝트로 가져와 GNU Tools for STM32 13.3.rel1과 Debug 구성을 사용합니다. `.ioc`, 링커 스크립트, 필요한 HAL·미들웨어 소스가 포함되어 있습니다.

다음과 같이 독립 빌드할 수도 있습니다.

```sh
python firmware/stm32h7/tools/build.py --toolchain-bin /path/to/arm-gnu/bin
```

r63 구성에서 추출한 소스·옵션 목록으로 번역 단위 93개를 빌드합니다. 결과는 `firmware/stm32h7/build/portable/`의 ELF/BIN/MAP이며 크기와 SHA-256을 표시합니다. 장치에는 접속하지 않습니다. 디버그 경로와 컴파일러 버전에 따라 해시가 달라질 수 있으며, 실제 보드에서 사용한 기존 ELF와 비트 단위 재현을 보장한다는 뜻은 아닙니다. Release에는 확인된 기존 r63 ELF/BIN과 해시를 보존합니다.

STM32 raw BIN의 시작 주소는 `0x08000000`이며 ELF에는 주소가 포함됩니다. 대상은 STM32H743VIH6입니다. 실제 기록 시에는 의도한 보드와 프로그래머를 선택하십시오.

## RP2040 SPI Writer

`firmware/flash-writer/`와 Pico SDK 2.2.0을 사용합니다. 핀 배치는 해당 README에 있습니다. CMake 빌드로 `frwr.uf2`를 생성합니다. `gui/`는 독립 flashrom GUI 소스이고, MiniKMB 앱에서는 별도 GPL helper를 실행합니다. 배포하는 바이너리에 맞는 대응 소스와 원래 고지를 보존하십시오.
