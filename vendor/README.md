# 외부 라이브러리 원본

[한국어](README.md) · [English](README.en.md)

아카이브는 상류 프로젝트의 원본 소스·저작권·라이선스를 보존합니다. 저장소의 MIT를 새로 적용하지 않습니다. API 키, 개인 Cargo/npm 설정, node_modules 빌드 트리는 포함하지 않습니다.

| 위치 | 범위 | 무결성·출처 |
|---|---|---|
| `rust/` | 네이티브 앱과 독립 SPI Writer GUI의 Cargo.lock에 고정된 원본. 플랫폼·빌드·테스트 의존성도 포함하며 모든 항목이 Windows 바이너리에 링크되는 것은 아님 | `rust/manifest.json`의 이름·버전·crates.io 주소·Cargo SHA-256 |
| `npm/` | Electron renderer/main의 package-lock에 지정된 패키지 원본. 선택적 플랫폼 패키지·빌드 도구 포함 | `npm/manifest.json`의 버전·레지스트리 주소·SRI·SHA-256 |
| `pico-sdk/` | RP2040에서 사용한 Pico SDK 2.2.0 코어와 TinyUSB. 미사용 Bluetooth/Wi-Fi/lwIP/mbedtls는 제외 | `pico-sdk/manifest.json`의 원본 리비전·아카이브 해시 |
| `flashrom/` | 별도 GPL SPI helper의 전체 대응 소스. flashrom 1.8.0, serprog bridge, Windows timeout 패치, frontend와 재빌드 안내 | 원본 `UPSTREAM-SHA256SUMS`, COPYING.rst와 파일별 고지 |
| `python/` | 선택적 호스트 진단에 사용하는 pyserial 3.5 | `python/manifest.json`과 소스 안의 BSD-3-Clause |

STM32 HAL/CMSIS, FreeRTOS, FatFs와 선택한 TinyUSB Device 소스는 `../firmware/stm32h7/`에서 직접 읽을 수 있습니다. TinyUSB는0.21.0, 커밋 `dae3f9a366bfcddbf9dcf1b48d7500286a849539`이며 로컬 패치와 UPSTREAM 고지를 포함합니다. FreeRTOS는10.3.1, HAL은 소스 기준1.11.5입니다.

글꼴과 고지는 `../kbm-rs/assets/fonts/`에 있습니다. Windows ZIP에는 probe-rs0.32.0, GPL helper, 펌웨어, 대응 소스·고지·의존성 목록을 제공합니다. 컴파일러 도구체인, Electron 플랫폼 런타임 다운로드, WSL 도구와 Windows DLL은 이 라이브러리 소스 묶음과 구분합니다.

## 아카이브 사용

Pico SDK는 `pico-sdk/pico-sdk-2.2.0-source.tar.gz`를 `vendor/pico-sdk/`에 풀고 생성된 `pico-sdk` 디렉터리를 `PICO_SDK_PATH`로 지정합니다.

Cargo는 원래 캐시·레지스트리와 lock 파일을 사용할 수 있습니다. 동봉한 원본을 명시적인 캐시 경로에 복사하려면 저장소 루트에서 `python vendor/scripts/restore-vendor.py --cargo-cache <registry cache directory>`를 실행합니다. 검증한 파일만 복사하며 전역 설정이나 레지스트리 인덱스를 변경하지 않습니다. 완전한 오프라인 빌드에는 미리 준비한 인덱스·캐시 또는 표준 `cargo vendor` 절차도 필요합니다.

npm 원본은 일반 레지스트리 tarball입니다. `npm cache add vendor/npm/<archive>.tgz`로 캐시를 채울 수 있으며 package-lock이 기준입니다. 네이티브 빌드 도구체인과 플랫폼 런타임은 별도 준비가 필요합니다.

바이너리를 재배포할 때는 라이선스 이름만 표시하는 것으로 끝내지 말고, 원래 고지와 필요한 GPL/MPL 대응 소스를 함께 유지하십시오.
