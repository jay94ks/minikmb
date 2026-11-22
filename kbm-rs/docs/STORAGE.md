# SD 저장소 이관: 메타데이터·매체·job

## 현재 실기 수용 범위

2026-11-21 사용자 지정: SD 전송 검증은10MB급 파일 한 건의
업로드로 충분합니다. HTTP 수신 후 SD job의 실제 완료, 저장된 최종 파일 크기와
내용 일치를 확인하고 전송 속도와 섹터 정렬에 따른 실제 바이트 수를 기록합니다.
다중 GiB·반복 장시간 전송은 완료
조건에서 제외합니다. 이하의 대용량/장시간 시험 잔여 표시는 이 기준으로 대체합니다.
기능별 모의 회귀·재연결·오류 복구 검증은 유지하며, 아직 실기를 통과했다는 뜻은
아닙니다. 시험 파일만 생성하고 기존 이미지/사용자 파일을 임의 교체하지 않습니다.

### 10MB 업로드 결과 · 2026-11-21

확인된 채널0의 초기화된 카드/빈 카탈로그에서 실제 인증 REST를 통해
**10,000,384바이트**를 전송했습니다(10MB를2048바이트 단위로 올림).
SD job 완료·최종 파일 크기·원본 CRC32 **2801311853** 일치를 확인했습니다.
MCU의 COMMIT은 SD 파일 전체를 다시 읽어 크기/CRC를 검사합니다. PC로 파일
전체를 다시 내려받은 시험은 아닙니다. 약153.24초, 검증 포함 평균65,260B/s였으며
무작위 패턴 본문 전체를 전송했고 sparse/zero-tail 생략을 사용하지 않았습니다.
이 속도를 USB FS의 최대 성능이라고 주장하지 않습니다.

원래100MB 시험은 사용자 요청으로 약14.5MB에서 정상 취소했고 정리 오류가 없었습니다.
이를 성공으로 세지 않고 새10MB 작업의 완료를 별도로 확인했습니다. 현재 카탈로그에는
`native-10mb-transfer.iso` 한 개가 있고 타깃에는 마운트하지 않았습니다. 이 파일은 전송
검증용 구조/본문 fixture이며 OS 부팅 검증용 매체가 아닙니다. 기존 파일 교체·포맷·
삭제는 하지 않았습니다. 공개용 관련 개발 기록(공개 대상 제외)는
UID/키/카드 UUID를 제외하고 실제 도구·원본 보고서 해시를 기록합니다.

도구 `examples/storage_review.rs`의 기본 동작은 읽기 전용입니다.
`--upload-10mb`를 명시해야 기록하며 기존 이미지/매체가 있거나 카드 식별이 다르면
거부합니다. 일반 자동 시험은 모의/임시 파일만 사용합니다. 추가 실사용 확인은
이번 구현·기본 검증을 마무리한 후 사용자가 직접 사용하며 필요 시 수정을 요청합니다.

현재 Rust 구현은 카드 상태, 이미지 목록 읽기, 채널 이름, 별도4KiB 사용자 값과
작업 수명, 광학 장치 토글·매체 연결, raw 경로/섹터 I/O, 파일 업로드와 FAT32/exFAT/EXT4 이미지 생성·포맷, 전체 SD 카드 포맷과 읽기 전용 폴더 스냅샷입니다.
실제 SD/MSC 수용 시험은 남아 있으며 이 문서를 전체 검증 완료로 해석하지 않습니다.

## 공통 명령

| MCP | REST | 인자 |
|---|---|---|
| `storage_action` | POST `/api/v1/active/storage/actions` | 아래 `action` 객체 |
| `storage_get_status` | GET `/api/v1/active/storage` | 없음; 현재 캐시와 진행 중 작업 |
| `storage_job_get` | POST `/api/v1/storage/jobs/get` | `{ "job_id": "..." }` |
| `storage_job_cancel` | POST `/api/v1/storage/jobs/cancel` | `{ "job_id": "..." }` |
| `storage_upload_prepare` | POST `/api/v1/active/storage/uploads/prepare` | 아래 이미지 사양; 단회 파일 수신 URL 반환 |

`storage_action`은 현재 active를 서버에서 고정합니다. UID·generation·대상 배열을
받지 않습니다. 인자가 없는 액션도 추가 필드를 거부합니다. 액션은 다음과 같습니다.

```json
{"action":"refresh"}
{"action":"card_info"}
{"action":"name_get"}
{"action":"name_set","name":"서버 1"}
{"action":"user_read"}
{"action":"user_write","bytes":[0,1,255]}
{"action":"optical_set","enabled":false}
{"action":"optical_mount","id":100,"mounted":true}
{"action":"disk_open","id":2,"path":"direct","writable":true}
{"action":"disk_sync","id":2}
{"action":"disk_read","id":2,"lba":0}
{"action":"disk_close","id":2}
```

반환된 `job_id`는 앱 작업의 불투명한 난수 ID입니다. **요청 수락은 완료가 아닙니다.**
`storage_job_get`으로 `state`가 `completed`/`failed`/`cancelled`인지 확인합니다.
결과의 `uid`/`generation`은 원래 채널이며 후속 요청에 UID를 다시 넣지 않습니다.
같은 키는 MCP에서 만든 job도 REST에서 조회할 수 있습니다. 다른 키는 거부합니다.
작업 이력 조회는 USB 명령을 보내지 않으므로 active가 없어도 가능합니다.

`phase`, `done`, `total`, `error`, `cleanup_error`, `side_effects_possible`과
읽기 전용 `mcu_job` 상태/오류/상세를 제공합니다. 취소 요청 수락도 MCU 종료를
뜻하지 않습니다. 이름/사용자 값 변경이 이미 완료된 뒤 취소될 수 있으며,
`side_effects_possible`은 롤백을 보장하지 않는다는 뜻입니다. MCU ACK 시간 초과나
잘못된 응답 뒤에 변경 명령을 자동 재전송하지 않습니다.

## 수명과 한도

- 전체 저장소 작업은 한 번에 하나만 수락하며, 완료 이력은 최대64개입니다.
  오래된 완료 이력은 제거되어 `JOB_NOT_FOUND`가 될 수 있습니다.
- 각 작업은 원래 active UID/세대, 관리 USB 세션, 생성 키와 생성 클라이언트 수명에
  묶입니다. HTTP 요청자의 단순 연결 종료는 수락한 작업을 버리지 않습니다.
  키 폐기·MCP 세션/서버 종료·명시적 취소는 작업 중단과 정리로 이어집니다.
- 채널 전환은 전역 active를 먼저 전환 상태로 예약해 새 제어를 막고, 기존 SD 작업의
  취소/종료를 확인한 후 HID·영상·TTY 정리를 이어갑니다. 취소는 원래 UID/세션과
  수락 때 받은 MCU job/전송 ID로만 보냅니다. 새 채널로 정리 명령을 재지정하지 않습니다.
- 정리는 `JOB_CANCEL/JOB_STATUS`, `CARD_USER_ABORT`, 소유 매체의 info/close/unmount의 고정된 ID/인자 조합만
  허용하는 별도 Scope를 사용합니다. 취소된 키나 세대 변경을 빌미로 임의의 장치
  명령을 우회하지 않습니다. 관리 세션이 바뀌면 이전 Wire를 재사용하지 않습니다.
- 수락된 ID가 불분명하거나 정리 완료를 확인하지 못하면 해당 MCU 세션을 불확실
  상태로 보존합니다. 새 MCU job과 채널 전환을 막고 재연결을 안내합니다.
  카드 정보/이름 같은 단순 조회는 진단을 위해 남깁니다.
- 스냅샷은 최종 성공한 목록과 현재 카드 정보를 공유합니다. 작업 실패/카드 교체는
  `stale`로 표시합니다. UUID가 바뀌면 이전 카드의 이미지/열린 경로 캐시를 비웁니다.
  저장 직후 검증한 사용자 길이·revision도 같은 캐시에 반영합니다.

## 보존한 v1.0 ABI

- `CARD_INFO(0x0308)`52바이트: present/profile/user 플래그, UUID, 저장 UID,
  실제 MCU UID, 별도 사용자 값 길이/revision/상태를 검사합니다.
  BOARD.ID 파일 자체나 백업을 목록·공유에 노출하지 않습니다. 내부 ID 생성/로딩은
  기존 펌웨어 책임이며 수동 초기화 액션을 새 UI/API에 추가하지 않았습니다.
- `CHANNEL_NAME_SET(0x031F)`→job17→`CHANNEL_NAME_GET(0x031E)` 재조회입니다.
  trim/NFC, 제어문자 거부, UTF-8 최대64바이트, 카드 등록 UID 일치를 검사합니다.
  이름 저장은 CARD_INFO와 이름 명령만 사용하고 광학 이미지/목록 지원에 의존하거나
  연결된 매체를 자동 분리하지 않습니다. 저장 후 값이 다르면 성공으로 표시하지 않습니다.
- 사용자 값은 최대4096바이트, 빈 값 저장도 지원합니다. 읽기는 revision 고정256B
  조각, 쓰기는 전송 ID/1008B 조각/누적 offset 확인→job6 commit→전체 재조회입니다.
  읽기 전후 UUID·revision·길이를 검사해 카드 교체/수정이 섞인 값을 반환하지 않습니다.
- 이미지 정보는 `IMAGE_INFO(0x0305)`/`DISK_INFO(0x0318)`/기능 정보이며 빈 슬롯의
  BAD_STATE/NOT_FOUND만 정상 빈 상태로 해석합니다. 다른 전송/MCU 오류를 숨기지 않습니다.
  카탈로그는 `DISK_LIST(0x031B)`job15→`DISK_LIST_RESULT(0x031C)`입니다.
  페이지당4개, 전체1024항목·1024페이지 한도, 전진 커서·중복 ID·형식/크기·reserved를
  검사합니다. 파일 시스템 경로를 받거나 BOARD.ID를 일반 파일로 읽지 않습니다.

## UI와 검증

상단의 디스크 공유는 좌측 선택 이미지 정보/우측 이미지 목록으로 열고, 채널 정보는
이름·카드 UUID/UID·UTF-8/HEX 사용자 값 편집으로 엽니다. 열거나 편집할 때 영상 HID를
해제하며 창을 닫으면 해당 창의 진행 중 작업을 정리합니다. UI 스레드는 USB I/O나
job 완료를 기다리지 않습니다. 이름/값/키 입력 원문을 운영 로그에 기록하지 않습니다.

일반 시험은 모의 MCU만 사용합니다. 다중 페이지/잘못된 커서, 이름 NFC/검증 실패,
등록 UID 불일치,4KiB 및 빈 값·offset 오류, 수락 중 취소 후 원래 MCU 정리와
전역 전환 예약, 불확실 ID 차단/세션 교체,64개 이력·같은 키의 MCP→REST 조회와
다른 키 거부를 검증합니다. 실제 SD 쓰기/포맷/매체 연결 시험은 실행하지 않았습니다.

전체 카드 포맷의 취소 불가 계약과 폴더 스냅샷 계약은 아래에 정리했습니다.
현재 + 메뉴는 광학/디스크 파일 업로드와 FAT32/exFAT/EXT4 이미지 생성입니다.

## 광학 장치와 디스크 경로

`optical_set`은 매체 분리와 별개인 장치 구성입니다. OFF는 마운트 매체를 먼저
분리하고 입력을 해제한 뒤 `031A` 요청/적용 플래그를 확인합니다. raw LUN은 끄지
않습니다. 열린 raw 경로가 있으면 구성 변경을 거부합니다. `optical_mount`는 현재
광학 이미지 ID를 확인하고 필요한 경우 광학 장치를 켠 뒤 MCU job 완료와 상태를
검증합니다. 업로드로 다른 광학 이미지를 교체하는 기능은 아직 이관 중입니다.

`disk_open`의 `path`는 `target`(타깃 PC), `management`(관리 PC), `direct`(마운트
없이 관리 명령)입니다. 반환된 MCU job의 object와 DISK_INFO의 lease·ID·경로·RW
권한이 일치해야 완료됩니다. 섹터 읽기/쓰기와 sync는 direct 경로에서만 허용합니다.
연결된 PC의 파일 시스템 캐시와 동시에 접근하지 않습니다. `disk_close`는 경로를
닫고 실제 NOT_FOUND 상태를 확인하며, BAD_STATE를 닫힘 성공으로 바꾸지 않습니다.

`disk_read`는 LBA 범위를 확인하고 MCU read job 완료 후 결과 두 부분(각256B)을
결합합니다. 쓰기는 다음 객체처럼 정확히512바이트와 `confirmed:true`가 필요합니다.
아래 `bytes`는 예시 표기이므로 실제 요청에서는512개의 정수 배열로 전달합니다.

```text
{"action":"disk_write","id":2,"lba":0,"bytes":[512개의 0..255 정수],
 "confirmed":true,"expected_lease":123,"expected_card_uuid":"카드 UUID"}
```

쓰기 job 완료 뒤 sync job까지 확인합니다. read/write/open/mount의 선택적
`expected_card_uuid`는 카드 교체를 거부하며 작업 후에도 같은 카드를 검사합니다.
`expected_lease`는 쓰기 확인 이후 닫기/다시 열기로 리스가 바뀌면 쓰기를 거부합니다.
GUI 확인 창은 이미지·LBA·512바이트·UUID·lease를 복사해 고정합니다. 한 번의
정상 쓰기를 검증하는 이 기능은 이미지 파일 시스템 탐색/파일 넣기 기능의 대체가
아니며, 그 기능은 별도 이관 대상입니다.

성공적으로 연결한 매체는 앱의 active 채널 소유이며 대화상자/작업 생성 클라이언트가
종료돼도 유지됩니다. 진행 중 job은 기존 취소 규칙을 따릅니다. 명시적 분리·채널
전환·앱 정상 종료 시 원래 관리 세션에서 소유 리스/광학 매체를 정리합니다.
단순 refresh는 기존 외부 매체를 자동 소유하거나 분리하지 않습니다.

연결이 끊겨 정리가 불가능하면 다른 채널 전환을 막고 오류를 남깁니다. 원래 UID를
다시 활성화하는 복구는 허용하며 새 연결에서 사용자가 명시적으로 경로를 닫을 수
있습니다. 원래 lease를 새 세션으로 자동 재전송하지 않습니다. 카드가 교체되면
이전 소유권을 해제하고 새 카드의 같은 ID 매체를 분리하지 않습니다.
일반 상태 스냅샷의 `storage_resources`/`storage_cleanup_error`는 active가 없어도
정리할 원래 UID를 보여줍니다. storage 상태에도 `resources`/`cleanup_error`가 있습니다.

모의 시험은 direct RW/256B 결과 분할, RO·exported 경로 거부, LBA/확인/리스/UUID
오류, 광학 OFF 순서·raw LUN 유지, 창 종료 후 연결 유지, 원래 세션 정리/실패,
새 세션 복구, 카드 ID 재사용, 수락 중 취소, 키·클라이언트·세대의 dispatch 재검사,
MCP open→REST write/read/close를 포함합니다. 실제 SD·USB 매체 시험은 수행하지 않았습니다.

## 광학·디스크 파일 업로드

UI의 목록 갱신 오른쪽 + 메뉴에서 부팅 ISO/UDF 또는 디스크 파일을 선택합니다.
원래 창을 소유자로 지정한 Windows 파일 선택기를 사용하고, 읽기 전용 핸들을 열며
다른 핸들의 쓰기/삭제 공유를 허용하지 않습니다. 확인 창은 파일 이름·크기·종류,
원래 MCU 세션·카드 UUID를 고정합니다. 선택한 디스크의 교체는 해당 ID를 별도로
확인합니다. 원본 파일에는 쓰지 않고 호스트 경로를 API로 받거나 운영 오류에 넣지 않습니다.

MCP/REST는 `storage_upload_prepare`에 다음과 같은 객체를 보냅니다.

```json
{"kind":"optical","name":"boot.iso","size":81920,"confirmed":true}
{"kind":"disk","format":1,"size":67108864,"confirmed":true}
```

`format`은1=FAT32,2=exFAT,3=EXT4이며 이미 포맷된 파일의 종류입니다. 새 파일을
포맷해 만드는 명령과 구별합니다. 선택적 `replace_id`를 지정하면 검사 후 그 디스크를
삭제/교체합니다. 선택적 `expected_card_uuid`는 원래 카드를 고정합니다. 이름은 UTF-8
120바이트 이하이며 경로 구분자·제어문자를 받지 않습니다. 광학은2048B 단위,
디스크는512B 단위이며 MCU 주소 범위와 실제 카드의 용량/파일 시스템 제한을 따릅니다.

반환된 `url`에 **같은 키의 Bearer 인증**과 `Content-Type: application/octet-stream`,
준비한 크기의 `Content-Length`를 사용해 파일 원문을 POST합니다. 본문은 JSON,
Base64, multipart가 아닙니다. chunked 전송·query·추가 UID는 받지 않습니다.
응답의 `example`은 실제 서버 주소를 사용한 curl 예시이며 키/파일 경로는 자리표시자입니다.
MCP 메시지에 대용량 본문을 넣지 않습니다. URL은10분·단회 사용이며 다른 키는 거부합니다.
미사용 URL은 최대32개, 동시에 받는 파일은 하나입니다. 잘못된 선언 크기는 주소를
소비하기 전에 거부하며, 본문 수신 중 오류/중단 후에는 새 URL을 발급해야 합니다.

REST는64KiB 조각으로 앱 소유 임시 파일에 기록합니다. 전체 파일을 RAM으로 모으지
않습니다. 실제 바이트 수·연결·키·원래 active 세대/MCU 세션을 계속 검사하며,
30초 동안 수신이 진행되지 않으면 중단합니다. 취소된 임시 파일은 핸들 종료로 제거합니다.
단순 HTTP 수신 완료는 SD 기록 완료가 아닙니다. 반환된 job_id를 조회해 전송 전 검사,
SD 준비, 전송, SD 검증과 최종 결과를 확인해야 합니다.

광학 검사 범위는 Electron v1.0과 같은 제한 ISO9660/UDF 볼륨+El Torito BIOS/UEFI
no-emulation 프로파일입니다. 전체 ISO/UDF 파일 시스템 파서는 아닙니다. 구조와
전체 CRC 검사가 끝나기 전에 기존 매체를 분리/교체하지 않습니다. 전송은1008B
payload와 MCU의8B offset ACK를 대조하고, COMMIT/job2 뒤 ID·크기·CRC·이름·분리
상태를 재조회합니다. 오류 후 자동 재전송하지 않고 원래 upload ID를 취소한 뒤
SYS STATUS로 업로드 종료까지 확인합니다. 수락 ID가 불명확하면 기존 격리 규칙을 따릅니다.

디스크 파일은 CREATE/job7의 zeroed 파일과 provisioning 리스를 검증한 뒤, 0이 아닌
512B 섹터를 기록합니다. 건너뛴0 구간도 전체 CRC에 포함하며 COMMIT/job8 완료와
경로 닫힘을 확인합니다. 성공 후 목록을 갱신하며 자동으로 PC에 마운트하지 않습니다.

`transfer`의 광학 속도/ETA는 MCU ACK로 확인된 payload만 최근5초의 단조 시계로
계산합니다. 검사/파일 생성/SD 검증 단계에는 전송 속도를 표시하지 않습니다.
5초 이상 ACK가 멈추면 속도0·ETA 미정으로 바뀝니다. raw의 진행 속도는 zeroed 구간을
포함한 논리 이미지 처리량이며 USB 선로 처리량과 다릅니다.

모의 시험은 ISO/UDF/카탈로그 오류, 사전 검사 이전 무변경, offset 실패 후 취소,
희소 디스크/zero tail CRC, 다른 키·주소 재사용·잘못된 크기·관리 세션 변경·수신
중 키 폐기와 MCP 주소 발급→REST 본문 수신→SD job 완료를 포함합니다.
실제 SD 기록 및 네이티브 파일 선택기 사용자 조작/잠금·장시간 전송 검증은 남아 있습니다.

## 이미지 생성과 선택 이미지 포맷

기존 `storage_action`의19개 액션에 다음 두 동작이 포함됩니다. 생성/포맷은 전체
SD 카드를 포맷하는 기능과 구별합니다. 일반 명령처럼 UID·호스트 경로는 받지 않습니다.

```json
{"action":"disk_create","format":1,"size_mib":64,"confirmed":true}
{"action":"disk_format","id":2,"format":2,"confirmed":true,"expected_size":9437696}
```

format은1=FAT32,2=exFAT,3=EXT4입니다. 새 이미지는64..32768MiB의 정수 용량입니다.
선택 이미지 포맷은 새로 조회한 카탈로그의 용량을 정확한512바이트 단위로 보존합니다.
기존 파일의 최소 용량은 FAT32=64MiB, exFAT=1MiB, EXT4=8MiB, 최대는32GiB입니다.
`expected_size`를 지정하면 선택 이후 용량 변경을 거부합니다. 두 액션 모두
`expected_card_uuid`를 지원하며 UI는 확인 때 원래 카드/ID/용량을 고정합니다.
포맷 시작 전에 모든 raw 경로가 닫혔는지 확인합니다.

WSL의 mkfs.fat/mkfs.exfat/mke2fs로 새 임시 파일을 만들고 fsck.fat/fsck.exfat/e2fsck의
읽기 전용 검사와 파일 크기/서명 검사를 통과해야 SD 작업으로 넘어갑니다. 준비 중에는
기존 SD 이미지와 연결된 광학 매체를 유지합니다. 반영 단계에서 광학 매체를 분리하고,
선택 ID 교체는 `DISK_DELETE/job16 → CREATE/job7 → 데이터/COMMIT` 순서로 진행합니다.
SD 반영을 시작한 뒤 오류·취소가 나면 기존 내용 복원을 보장하지 않습니다.

네이티브 실행기는 API가 지정한 경로를 받지 않으며, 새 `minikmb-format-*` 임시
디렉터리 안의 regular file `disk.img`만 도구에 전달합니다. Windows 드라이브 경로를
WSL 경로로 변환하며 UNC·상대 경로·장치 경로는 허용하지 않습니다. 인자는 shell
문자열로 조합하지 않습니다. 도구 stdout/stderr의 호스트 경로를 오류/로그로 내보내지 않습니다.
재귀 정리 전에는 실제 절대 디렉터리가 생성 때의 canonical path/부모/prefix와 일치하는지 검사합니다.

WSL supervisor는 도구 프로세스 그룹을 따로 만들고 각 단계120초 제한과 앱의
heartbeat를 확인합니다. 작업 취소/키 폐기/원래 채널·관리 세션 변경은 cancel로
이어집니다. 앱 heartbeat가3초 끊겨도 자식을 종료합니다. 종료를 확인한 nonce
receipt가 있어야 폴더를 지웁니다. 종료를 확인하지 못하면 임시 폴더를 보존하고
`FORMAT_PROCESS_UNCONFIRMED` 오류를 반환합니다. 이 호스트 준비 실패로 SD의 기존
이미지를 삭제하지 않습니다. 임시 파일 정리 오류와 MCU 정리 오류는 구별하며,
호스트 정리 오류만으로 MCU 세션을 격리하지 않습니다.

일반 모의 시험은 생성 실패/취소 시 무변경, 정확한 기존 용량, 생성/선택 ID만
교체, 최소 용량·확인·UID/경로 거부와 인증된 REST 생성/MCP 포맷을 포함합니다.
별도 opt-in 로컬 WSL 시험은 세 형식의64MiB 파일, 비-MiB exFAT 파일, 서명/크기,
정리 및 취소 응답을 통과했습니다. 모든 대상은 앱 소유 임시 파일이며 실제 SD나
물리 디스크를 포맷하지 않았습니다. 실제 SD 적용·대용량/장애/강제 종료 검증은 남아 있습니다.

## 전체 SD 카드 포맷과 결과 복구

```json
{"action":"card_format","format":2,"confirmed":true,"expected_card_uuid":"카드 정보에서 읽은 UUID"}
{"action":"card_format_status"}
```

전체 카드는1=FAT32,2=exFAT만 지원합니다. 세 형식의 개별 이미지 포맷과 다릅니다.
카드 UUID·존재 여부와 raw 경로 닫힘을 확인하고, 연결된 광학 매체를 분리한 뒤
`030F [format,0,0,0]`을 한 번 전송합니다. 응답은4바이트 MCU job ID, kind14입니다.
내부 BOARD.ID는 펌웨어가 다시 만들며 앱이 직접 생성하거나 일반 목록에 노출하지 않습니다.
완료 시 CARD_INFO의 등록 UID/현재 UID·사용자 값 길이0과 빈 채널 이름을 읽어 확인합니다.

요청 직전 단일 저장소 mutex에서 취소/채널 전환과 경합을 해결하고 취소 불가 상태를
설정합니다. 수락 확인 중부터 취소는 `JOB_NOT_CANCELLABLE`, 다른 채널 선택/해제는
`CARD_FORMAT_PENDING`입니다. 이 거부는 활성 세대·영상·입력을 무효화하지 않습니다.
수락 후 클라이언트 종료·키 삭제·앱 종료는 펌웨어 포맷을 취소하지 않습니다. 완료 감시는
원래 관리 USB 세션의 job 상태/카드 정보/이름 **읽기만** 허용하는 제한 scope를 사용합니다.
kind14는 일반 SYS_CANCEL/매체 정리 경로로 보내지 않습니다. 결과 조회는 여전히 인증이 필요합니다.

ACK가 유실/손상되거나 USB가 끊기면 성공을 가정하지 않으며 요청도 재전송하지 않습니다.
`card_format` 보류 정보(UID/MCU job ID/단계/취소 불가)를 저장소 상태와 앱의
`storage_card_format`에 노출합니다. 보류 중에는 원래 UID 선택과 읽기 전용 상태 확인만
허용합니다. `card_format_status`는 SYS_STATUS/SYS_JOB을 조회하며 포맷 명령을 보내지 않습니다.
진행 중이면 보류를 유지합니다. 종료를 관찰하면 `observed_terminal`, 재부팅 등으로
원래 job을 확인할 수 없고 현재 포맷도 진행 중이 아니면 `unknown`을 반환합니다.
이 복구 결과로 이전 실패한 앱 job을 성공으로 바꾸지 않습니다. 앱 재시작 뒤 목록/카드
정보를 조회하다 진행 중 포맷을 발견하면 목록 job을 시작하기 전에 보류를 다시 설정합니다.

UI의 SD 정보 탭에 전체 포맷 확인 창과 결과 확인을 추가했습니다. 실제 SD 포맷은
시험하지 않았습니다. 모의 MCU로 사전 취소/UUID/열린 경로 거부, 수락 중 취소와
전환 거부·세대 유지, 키/클라이언트 폐기, ACK 유실, 재연결/재부팅/앱 종료와 재시작,
프로필/이름/사용자 값 초기화 및 인증된 REST 요청/MCP 상태 확인을 검증했습니다.

## 읽기 전용 PC 폴더 스냅샷

앱의 SD 이미지 목록 `+ → PC 폴더 공유…`에서 폴더를 선택합니다. 소유 창이 지정된
네이티브 폴더 선택기는 읽기 전용으로 루트를 확인하고1시간 유효한32바이트 난수
`source_id`를 만듭니다. 최대16개이며 원래 활성 UID/세대·관리 USB 세션에 고정됩니다.
`storage_get_status.folder_sources`에는 식별자·폴더 이름·만료 시각만 나옵니다.
호스트 경로를 API로 받거나 반환하지 않습니다. 서버가 폴더 선택기를 자동으로 열지 않습니다.

```json
{"action":"folder_share","source_id":"앱에서 선택한 64자리 식별자","confirmed":true}
{"action":"folder_forget","source_id":"같은 식별자"}
```

공유는 현재 원본으로 **새** FAT32 스냅샷을 만들며 기존 이미지 ID를 자동 삭제하거나
덮어쓰지 않습니다. UI는 확인 창의 카드 UUID를 고정하고 API도 `expected_card_uuid`를
지원합니다. 열린 디스크 경로는 먼저 닫아야 합니다. 원본 검사와 임시 이미지 생성이
성공한 뒤에만 광학 매체를 분리하고 SD에 기록합니다. 기존 image import의 job/lease·
CRC/COMMIT 확인을 재사용하며, 완료 후 타깃 경로에 RO로 연결합니다.
원래 선택 창/클라이언트를 닫아도 완성된 매체 연결은 유지합니다. 채널 전환/앱 종료는
기존 매체 수명 규칙대로 원래 세션에서 닫습니다. `folder_forget`은 선택 목록의 권한만
해제하며 이미 만들어진 SD 이미지·연결 매체·원본 파일을 삭제하지 않습니다.

원본 편집은 기존 스냅샷에 자동 반영하지 않습니다. `선택한 PC 폴더 → 새 스냅샷…`이나
같은 source_id의 새 명령으로 수동 갱신합니다. 채널 세대/관리 세션이 바뀌거나 만료되면
폴더를 다시 선택해야 합니다. 앱 시작 시 공유를 자동 재개하지 않습니다.

Rust FAT32 생성기는8섹터/클러스터·두 FAT·백업 부트/FSInfo·VFAT UTF-16 긴 이름·
하위 디렉터리·빈 파일·RO 속성을 기록합니다. 짧은 별칭이 다른 긴 이름과 충돌하지
않게 배정합니다. 최대10,000항목·깊이32·디렉터리 메타데이터8MiB·이미지32GiB이며
각 파일은4GiB 미만입니다. 이름/대소문자 충돌·크기 제한 위반은 실패합니다.

심볼릭 링크/정션/특수 파일은 포함하지 않습니다. Windows 원본 핸들은 읽기 전용·
FILE_SHARE_READ와 OPEN_REPARSE_POINT를 사용하고, 핸들의 실제 경로가 승인 루트 안인지
확인합니다. 볼륨/파일 ID·생성/수정/변경 시각·크기를 전후 비교하고 마지막에 다시
검사합니다. 승인한 루트 자체가 교체되어도 거부합니다. 원본은 쓰기로 열지 않습니다.

생성 대상은 새 앱 소유 임시 regular file입니다. 파일 복사는64KiB 단위이며 검사/
취소/원래 채널·키·세션 확인을 반복합니다. 모든 원본 검사를 끝낸 뒤에는 임시 파일만
전송하므로 이후 원본 변경이 전송에 섞이지 않습니다. 할당 클러스터는 앞쪽에 모으고,
증명된 zero tail은 SD_CREATE의0 채움을 사용해 쓰기를 생략합니다. CRC는 생략된
0 구간을 포함한 **전체** 파일을 대상으로 계산합니다.

파일시스템 구조/RO 연결·CRC/취소·실패·원래 채널/세션 검증은 모의 MCU와 임시
원본으로 검사합니다. 실제 선택기 조작·실제 SD/타깃 마운트·대용량/종료 수용 시험은
별도로 남아 있습니다. 외부 읽기 전용 fsck/mtools 검사는 opt-in 시험으로 분리합니다.

로컬 opt-in 시험은 한글 하위 폴더/긴 파일명/빈 파일을 포함한 새 임시 이미지에
`/usr/sbin/fsck.fat -n`을 적용하고 `/usr/bin/mtype`으로 fixture 파일 내용을 다시
읽어 통과했습니다. 실제 SD/물리 디스크는 사용하지 않았습니다.

```powershell
cargo test --lib storage::folder_image::tests::generated_fat32_passes_readonly_wsl_fsck -- --ignored --exact --nocapture
```

Windows의 WSL에 dosfstools/mtools가 설치되어 있어야 합니다. 일반 자동 시험에서는
이 외부 도구 시험을 실행하지 않습니다. `--screenshot ... --style-preview --folder-preview`의
장치 없는 자체 렌더도 확인했으며, 실제 폴더 선택기 조작 시험과는 구분합니다.
