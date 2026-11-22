# 라이선스 적용 범위와 저작자 고지

[한국어](NOTICE.ko.md) · [English](NOTICE.md)

Copyright (c) 2026 jay94ks. MiniKMB에서 직접 작성한 코드·하드웨어 설계·문서는 개별 파일에 다른 고지나 외부 자료가 있는 경우를 제외하고 MIT로 공개합니다. 개발에는 OpenAI Codex의 GPT 6 Astra와 Sol을 활용했습니다. **회로도는 jay94ks가 직접 작성했습니다.** 타깃 USB-C HS 구성의 검토 누락은 제작자의 설계 회고이며 GPT의 회로도 작성 오류로 귀속하지 않습니다.

이 한글판은 적용 범위를 설명하는 안내입니다. [MIT LICENSE](LICENSE), [영문 고지](NOTICE.md)와 각 외부 구성 요소의 원래 라이선스 문구를 대체하지 않습니다. 루트 MIT는 외부 라이선스·상표·제조사 문서나 펌웨어 블롭을 다시 허가하는 조항이 아닙니다.

- **CMSIS 및 ST 디바이스 패키지:** `firmware/stm32h7/Drivers/CMSIS`의 Apache-2.0과 ST 디바이스 패키지 고지를 보존합니다.
- **STM32 HAL:** 원래 패키지 고지와 BSD-3-Clause 적용 조건을 보존합니다.
- **TinyUSB:** MIT이며 `UPSTREAM.md`에 출처와 로컬 패치를 기록합니다.
- **FreeRTOS:** MIT입니다. **FatFs:** ChaN의 원래 저작권·허용 고지를 소스에 보존합니다. 로컬 수정본도 이 고지를 유지합니다.
- **Pico SDK:** 실제 사용한 SDK와 라이브러리 소스의 파일별 조건을 유지합니다. BSD-3-Clause와 TinyUSB MIT 등이 포함됩니다.
- **Rust·npm 라이브러리:** `vendor/rust`와 `vendor/npm`의 원본 패키지와 manifest에 버전·출처·해시를 기록합니다. 각 패키지의 LICENSE/NOTICE를 확인하십시오. Windows ZIP에도 고지와 MPL 소스 아카이브를 제공합니다.
- **flashrom·SPI helper:** 별도 실행하는 GPL 계열 도구입니다. `vendor/flashrom`의 대응 소스, COPYING과 개별 파일 조건이 적용되며 MIT로 다시 허가하지 않습니다. 독립 Writer GUI는 libflashrom과 링크하므로 결합 배포물의 GPL 조건도 준수해야 합니다.
- **probe-rs 0.32.0:** MIT OR Apache-2.0이며 별도 실행 파일로 호출합니다. 독점 ST-Link CLI를 재배포하지 않습니다.
- **Noto Sans KR:** SIL OFL-1.1입니다. **Lucide:** ISC와 Feather 유래 부분의 MIT를 유지합니다. 글꼴 옆에 원래 고지가 있습니다.
- **KiCad 표준 라이브러리 유래 형상:** 해당 CC-BY-SA-4.0 및 KiCad 예외 등 원래 조건을 유지합니다. 공급사 형상·핀 데이터는 해당 공급사의 출처를 고지합니다.
- **데이터시트·제조사 문서:** 원문을 보존하며 원 저작권자의 권리가 유지됩니다. MIT 대상이 아니며 저장소 수록 자체가 추가 권한을 부여하지 않습니다.
- **MS2130_sample_20221026.bin:** 제작자가 제공해 실기에서 사용한 MS2130 펌웨어입니다. 크기는64,468바이트, SHA-256은 `32c729ced368413fc16ff000bb2a044b6991aa5f3d04fa6cfd63056f40779538`입니다. 별도 공개 소스·재배포 라이선스는 제공되지 않았습니다. MIT 또는 오픈소스 ROM으로 표시하지 않으며, 재사용 조건은 원 권리자에게 확인해야 합니다.

개발 사진은 제작자가 공개용으로 제공했습니다. 사진 속 외부 제품·로고·소프트웨어의 권리는 각 권리자에게 있으며 후원이나 보증을 뜻하지 않습니다.
