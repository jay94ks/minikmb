# kbm-master

RP2040 USB CDC와 두 대의 `kbm-slave` 사이를 연결하는 dual-UART bridge입니다.
USB CDC에서 포트와 요청 ID를 포함한 ROUTE 명령을 받아 UART0/1로 동시에
전달합니다. UART에서는 기존 slave frame을 사용하며, 실제 slave 응답을
포트·요청 ID와 함께 USB CDC 호스트로 전달합니다. 전역 SELECT는 없습니다.

## 기본 연결

- UART0: TX GPIO 0, RX GPIO 1
- UART1: TX GPIO 4, RX GPIO 5
- 두 UART 모두 921600-8-N-1
- 상태 LED: Pico 기본 LED 핀 GPIO 25, active-high
- XOSC startup multiplier: `48`
- USB product: CDC ACM (`VID 8857`, `PID 1223`)

VBUS_DET는 사용하지 않습니다. TinyUSB가 USB configured 상태이고 suspend가
아니면 LED를 계속 켜며, USB CDC 제어가 불가능한 동안에는 500ms마다 LED
상태를 반전해 점멸합니다.

각 TX는 상대 slave의 RX에, 각 RX는 상대 slave의 TX에 연결하고 GND를
공통으로 연결합니다. 모든 제어 명령에 대상 UART를 지정하십시오.

## 빌드

Windows toolchain이 정상이라면 다음 명령을 사용합니다.

```powershell
cmake -S . -B build -G Ninja
cmake --build build
```

현재 환경처럼 Windows ARM GCC 실행에 문제가 있으면 WSL toolchain을 쓰는
helper를 실행합니다.

```powershell
.\build-wsl.ps1
```

결과물은 `build-wsl/kbm-master.uf2`입니다. GPIO와 baud는 CMake cache의
`KBM_UART0_TX_PIN`, `KBM_UART0_RX_PIN`, `KBM_UART1_TX_PIN`,
`KBM_UART1_RX_PIN`, `KBM_UART_BAUD`로 변경할 수 있습니다.

## 실행 구조

- 코어 0: TinyUSB CDC, 바이트 단위 progressive decoder, 로컬 명령 처리
- 코어 1: UART0/UART1 송수신, 각 포트 decoder, PING, 500ms liveness 판정
- 부팅 상태: 전역 선택 없음, 두 포트 모두 offline
- UART0/1 독립 FIFO 각 64개, USB 응답 FIFO 64개
- 포트별 한 개 명령의 ACK를 기다리며 다른 포트는 계속 진행

PC는 `STATUS(71)`로 protocol version3와 UART0/UART1의 2비트 online mask를 조회할
수 있고, 이후 어느 포트든 상태가 바뀌면 `ONLINE_CHANGED(F2)` 비동기 event를
받습니다. 각 UART는 마지막 송신 후 100ms가 지나면 독립적으로 PING됩니다.
STATUS에는 포트별 TX frame, RX byte, 정상 RX frame, decode error 카운터도
포함되어 배선/baud/프레임 문제를 구분할 수 있습니다.
UART 응답은 송신 순서뿐 아니라 예상 response opcode도 대조하므로 timeout 뒤
도착한 오래된 PING 응답이 다음 호스트 명령의 ACK로 오인되지 않습니다.
반대로 내부 PING 응답이 유실되어도 이후 호스트 명령의 올바른 ACK를 찾아
전달합니다. 이때 호스트 명령 자체의 순서는 건너뛰지 않습니다.

USB descriptor에는 Pico SDK 호환 reset vendor interface가 포함됩니다.
`picotool` reset 명령으로 BOOTSEL 업데이트 모드에 진입할 수 있고, master CDC는
1200 baud touch 방식도 지원합니다. 별도 커스텀 부트로더는 사용하지 않습니다.

ROUTE(73)의 payload는 `port:u8 request_id:u16le slave_op:u8 slave_payload`이며,
응답(F3)은 `port request_id:u16le bridge_status slave_response_op slave_payload`입니다.
수신 시 포트와 요청 ID를 큐에 고정하므로 앱의 화면 매핑 변경이 기존 명령을
다른 포트로 보내지 않습니다. 한쪽 timeout은 해당 요청에 오류로 반환하고,
새 PING ACK로 재동기화한 뒤 해당 포트의 큐를 계속 처리합니다.
CDC 시리얼·NCM 네트워크·임시 UART 속도 시험은 새 slave 펌웨어도 필요합니다.
master, slave 두 대, 앱을 함께 업데이트하되 보드는 반드시 한 대씩 플래싱합니다.
상세 wire format과 master opcode는 [PROTOCOL.md](PROTOCOL.md)를 참고하십시오.

## Decoder 단위 테스트

프로젝트 루트에서 `./work/test-firmware-host.ps1`을 실행합니다. 모든 펌웨어 테스트와
최적화는 WSL에서 수행하며 Pico SDK 없는 C++ 목업 및 두 C codec suite를 함께 검사합니다.
추상화/실제 구현체/주입 계약은 [architecture](../common/firmware/README.md)에 있습니다.

운영 baseline은 **921600 bps**이며 build-wsl.ps1은 기존 CMake cache도 갱신합니다.
다른 값을 명시할 때는 `./build-wsl.ps1 -Baud 115200`처럼 양쪽을 동일하게 지정합니다.
USB NCM link speed 표시는 route preference 유도를 위해 115200 bit/s로 유지합니다.

`NCM 모드` 앱 버튼은 `77(native,dhcp)`를 보내 선택적 master USB NCM과 펌웨어 DHCP를
제어합니다. 변경 시 두 slave NCM도 OFF→ON합니다. DHCP 서버 식별자 및 master PC IP는
.1, slave 선호 주소 .100/.101입니다. 앱 네트워크/포워딩과 상호 배타적입니다.
RP2040 일반 재부팅(60)은 BOOTSEL 업데이트와 별개입니다. 자세한 전환/갱신 제약은
PROTOCOL.md를 참고하고 실보드 USB 재열거·DHCP는 별도로 검증하십시오.

가상 디스크는 앱의 폴더 FAT32 sector를 slave MSC로 중계합니다. 44/45/46/47과
F8 비동기 read event를 사용하고 DATA45는 HID보다 낮은 bulk 우선순위입니다.
native NCM 모드에서도 동작하지만 KBM Control을 종료하면 매체는 not-ready가 됩니다.

## v3 펌웨어/앱 동시 업데이트

프레임은 `STX OP LEN:u16le PAYLOAD ETX CHK`, 길이 상한4118bytes입니다.
CHK는 OP+LEN의 두 바이트+decoded payload 합의 하위8bits입니다.
일반 제어는 여전히 짧은 가변 길이이며 FS 데이터만 최대4096bytes를 지원합니다.
slave는512-byte sector96개(48KiB) LRU 캐시와 별도4KiB CRC 조립 버퍼를 사용합니다.
모든 펌웨어 packet queue/buffer는 정적 배열입니다. v2와 wire 비호환이므로
앱/master/slave를 함께 업데이트해야 합니다. FS는 프레임 경계에서 HID에 양보하며
최대 프레임의 worst-case UART 선점 불가 시간은 약89.4ms입니다.


