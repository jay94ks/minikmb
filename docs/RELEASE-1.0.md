# v1.0 공개판 안내

[한국어](RELEASE-1.0.md) · [English](RELEASE-1.0.en.md)

[GitHub Release](https://github.com/jay94ks/minikmb/releases/tag/v1.0.0)에 Windows 앱·펌웨어·ROM·고지와 해시 파일을 제공합니다. 내부 버전은 네이티브 앱0.1.0 preview, STM32 펌웨어1.0.0 FS-HID r63입니다.

| 파일 | 용도 |
|---|---|
| `MiniKMB-Native-0.1.0-windows-x64-preview-267a4d46464d.zip` | 앱·프로그래밍 도구·펌웨어·고지·대응 소스 |
| `MiniKMB-FS-HID-r63.elf` / `.bin` | STM32H743VIH6용 현재 펌웨어. raw BIN 시작 주소0x08000000 |
| `kbm-master.uf2` / `kbm-slave.uf2` | RP2040 KVM 관리·타깃 펌웨어 |
| `frwr-0.1.0.uf2` | H7 보드의 MS2130용 외부 SPI ROM Writer 펌웨어 |
| `MS2130_sample_20221026.bin` | MS2130 외부 플래시에 사용하는64,468바이트 이미지 |
| `firmware-notices.zip`, `LICENSE.txt`, `NOTICE.md` | 직접 작성 코드와 외부 구성 요소의 고지 |
| `release-manifest.json`, `SHA256SUMS` | 빌드 출처·파일 크기·SHA-256 |

직접 확인한 앱 실행 파일의 SHA-256은 `267a4d46464d1cb675d351905a6ce24eb296202c8462bd8f7124c68668f01d85`입니다. 이번 한·영 문서 업데이트는 기존 바이너리를 교체하지 않습니다.

## HS와 설계 회고

타깃은 FS로 구현했습니다. 설계자는 USB-C 삽입 방향 감지·신호 선택 회로의 부재와 그에 따른 HS 경로의 임피던스 불일치를, HS 연결이 안정적으로 수립되지 않거나 연결 후 오류율이 높았던 원인으로 판단합니다.

회로도는 jay94ks가 직접 작성했으며, 해당 부분의 검토를 충분히 마치지 않고 아트웍으로 진행한 설계 검토 누락으로 기록합니다. GPT의 회로도 작성 실수로 귀속하지 않습니다. 이 설명은 설계자의 회고이며 원인 분리 계측으로 확정된 결과와 구분합니다. 자세한 범위는 [알려진 제한](KNOWN-ISSUES.md)을 참고하십시오.

MS2130 USB 3.x 영상은 MCU/USB3300 타깃 경로와 별개이며 외부 SPI 플래시·ROM을 적용한 뒤 동작을 확인했습니다.

직접 작성한 코드·설계·문서는 MIT입니다. MS2130 ROM과 외부 도구·라이브러리의 권리는 별도이며, ROM에 대한 공개 재배포 라이선스는 확인되지 않았습니다. [고지](../NOTICE.ko.md)와 원문 라이선스를 확인하십시오.
