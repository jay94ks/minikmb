# MiniKMB USB FS 관리 프로토콜 v1.0

설계 확정: 2026-11-16. 펌웨어 미구현·실기 미검증. 이 문서의 MUST/금지는 v1 구현 규칙이다. 과거 RP2040 UART v3와 wire 호환되지 않는다.

## 1. 범위와 연결

MCU 하나마다 USB FS CDC ACM 한 개를 사용한다. 그 byte stream에서 관리 명령, 응답, 디버깅, 업로드, 타깃 CDC, NCM frame을 다중화한다. 보드 선택은 호스트가 MCU UID serial로 CDC 장치를 선택하여 수행한다. 전역 SELECT나 MCU 간 ROUTE는 없다. 타깃 USB HS의 CDC ACM은 별개의 타깃 시리얼 인터페이스다.

관리 CDC baud/line coding은 전송 속도를 정하지 않는다. DTR은 세션의 유효성 기준으로 사용하지 않고 HELLO와 PING lease를 사용한다. CDC를 여는 프로세스는 하나로 제한한다. USB packet, TinyUSB read, OS read 경계를 메시지 경계로 해석하지 않는다.

FS descriptor 배정: interface 0=CDC control, interface 1=CDC data(IAD로 묶음), EP0 control, 0x81 interrupt IN notification(16 B, interval 16 ms), 0x02 bulk OUT와 0x82 bulk IN(각 max packet 64 B). 디버깅 전용 interface/endpoint는 추가하지 않는다. 이 번호는 FS controller 내부 번호이며 HS endpoint와 독립이다. 실제 descriptor와 호스트 CDC driver binding은 M1에서 검증한다. VID/PID는 사용 권한이 있는 값으로 별도 지정하고, serial은 STM32 UID 주소 오름차순의 12 byte를 24자리 대문자 hex로 표현한다(HELLO uid와 동일 byte 순서).

프로토콜은 인증·암호화 기능을 제공하지 않는다. 신뢰하는 로컬 관리 프로세스를 전제로 하며 CRC나 session ID를 인증 수단으로 표현하지 않는다. 네트워크 공개 제어는 별도 호스트 계층의 범위다.

## 2. 프레임

```text
USB CDC stream = COBS(HEADER[24] || PAYLOAD[0..1024] || CRC32[4]) || 00
```

모든 다중 byte 정수는 **little-endian**, 부호 있는 값은 2의 보수다. 정렬 padding은 없고 C struct를 그대로 송신하지 않는다. 모든 reserved/flags는 0이다. 문자열은 명시 길이의 UTF-8이며 NUL 종결자를 포함하지 않는다.

| offset | byte | 필드 | 정의 |
|---:|---:|---|---|
| 0 | 2 | magic | 0x4D4B, wire `4B 4D` |
| 2 | 1 | version | 1 |
| 3 | 1 | kind | 1 REQUEST, 2 RESPONSE, 3 EVENT, 4 STREAM |
| 4 | 1 | flags | 0 |
| 5 | 1 | channel | 아래 채널 번호 |
| 6 | 2 | opcode | 채널 내 명령 번호 |
| 8 | 8 | session_id | HELLO만 0, 그 외 현재 세션 |
| 16 | 4 | sequence | 요청 번호 또는 비동기 메시지 번호 |
| 20 | 2 | payload_length | 0..1024 |
| 22 | 2 | reserved | 0 |
| 24 | 가변 | payload | opcode별 본문 |
| 마지막 | 4 | crc32 | 인코딩 전 header+payload 전체의 CRC |

CRC는 CRC-32/ISO-HDLC: poly=0x04C11DB7, reflected poly=0xEDB88320, init=0xFFFFFFFF, refin/refout=true, xorout=0xFFFFFFFF. `123456789`의 결과는 0xCBF43926이고 wire는 `26 39 F4 CB`다. STM32 CRC peripheral 기본 설정과 일치한다고 가정하지 않는다.

COBS는 0 byte를 제거하는 블록 인코딩이다. code 1..254는 다음 code−1개의 nonzero byte 뒤에 0을 복원한다(블록이 입력의 끝이면 0을 붙이지 않음). code 255는 254개 byte를 복사하고 0을 복원하지 않는다. 송신 encoder는 마지막 code 블록을 항상 출력한다. 모든 원본 byte 값이 허용된다.

원본 최대 1052 B, COBS 최대 1057 B, delimiter 포함 **1058 B**. parser는 1088 B 고정 수신 슬롯으로 충분하다. 출력 크기는 반드시 검증하고 길이 필드를 믿고 메모리를 할당하지 않는다.

### 복구

- 연결/재동기화 시작 시 `00`을 전송해 이전의 미완성 프레임을 끊는다. 빈 delimiter는 무시한다.
- 1057 B 초과 encoded 입력은 다음 `00`까지 버린다. COBS·길이·CRC 오류 프레임은 응답 없이 폐기하고 카운터만 증가시킨다. 손상된 요청 번호로 오류 응답을 만들지 않는다.
- 정상 CRC 이후 version/channel/kind/flags/opcode와 정확한 payload 크기를 검증한다. MCU는 REQUEST 외의 kind 입력에 응답하지 않고 폐기한다. 현재 header 형식으로 해석 가능한 version 불일치 REQUEST는 version=1 RESPONSE로 UNSUPPORTED를 반환한다. HELLO 외 session 불일치에는 요청 session을 echo한 NO_SESSION을 반환하며 상태를 변경하지 않는다. 명령별 최대 길이도 검사한다.
- 부분 프레임이 2초 동안 추가 입력 없이 남으면 다음 delimiter까지 discard한다. timeout은 OS read 경계가 아니다.
- 송신 중인 한 프레임 안에 다른 프레임을 삽입하지 않는다. 우선순위 선점은 프레임 경계에서만 가능하다.

## 3. 채널과 요청 수

| channel | 이름 | 미응답 REQUEST 상한 | 용도 |
|---:|---|---:|---|
| 0 | SYSTEM | 1 | HELLO, PING, STATUS, credit, 긴 작업 조회·취소 |
| 1 | HID | 1 | 키·마우스·전체 해제 |
| 2 | SERIAL | 1 | 타깃 CDC 중계 |
| 3 | STORAGE | 4 | 업로드 DATA만 window 4; 나머지는 barrier |
| 4 | NETWORK | 1 | NCM Ethernet frame |
| 5 | DEBUG | 1 | 로그 설정·조회 |

다른 채널은 독립 진행한다. STORAGE 처리 대기로 SYSTEM PING/취소가 막히면 안 된다. 한 MCU에서 최대 미응답 요청은 합계 9개다. 두 MCU의 세션·큐는 관리 호스트에서도 독립이다.

### 번호, 중복, ACK

각 채널의 REQUEST sequence는 세션 시작 후 1부터 연속 증가한다. HELLO는 예외적으로 session=0/sequence=1이고 세션 내 번호를 소비하지 않는다. 번호 0과 wrap은 금지하며 소진 전에 새 세션을 연다.

- RESPONSE는 요청의 channel/opcode/sequence/session을 그대로 반환한다. HELLO 응답 header session만 새 session ID다.
- 요청 접수 시 채널 expected sequence를 증가시킨다. 정상 형식의 요청이 UNSUPPORTED/BUSY 등으로 거절되어도 응답을 저장하고 번호를 소비한다.
- 마지막 4개 요청의 **정확한 header+payload**와 최종 응답을 채널별로 보존한다. 같은 번호·같은 내용은 응답만 재전송하고 부작용을 재실행하지 않는다. 진행 중 복제 요청은 원본 완료 응답을 기다리며 재실행하지 않는다.
- 같은 번호·다른 내용은 ID_CONFLICT. 과거 번호가 캐시에 없으면 STALE_SEQUENCE. 미래 번호가 expected보다 크면 SEQUENCE_GAP. 이 세 오류는 번호를 소비하지 않는다.
- window 4 호스트는 가장 오래된 미확인 요청부터 연속해서 확인된 만큼만 window를 전진한다. ACK hole을 남긴 채 새 DATA를 계속 전송하지 않는다.
- BUSY를 받은 요청을 나중에 다시 시도할 때는 **새 번호**를 사용한다. 동일 번호를 반복하면 캐시된 BUSY가 돌아온다.
- 응답 슬롯과 요청 저장 공간을 먼저 확보한 뒤 명령을 수락한다. 큐 포화 상태에서 성공을 먼저 ACK하지 않는다. 규약을 지키는 9개 요청의 응답 공간은 항상 예약한다.

정상 RESPONSE payload는 `status:u16, reserved:u16=0, detail:u32, result[]`다. status=0의 detail은 0. 오류에는 별도 result가 없다. 성공 result는 최대 256 B이며 ECHO도 이 상한을 따른다.

| 값 | status | detail |
|---:|---|---|
| 0 | OK | 0 |
| 1 | UNSUPPORTED | 지원하지 않는 opcode/version 등 값 |
| 2 | BAD_PAYLOAD | 잘못된 payload offset, 식별 불가 시 0 |
| 3 | BAD_STATE | 현재 storage state |
| 4 | BUSY | 재시도 권장 ms, 보통 10 |
| 5 | NO_SESSION | 0 |
| 6 | STALE_SEQUENCE | 다음 expected sequence |
| 7 | ID_CONFLICT | 0 |
| 8 | SEQUENCE_GAP | 다음 expected sequence |
| 9 | NOT_READY | 0=일반, 1=HS 미연결, 2=카드 없음, 3=CDC 미개방 |
| 10 | RANGE | 0 |
| 11 | IO_ERROR | 계층별 내부 오류 코드; DEBUG로 추가 설명 |
| 12 | CHECKSUM_MISMATCH | 0 |
| 13 | CANCELLED | 0 |
| 14 | NO_SPACE | 0 |
| 15 | FRAGMENT_LIMIT | extent 상한 |
| 16 | READ_ONLY | 0 |
| 17 | NOT_FOUND | 0 |

오류 코드의 내부 detail은 안정적인 API로 추측하지 않는다. 펌웨어별 의미가 필요하면 별도 enum을 문서화한다. CRC 오류는 위 RESPONSE 대상이 아니다.

EVENT/STREAM은 MCU→호스트 전용이며 channel별로 공유하는 별도 sequence 카운터를 사용한다(1부터, REQUEST와 별개). wrap 전 세션을 종료한다. 이벤트 유실은 sequence gap 및 STATUS로 검출 가능하며 ACK하지 않는다. 호스트→MCU 데이터는 REQUEST로 보내 수락 응답을 받는다.

## 4. 세션과 연결 단절

HELLO payload: `min_version:u8=1, max_version:u8=1, reserved:u16=0, host_nonce:u64(nonzero)`.

성공 result: `echo_nonce:u64, uid[12], fw_major:u16, fw_minor:u16, fw_patch:u16, max_payload:u16=1024, features:u32, lease_ms:u32=2000, upload_window:u8=4, reserved[3]=0` (40 B).

HELLO의 version 범위에 1이 없거나 min>max이면 UNSUPPORTED이며 세션을 바꾸지 않는다. opcode와 레이아웃 변경이 필요한 호환 불가 수정은 새 version으로 한다. 기존 payload 뒤에 필드를 임의 추가하지 않는다.

session ID는 MCU RNG로 생성하는 0이 아닌 u64다. RNG 오류 시 세션을 만들지 않고 NOT_READY를 반환한다. active session에서 같은 nonce의 HELLO는 기존 ID와 HELLO 결과를 재전송한다. 다른 nonce는 기존 세션을 안전하게 닫은 후 새 세션을 만든다. 종료 처리가 진행 중이면 HELLO에 BUSY를 반환한다. nonce는 재시도 시 유지하고 새 연결에는 새 난수를 사용한다.

features bit: 0=HID, 1=target CDC, 2=storage upload, 3=예약(0), 4=read-only optical MSC, 5=NCM frame bridge, 6=debug messaging, 7..31=0. **구현된 기능만** 설정한다. 예약된 opcode를 구현했다고 주장하지 않는다.

관리 호스트는 500 ms마다 SYSTEM PING을 보내며 MCU는 마지막 정상 current-session REQUEST부터 2000 ms lease를 갱신한다. 대용량 작업 중에도 heartbeat를 유지한다. lease 만료, FS unmount/reset, 또는 다른 nonce의 HELLO는 세션을 종료한다. USB suspend는 lease를 정지하지 않는다.

종료 시: 미실행 HID 폐기·가능한 즉시 전체 release, SERIAL 큐 폐기, NCM link down·조립 frame 폐기, credit/log 구독 초기화. 업로드/검증 작업은 취소하여 부분 이미지를 제공하지 않는다. 이미 완료되어 SERVING 중인 읽기 전용 이미지는 **타깃 부팅을 끊지 않도록 계속 제공**한다. 새 세션에서 STATUS로 확인 후 명시적으로 unmount한다. 재부팅 후에는 자동 mount하지 않는다.

새 세션은 이전 요청의 실행을 재개하지 않는다. 키 DOWN, mouse wheel, serial write 등을 새 세션에서 자동 재전송하지 않는다. 세션 중 timeout은 동일 번호/동일 payload 재시도만 허용한다. 일반 응답 timeout 1000 ms, 최대 2회 재시도 후 session recovery. STORAGE DATA는 5000 ms. 긴 작업은 아래 job 방식이라 수십 초 동안 REQUEST 응답을 붙잡지 않는다.

## 5. SYSTEM 명령 (channel 0)

표의 result는 공통 RESPONSE 8 B prefix 이후다. 빈 payload/result는 0 B다.

| opcode | 이름 | REQUEST payload | OK result |
|---:|---|---|---|
| 0x0001 | HELLO | 위 12 B | 위 40 B |
| 0x0002 | PING | cookie:u64 | cookie:u64, uptime_ms:u64 |
| 0x0003 | INFO | 빈 값 | HELLO의 40 B result와 동일(현재 nonce 포함) |
| 0x0004 | STATUS | 빈 값 | 아래 64 B |
| 0x0005 | ECHO | 임의 0..256 B | 동일 bytes |
| 0x0006 | RX_LIMIT | channel:u8, reserved[3], limit:u64 | sent_payload_bytes:u64, effective_limit:u64 |
| 0x0007 | JOB_STATUS | job_id:u32 | 아래 32 B |
| 0x0008 | JOB_CANCEL | job_id:u32 | 빈 값, 취소 요청 접수 |
| 0x0009 | UPLOAD_CANCEL | upload_id:u32 | 빈 값, STORAGE ABORT와 동일한 취소 접수 |

STATUS result 순서: `uptime_ms:u64, features:u32, hs_state:u8, storage_state:u8, cdc_open:u8, ncm_up:u8, active_job:u32, image_id:u32, upload_id:u32, log_dropped:u32, crc_errors:u32, framing_errors:u32, upload_received:u64, upload_total:u64, last_error:u16, reserved:u16, event_dropped:u32` = 64 B. 카운터는 부팅 후 u32 누적(mod 2^32), uptime·byte 수는 u64.

hs_state: 0=unconfigured, 1=FS configured, 2=HS configured, 3=suspended. storage_state: 0=NO_CARD, 1=IDLE, 2=PREPARING, 3=UPLOADING, 4=VERIFYING, 5=IMAGE_READY, 6=MOUNTING, 7=SERVING, 8=UNMOUNTING, 9=ERROR, 10=FORMATTING. bool은 0/1.

JOB_STATUS result: `job_id:u32, state:u8, kind:u8, result_status:u16, detail:u32, object_id:u32, done_bytes:u64, total_bytes:u64` = 32 B. state 0=running,1=success,2=failed,3=cancelled. kind 1=upload begin,2=upload commit,3=mount,4=unmount,5=card init,6=user value write,7=disk create,8=disk commit,9=disk open,10=disk close,11=disk read,12=disk write,13=disk sync. running result_status=OK는 완료를 뜻하지 않는다. 완료 후 object_id는 upload ID 또는 image ID이며 필요 없는 경우 0.

긴 작업 명령의 OK+job_id는 job 접수 ACK다. JOB_STATUS 또는 JOB_CHANGED의 terminal state로 실제 완료를 판정한다. 완료 EVENT는 유실 가능하므로 호스트는 최대 500 ms 간격 polling으로 보완한다. 이미 terminal인 JOB_CANCEL은 상태를 바꾸지 않고 OK, 알려지지 않은 job ID는 NOT_FOUND다.

job는 한 MCU에 동시 1개, 완료 이력 8개를 RAM에 보관한다. 다음 job를 받아도 이력에 남으며 job ID는 부팅 중 재사용하지 않는다. overflow 전 새 작업을 거절한다. 재부팅 후 이력은 사라진다. 취소는 저장소 작업의 안전한 경계에서 적용되며 성공 완료와 경합하면 실제 최종 상태가 우선이다. CANCEL 응답 자체는 취소 완료 보증이 아니다.

## 6. HID 명령 (channel 1)

| opcode | 이름 | REQUEST payload |
|---:|---|---|
| 0x0101 | KEY_DOWN | usage:u16 |
| 0x0102 | KEY_UP | usage:u16 |
| 0x0103 | MOUSE_MOVE | x:u16, y:u16 |
| 0x0104 | BUTTON_DOWN | button:u8 |
| 0x0105 | BUTTON_UP | button:u8 |
| 0x0106 | WHEEL | vertical:i16, horizontal:i16 |
| 0x0107 | RELEASE_ALL | 빈 값 |

키는 USB keyboard usage page 0x07의 0x04..0xA4 및 modifier 0xE0..0xE7 중 descriptor가 지원하는 값이다. 비지원 값은 UNSUPPORTED. 기본 boot profile은 일반키 최대 6개+modifier이며 7번째 일반키 DOWN은 BUSY, 상태 불변이다. MOVE는 0..32767. button=1 left,2 right,3 middle,4 back,5 forward. wheel은 각 축 −127..127, 0 허용이다.

성공 result는 `hid_ticket:u32`. ACK는 **HID 큐 수락**이며 타깃 실행 완료가 아니다. 동일 키 DOWN/UP·button 상태 설정은 중복 상태를 만들지 않는다. wheel은 누적 상대 동작이므로 요청 dedup이 필수다. FIFO 용량 32개, DOWN/UP 순서 유지. v1은 MOVE 병합도 하지 않는다.

ticket은 부팅 중 재사용하지 않는다. HS USB report 전송 완료 시 HID_DONE EVENT(0x0180)를 보낸다: `ticket:u32, result_status:u16, reserved:u16`. 이는 PC 앱이 입력을 처리했다는 보증이 아니다. 완료 이벤트 큐는 64개를 보관한다. 이벤트를 잃어도 DOWN을 새 번호로 다시 보내지 않는다.

RELEASE_ALL은 별도 예약 슬롯을 이용한다. 미실행 HID를 취소하고 이미 전송 중 report가 끝난 뒤 neutral report를 보낸다. 취소된 ticket은 CANCELLED 이벤트 대상이다. HS 미연결이어도 내부 상태 해제 요청을 수락하며 다음 연결의 첫 report는 neutral이다. 나머지 명령은 HS 미연결이면 NOT_READY, UPLOADING/VERIFYING/PREPARING/FORMATTING이면 BAD_STATE.

## 7. 타깃 CDC (channel 2)

- `0x0201 SERIAL_WRITE`: payload=`data[1..256]`, result=`accepted_bytes:u16`(전체 길이). 전부 저장할 여유가 없으면 BUSY이며 부분 수락하지 않는다. 타깃 USB가 구성되지 않았거나 DTR로 열지 않았으면 NOT_READY. FS와 HS 모두 같은 중계 프로토콜을 사용한다. ACK는 큐 수락으로 실제 타깃 소비와 구분한다.
- `0x0202 SERIAL_STATUS`: 빈 request. result=`open:u8, stop_bits:u8, parity:u8, data_bits:u8, baud:u32, tx_queued:u32, rx_queued:u32`. line coding은 타깃이 설정한 CDC 값이다. 설정을 관리 PC baud와 혼동하지 않는다.
- `0x0203 SERIAL_STATS`: 빈 request. result=`tx_discarded:u32, rx_discarded:u32`. 브리지에서 폐기한 바이트의 부팅 후 누적값이며 전송을 이미 시작한 USB 패킷은 장치·호스트에서 완료될 수 있다.
- `0x0280 SERIAL_RX`: STREAM, payload=`data[1..256]`, 아래 credit 규칙 적용.

타깃→관리 버퍼가 차면 TinyUSB CDC OUT 읽기/수신 rearm을 제어해 backpressure를 걸며 bytes를 조용히 버리지 않는다. 연결 단절·업로드 전환으로 큐를 폐기하면 상태 이벤트와 카운터로 알린다. 보존 가능한 범위는 세션 내이며 재연결 간 serial exactly-once는 보장하지 않는다.

## 8. 저장소와 업로드 (channel 3)

v1은 **한 개의 active image**를 제공한다. 서버가 내부 경로를 소유하고 사용자가 보낸 filename은 표시 이름일 뿐 경로로 사용하지 않는다. 새 UPLOAD_BEGIN은 기존 매체를 먼저 해제하고 현재 active image를 교체하는 명시적 작업이다. 관리 UI는 이 의미를 표시한다. 이전 이미지 보존이나 두 이미지 용량 확보는 v1 보장이 아니다.

| opcode | 이름 | REQUEST payload | OK result |
|---:|---|---|---|
| 0x0301 | UPLOAD_BEGIN | size:u64, crc32:u32, reserved[28]=0, media_kind:u8, reserved:u8=0, name_len:u16, name[name_len] | job_id:u32 |
| 0x0302 | UPLOAD_DATA | upload_id:u32, offset:u64, data[1..1008] | next_accepted_offset:u64 |
| 0x0303 | UPLOAD_COMMIT | upload_id:u32 | job_id:u32 |
| 0x0304 | UPLOAD_ABORT | upload_id:u32 | 빈 값, 취소 접수 |
| 0x0305 | IMAGE_INFO | 빈 값 | 아래 result |
| 0x0306 | IMAGE_MOUNT | image_id:u32 | job_id:u32 |
| 0x0307 | IMAGE_UNMOUNT | image_id:u32 | job_id:u32 |

현재 프로파일은 media_kind=1인 읽기 전용 2,048-byte 광학 이미지다. 값 2는 예약하며, 1 이외에는 UNSUPPORTED를 반환한다. 크기는 0보다 큰 2,048의 배수다. name_len=1..120, UTF-8 valid, NUL/control/slash/backslash 금지. 펌웨어는 이 필드와 저장 이미지 전체 CRC-32/ISO-HDLC을 검증한다. 경로 이동·임의 파일 읽기/삭제 명령은 제공하지 않는다.

이미지 CRC32는 프레임과 같은 CRC-32/ISO-HDLC를 원본 이미지 전체에 적용한다. UPLOAD_BEGIN의 예약 필드는 0이어야 한다.

관리 호스트는 El Torito BIOS 또는 UEFI 무에뮬레이션 부팅 항목이 있는 ISO 9660·UDF 광학 이미지를 준비한다. 호스트 도구가 볼륨·부팅 카탈로그의 공통 구조를 검사한 뒤, 이미지를 이 표의 내부 BEGIN/DATA 형식으로 전송한다. 데이터 연결 순서와 이미지는 그대로 보존하며 타깃은 이를 표준 광학 블록으로 읽는다. M6 호스트 준비 및 검증 (development reference omitted)

IMAGE_INFO result: `image_id:u32, size:u64, media_kind:u8, mounted:u8, name_len:u16, crc32:u32, reserved[28]=0, name[name_len]`. 이미지가 없으면 NOT_FOUND. name_len 포함 최대 168 B. image_id는 부팅 중 유일하며 카드 재삽입/재검증 후 새 ID를 부여한다. 저장된 파일을 복구할 때 검증 완료 전에는 active로 표시하지 않는다.

BEGIN은 job 접수 후 PREPARING에 들어가며 HID release, serial/NCM 중단, MSC no-media·진행 중 읽기 정리, FatFs mount와 임시 파일 준비를 수행한다. job 성공의 object_id가 upload_id다. 이 성공을 확인한 뒤 DATA를 전송한다. BEGIN job 실패 시 일부 이전 기능이 해제되었을 수 있으므로 STATUS로 확인한다.

DATA는 offset=현재 next accepted offset일 때만 수락한다. 한 프레임의 data는 최대 1008 B(payload 전체 1020 B), 중간 chunk는 1008 B, 마지막만 작게 한다. 호스트는 최대 4개 chunk를 순서대로 pipeline할 수 있다. 수락 응답은 RAM staging에 복사되었다는 뜻이며 SD 영구 저장 완료가 아니다. 재전송은 같은 번호·내용을 사용한다. 범위 초과·불연속 offset은 RANGE, 이때 detail은 0이고 STATUS의 upload_received로 위치를 재확인한다. 비동기 SD 기록 실패 시 업로드를 ERROR로 바꾸고 이후 DATA는 BAD_STATE다.

COMMIT은 앞선 모든 DATA 응답 수신 후 전송한다. worker가 남은 기록을 끝내고 f_sync/close, 저장 파일을 다시 읽어 CRC32 검증, extent map 작성, FatFs unmount, image 등록까지 수행한다. 성공 JOB_STATUS의 object_id가 새 image_id다. 이 시점이 업로드 완료이며 아직 타깃에 mount한 것은 아니다. 파일 수락 ACK를 완료로 표시하지 않는다.

extent 최대 4096개, 각 16 B, 총 64 KiB. 넘으면 FRAGMENT_LIMIT(4096)로 실패하고 파일은 serving하지 않는다. 파일 offset·크기 계산은 u64 checked arithmetic. FatFs를 unmount한 후 FIL/f_read를 사용하지 않는다. MSC는 불변 extent map을 통해 파일 내용만 읽는다.

ABORT/CANCEL은 진행 중 DMA를 안전하게 끝낸 후 파일을 닫고 incomplete 표시를 유지한다. ABORT의 최종 결과는 STATUS(storage가 UPLOADING/VERIFYING을 벗어남)로 확인한다. BEGIN 진행 중에는 SYSTEM JOB_CANCEL을 사용한다. 전원 차단/lease 만료 후 업로드 재개는 v1에서 지원하지 않으며 새 BEGIN으로 다시 시작한다.

STORAGE의 비-DATA 명령은 같은 채널의 미응답 DATA가 모두 확인된 뒤 보낸다. SYSTEM STATUS/JOB_CANCEL/UPLOAD_CANCEL은 다른 채널이므로 언제든 전송 가능하다. DATA 응답을 기다리는 동안 업로드를 중단하려면 SYSTEM UPLOAD_CANCEL을 사용한다. PREPARING/UPLOADING/VERIFYING에서는 HID RELEASE_ALL, SYSTEM, DEBUG, 업로드 관련 명령만 허용한다. IMAGE_READY 이후 HID는 다시 허용하지만 NCM은 명시적인 재활성화가 필요하다. 이미지 serving 자체는 관리 연결이 끊어져도 계속한다.

### 카드 보드 ID와 사용자 정의 식별값

| opcode | 이름 | REQUEST payload | OK result |
|---:|---|---|---|
| 0x0308 | CARD_INFO | 빈 값 | 아래 52 B |
| 0x0309 | CARD_INIT | board_uuid[16] | job_id:u32 |
| 0x030A | CARD_USER_READ | revision:u32, offset:u16, length:u16(1..256) | data[length] |
| 0x030B | CARD_USER_BEGIN | total_length:u16(0..4096) | transfer_id:u32 |
| 0x030C | CARD_USER_DATA | transfer_id:u32, offset:u16, data[1..1008] | next_offset:u16 |
| 0x030D | CARD_USER_COMMIT | transfer_id:u32 | job_id:u32 |
| 0x030E | CARD_USER_ABORT | transfer_id:u32 | 빈 값 |
| 0x030F | CARD_FORMAT | filesystem:u8 (1 FAT32, 2 exFAT), reserved[3]=0 | job_id:u32 |

CARD_INFO 결과는 `flags:u8, reserved:u8=0, user_length:u16, revision:u32, board_uuid[16], registered_uid[12], controller_uid[12], profile_status:u16, user_status:u16`이다. flags bit 0은 카드 존재, bit 1은 유효한 보드 ID, bit 2는 유효한 사용자 값으로 해석한다. 값이 없으면 해당 필드를 0으로 읽으며, 사용자에게 배정된 0바이트 값과 미설정 상태는 bit 2로 구분한다. 읽는 동안 새 값이 커밋되거나 카드가 바뀌면 이전 revision으로 이어서 읽을 수 없다.

CARD_INIT은 이미 포맷된 저장 볼륨에 MiniKMB 메타데이터를 생성한다. 동일한 UUID 및 MCU UID로 반복하면 기존 정보를 유지한다. 다른 식별자로 덮어쓰라는 요청은 BAD_STATE다. 카드 식별 조회는 업로드와 광학 제공 중에도 가능하다. 카드에 기록하려면 광학 매체를 관리에서 분리하고 업로드를 끝낸다.

유저 값 전송은 한 세션이 소유하며 연속 오프셋을 요구한다. 먼저 전송을 받은 뒤 COMMIT job에서 CRC32를 포함해 저장한다. 커밋 완료 전에는 이전 값을 읽을 수 있다. BEGIN 뒤 세션이 만료되거나 카드가 분리되면 미커밋 데이터는 폐기한다. 취소와 실제 저장 완료가 경합하면 최종 JOB_STATUS를 따른다. 두 저장 슬롯 중 CRC32가 유효한 최신 세대를 사용한다.

CARD_FORMAT은 선택한 MCU의 microSD에 새 단일 파티션과 빈 파일시스템을 만든다. 카드의 기존 데이터는 삭제된다. 광학·쓰기 디스크가 모두 분리되어 있고 저장소 작업이나 사용자 값 전송이 진행 중이지 않을 때 수락한다. 실행 중 FORMATTING으로 표시하고 MSC에는 매체를 제공하지 않는다. 세션이 끊겨도 이미 수락한 포맷은 진행되며 JOB_CANCEL은 BAD_STATE를 반환한다. JOB_STATUS의 kind는 14다. FatFs가 지정한 파일시스템으로 다시 마운트하면 성공과 IDLE을 반환하며, CARD_INIT과 이미지 업로드는 다시 실행할 수 있다. 호스트 예시 `python tools/card_profile.py --uid <MCU-UID> format exfat --erase`.

### 관리 직접 쓰기 디스크와 USB MSC

| opcode | 이름 | REQUEST payload | OK result |
|---:|---|---|---|
| 0x0310 | DISK_CREATE | disk_id:u32, sectors:u32, format:u8, reserved[3]=0 | job_id:u32 |
| 0x0311 | DISK_COMMIT | lease:u32, crc32:u32 | job_id:u32 |
| 0x0312 | DISK_OPEN | disk_id:u32, writable:u8, path:u8, reserved[2]=0 | job_id:u32 |
| 0x0313 | DISK_CLOSE | lease:u32 | job_id:u32 |
| 0x0314 | DISK_READ | lease:u32, lba:u32 | job_id:u32 |
| 0x0315 | DISK_WRITE | lease:u32, lba:u32, data[512] | job_id:u32 |
| 0x0316 | DISK_RESULT | lease:u32, read_job_id:u32, half:u8(0/1) | data[256] |
| 0x0317 | DISK_SYNC | lease:u32 | job_id:u32 |
| 0x0318 | DISK_INFO | 빈 값 | disk_id:u32, sectors:u32, lease:u32, format:u8, state:u8, writable:u8, reserved:u8=0 |

disk_id는 1..0x0FFFFFFF, format은 1=FAT32, 2=exFAT, 3=EXT4다. 각 ID로 별도 파일을 만든다. DISK_CREATE가 성공하면 0으로 초기화된 파일의 리스 ID를 JOB_STATUS object_id로 받는다. 관리 호스트가 포맷된 이미지에서 0이 아닌 섹터를 기록하고 원본 이미지 전체의 CRC-32/ISO-HDLC를 DISK_COMMIT으로 보낸다. SD 파일을 다시 열어 전체 CRC가 일치한 뒤 완료 기록을 만든다. COMMIT은 리스를 닫는다. 완료 기록을 쓰기 전에 끊긴 생성은 같은 ID로 다시 시작할 수 있다. 유효한 완료 기록이나 길이가 정상인 손상 기록은 생성으로 덮어쓰지 않는다.

완료된 디스크는 DISK_OPEN으로 읽기 전용 또는 쓰기 가능 리스를 연다. path는 0=관리 직접 섹터 I/O, 1=타깃 MSC, 2=관리 MSC다. JOB_STATUS의 성공 object_id는 해당 리스다. 읽기 완료 뒤 두 DISK_RESULT로 한 섹터를 꺼낸다. 새로운 섹터 요청이 들어오거나 리스가 닫히면 이전 읽기 결과는 폐기한다. 쓰기 작업 성공은 파일 쓰기 완료이며, 명시적 DISK_SYNC나 성공한 DISK_CLOSE를 기다려 기록의 동기화를 확인한다. 관리 직접 접근은 CDC 세션 만료 뒤 워커가 닫는다. MSC로 노출한 디스크는 CDC 세션이 끝나도 유지된다. 열려 있는 디스크는 동일한 SD 파일시스템의 광학 업로드·카드 메타데이터 쓰기와 겹치지 않는다. 기존 광학 이미지의 읽기는 독립적으로 유지된다. 하나의 쓰기 디스크 이미지는 어느 순간에도 한 경로에만 노출된다. 다른 경로로 넘기려면 기존 OS에서 복사와 파일시스템 쓰기를 끝내고 안전하게 분리한다. OS가 SCSI START STOP UNIT의 eject를 보내면 워커가 닫는다. OS에서 파일시스템 사용을 끝냈고 eject를 보내지 않는 경우 새 CDC 세션에서 DISK_INFO로 리스를 읽어 DISK_CLOSE를 실행할 수 있다. MSC는 SD 파일을 동기화한 뒤 WRITE(10) 성공을 반환한다.

상태는 1=열기/준비 대기, 2=새 이미지 준비 중, 3=관리 직접 접근, 4=타깃 MSC, 5=관리 MSC다. 진행 중인 다른 저장소 job이나 다른 리스가 있으면 BAD_STATE다. FAT32 SD 볼륨에서는 단일 이미지 파일 크기에 FAT32의 제한이 적용된다. 광학 이미지와 쓰기 디스크의 파일 이름과 용량은 각각 독립적이다.

## 9. 네트워크 (channel 4)

- `0x0401 NET_SET`: payload=`enabled:u8(0/1)`; result=빈 값. 기본 OFF. 타깃 NCM USB가 구성됐고 관리 세션이 유지되며 업로드 중이 아닐 때 ON. FS 개발 프로필과 HS 프로필에 공통으로 적용한다. host NIC 생성/IP 설정까지 자동 완료되는 의미가 아니다.
- `0x0402 NET_FRAME`: 아래 fragment payload, result=빈 값. ACK는 fragment/frame 큐 수락이다.
- `0x0403 NET_STATS`: 빈 request; result=`target_rx_dropped:u32, target_tx_dropped:u32, credit_dropped:u32, enabled:u8, link_up:u8, reserved:u16=0` (16 B). 카운터는 부팅 후 누적이다.
- `0x0480 NET_RX`: MCU→호스트 STREAM, 같은 fragment payload.

fragment=`frame_id:u32, offset:u16, total_length:u16, frame_crc32:u32, reserved:u32=0, data[1..1008]`. 전체 frame은 Ethernet header 포함 14..1518 B, FCS 제외, IP MTU 최대 1500. 전체 CRC는 §2 알고리즘으로 Ethernet bytes 전체에 계산한다. NIC로 넘기기 전 길이·모든 fragment·전체 CRC를 검사한다.

방향별 frame_id는 1부터 증가, wrap 전 새 세션. 한 방향에 동시에 조립하는 frame은 1개다. offset=0부터 연속, 앞 fragment 1008 B/마지막 잔여 길이. 중간에 다른 frame ID를 끼우지 않는다. 조립 timeout 1000 ms 또는 단절 시 전체 frame 폐기. 실패 frame ID는 재사용하지 않는다. H→D 재시도는 동일 요청 sequence만 허용하며 새로운 frame ID 재발송은 상위 네트워크 정책에 맡긴다.

현재 양방향 완성 frame 큐는 각각16개(정적 할당). credit·큐가 부족하면 D→H는 해당 frame을 통째로 drop하고 카운터를 올린다. H→D는 시작 fragment 수락 전에 전체 frame 슬롯을 예약하며 없으면 BUSY. 마지막 fragment CRC가 틀리면 CHECKSUM_MISMATCH, NIC 미주입. 데이터 plane 목표는 각각 200 kbit/s 이상이다. 구현과 실기 범위는 M8 검증 (development reference omitted)에 기록한다.

## 10. 디버깅 (channel 5)

- `0x0501 LOG_CONFIG`: `enabled:u8, min_level:u8, reserved:u16, module_mask:u32, max_records_per_sec:u16, reserved:u16`. result=동일 12 B 설정. level=0 TRACE,1 DEBUG,2 INFO,3 WARN,4 ERROR,5 FATAL. module bit:0 core,1 usb_fs,2 usb_hs,3 protocol,4 hid,5 serial,6 storage,7 network,8 rtos; 나머지 0. level 이상의 기록만 선택한다. rate=1..100, 세션 초기 설정은 disabled/INFO/all known modules/20. 업로드 중 실제 rate는 min(설정값,10).
- `0x0502 LOG_STATS`: 빈 request; result=`produced:u32, sent:u32, dropped:u32, queued:u32` (부팅 후 누적).
- `0x0580 LOG_RECORD`: STREAM payload=`uptime_ms:u64, log_seq:u32, level:u8, module:u8, text_len:u16, text[text_len]`, text_len=0..240. UTF-8, 잘릴 때 code point 경계에서 자른다. log_seq는 부팅 중 로그 생성 시 증가하며 필터/queue drop으로 gap 가능(mod 2^32).

별도 고정 로그 큐 128개×256 B. 반복 오류 로그는 rate limit한다. 신뢰성 있는 응답 큐에 로그를 넣지 않는다. 프레임 sequence는 전송 시 부여하고 로그 생성 번호와 구분한다. 연결 전 로그는 이 유한 ring에만 보관하며 overflow 시 오래된 것을 폐기한다. subscribe 시 이전 기록을 순서대로 보낼 수 있고 원래 uptime을 유지한다. 세션 종료 후 구독은 OFF다.

## 11. MCU→호스트 credit와 우선순위

STREAM(channel 2/4/5)의 payload byte 수만 계수한다. channel별 `sent_payload_bytes:u64=0`, `limit:u64=0`으로 세션을 시작한다. RX_LIMIT은 **누적 절대 상한**을 설정한다. 수신 처리 완료한 byte 수만큼 호스트가 limit을 늘린다. 중복·이전 limit은 현재값을 유지하여 credit를 두 번 부여하지 않는다. limit−sent가 32768 B를 초과하는 요청은 RANGE다. counter overflow 전 새 세션을 연다.

초기 권장 credit: SERIAL4096B, NETWORK32768B, DEBUG4096B. NETWORK 호스트는 실제 소비16384B마다 누적 상한을 갱신한다(2026-11-20 r58). host가 STREAM parser/소비 큐에서 실제로 비운 뒤 credit를 갱신한다. 요청/응답과 EVENT는 credit 대상이 아니며, credit가 0이어도 PING/STATUS/취소가 동작한다. NETWORK는 전체 frame의 모든 fragment payload 합계 credit를 예약한 뒤 첫 fragment를 보낸다.

EVENT는 상태 변경 알림이다. channel 0/opcode 0x0080 STATE_CHANGED의 payload는 STATUS 64 B와 같고, 0x0081 JOB_CHANGED는 JOB_STATUS 32 B와 같다. state 알림은 합칠 수 있다. 이벤트 큐 16개, overflow 시 event_dropped 증가, STATUS/JOB_STATUS로 복구한다. HID_DONE도 이 큐를 사용한다.

송신 우선순위: (1) RESPONSE/긴급 상태 (2) HID 완료·job/state EVENT (3) SERIAL과 NETWORK STREAM을 round-robin (4) DEBUG STREAM. high-priority가 연속 8프레임이면, low-priority가 준비되어 있고 안전 관련 응답이 대기하지 않을 때 data 1프레임을 허용한다. DEBUG는 데이터가 없거나 100 ms 동안 전송 기회가 없을 때 최대 1프레임을 허용한다. CREDIT가 없으면 우선순위와 무관하게 STREAM 전송 금지.

호스트도 HID·PING·취소를 data보다 먼저 송신한다. 미리 OS write에 쌓는 data는 최대 2프레임으로 제한한다. MCU TinyUSB TX에는 한 번에 1 encoded 프레임만 진행시킨다. 큰 FIFO에 저우선 데이터 수십 개를 미리 넣어 우선순위를 무효화하지 않는다. FS의 물리 전송시간·OS scheduling 때문에 경성 실시간 지연 보장은 하지 않는다.

## 12. 구현 순서와 검증

M0.1 설정 정리와 M1 관리 FS 검증 후에는 타깃 FS로 M4 HID, M5 SERIAL, M6/M7 STORAGE, M8 NETWORK의 구현과 기능 검증을 진행한다. 이후 타깃 HS를 복구해 M2·M3 기준으로 다시 검증한다. 미구현 기능은 features=0 및 UNSUPPORTED 응답.

필수 시험: 모든 byte 값·0/최대 payload·read 1 byte씩·여러 frame 합침·CRC/COBS 손상 후 다음 프레임 복구, sequence 중복/충돌/누락, session 바뀐 후 오래된 frame 거절, window ACK hole, 로그 폭주와 credit 0, 업로드 중 PING/취소, KEY_DOWN ACK 유실 후 중복 미실행, lease 만료 release, 부팅/카드 제거/업로드 중단.

호스트용 [framing 기준 벡터](protocol-v1-vectors.json)를 펌웨어와 앱 양쪽에서 사용한다. 벡터 생성·검증은 wire 설계 검증이며 TinyUSB·보드 동작 검증을 대체하지 않는다. 메모리 수치는 [MEMORY-MAP.md](MEMORY-MAP.md)를 따른다.

2026-11-16 호스트 계산 결과: CRC bitwise 구현과 zlib 비교 일치, COBS 경계 길이와 300개 무작위 payload round-trip 통과, CRC 손상 거절, stream read 크기 1/3/64/127/4096 B에서 같은 프레임 복원. command dispatcher·dedup·lease·credit의 실제 구현 시험은 아직 수행하지 않았다.

## 고속 관리 CDC 프로필

`MINIKMB_MANAGEMENT_FAST_CDC=1`은 관리 PID를 `CAFE:4004`로 지정하고, 64 B Full Speed 패킷을 최대 2 KiB씩 한 USB OUT 전송에 받는다. 호스트는 64 B의 배수인 CDC 쓰기 뒤에 빈 COBS 구분자 1 B를 같은 쓰기로 붙여 짧은 패킷을 만든다. 프로젝트 관리 도구는 두 프로필을 같은 MCU UID로 식별하며 해당 패딩을 적용한다. 기본 `CAFE:4002`는 USB OUT 수신을 64 B 단위로 처리한다. 요청·응답 내용, DATA 최대 1008 B 및 WINDOW 4는 두 프로필에서 같다.


### HID 자동 중립 보고서의 독립 진행

키보드와 마우스의 자동 중립·idle 보고서는 서로 다른 interrupt IN 엔드포인트를 사용한다. 마우스 인터페이스를 폴링하지 않는 부트 키보드 호스트에서도 키보드가 진행해야 한다. 일반 요청의 FIFO 및 ticket/실제 완료 구분은 유지한다. 명시적 RELEASE_ALL은 키보드·마우스 실제 해제 전송이 둘 다 완료돼야 성공 완료 이벤트를 내고 이후 일반 요청을 진행한다. 자동 중립 보고서에는 호스트 요청 ticket을 합성하지 않는다. 이 동작은 wire 값이나 관리 ACK 형식을 변경하지 않는다.


## 2026-11-20 r60: SD catalog and optical device visibility

Channel 3 extensions; existing frame/version unchanged. Little-endian integers. `EMULATION_INFO` is available without a storage job. Other commands retain session ownership and the single storage worker.

| Opcode | Request | Reply |
|---|---|---|
| 0x0319 EMULATION_INFO | empty | capabilities:u32, desired_optical:u8, applied_optical:u8, reserved[2]=0 |
| 0x031A OPTICAL_SET | enabled:u8 (0/1) | empty ACK; asynchronous target reconfiguration |
| 0x031B DISK_LIST | cursor:u32 (start 0) | job_id:u32; job kind 15 |
| 0x031C DISK_LIST_RESULT | successful list job_id:u32 | next_cursor:u32, count:u8 (0..4), reserved[3]=0, entries[count] |
| 0x031D DISK_DELETE | disk_id:u32 | job_id:u32; job kind 16 |

Capabilities bits: 0 optical visibility, 1 paged catalog, 2 delete (host-side image format). Initial value 7. Both optical desired/applied default ON after power-up. Poll 0319 for application; ACK alone does not prove USB reconfiguration. Optical OFF removes the optical LUN, rather than merely ejecting its medium. Target USB disconnect/deinit/delay/reinit is handled by its task; management USB remains active. With optical ON LUN0=optical/LUN1=raw disk; OFF exposes only LUN0=raw disk. HID remains compiled ON. Unmount active optical media and close raw leases before toggling; no upload/job may be active.

Each catalog entry: disk_id:u32, sectors:u32, format:u8 (1 FAT32/2 exFAT/3 EXT4), reserved[3]=0. Only valid `.OK1` markers with matching image length are listed. Cursor is a directory offset; next_cursor=0 ends the scan. Catalog can run with an open raw image, but not during another storage job. Do not mutate the directory between pages. Result is bound to the requesting session and completed job until the next catalog operation. Optical v1 still stores one active optical image, returned by IMAGE_INFO rather than this catalog.

DISK_DELETE requires all raw leases closed and an idle non-serving storage state. It deletes only the bounded-ID marker and image; marker removal occurs first. A host formatting an image prepares and checks its new filesystem first, then explicitly confirms deletion/recreation of that ID and capacity. Failure after deletion can leave an incomplete image; it is not an atomic replacement. PC physical disks and the entire SD card are not targets of this command.


## 2026-11-20 r61: internal card identity and channel name

BOARD.ID is internal metadata. The storage worker reads the silicon UID before task creation and loads/creates identity on card insertion and immediately after successful card format, without HELLO or any management command. Existing identities are preserved. Only a missing profile is initialized; corrupt profile I/O/checksum errors are not silently overwritten. A new board identifier is UID[12]+ASCII MKB1[4], an opaque stable board identifier, not a random UUIDv4. Legacy assigned IDs remain unchanged. CARD_INIT remains a compatibility command; the app does not expose or depend on it. CARD_FORMAT now completes with an initialized profile, empty name and no user value.

Channel3: 0x031E CHANNEL_NAME_GET accepts empty payload, returns length:u16, reserved:u16=0, UTF-8 name[length]. It reads cached worker state, not FatFs in USB context. NOT_READY without card, NOT_FOUND before profile readiness, BAD_STATE if the registered UID differs from this MCU. Legacy unnamed profiles return empty names.

0x031F CHANNEL_NAME_SET accepts length:u16 + UTF-8 name[length] (0..64), returns job_id:u32; JOB_STATUS kind17 confirms completion. Strict UTF-8, no ASCII/C1 controls or line separators. Empty clears the name. Requires registered UID match, no raw lease/upload/user transfer/other active job, and unmounted optical media. File I/O runs only in the SD worker. Name changes do not modify the opaque 4KiB user value.

BOARD.ID keeps its original 40-byte base (magic, board ID, MCU UID, CRC). Named form appends 76 bytes: MKN1[4], length:u16, reserved:u16=0, UTF8[64] zero-filled, CRC32:u32 over the first112 bytes. Total116. Both40/116-byte forms are readable. A checked BOARD.TMP is written/closed before BOARD.ID→BOARD.BAK and TMP→ID rename. Reads prefer valid ID, then BAK, then TMP after interruption; every candidate is validated. These internal files are excluded from normal image lists and virtual disk contents.


### 2026-11-22 HID scheduling clarification

The target HID queue reserves16 static pending slots for keyboard and16 for mouse. Each interface preserves request order; independent interfaces may complete tickets out of submission order. The host must match HID_DONE by ticket and must not delay a keyboard UP while awaiting an unrelated mouse completion. RELEASE_ALL cancels pending requests and retains its completion ticket until both actual neutral reports finish. Once one interface completes its neutral, its new requests can progress even if the other remains unpolled. ACK is queue acceptance; HID_DONE success is actual endpoint completion. Wire opcode/payload/version unchanged.
