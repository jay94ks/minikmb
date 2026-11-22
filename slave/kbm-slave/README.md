# kbm-slave

RP2040의 코어 0이 UART 패킷을 progressive 방식으로 디코딩해 멀티코어
큐에 넣고, 코어 1이 TinyUSB HID 키보드/마우스로 순서대로 실행하는
펌웨어입니다.

마우스 X/Y는 상대 이동량이 아니라 HID logical range `0..32767`의 절대
좌표입니다. wheel과 pan만 기존처럼 signed relative 값으로 동작합니다.

## 기본 하드웨어 설정

- RP2040 SDK: `C:/ARM/RP2040/pico-sdk`
- UART: UART0, 921600-8-N-1
- TX: GPIO 0
- RX: GPIO 1
- 상태 LED: Pico 기본 LED 핀 GPIO 25, active-high
- USB: RP2040 내장 USB device
- XOSC startup multiplier: `48`

UART와 핀은 configure 시 바꿀 수 있습니다. 선택할 수 있는 핀은 보드에서
사용 가능한 GPIO 0~9, 19~29 중 해당 UART 기능을 지원하는 핀으로 제한됩니다.

VBUS_DET 핀은 사용하지 않습니다. TinyUSB가 USB configured 상태이고 suspend가
아닌 동안 LED가 계속 켜지며 HID 제어가 불가능한 동안에는 500 ms 간격으로
상태가 바뀌어 점멸합니다.

```powershell
cmake -S . -B build -G Ninja `
  -DKBM_UART_ID=0 `
  -DKBM_UART_TX_PIN=0 `
  -DKBM_UART_RX_PIN=1 `
  -DKBM_UART_BAUD=921600 `
  -DKBM_USB_VID=0x8857 `
  -DKBM_USB_PID=0x1222
cmake --build build
```

결과물은 `build/kbm-slave.uf2`입니다.

현재 Windows ARM GCC가 실행되지 않는 환경에서는 WSL의 ARM GCC로 빌드하고
Windows `picotool`로 UF2를 만드는 helper를 사용할 수 있습니다.

```powershell
.\build-wsl.ps1
```

이 경우 결과물은 `build-wsl/kbm-slave.uf2`입니다.

## 실행 구조

- 코어 0: UART 소유, 바이트 단위 decoder, 명령 검증, 큐 입력, ACK 전송
- 코어 1: TinyUSB 소유, 키/버튼 상태 유지, HID report 순차 전송
- 코어 간 큐: 32개 패킷, 패킷당 payload 최대 255바이트

ACK는 큐에 명령을 넣은 뒤에만 전송됩니다. 송신자는 매 패킷의 ACK를 받은
후 다음 패킷을 보내야 합니다. 이렇게 하면 USB가 느리거나 연결되지 않은
경우에도 UART 송신 측까지 backpressure가 전달되며, ACK를 받은 HID 전이는
버려지지 않습니다. USB 전송 중 분리되면 캐시된 report는 재연결 후 다시
전송됩니다.

상세 wire format과 opcode는 [PROTOCOL.md](PROTOCOL.md)를 참고하십시오.

USB descriptor에는 Pico SDK 호환 reset vendor interface가 포함됩니다.
`picotool` reset 명령으로 BOOTSEL 업데이트 모드에 진입할 수 있으며 별도
커스텀 부트로더는 사용하지 않습니다.

## Decoder 단위 테스트

USB 복합 구성은 HID(interface 0), CDC serial(1/2), RP2040 표준 reset(3)이며
읽기 전용 MSC(4)와 CDC-NCM은 master PC가 활성화할 때만 추가합니다.
NCM은 MSC 없이 4/5, 있으면 5/6입니다. OFF이면 장치 자체를
숨기며 300ms USB 재열거로 HID/CDC도 잠시 끊깁니다. IP/TCP/UDP는 KBM Control 또는
native 모드의 PC OS가 처리하고 native DHCP는 master RP2040에 있습니다.
네트워크는 부팅 시 OFF이며 8프레임 RX/TX 큐와 64-byte 분할/CRC32/credit를
사용합니다. HID report와 UART ACK가 먼저 처리됩니다. SDK를 수정하지 않고
빌드 디렉터리에 NCM driver 변형을 생성해 115200bit/s 속도를 광고합니다.
표준 NCM 드라이버가 있는 OS가 필요하며 Windows 11의 UsbNcm을 사용합니다.
최근 Windows UsbNcm의 NTB divisor 검증을 통과하도록 생성된 driver의 IN/OUT
divisor를 4로 맞춥니다(TinyUSB #3913/#3914). 원본 SDK는 수정하지 않습니다.

프로젝트 루트에서 `./work/test-firmware-host.ps1`을 실행합니다. WSL에서 Pico SDK 없이
C++ 주입 목업과 기존 C codec suite를 함께 실행합니다. 펌웨어 테스트/최적화는 WSL
전용입니다. [C++ 아키텍처](../../common/firmware/README.md)를 참고하십시오.
build-wsl.ps1 기본/기존 cache 강제 적용값은 921600이며 임시 baud 시험 후에도
이 속도로 복귀합니다. 표준 USB reset과 별도로 60 `RP20`은 일반 RP2040 재부팅입니다.

## 폴더 가상 디스크

KBM Control의 `가상 디스크`에서 폴더/대상 UART를 지정해 공유합니다. Slave PC에는
읽기 전용 FAT32 이동식 디스크로 보이며, 부팅/공유 OFF일 때는 MSC 장치 자체가 숨겨집니다.
512-byte sector를 64-byte 저우선 조각으로 읽고 CRC를 확인합니다. 보드 flash에는
파일을 저장하지 않고 host 쓰기는 거부합니다. 앱이 계속 실행되어야 하며 heartbeat가
5초 없으면 MSC를 숨깁니다. 노출 변경은 300ms USB 재열거로 HID/CDC도 잠시 끊깁니다.
새로고침으로 새 generation과 UNIT ATTENTION을 적용합니다.

## v3 펌웨어/앱 동시 업데이트

프레임은 `STX OP LEN:u16le PAYLOAD ETX CHK`, 길이 상한4118bytes입니다.
CHK는 OP+LEN의 두 바이트+decoded payload 합의 하위8bits입니다.
일반 제어는 여전히 짧은 가변 길이이며 FS 데이터만 최대4096bytes를 지원합니다.
slave는512-byte sector96개(48KiB) LRU 캐시와 별도4KiB CRC 조립 버퍼를 사용합니다.
모든 펌웨어 packet queue/buffer는 정적 배열입니다. v2와 wire 비호환이므로
앱/master/slave를 함께 업데이트해야 합니다. FS는 프레임 경계에서 HID에 양보하며
최대 프레임의 worst-case UART 선점 불가 시간은 약89.4ms입니다.


