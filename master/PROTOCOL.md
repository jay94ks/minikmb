# KBM master protocol v3

## 읽기 전용 가상 디스크 (v3, KBM Control 실행 필요)

512-byte sector를 기준으로 동작하며 DATA 한 패킷의 실제 데이터는 최대4096bytes입니다.
모든 패킷을4KiB로 보내는 것이 아닙니다. PING/ACK/HID는 짧은 가변 길이를 유지하고
FS도 요청된1..8개 sector(512..4096bytes)만 전달하며 padding하지 않습니다.

| Inner OP | Payload | Slave ACK |
|---|---|---|
|44 DISK_CONFIG|enabled:u8 generation:u32 sectors:u32|C4 status,queued|
|45 DISK_DATA|generation:u32 read_id:u32 lba:u32 offset:u16 crc32:u32 data[1..4096]|C5 status,queued|
|46 DISK_STATUS|empty|C6 status,flags|
|47 DISK_FAIL|generation:u32 read_id:u32 lba:u32|C7 status,queued|

모든 정수LE. generation!=0, OFF sectors=0, 최대32GiB. CONFIG는2초마다 heartbeat,
5초 만료/OFF/OS eject이면 MSC 자체를 숨깁니다. 변경 시300ms USB 재열거로
HID/CDC/NCM도 잠시 끊깁니다. flags bit0=configured,bit1=ready.
ACK는 큐 수락이며 CRC 완료/OS 마운트 완료가 아닙니다.

slave F8는 `generation:u32 read_id:u32 lba:u32 count:u16` (UART14bytes),
USB는 앞에 port:u8을 붙인15bytes입니다. count=1..8이며 실제 USB 읽기 크기를
따릅니다. 별도 credit 없이 미완료 요청을500ms 후 반복합니다. 앱은 해당 LBA부터
count*512bytes만 읽어 보통 DATA 하나로 응답합니다. offset은 블록 내 byte offset,
CRC32는 전체 count*512bytes(IEEE) 기준입니다. 분할 응답도 가능하나 순서/토큰/
전체 CRC를 확인하기 전에는 캐시에 넣거나 USB로 노출하지 않습니다.
FAIL은12-byte token 유지, 읽기 timeout4초. generation 변경/꺼내기는 캐시를 무효화합니다.

slave 캐시는512bytes씩96개=48KiB, 섹터별 LBA/valid/LRU를 관리합니다.
4KiB 고정 캐시 블록이나 전체 파일 캐시가 아닙니다. CRC 조립은 별도4KiB입니다.
master UART별 일반64/네트워크bulk16은 작은 정적 요소를 유지합니다.
큰 FS는 UART별1-entry 전용 정적 큐, slave도1-entry 큰 FS 정적 큐를 사용합니다.
모든 패킷 큐는 SDK의 capacity+1 ring slot까지 정적 배열로 예약하며 calloc하지 않습니다.

FS도 저우선이며 다음 프레임을 시작하기 전에 HID/일반 제어를 우선합니다.
단, 전송 중인 프레임은 끊지 않습니다.4KiB DATA의 최악 escape UART 프레임은
8234bytes,921600-8-N-1에서 약89.4ms입니다(일반 데이터 약45ms).
네트워크는 기존64-byte 조각을 유지합니다. UART stop-and-wait/실제 ACK를 지키며
host packet을 pipeline하지 않습니다. 네이티브 NCM 중에도 FS 명령은 허용됩니다.

기본 공유OFF, 원본/flash 쓰기 없음. 새로고침은 OFF 후 새 generation으로 ON,
앱/연결 종료 시 서비스 중지, lease 만료 후 숨김입니다. 앱 main의 native server만
F8에 응답하며 renderer는 제어/상태 UI만 담당합니다.


## Frame

USB CDC와 UART 모두 `STX OP LEN:u16le PAYLOAD ETX CHK`입니다. v2와 wire 비호환입니다.
STX=`02`, ETX=`03`, ESC=`1B`, LEN은16비트LE decoded payload 길이(구현 상한4118)입니다.
payload 안의 02/03/1B만 각각 `1B 02`/`1B 03`/`1B 1B`로 escape합니다.
CHK는 `(OP + LEN_low + LEN_high + decoded payload 바이트 합) & FF`입니다.
decoder는 바이트 단위 progressive 방식으로 read 분할/결합과 무관하게 동작하며
오류 후 다음 STX에서 재동기화합니다. 다중 바이트 정수는 little-endian입니다.

## 전역 선택 제거와 동시 제어

SELECT(70)는 제거됐습니다. 대상 없는 기존 HID/PING 요청도 전달하지 않고
status 07을 반환합니다. 각 ROUTE 요청에 port를 넣어 UART0/1을 독립적으로
제어합니다. master는 UART별 64개 FIFO, USB 응답 64개 FIFO를 사용합니다.
각 포트의 host 명령은 한 개씩 ACK를 기다리지만 다른 포트는 계속 진행합니다.
두 포트의 응답 순서는 서로 바뀔 수 있습니다.

## `73` ROUTE request

```text
port:u8 request_id:u16le slave_op:u8 slave_payload:bytes
```

- port: 00 또는 01. request_id는 호스트가 부여하며 포트 내 미완료 요청끼리
  중복되지 않아야 합니다. 앱은 두 포트를 합쳐 uint16 카운터를 사용합니다.
- 최소 payload 길이는4, 일반 slave payload는 최대251bytes, FS DATA만4114bytes입니다.
- slave_op는 00..6F 또는 7F이며 32/42(내부 수신 credit)는 외부 요청으로 금지합니다.
- master는 envelope를 제거하고 기존 slave frame으로 재인코딩합니다.
  예: KEY_DOWN은 `10 usage`이며 slave 펌웨어의 HID 규격은 그대로입니다.
- 연결 시 STATUS의 version을 검사한 뒤 요청하십시오. v1/v2 앱·펌웨어와 wire 호환되지 않습니다.

### `F3` ROUTE response

```text
port:u8 request_id:u16le bridge_status:u8 slave_response_op:u8 slave_payload:bytes
```

bridge_status=00이면 실제 slave 응답이며, slave_response_op는 `slave_op | 80`,
slave payload의 첫 바이트가 slave status입니다. 성공 ACK를 합성하지 않습니다.
HID/시리얼 WRITE의 정상 slave payload는 `[status, queued_count]`이며, ACK는
큐 수락을 뜻하고 USB 실행/원격 프로그램 수신 완료를 뜻하지 않습니다.

bridge_status가 0이 아니면 slave payload는 없고 op=00입니다. 포트·요청 ID는
원래 요청의 값을 돌려줍니다. 잘못된 envelope에 해당 필드가 없으면 port=FF,
id=0000을 사용합니다. 응답은 최소 5바이트, slave payload는 최대 250바이트입니다.
더 큰 slave 응답은 bridge status 02로 반환합니다.

| Bridge status | 의미 |
|---:|---|
| 00 | 실제 slave 응답 전달 |
| 01 | 지원하지 않는 명령 |
| 02 | 길이 오류 |
| 03 | 포트/내부 opcode 오류 |
| 04 | escape 오류 |
| 05 | ETX 오류 |
| 06 | checksum 오류 |
| 07 | 대상 envelope 없는 구 명령 |
| 08 | 해당 UART 송신 큐 가득 참 |
| 09 | UART ACK timeout |
| 0A | 네이티브 모드 소유/전환 중인 네트워크 또는 UART 속도 설정 충돌 |

frame decode 오류로 요청 ID를 신뢰할 수 없는 경우는 아래 local 오류 응답입니다.
호스트는 자신의 포트·요청 ID·내부 응답 opcode를 모두 대조하고, 일치하지 않는
늦은 USB 응답을 다음 요청의 ACK로 사용하지 않습니다. timeout 뒤 HID DOWN을
자동 재시도하지 마십시오. 명령은 실행됐으나 ACK만 유실됐을 수 있습니다.

## `71` STATUS request / `F1` response

request payload는 없습니다. 정상 응답은 21바이트입니다.

```text
status:u8 version:u8 online_mask:u8
uart0_tx:u16 uart0_rx_bytes:u16 uart0_rx_frames:u16 uart0_decode_errors:u16
uart1_tx:u16 uart1_rx_bytes:u16 uart1_rx_frames:u16 uart1_decode_errors:u16
uart0_serial_flags:u8 uart1_serial_flags:u8
```

version=03, online_mask bit0=UART0, bit1=UART1입니다. 카운터는 uint16 rollover를
허용합니다. UART TX에는 요청 및 내부 serial credit도 포함됩니다.
다른 local 오류 응답은 `request_op | 80` / `[status, version=03, online_mask]`입니다.

## 비동기 event

### `F2` ONLINE_CHANGED

`port:u8 online:u8`이며 값이 실제로 바뀔 때만 전송합니다.
모든 이벤트는 응답 사이에 삽입될 수 있으며 대기 중인 ACK를 소비하지 않습니다.

### `F4` SERIAL_DATA — slave USB 호스트 → 제어 PC

```text
port:u8 serial_sequence:u16le data:bytes (1..248)
```

slave의 USB COM에서 받은 raw byte입니다. 텍스트/바이너리 모두 가능하며
STX/ETX/ESC는 일반 payload byte로 escape합니다. 메시지/문자 경계를 가정하지 않습니다.
serial_sequence는 slave transport용이며 ROUTE request_id와 별개입니다.
master가 중복 재전송을 제거하므로 제어 PC는 추가 UART credit를 보낼 필요가 없습니다.

### `F5` SERIAL_STATE

`port:u8 flags:u8`이며 slave USB COM 제어 신호를 전달합니다.

| Bit | 의미 |
|---:|---|
| 0 | slave USB configured이고 suspend 상태가 아님 |
| 1 | slave 호스트가 DTR을 설정함 (COM 열림 신호) |
| 2 | slave 호스트가 RTS를 설정함 (수신 준비 신호) |

slave USB → UART 수신 준비는 `(flags & 03)==03`, UART → slave USB 송신 준비는
`(flags & 07)==07`입니다. USB의 DTR/RTS를 프로토콜로 전달하며 물리 UART
RTS/CTS 핀은 필요 없습니다. 신호는 호스트 프로그램이 설정하는 값이지,
상대 프로세스가 실제로 데이터를 읽었다는 보증은 아닙니다.

## 시리얼 중계 요청

- `ROUTE(port, id, 30, data[1..251])`: slave USB COM으로 raw data 전송.
  응답 inner op=B0, payload=`[slave_status, queued_count]`.
  slave status 07은 DTR/RTS 준비 안 됨, 08은 serial TX 큐 가득 참입니다.
- `ROUTE(port, id, 31, empty)`: 현재 slave 제어 신호 조회.
  응답 inner op=B1, payload=`[status, flags]`.
- master는 1초 간격으로 내부 31 요청도 보내 상태를 동기화합니다.
- master USB는 주 제어 CDC 하나(`8857:1223`)만 사용합니다. slave COM은
  `8857:1222`의 CDC interface 1이므로 VID/PID로 두 역할을 구분할 수 있습니다.

## UART serial credit 및 큐

slave는 한 번에 하나의 F4(`sequence:u16 data[1..248]`)만 미확인 상태로 두고,
500ms마다 재전송합니다. master는 USB 송신 큐에 수락한 후 UART 내부 명령
`32 sequence:u16`을 보내 credit를 반환합니다. 32 자체에는 응답이 없습니다.
동일 sequence의 재전송은 USB로 중복 전달하지 않고 credit만 다시 보냅니다.
큐가 가득 차면 한 이벤트를 보류하고 credit를 주지 않아 USB CDC까지 역압을 전파합니다.
slave serial TX 큐와 UART event 큐는 각각 64개입니다.

두 펌웨어는 UART 송신을 바이트 단위 비차단으로 진행해 긴 serial frame을
보내면서도 UART 수신과 다른 포트를 서비스합니다. ACK는 frame 경계에서 우선합니다.
버퍼링 용량은 유한하므로 호스트는 WRITE ACK/큐 가득 참 및 DTR/RTS를 처리해야 합니다.

## PING 및 timeout

각 UART가 100ms 무송신이면 PING을 보내며 내부 PING 응답은 PC로 전달하지 않습니다.
가장 오래된 미응답 송신이 500ms에 도달하면 해당 포트를 offline으로 바꾸고
미완료 host 요청에 bridge status 09를 반환합니다. 새 PING ACK로 재동기화한 뒤
해당 포트 큐를 재개합니다. 다른 포트는 중단하지 않습니다.
응답은 순서와 예상 opcode를 대조합니다. 내부 PING/상태 조회 응답만 유실된
경우 이후 호스트 ACK를 찾을 수 있지만 호스트 요청 자체를 건너뛰지 않습니다.
serial 이벤트는 이 응답 추적과 분리하며, offline 시 serial flags도 0으로 갱신합니다.

## CDC-NCM 이더넷 중계

기본 앱 모드에서 master는 NCM을 숨깁니다. 주 제어 CDC에 다음 ROUTE/event를
사용하고, slave의 NCM과 제어 PC 사이에서 raw Ethernet frame만 운반합니다.
프레임은 FCS를 제외한 14..1514바이트입니다. IP/TCP/UDP/DHCP는 KBM Control
Electron main의 lwIP/WASM과 DHCP 서버에서 처리합니다. slave 간 중계는 없습니다.

앱 기본 /24는 192.168.127.0, master/가상 서버/기본 게이트웨이는 192.168.127.1입니다.
DHCP option 3으로 .1을 배포하고 범위 .100..199에서 주소를 할당합니다.
낮은 NCM link speed로 기존 Ethernet의 자동 metric을 우선하도록 유도합니다.
현재 PC loopback 입구→slave TCP/UDP 포워딩을 제공하며 외부 Internet NAT/DNS는
이 기능에 포함되지 않습니다.

| Inner OP | Request payload | 실제 slave response payload |
|---:|---|---|
| 40 NET_WRITE | 아래 fragment | C0: status, queued_count |
| 41 NET_STATUS | 없음 | C1: status, flags, mac[6], advertised_bps:u32 |
| 43 NET_ENABLE | enabled:u8 (0/1) | C3: status, queued_count |

flags bit0=slave USB NCM data alternate 1이고 USB ready, bit1=master가 활성화 요청함입니다.
부팅 시 disabled이며 USB descriptor에서 NCM 자체가 숨겨집니다. 43으로 켜고 끄면
USB를 300ms 분리 후 재열거하므로 slave HID/CDC도 잠시 끊깁니다.
43 ACK는 USB 재열거 완료가 아닙니다. 앱은 41의 ready를 기다린 뒤 DHCP를 시작합니다.
40은 NCM 준비·enabled가 아니면 slave status 07, 완성 프레임의 USB 송신 큐가
가득 차면 08, 길이/순서/CRC 오류이면 02/03을 반환합니다.
중간 fragment의 ACK는 조각 수락, 마지막 ACK는 검증된 프레임의 큐 수락입니다.

```text
frame_id:u16 total_length:u16 offset:u16 crc32:u32 data[1..64]
```

CRC32는 전체 decoded Ethernet frame의 IEEE CRC-32(반사 다항식 EDB88320,
초기값 FFFFFFFF, 최종 XOR FFFFFFFF)입니다. 조각은 offset=0부터 순서대로
보내며 동일 프레임의 id/길이/CRC를 유지합니다. 호스트는 포트별 프레임 하나를
끝낸 뒤 다음 프레임을 보냅니다. 실패하면 프레임을 버리고 다음 id/offset0으로
시작합니다. Ethernet/TCP 재전송은 상위 스택이 담당하고 HID를 재시도하지 않습니다.

slave 호스트→제어 PC의 `F6` payload는 `port:u8 fragment`입니다. UART에서는
port 없이 같은 fragment를 전송합니다. master는 별도 USB 저우선 큐에 수락한 뒤
내부 `42 frame_id:u16 offset:u16` credit를 보냅니다(42에는 응답 없음).
slave는 한 조각씩 credit를 기다리고 500ms 후 동일 조각을 재전송합니다.
master가 id/offset 중복을 제거하므로 PC는 추가 credit를 발신하지 않습니다.
F6는 명령 ACK가 아니며 ACK 추적을 절대 소비하지 않습니다.

큐는 master UART별 일반 64개 + Ethernet 16개, USB 일반 64개 + Ethernet 16개,
slave Ethernet RX/TX 각 8프레임입니다. PC는 링크별 RX/TX 각 32프레임으로 제한합니다.
ACK/HID/일반 제어를 먼저 처리하고 Ethernet은 frame 경계가 아닌 **64-byte 조각
경계마다** 양보합니다. 이미 전송 중인 UART frame은 중간에 끊지 않으며 최악의
escape를 포함한 한 network 조각은 운영 921600bps에서 약 2ms 이내입니다.
TCP는 재전송·수신 윈도우 역압, UDP/큐 초과는 제한된 드롭 카운터로 처리합니다.

## 임시 UART 속도 시험

`ROUTE(port,id,50, baud:u32 lease_ms:u16)`는 1000..10000ms 동안만 UART 속도를
바꿉니다. 허용 속도는 115200/230400/460800/921600/1000000bps입니다.
slave D0 ACK는 기존 속도로 끝까지 전송하고 약 10ms 후 전환합니다.
master는 정상 D0 ACK 수신 후 30ms guard 동안 송신을 멈췄다가 같은 속도로
전환합니다. **양쪽은 PC 명령 없이도 lease 만료 시 각자의 CMake baseline으로
복귀**하며 flash에 저장하지 않습니다. master가 SET ACK를 못 받았더라도 slave가
전환했을 수 있으므로 PC는 lease+400ms 이상 기다려야 합니다.

`ROUTE(port,id,51, data[1..248])`의 D1 응답은 `status:u8 data`이며 원본을 그대로
반환합니다. HID 동작은 없습니다. 앱의 UART 시험은 네트워크/포워딩을 끄고 해당
포트 신규 HID를 잠근 뒤 각 속도에서 64개 패턴(55/AA,00/FF,escape,혼합)을
왕복 비교합니다. 오류가 나면 해당 속도를 재시도하지 않고 자동 복귀를 기다립니다.
1 Mbps는 ADuM1201ARZ 데이터시트 상한이지 실보드 안정성 보증이 아닙니다.
시험 결과는 짧은 부하 점검입니다. 운영 기본값 및 lease 후 복귀 속도는
921600bps이며 두 펌웨어를 같은 baseline으로 업데이트해야 합니다.

## `60` Slave RP2040 일반 재부팅

`ROUTE(port,id,60,52 50 32 30)` (`RP20`)은 지정 slave의 RP2040만 재부팅합니다.
실제 E0 `[00,00]` ACK의 software/hardware UART 송신이 모두 끝나고 100ms가
지난 뒤 watchdog으로 정상 flash boot합니다. PC 재부팅/BOOTSEL 명령이 아닙니다.
수락 후 새 명령을 받지 않으며 기존 HID/serial/network 큐는 재부팅으로 폐기됩니다.
ACK 유실 시 자동 재시도하지 말고 PING/상태로 복귀를 확인하십시오.
앱 모드에서 ON이던 네트워크만 복귀 후 43/41로 다시 활성화하고 DHCP 임대를
새로 만듭니다. 다른 UART의 네트워크/리스너는 종료하지 않습니다.

## `77` Master 네이티브 NCM 모드 / `F7`

빈 payload는 조회, SET payload는 **`native:u8 dhcp:u8`** (각 0/1, 정확히 두 바이트)입니다.
DHCP 사용 여부는 모드 전환의 인자이며 독립적인 UART 명령이 아닙니다.
응답은 `status requested_native requested_dhcp visible_ncm transition_mask` 5바이트입니다.
mask bit0/bit1은 slave OFF→ON 전환 대기입니다. SET 성공은 전환 수락이고 완료는
아닙니다. F7 송신 후 guard를 두고 master USB를 재열거합니다. 재연결 후 조회하십시오.
전환 중 SET은 0A이며 STATUS 형식/프로토콜 version3는 바꾸지 않습니다.

master는 CDC0/1 + reset2를 항상 유지하며 native ON에서만 NCM3/4를 추가합니다.
기본 OFF, flash에 모드를 저장하지 않습니다. SET 시 두 slave를 각각 43 OFF ACK →
600ms 대기 → 43 ON ACK 순으로 순환시킵니다(모드 OFF/DHCP 인자 변경도 포함).
offline slave는 다른 포트를 막지 않습니다. native ON이면 복귀 후 자동 활성화합니다.
native OFF 전환의 마지막 NCM ON은 명시적 master 활성화이며, 앱 네트워크/포워딩은
자동 시작하지 않습니다. 숨기려면 해당 slave에 43 OFF를 보냅니다.

native ON은 OS NCM ↔ UART0/1 slave Ethernet을 펌웨어가 직접 중계합니다.
MAC 학습으로 알려진 unicast는 해당 UART, broadcast/unknown은 준비된 UART로만
전달합니다. slave→slave 전달은 없습니다. 포트별 USB→UART 큐 8프레임,
UART→USB 큐 8프레임, CRC 검증/조각 credit/실제 ACK를 유지하며 HID가 우선입니다.
앱의 DHCP/IP/TCP/UDP/포워딩은 모두 중지됩니다. 40/43/50 ROUTE는 0A로 거부하고
41 조회, HID, serial, 60 재부팅은 허용합니다. 큐에 남은 bulk도 충돌 오류로 처리합니다.

### 네이티브 DHCP (`native=1, dhcp=1`)

DHCP는 master RP2040 core 0에서 처리합니다. **서버 식별자(option54)와 source IPv4는
192.168.127.1이고 master에 연결된 PC에도 .1을 배정**합니다. UART0=.100,
UART1=.101을 우선 배정하며 추가 slave MAC에는 .102..199를 배정합니다.
/24 mask, router option3=.1, DNS 없음, slave lease 3600초, 유한 lease 8개입니다.
master PC .1은 무기한 lease(FFFFFFFF)로 PC 자신의 .1로 향하는 OS 내부 unicast
갱신 문제를 피합니다. PC가 임의로 유한 lease/self-renewal을 사용하면 펌웨어는
USB에 나오지 않는 패킷을 가로챌 수 없습니다.

RP2040은 .1의 별도 일반 IP endpoint가 아닙니다. DHCP UDP 목적지 67 요청만
host/UART 어느 방향이든 포워딩 전에 소비하고 같은 ingress로 OFFER/ACK/NAK를
돌려줍니다. `.1` ARP 및 다른 TCP/UDP/IP는 PC 소유이며 가로채지 않습니다.
모드 전환 시 lease를 초기화하고 slave offline/60 재부팅 시 해당 UART lease만
초기화합니다. native DHCP OFF이면 DHCP 응답을 전혀 생성하지 않으므로 OS에서
수동 주소/외부 DHCP를 구성해야 합니다. 별도 NAT/DNS/인터넷 공유는 제공하지 않습니다.
규격 참고: [RFC2131](https://www.rfc-editor.org/rfc/rfc2131),
[RFC2132](https://www.rfc-editor.org/rfc/rfc2132).

## slave 부팅 세션 / HID 상태 이벤트

slave F9 LEN16을 받으면 master는 USB에 F9 LEN17 `[port,slave_report]`를 먼저
보고하고, 새 부팅 nonce에 한해서 해당 UART의 응답 tracker·진행 중 요청·기존
normal/bulk/4KiB 큐·serial dedup·network 조립 상태를 정리합니다. 다른 UART는 유지합니다.
진행 중/기존 큐 요청은 F3 bridge status0B(SLAVE_RESTARTED)로 취소 완료합니다.
이는 이전 동작이 전혀 실행되지 않았다는 보장이 아닙니다. 이미 발신된 신호는
되돌릴 수 없으며 USB/앱으로 전달 중인 과거 응답도 새 세션에서 성공으로 간주하면 안 됩니다.

취소는 USB 큐 여유에 맞춰 매 loop 최대 한 건씩 비차단 처리하고 새 큐의 경계는
보존합니다. DMA 소유 TX 버퍼는 수정하지 않고 송신/UART BUSY 종료 후 내부62로
nonce를 확인합니다. 이후 PING 응답으로 resync합니다. 같은 nonce 재전송은62만
재발신하며 큐를 또 비우지 않습니다.62는 PC ROUTE에서 사용 불가입니다.

FA LEN52 상태 보고는 USB FA LEN53 `[port,snapshot]`로 비차단 중계하고 ACK를 소비하지
않습니다.63 LEN0은 읽기 전용 routed HID 상태 조회이며 E3를 그대로 감쌉니다.
snapshot 레이아웃은 slave PROTOCOL의 USB/HID 진단 절을 참조하십시오.
앱은 해당 포트의 native/renderer 요청을 취소하고 디스크·앱 네트워크를 OFF하며,
키·마우스의 이전 로컬 추적 상태를 버립니다. native NCM 모드 정책은 firmware가
새 slave에 다시 적용합니다. legacy slave는 F9/FA를 내지 않으므로 상태 미지원으로 표시합니다.

## 특수 키와 System Control

Windows/GUI E3/E7, Ctrl+Alt+Delete 등의 조합은 기존10/11 DOWN/UP을 포트별로
순서대로 ROUTE합니다. 조합 opcode는 없습니다. System Control DOWN/UP은 slave12/13,
payload1byte를 기존 고우선 제어 큐로 중계하며 실제92/93(status,queued)를 감쌉니다.
slave0800은 page01 입력40종(81..8F,97..98,9A..9B,A0..AA,B0..B7,C9..CA)을
지원합니다. master는 이를 해석하거나 HID 키 조합으로 대체하지 않습니다.
master 동작 변경은 없으며 ACK는 명령 수락이지 실제 PC 절전/종료 확인이 아닙니다.
HID report ID3/USB descriptor 변경은 slave에만 있습니다. 명시적 slave BAD_OPCODE/
BAD_VALUE 거절은 펌웨어 미지원으로 구분하지만, 호스트 미지원이나 OS 정책에 의한
무시는 정상 큐 수락 ACK만으로 판별할 수 없습니다.

## PC 깨우기 ROUTE (slave61)

`73 [port, request_id:u16le, 61]`로 USB 절전 해제를 요청합니다.
master는 기존 고우선 제어 큐로 전달하고 실제 `E1 [status,result]`를
`F3 [port,id_lo,id_hi,bridge_status,E1,status,result]`로 감쌉니다.
result01..05는 정상적인 무시 결과이며 오류/재시도 대상이 아닙니다.
result06은 처리기한 만료로 결과 미확인이며 미지원 무시와 구분합니다.
result00=신호 발신(기상 확인 불가),01=이미 활성,02=호스트 미허용,
03=USB 미연결,04=USB 재연결 중,05=미지원/스택 거부,06=USB core 처리기한 만료.
정의는 slave PROTOCOL.md와 common/wake_protocol.h를 따릅니다.
미지원 구형 slave의 BAD_OPCODE(01)는 앱에서 result05로 정규화합니다.
UART 자체 OFFLINE/통신 실패와 미지원 상태는 구분하며 HID 입력을 합성하지 않습니다.

종료된 PC의 실제 유선 NIC로 보내는 WOL Magic Packet은 KBM Control PC에서
지정 LAN 어댑터로 직접 송신합니다. master UART/가상 NCM을 통해 보내는 명령이
아니며 응답이 없으므로 장치 지원 여부나 전원 켜짐을 확인할 수 없습니다.

## 예시 frame

```text
STATUS request
02 71 00 00 03 71

UART0 PING, request_id=0001
02 73 04 00 00 01 00 00 03 78

위 요청의 정상 slave ACK (status=0, queued=0)
02 F3 07 00 00 01 00 00 80 00 00 03 7B

UART1 PING, request_id=0002 (id의 02는 escape)
02 73 04 00 01 1B 02 00 00 03 7A
```
