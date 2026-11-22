# frwr의 외부 구성 요소

| 구성 요소 | 고정 버전 / SHA | 라이선스 | 원본 |
| --- | --- | --- | --- |
| Raspberry Pi Pico SDK | 2.2.0 / `a1438dff1d38bd9c65dbd693f0e5db4b9ae91779` | BSD-3-Clause 및 개별 소스 고지 | https://github.com/raspberrypi/pico-sdk |
| TinyUSB | `86ad6e56c1700e85f1c5678607a762cfe3aa2f47` | MIT | https://github.com/hathach/tinyusb |
| flashrom (별도 호스트 실행 파일) | v1.8.0 / `90b6b4376c7480a223e7bb5642e7116315ac6d09` | GPL-2.0 계열, 세부 사항은 해당 소스의 COPYING.rst 및 파일 고지 | https://github.com/flashrom/flashrom |
| pyserial (호스트에 별도 설치) | 실기 사용 3.5 | BSD-3-Clause | https://github.com/pyserial/pyserial |

Pico SDK와 TinyUSB 원본은 수정하지 않았다. Clang 22 호환 헤더와 빌드 설정은 frwr 내부에 있다. flashrom은 위 리비전의 원본을 수정 없이 Windows x86_64 serprog 전용으로 빌드했다. 재현 절차는 `build-flashrom.ps1`과 `cmake/flashrom-mingw.ini`에 있다.

`package.py`가 만드는 로컬 배포 ZIP에는 Pico SDK/TinyUSB 라이선스 파일과 flashrom의 실행 파일·대응 소스 전체(.git 제외)를 포함한다. flashrom 소스의 개별 저작권 고지를 유지한다. 사용자 ROM과 플래시 백업·readback 바이너리는 배포 ZIP에 포함하지 않고 로컬 검증 디렉터리에 보존한다.
