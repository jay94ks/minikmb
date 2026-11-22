# KBM UART protocol v3

## Frame

```text
STX OP LEN:u16le PAYLOAD ETX CHK
```

| Field | Size | Meaning |
|---|---:|---|
| `STX` | 1 | `0x02` |
| `OP` | 1 | opcode; response sets bit 7 |
| `LEN` | 2 | decoded payload byte count, little-endian, 구현 상한4118 |
| `PAYLOAD` | `LEN` decoded bytes | only this field is escaped |
| `ETX` | 1 | `0x03` |
| `CHK` | 1 | low 8 bits of `OP + LEN_low + LEN_high + decoded payload bytes` |

`PAYLOAD`의 `STX(02)`, `ETX(03)`, `ESC(1B)`는 각각 `ESC 02`,
`ESC 03`, `ESC 1B`로 전송합니다. `OP`, `LEN`, `ETX`, `CHK`는 위치로
구분되므로 escape하지 않습니다.

Decoder는 UART에서 바이트가 들어올 때마다 상태가 진행됩니다. 한 프레임이
여러 read에 나뉘거나 여러 프레임이 한 번에 들어와도 동작합니다. payload의
escape 오류, 이른 ETX, checksum 오류가 발생하면 다음 STX에서 다시
동기화합니다.

Decoder는 완성된 frame만 명령 큐에 넣습니다. UART read 경계는 frame 경계로
취급하지 않으며, 한 번의 read에 frame 일부만 있거나 여러 frame이 연속으로
있어도 결과가 같아야 합니다.

v2와 wire 호환되지 않으며 앱·세 보드를 함께 업데이트해야 합니다.
모든 16비트 수는 little-endian입니다. signed 값은 two's complement입니다.

## Requests

### `00` PING

Payload가 없습니다. 전송 경로와 큐 수락 여부를 확인합니다.

### `10` KEY_DOWN

```text
hid_usage:u8
```

HID keyboard usage 하나를 누릅니다. modifier usage `E0`~`E7`도 같은 방식으로
전송하며 펌웨어가 modifier bitmap으로 변환합니다.

### `11` KEY_UP

```text
hid_usage:u8
```

HID keyboard usage 하나를 놓습니다. 조합키는 필요한 키를 차례로 down한 뒤
역순으로 up하여 구성합니다. 각 down/up은 앞 USB report가 완료된 후 별도
report로 전송되므로 중간 전이가 합쳐지지 않습니다.

일반 키가 동시에 6개를 넘으면 표준 boot-keyboard rollover report를 보내며,
펌웨어 내부에는 전체 눌림 상태를 유지합니다. 다시 6개 이하가 되면 실제
상태를 복원해 전송합니다.

### `20` MOUSE_MOVE

```text
x:u16le y:u16le wheel:i8 pan:i8
```

현재 버튼 상태를 유지하며 절대 좌표 이동과 wheel/pan report를 보냅니다.
`x`, `y`의 허용 범위는 HID logical coordinate `0..32767`이며 각각 little-endian
2바이트입니다. `(0,0)`은 화면 좌상단, `(32767,32767)`은 우하단에 대응합니다.
호스트 OS가 다중 모니터 또는 좌표 변환 정책을 적용하면 실제 pixel 위치는 그
정책에 따라 매핑됩니다. wheel/pan은 상대값이며 허용 범위는 -127~127입니다.

버튼 down/up report는 마지막 MOUSE_MOVE의 절대 좌표를 유지합니다. 부팅 직후
저장 좌표는 `(0,0)`이므로 클릭이나 드래그 전에 MOUSE_MOVE로 위치를 지정하는
것을 권장합니다.

### `21` MOUSE_DOWN

```text
buttons:u8
```

지정한 버튼 bit를 현재 상태에 추가하여 누릅니다.

### `22` MOUSE_UP

```text
buttons:u8
```

지정한 버튼 bit를 현재 상태에서 제거하여 놓습니다.

button bit는 left=`01`, right=`02`, middle=`04`, back=`08`, forward=`10`입니다.
마우스 동작은 다음 원자 명령 조합으로 만듭니다.

```text
클릭       DOWN(left), UP(left)
더블 클릭  DOWN(left), UP(left), DOWN(left), UP(left)
롱 홀드    DOWN(left), 원하는 시간 대기, UP(left)
드래그     MOVE(start), DOWN(left), MOVE(...), [MOVE(...) ...], UP(left)
```

클릭 간격, 홀드 시간, 드래그 속도는 송신 측이 명령을 보내는 간격으로
결정합니다. 한 request의 정상 ACK를 받은 뒤 다음 request를 보내십시오.

### `7F` RELEASE_ALL

Payload가 없습니다. 키보드 키와 마우스 버튼을 모두 놓습니다.

### `30` SERIAL_WRITE

payload는 raw byte 1..251개이며 USB CDC COM으로 전송합니다. 오디오/텍스트와
무관한 binary-safe 스트림입니다. DTR/RTS 준비 상태에서 별도 64-entry TX 큐에
수락하며 ACK는 `[status, serial_queued_count]`입니다. status 07은 COM 준비 안 됨,
08은 큐 가득 참입니다. 수락 뒤 DTR/RTS가 내려가면 남은 데이터는 준비 복구까지
보관합니다. HID report 완료 대기와 CDC 서비스는 독립적입니다.

### `31` SERIAL_STATUS

payload가 없으며 응답 B1 payload는 `[status, flags]`입니다.
flags bit0=USB configured/not suspended, bit1=DTR, bit2=RTS입니다.
USB 호스트→UART 수신 준비는 `(flags & 03)==03`, UART→USB 호스트 송신 준비는
`(flags & 07)==07`입니다. DTR/RTS는 USB CDC line-state이며 물리 GPIO는 추가하지 않습니다.
host가 COM을 열 때 DTR/RTS를 설정해야 하며, 이는 실제 응용 프로그램의 read 완료를
의미하지는 않습니다. COM baud 설정은 115200 UART 링크 속도를 변경하지 않습니다.

### `F4` SERIAL_DATA event / `32` SERIAL_RX_ACK

slave USB COM에서 받은 데이터는 비동기 UART event F4로 전송합니다.
payload는 `serial_sequence:u16le data:bytes[1..248]`입니다.
master의 내부 credit 명령 `32 serial_sequence:u16le`를 기다리고,
500ms 내 credit가 없으면 같은 이벤트를 재전송합니다. 32에는 별도 ACK가 없습니다.
master는 sequence 중복을 제거하고 USB 응답 큐 수락 후 credit를 반환합니다.
credit 대기 중에는 USB CDC에서 다음 데이터를 읽지 않아 유한 버퍼의 역압을 유지합니다.
F4는 HID/시리얼 명령의 응답이 아니며 응답 순서를 소비하지 않습니다.

### `F5` SERIAL_STATE event

payload `flags:u8`로 USB/DTR/RTS 상태 변경을 알립니다. master는 포트를 붙여
PC로 전달하며 STATUS/31로도 현재 상태를 동기화할 수 있습니다.
UART TX는 비차단이며 ACK와 event를 frame 경계에서 직렬화해 바이트가 섞이지 않습니다.

## USB 인터페이스

VID/PID `8857:1222`, interface 0=keyboard/absolute mouse HID,
interface 1/2=CDC control/data, interface 3=RP2040 reset입니다.
MSC/NCM 모두 기본 숨김이며 MSC만 ON=interface4, NCM만 ON=interface4/5,
둘 다 ON=MSC4 + NCM5/6입니다. bcdDevice=0600 | NCM(bit0) | MSC(bit1)입니다.
master 주 제어 COM은 `8857:1223`이므로 같은 PC에서 구분할 수 있습니다.

NCM MAC은 unique board ID에서 파생한 locally administered unicast 주소입니다.
NCM link speed notification은 보수적으로 115200bit/s를 광고합니다. Windows의
자동 metric이 낮은 링크 속도에 더 높은 비용을 주도록 유도하지만 OS의 수동
route metric까지 강제하지 못합니다. 앱 DHCP는 master/기본 게이트웨이를
192.168.127.1로 배포합니다(option 3). 기존 유선 Ethernet을 우선하는 것은
낮은 링크 속도에 따른 자동 metric에 맡기며 DNS는 배포하지 않습니다.

## Ethernet 및 링크 시험 명령

`40 NET_WRITE`, `41 NET_STATUS`, `42 NET_CREDIT`, `43 NET_ENABLE`, `F6 NET_DATA`,
`50 TEMP_BAUD`, `51 ECHO`의 payload/CRC/credit/lease는
[master 프로토콜의 Ethernet·속도 시험 절](../../master/PROTOCOL.md)을 따릅니다.
UART에서는 ROUTE envelope와 F6의 port prefix만 없습니다.

- Ethernet은 14..1514 bytes(FCS 제외), 64-byte 조각마다 id/total/offset/CRC32.
- 40의 정상 C0는 `[status, queued_count]`. 마지막 조각은 전체 CRC 검증 후에만
  RX 호스트로 향하는 USB TX 큐에 넣습니다. 오류/큐 초과 프레임은 주입하지 않습니다.
- 41의 C1은 `[status, flags, mac[6], advertised_bps:u32]`, flags=USB NCM ready(bit0),
  enabled(bit1). 43은 `[0|1]`이며 기본 disabled, C3는 일반 2-byte ACK입니다.
- F6는 `frame_id:u16 total:u16 offset:u16 crc32:u32 data[1..64]`이고
  42 credit는 `frame_id:u16 offset:u16`입니다. 42 자체 ACK는 없습니다.
  500ms 재전송/하나씩 credit 대기, master 중복 제거 및 유한 큐로 역압을 전파합니다.
- slave Ethernet RX/TX 각 8프레임, UART 저우선 event 2개입니다. UART ACK와 HID
  report 시작을 네트워크보다 우선하며 TinyUSB는 core 1 한 곳에서만 사용합니다.
- 50은 `baud:u32 lease_ms:u16`(1..10초), 51은 data[1..248]이며 D1은 `[status,data]`.
  속도 제한은 ADuM1201ARZ에 맞춰 최대 1Mbps, lease 후 무조건 baseline으로 복귀합니다.
- IP/TCP/UDP/DHCP는 slave에 구현하지 않습니다. 앱 모드는 PC 앱, native 모드는
  master RP2040의 선택적 DHCP가 담당합니다. slave 펌웨어가
  default route를 만들거나 다른 slave 네트워크를 bridge하지 않습니다.

## Responses and flow control

### NCM 숨김 및 일반 재부팅 추가 규격

부팅 시 NCM은 disabled/hidden입니다. 43 `[01]`이면 descriptor에 NCM을 추가하고,
43 `[00]`이면 제거합니다. 양쪽 모두 USB detach 300ms 후 재열거하므로 HID/CDC도
잠시 끊깁니다. 항상 HID0, CDC1/2, reset3이며 NCM은 MSC 없이 4/5,
MSC가 있으면 5/6입니다. MSC/NCM 전환은 하나의 detach/reconnect 타이머를 공유합니다.
C3 ACK는 설정 수락이며 NCM ready까지 보장하지 않습니다. 41 flags bit0을 확인합니다.

`60` payload는 정확히 ASCII `RP20` (`52 50 32 30`)입니다. E0 `[00,00]` ACK를
UART software/hardware FIFO 끝까지 보낸 뒤 100ms guard 후 watchdog 정상 부팅합니다.
이는 RP2040 재부팅으로 연결 PC나 BOOTSEL에 영향을 주지 않습니다. ACK 유실 시
무조건 재시도하지 마십시오. 수락 후 입력을 중단하며 기존 큐는 재부팅으로 폐기됩니다.
NCM은 다시 hidden/OFF로 시작하므로 master가 필요한 경우 다시 활성화합니다.
운영 UART 및 임시 baud lease 후 복귀 기본값은 921600-8-N-1입니다.

일반 응답 opcode는 `request OP | 0x80`이고 payload는 다음 두 바이트입니다.
31(제어 신호), 41(네트워크 정보), 51(echo)은 위에서 정의한 별도 payload를 사용합니다.
32/42 credit에는 응답이 없습니다.

```text
status queued_packet_count
```

| Status | Meaning |
|---:|---|
| `00` | 큐에 정상 수락 |
| `01` | 알 수 없는 opcode |
| `02` | 잘못된 payload 길이 |
| `03` | 잘못된 payload 값 |
| `04` | payload escape 오류 |
| `05` | ETX 위치/값 오류 |
| `06` | checksum 오류 |
| `07` | SERIAL_WRITE의 USB/DTR/RTS 또는 NET_WRITE의 NCM 준비 안 됨 |
| `08` | SERIAL/NET TX 큐 가득 참 |

송신자는 정상 ACK를 받은 뒤 다음 request를 보내는 stop-and-wait 방식을
사용해야 합니다. ACK는 USB 실행 완료가 아니라 내부 큐 수락을 의미합니다.

## 읽기 전용 USB MSC 가상 디스크 (v3)

MSC는 공유를 켰을 때만 보이며 부팅/OFF/eject/5초 heartbeat 만료 시 숨겨집니다.
300ms USB 재열거로 HID/CDC/NCM도 잠시 끊깁니다. HID0,CDC1/2,reset3 유지,
MSC4, NCM은 MSC 없이4/5 또는 MSC와 함께5/6입니다.

44 CONFIG는 `enabled:u8,generation:u32,sectors:u32` (9bytes), generation!=0,
OFF sectors=0. 최대32GiB. 같은 설정은 heartbeat이고 cache/eject를 초기화하지 않습니다.
새 generation은 모든 캐시 무효화와 SCSI UNIT ATTENTION입니다.
F8는 `generation:u32,read_id:u32,lba:u32,count:u16` (14bytes), count=1..8.
USB READ10이 필요한 크기만 요청하며 고정4KiB fetch/padding하지 않습니다.
45 DATA는 `generation:u32,read_id:u32,lba:u32,offset:u16,crc32:u32,data[1..4096]`.
보통 count*512bytes를 한 DATA로 수신합니다. 작은 분할 응답도 수용하지만 전체 요청
길이·토큰·순서·CRC32가 일치하기 전에는 어떤 sector도 USB/캐시에 노출하지 않습니다.
47 FAIL은12-byte generation/id/LBA, 읽기 timeout4초, F8 재요청500ms입니다.
46 STATUS=[status,flags], bit0=configured,bit1=ready. C4/C5/C7=[status,queued].
ACK는 큐 수락이며 실행/CRC 완료를 의미하지 않습니다.

캐시는512-byte sector96개=48KiB입니다. sector별 LBA/valid/LRU를 관리하며,
4KiB 단위로만 교체하지 않습니다. CRC 조립용4KiB와 UART 수신/큐 버퍼는 별도입니다.
TinyUSB MSC 작업 버퍼의 상한4096bytes는 USB endpoint packet size64와 별개이며,
작은 SCSI 요청은 그대로 작게 처리합니다. READ callback은 UART를 기다리지 않습니다.
cooperative MSC driver와 MIT notice를 유지하고 SDK는 수정하지 않습니다.

HID는 다음 FS 프레임보다 우선하지만 이미 시작한 최대4KiB frame은 선점하지 않습니다.
최악 escape 시 UART 약89.4ms 지연이 가능합니다. normal/serial/network queue는
작은 정적 요소를 유지하고 큰 FS는 별도1-entry 정적 큐입니다.
모든 queue storage는 정적 배열(capacity+1 sentinel 포함), 런타임 heap 할당 없음입니다.
호스트 쓰기는 DATA PROTECT로 거절하며 source/flash에 저장하지 않습니다.

## 부팅 세션 보고 / 확인 (F9 / 62)

전원 재투입 시 slave 큐는 정적으로 초기화되어 이미 비어 있습니다. UART 초기화 후
USB 준비 여부와 무관하게 F9를 보내며, matching62를 받을 때까지250ms마다 재전송합니다.
F9 LEN16: `[schema=1,wire=3,firmware:u16le,boot_nonce:8bytes,reset_reason,
usb_flags,hid_queue_depth,reserved=0]`. firmware0801, nonce는 부팅별63-bit random
(재사용/영구 저장 안 함), reset_reason0=전원/기타(정확한 구별 불가),1=watchdog입니다.
usb_flags bit0=mounted,bit1=ready,bit2=suspended입니다.

62 LEN9 `[schema=1,boot_nonce:8bytes]`는 master 내부 확인으로 일반 ACK를 만들지 않습니다.
일치하기 전 입력·설정·DATA는07로 거절하고 PING/ECHO/상태 읽기는 허용합니다.
master는 새 nonce당 한 번 해당 UART의 이전 요청을 취소하고 확인을 보내며,
중복F9는 새 요청 큐를 다시 비우지 않습니다. 구형 master는62를 보내지 않으므로
master를 먼저 업데이트해야 합니다. USB report descriptor는0800과 동일합니다.

HID 큐가 꽉 차면 수락하지 않고08 QUEUE_FULL을 즉시 응답합니다. 큐 대기 때문에
UART decoder/PING/상태 읽기를 멈추지 않습니다. 성공 수락된 DOWN/UP은 여전히
순서대로 실행하며, 거절/ACK 유실 뒤 DOWN을 자동 재시도하지 마십시오.

## USB/HID 진단 (FA / 63)

FA는 core1이 소유한 상태 snapshot을 별도 정적1-entry 큐로 core0에 전달하여 UART로
보고합니다. lifecycle 변화 때 즉시(최대50Hz sampling), 일반 snapshot은1초마다입니다.
ACK/부팅 보고 뒤 우선순위이며 HID 큐가 차도 보고합니다. 상태 보고만 coalesce/drop
가능하고 입력 명령은 coalesce하지 않습니다. 63 LEN0은 E3 `[status,snapshot]`을
반환합니다. 첫 snapshot 이전에는07입니다. UART core는 TinyUSB를 호출하지 않습니다.

snapshot LEN52, 모든32-bit 값 little-endian:

| offset | 값 |
| --- | --- |
| 0 | schema1 |
| 1 | flags: mounted1,ready2,suspended4,HID 제출 가능8,visibility stable16,report waiting32,submitted64 |
| 2..4 | HID 큐 깊이0..32, 대기 report ID0..3, reset 원인0/1 |
| 5..7 | reserved0 |
| 8,12,16 | uptime ms, bus reset 누계, USB epoch |
| 20,24,28 | HID 제출 누계, USB 전송 완료 누계, 현재 보고서 대기 ms |
| 32,36,40 | SETUP 누계, HID 큐 거절 누계, slave UART RX 오류 누계 |
| 44,48 | 정상 입력 activity generation, reserved0 |

USB 설정/전송 완료는 호스트의 실제 키 해석 성공이 아닙니다. VBUS_DET가 없어
절전과 물리 분리를 일반적으로 구별할 수 없습니다. 현재 타깃은 전원 OFF 시 USB
전원이 사라진다는 사용자 확인이 있으므로 다음 F9는 slave MCU의 새 부팅입니다.
키 usage·payload·입력 문자열은 진단 보고에 포함하지 않습니다.

## System Control 전체 입력 (12/13)

`SYSTEM_DOWN=12`, `SYSTEM_UP=13`, LEN=1, payload는 Generic Desktop(page01)의
usage1byte입니다. Keyboard(page07)의 Power(66)와 다릅니다.
USB-IF HUT1.7의 System Control 입력40종을 허용합니다:
`81..8F`, `97..98`, `9A..9B`, `A0..AA`, `B0..B7`, `C9..CA`.
각 입력 이름/순서는 `common/system_controls.def`와 앱의
`src/shared/system-controls.ts`에 기록하고 교차 테스트합니다.
80은 collection,99는 Function Shift Lock Indicator 출력이므로 입력으로 거절합니다.
그 외 미정의/예약 값도 BAD_VALUE(03)입니다. 구형0700 펌웨어는81만 허용합니다.
응답92/93은 기존 `[status,queued]`이며 **명령 큐 수락**이지 USB report 전송이나
대상 OS 종료 완료가 아닙니다. DOWN/UP을 합치거나 firmware에서 pulse하지 않습니다.
앱이 DOWN 후 UP을 조합하며 ACK가 없어도 DOWN을 재시도하지 않습니다.
USB 비활성 상태의 DOWN은07(USB/serial not ready)로 거절하며 DTR/RTS는 필요 없습니다.
수락 이후 USB 연결/절전 epoch가 바뀐 전원 DOWN은 안전상 실행하지 않고, 전송
대기 report도 neutral로 바꿔 다음 부팅/재연결 때 늦게 전원 버튼이 눌리지 않게 합니다.
이 안전 취소는 System Control DOWN에 적용하며 일반 키보드/마우스 순서는 보존합니다.
이미 USB 전송에 제출되었거나 호스트가 받은 신호의 취소는 보장할 수 없습니다.

HID report ID3은 별도 System Control top-level collection의5-byte/40-bit report입니다.
명시적 usage40개를 위 순서로 나열하며 bit0=Power Down, bit13=Cold Restart,
bit14=Warm Restart, bit39=Rotation Lock Slider입니다. 각 bit의 DOWN/UP을 보존합니다.
키보드ID1/마우스ID2는 유지하며 USB bcdDevice는0800|function_mask입니다.
RELEASE_ALL은 키보드→마우스→시스템 report를 차례로 해제하고 각각 완료를 기다립니다.
전원 DOWN/UP, 일반 UP, RELEASE_ALL은 자동 Remote Wakeup을 유발하지 않습니다.
다음 정상 키/마우스 DOWN·MOVE가 중립 해제 뒤에 대기해도 새 activity generation으로
필요한 깨우기는 가능합니다. 기존61/WOL과 달리 이 기능은 OS의 전원 버튼 정책을
요청합니다. 지원하지 않거나 정책상 무시되는지는 USB ACK로 알 수 없습니다.
케이스 전원선 강제 차단,4초 하드웨어 강제 종료, S5 전원 켜기를 보장하지 않습니다.
Cold/Warm Restart도 host 구현이 필요하며 물리 RESET 핀 제어가 아닙니다.
Wake Up(83) report도 USB 활성 상태에서만 발신합니다. USB 절전 해제61/WOL과
혼동하지 마십시오. 일반 Fn 키 배열은 제공하지 않으며 System Function Shift97/98은
규격에 있는 별도 System Control 입력으로만 열거합니다.

응답은 펌웨어 거절(BAD_OPCODE/BAD_VALUE=미지원,07=USB 비활성)과 큐 수락을
구분하지만 호스트의 HID usage 지원/실행 여부를 알려주지 않습니다. 앱은 수락을
‘실제 동작 미확인’으로 표시하고 사용자의 동작함/안 됨 기록을 별도로 저장합니다.

USB 세션은 매 core1 loop에서 mounted/suspended를 polling합니다. RP2040의 강제
VBUS 감지는 분리와 절전을 구분하지 못하며 bus reset은 umount 콜백을 부르지 않을
수 있습니다. IRQ event hook은 reset/unplug 세대만 기록하고 polling이 이를 소비해
오래된 endpoint 제출 상태를 풀고 System Control epoch를 무효화합니다. 단순 절전이나
SOF 부재만으로 강제 detach하지 않습니다. TinyUSB가 감지하지 못한 물리 분리 자체를
이 방식으로 확정할 수는 없으며, 실제 타깃 재부팅 복구는 실기 확인이 필요합니다.

Windows GUI 키는 기존 KEY_DOWN/UP usageE3(left)/E7(right)로 이미 전달됩니다.
Ctrl+Alt+Delete는 E0→E2→4C DOWN 후4C→E2→E0 UP으로 호스트가 조합합니다.
별도의 조합/클릭/홀드 opcode는 추가하지 않습니다.

참고: [Microsoft keyboard input](https://learn.microsoft.com/en-us/windows/win32/inputdev/about-keyboard-input),
[HID System Power Down](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/acpi-button-device).
[USB-IF HID Usage Tables1.7](https://www.usb.org/sites/default/files/hut1_7.pdf).

## PC USB 깨우기 (61)

`61` LEN=0 → `E1` payload `[status, result]`. 정상 형식의 명령은 status=0이며,
깨우기 지원/상태에 따른 무시는 오류가 아니라 아래 result로 구분합니다.

| result | 의미 |
| --- | --- |
| 00 | USB Remote Wakeup 신호 발신; 실제 PC 기상 완료는 확인하지 않음 |
| 01 | 무시: USB가 suspend 상태가 아님 |
| 02 | 무시: 호스트가 Remote Wakeup을 허용하지 않음 |
| 03 | 무시: USB 미연결/미구성 |
| 04 | 무시: 선택 USB 기능 재연결 중 |
| 05 | 무시: USB 스택이 깨우기 요청을 지원/수락하지 않음 |
| 06 | USB core 처리100ms 기한 만료, 결과 미확인 (미지원 무시와 다름) |

UART core가 요청하고 USB core만 TinyUSB를 호출합니다. 최소5ms suspend 이후
한 번 신호를 보내며 키보드/마우스 report나 전원 키, RP2040 재부팅을 보내지
않습니다. 미지원 결과는 자동 재시도하지 않습니다. 기존 HID 큐에 의한 자동
깨우기도 suspend당 한 번만 시도하며 down/up report 순서는 유지합니다.
이 ACK는 큐 길이가 아닌 깨우기 결과입니다. 잘못된 LEN은 기존 BAD_LENGTH.
USB가 대기 전원을 받지 않거나 PC BIOS/OS가 지원하지 않으면 기상하지 않습니다.
완전 종료(S5)의 전원 켜기는 보장하지 않으며 실제 유선 LAN WOL 지원에 따릅니다.

## Wire examples

공백은 설명을 위한 것으로 실제 wire에는 포함되지 않습니다.

```text
KEY_DOWN A (HID usage 04)
02 10 01 00 04 03 15

KEY_UP A
02 11 01 00 04 03 16

MOUSE_DOWN left
02 21 01 00 01 03 23

MOUSE_UP left
02 22 01 00 01 03 24

MOUSE_MOVE center x=16384, y=16384, wheel=0, pan=0
02 20 06 00 00 40 00 40 00 00 03 A6

MOUSE_MOVE x=2, y=3, wheel=0, pan=0
02 20 06 00 1B 02 00 1B 03 00 00 00 03 2B
         ^^^^^       ^^^^^ escaped payload bytes
```

예를 들어 왼쪽 버튼 롱 홀드는 `MOUSE_DOWN left`의 정상 ACK를 받은 뒤 원하는
시간을 기다리고 `MOUSE_UP left`를 보내 구성합니다. 드래그는 시작 절대 좌표로
MOVE한 뒤 DOWN과 UP 사이에 하나 이상의 절대 MOVE frame을 보냅니다.

