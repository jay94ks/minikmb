# Rust NCM 이관 기록과 연결 계약

2026-11-21. 프레임/credit·DHCP·메모리 내 TCP/UDP 기반에 **NCM 에뮬레이션 모드 관리자·
UI·REST/MCP**와 **호스트 TCP/UDP·DNS 중계**를 연결했습니다. 모의 MCU와 로컬 echo를
검증했으며 타깃 SYN 스캐너와 loopback TCP/UDP 포워딩도 UI/API에 연결했습니다. 실제 보드 NCM/인터넷 공유·전송률의 실기
완료를 뜻하지 않습니다.

## 스택과 검증 근거

[smoltcp 0.14.0](https://docs.rs/smoltcp/0.14.0/smoltcp/)을 정확한 버전으로 고정했습니다.
공식 [릴리스](https://github.com/smoltcp-rs/smoltcp/releases/tag/v0.14.0)와 로컬 crate
소스를 확인했습니다. 라이선스는 crate Cargo.toml의0BSD입니다. 기본 기능을 끄고
std/medium-ethernet/proto-ipv4/socket-tcp/socket-udp만 사용합니다. Host NIC·TAP·OS
라우트·방화벽·ICS를 생성하거나 변경하는 경로는 추가하지 않았습니다.

`work/export-minikmb-network-vectors.cjs`는 기존 Electron의 MiniNetworkWire/DhcpServer를
합성 요청/FakeConnection으로 실행해 `src/network/fixtures/v1.json`을 만듭니다. 실제
USB·NIC·소켓·사용자 파일에 접근하지 않습니다. 일반 Rust 시험은 이 고정 fixture만
사용하므로 Node/Electron을 실행 환경에 요구하지 않습니다.

- 기존 v1.0의 NCM 조각/credit 요청과 DNS 없는 DHCP OFFER·호스트 모드 DNS 옵션
  응답을 실제 TypeScript 출력 바이트와 비교했습니다.
- 두 실제 smoltcp 인스턴스 사이에서 ARP와 NCM의1008B 조각/전체 CRC 재조립을 거쳐
  64KiB TCP 스트림 및1472B UDP payload 왕복을 검증했습니다. 모든 패킷은 메모리 안에서만 이동합니다.
- 부분/CRC/예약 필드/재전송·기한·순서 유실·다른 세션·credit 초과를 검사했습니다.
- DHCP ACK/NAK/RELEASE·임대 만료/주소 소진·잘못된 checksum/relay/옵션, 사설 .1
  게이트웨이·pool/타깃 주소 제한을 검증했습니다.
- 실제 보드 NCM ON/OFF·관리 CDC/타깃 NCM 실기·전송률은 이번 단계에서 시험하지 않았습니다.

## MCU 프레임과 세션

기존 펌웨어 `management_network.inc`의 ABI를 사용합니다.

| 경로 | 내용 |
|---|---|
| ch4/0401 |1바이트0/1, NCM OFF/ON. 성공 ACK 본문은 비어 있음 |
| ch4/0402 | id:u32le / offset:u16le / total:u16le / crc32:u32le / 예약0:u32 / Ethernet 조각 |
| ch4/0403 |16B: MCU rx/tx/credit drop 각u32le, enabled/up 각u8, 예약0:u16 |
| ch4/0480 STREAM |0402와 같은 조각 헤더와 데이터, 독립적인 이벤트 sequence |
| ch0/0006 | `[4,0,0,0] + absolute_limit:u64le`, ACK는 sent:u64le / limit:u64le |

Ethernet wire 길이는14..1518B, 조각은 정확히 min(1008,남은 길이)입니다. 프레임 전체
CRC를 확인하기 전에는 스택에 전달하지 않습니다. 스택은 v1.0처럼 FCS 없는14..1514B의
ARP/IPv4만 처리하고 IPv6/L2 브리징을 추가하지 않습니다. 재조립은 최대1518B 하나,
기한1초입니다. 중복 이벤트의 payload는 credit에 두 번 합산하지 않습니다.
이벤트 sequence 유실/credit 초과는 재동기화가 필요한 오류로 구분합니다.

`Transmitter`는 한 관리 세션에서 OFF/ON을 거쳐 유지해야 합니다. 프레임 ID는 첫 조각을
보내기 전에 할당하고, 취소/명시적 실패/불확실 ACK가 나도 되돌리지 않습니다.0과
wraparound를 금지하며 다른 관리 세션으로 재사용하지 않습니다. 수신 credit도 세션의
절대 누적값입니다.32KiB 윈도우를 사용하며 남은 양이16KiB 이하면 갱신합니다.

네이티브 backend는 검증된 CAFE 관리 연결과 HELLO feature bit5만 사용합니다.
요청을 큐에 넣을 때 원래 세션을 검사하고, 실행할 때 UID/세대·세션·작업 수명·
키/클라이언트 수명을 검사합니다. NCM 정리 scope는 **원래 세션의 OFF와 상태 읽기만** 허용합니다.
HID 입력이나 NCM ON/프레임 송신에 정리 scope를 사용할 수 없습니다. 일반 HID 정리
scope에서 NCM OFF 권한을 제거해 새 세션으로 요청이 넘어가지 않게 했습니다.

HID/SYSTEM/TTY 우선, 네트워크는 기존 MCU 워커의 낮은 우선순위 ch4입니다. 스택
RX/TX는 각각32프레임 한도이며 출력이 가득 차면 smoltcp의 TX token을 주지 않아
역압을 적용합니다. ch4 이벤트 버스는32KiB credit의 작은 조각 전체를 보관할 수 있는
2048개로 제한했습니다. 다른 채널의256개 버스와 분리해 TTY 이벤트를 밀어내지 않습니다.

## DHCP 프로필

기본값은192.168.127.1/24, pool100..199, 임대3600초입니다. 사설 서브넷의 .1만
서버/게이트웨이로 허용하고 pool은2..254 내 최소2개 주소, 임대는60..86400초입니다.
router option3은서버 .1이며 **DNS option6은 호스트 공유일 때만** 광고합니다.
host 모드에서는 이 옵션과 실제 TCP/UDP DNS 중계를 함께 켭니다. DHCP 릴레이 요청과
잘못된 IP/UDP checksum은 처리하지 않습니다.

## 서비스 연결과 자원 수명

다음1..7과 loopback 포워딩은 구현하고 모의/로컬 시험을 통과했습니다. 실제
NCM/타깃 USB 실기는 별도로 남아 있습니다. 설정 저장·이관은 구현했으며
[저장 계약](PREFERENCES.md)에 자동 시험과 남은 파일 선택기 실기를 구분했습니다.

1. GUI/REST/MCP가 같은 전역 활성 채널의 NCM 관리자 하나를 사용합니다. 모드는
   off/emulation/host이며 시작은OFF, host/emulation은 상호 배타적입니다. 호스트 DNS
   준비 실패는 기존 에뮬레이션을 끄지 않습니다. 모드 변경
   수락 작업은 요청이 끊겨도 정리까지 끝내고, 미실행 요청은 원래 키/클라이언트 수명을 검사합니다.
2. 관리 세션별 송신 ID 객체를 유지합니다. OFF 때 객체를 버리면 MCU의 last_in_id와
   달라집니다. 새 관리 세션에는 이전 큐/프레임/credit을 이전하지 않습니다.
3. ON 준비는 원래 HID를 먼저 해제하고 `NET_SET(0) → OFF 상태 확인 → CREDIT(0)`으로
   이전 sent/limit을 확인합니다. OFF ACK와 credit ACK까지의 옛 이벤트를 배출한 뒤
   receiver를 그 누적값으로 동기화하고 새 윈도우를 광고합니다. 단일 MCU TX 버퍼의
   전송 순서/누적값에 맞춰 동기화합니다. ACK보다 먼저 도착한 이벤트·이전 watermark·
   credit 갱신 ACK 전에 새 상한으로 들어온 수신을 모의 시험으로 확인했습니다.
4. 완성된 NCM 자원 수명은 UI 대화상자 수명과 분리합니다. 채널 전환/종료/오류 때
   원래 HID를 해제하고 큐/스캔/프록시를 중단한 뒤 원래 세션에서 OFF를 확인합니다.
   OFF 결과 불명이면 다른 채널로 넘어가지 말고 원래 UID의 복구 경로를 제공합니다.
   정리 대기6초가 지나도 워커 JoinHandle을 버리지 않습니다. 그 워커의 마지막 OFF가
   끝난 것을 확인한 뒤에만 새 ON을 허용해 늦은 정리가 새 연결을 끄지 않게 합니다.
5. RX/스택과 TX ACK 대기는 분리하고 유한 큐를 사용합니다. 수신 credit을 먼저 발행한
   상한으로 검사해 ACK보다 앞서 도착한 STREAM을 처리합니다. 오류 뒤 암묵적 재송신은 하지 않습니다.
6. 호스트 공유는 Tokio TCP/UDP/DNS 프록시로 현재 PC 경로를 사용합니다. 총32 flow,
   TCP slot8/UDP slot24와 유한 큐·idle/connect timeout을 유지합니다. 기존 PC
   서비스/OS NIC 설정을 바꾸지 않습니다. 새 loopback 포워딩도 같은32개 연결 한도를 공유합니다.
7. 스캐너는 NCM이 켜진 선택 타깃의 /24 주소만, 명시적인 시작/중지로 실행합니다.
   시험은 합성 프레임 또는 로컬 echo만 사용하고 실제 타깃을 자동 스캔하지 않습니다.

UI/API/프록시 연결 뒤에도 두 MCU 동시 실기는 사용자 요청대로 필수 검증에 넣지 않습니다.
NCM 기능과 전환/재연결·성능 회귀의 전체 이관 완료 판정은 후속 구현·시험 후에 합니다.

## 공통 명령과 화면

| MCP | REST | 인자 |
|---|---|---|
| `network_forward_configure` | POST `/api/v1/active/network/forward/configure` | `rules` 배열; 빈 목록 허용, 포트는 열지 않음 |
| `network_forward_start` | POST `/api/v1/active/network/forward/start` | `rules`1..32개 |
| `network_forward_stop` | POST `/api/v1/active/network/forward/stop` | `{}` |
| `network_scan_start` | POST `/api/v1/active/network/scan/start` | 아래 타깃/포트/대기 시간 객체 |
| `network_scan_stop` | POST `/api/v1/active/network/scan/stop` | `{}` |
| `network_get_status` | GET `/api/v1/active/network` | 없음; query/UID 거부 |
| `network_configure` | POST `/api/v1/active/network/configure` | 아래 설정 객체 |
| `network_set_mode` | POST `/api/v1/active/network/mode` | `{"mode":"off"}` 또는 `emulation`/`host` |

```json
{"server_ip":"192.168.127.1","pool_start":100,"pool_end":199,"lease_seconds":3600}
```

설정 변경은 OFF에서만 허용하며 UI와 같은 검증을 적용합니다. 모드 작업은 최대4개
(실행 중 포함)로 제한하고 전환 gate에서 직렬화합니다. 대기 중 키/클라이언트가
종료되면 ON을 보내지 않습니다. 성공한 같은 모드 재요청은 USB를 다시 연결하지 않습니다.
현재 에뮬레이션은 DHCP/ARP/기본 IPv4 스택을 제공합니다. host는 현재 PC의 경로를
사용하는 IPv4 TCP/UDP 및 DNS 중계를 제공합니다. 타깃 SYN 스캐너는 아래 명령으로
명시적으로 실행합니다. loopback 포워딩도 별도의 명시적 시작/중지로 제어합니다.

완성된 NCM 자원은 앱 소유입니다. 창/MCP 세션을 닫거나 생성 키를 삭제해도 이미
켜진 링크는 유지되며, 새 제어 요청의 권한은 즉시 폐기됩니다. 반대로 ON 수락을
기다리는 중 키/클라이언트가 종료되면 원래 세션을 OFF로 정리하고 오류를 보고합니다.
전체 앱 종료/채널 전환/네트워크 오류는 수신·스택·송신을 중단하고 원래 HID를 해제한
후 OFF를 확인합니다. 정리 실패는 `NETWORK_CLEANUP_REQUIRED`와 불명확 결과로
표시합니다. 원래 세션이 끊겼으면 원래 UID를 다시 활성화한 뒤 off로 복구할 수 있습니다.
예전 요청/프레임을 새 세션에 재전송하지 않습니다.

상태는 원래 UID/세대, mode/phase/config, 마지막 MCU enabled/link_up/drop 통계,
앱의 RX/TX 프레임·바이트·유한 대기량·드롭·DHCP 임대, host_flows와 관측 시각·정리 오류를
반환합니다. `observed_at:null`은 아직 MCU를 조회하지 않은 앱 기본 OFF 상태입니다.
`cleanup_required:true`는 정상 ON 상태에서도 나올 수 있으며, 전환 전에 OFF 확인이
필요한 자원을 뜻합니다. 정리 실패 여부는 phase와 cleanup_error로 구분합니다.
활성 채널이 없으면 일반 명령은 NO_ACTIVE_CHANNEL이며 앱 스냅샷의
`network_resource`로 원래 자원의 UID/정리 상태를 조회할 수 있습니다.

상단바의 네트워크 창에 에뮬레이션 ON·끄기/OFF 확인·DHCP 설정·상태/임대 목록을
연결했습니다. 창을 닫아 링크를 끄지 않으며, 창이 열려 있는 동안 텍스트 입력을
타깃 HID로 보내지 않습니다. 실제 선택 채널을 바꾸면 창을 닫고 원래 링크를 정리합니다.
장치 없는 `--screenshot <절대 PNG> --style-preview --network-preview`로 배치를
확인했습니다. 에뮬레이션/호스트 공유 버튼과 OFF 확인은 같은 모드 관리자를 사용합니다.

일반 모의 시험은 OFF/ON의 ID·credit 유지, 유한 큐와 ACK 전 수신, 수락 중 키 폐기,
성공 후 클라이언트 종료, 요청자 task 취소 뒤 완료, 종료 대기 시간 초과의 handle 보존,
다른 UID 전환 차단/원래 연결 복구, 손상 ACK 뒤 무재전송, 설정/지원 범위/제어 작업
한도와 NO_ACTIVE_CHANNEL/외부 UID 거부를 포함합니다. 인증된 REST 설정/상태/끄기와
MCP ON도 같은 모의 MCU로 검증했습니다. 실제 NCM ON/OFF·NIC 변경·타깃 스캔은 하지 않았습니다.

## 호스트 프록시의 경계와 검증

IPv4 TCP/UDP 패킷의 헤더 길이·전체 길이·조각·IP/전송 checksum과 source /24를
검증한 뒤 가상 서버의 내부 포트로 변환합니다. 출력은 원래 remote IP/port로
돌리고 IP/전송 checksum을 다시 계산합니다. 출력 대기 큐에 넣기 전에 변환하므로
slot 재사용으로 이전 패킷이 다른 목적지로 바뀌지 않습니다. loopback·멀티캐스트·
link-local·TTL 소진·외부 source와 내부 변환 포트의 직접 접근은 프록시하지 않습니다.

TCP는 최대8개, UDP는24개이며 합계32개입니다. TCP 양 방향 smoltcp 버퍼는 각각16KiB,
호스트 메시지 큐는 방향별8개/조각4KiB입니다. TCP half-close 후 반대 방향의 남은
데이터를 전달하고 connect10초/idle120초를 적용합니다. UDP는1472B/큐8개,
connect2초/idle30초이며 초과/가득 찬 수신은 드롭 통계로 합산합니다. 실패한 flow만
닫고 NCM 전체를 암묵적으로 재시작하지 않습니다. 종료 때 스택을 먼저 버려 원래
프록시 작업/소켓을 중단한 다음 원래 MCU 세션의 OFF를 확인합니다.

공유 시작 시 Windows의
[GetAdaptersAddresses](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getadaptersaddresses)를
AF_UNSPEC으로 읽어 IPv4·IPv6 DNS를 함께 찾습니다. Windows가 반환한 가동 중
인터페이스/DNS 순서에서 첫 유효 주소를 선택하며, 없으면 DNS_UNAVAILABLE로
기존 링크를 변경하지 않고 host 시작을 거부합니다.
[DNS 노드](https://learn.microsoft.com/en-us/windows/win32/api/iptypes/ns-iptypes-ip_adapter_dns_server_address_xp)의
주소는 IPv4/IPv6 양쪽을 지원합니다. IPv6 scope ID를 보존하고 link-local에 ID가
없으면 해당 인터페이스의 Ipv6IfIndex를 사용하며 둘 다 없으면 그 주소는 사용하지 않습니다.
IPv4-mapped IPv6는 IPv4로 정규화합니다. unspecified·multicast·broadcast를 제외하며
OS가 지정한 loopback DNS resolver는 허용합니다. 타깃의 서버 .1:53 TCP/UDP는
선택한 DNS로 중계하고 응답은 가상 .1:53으로 복원합니다. 일반 연결은 Tokio 소켓이
기존 PC 경로를 사용합니다. 타깃 IPv6/ICMP/L2 브리징은 공유 범위에 추가하지 않았습니다.

조회 버퍼는 정렬된15KiB로 시작해 크기 변경을 최대3회/1MiB까지 재조회합니다.
반환 포인터·정렬·구조 길이/주소 길이·순환과256개 인터페이스/4096개 DNS 노드 상한을
검사합니다. DNS 목록을 공유한 인터페이스도 각각의 scope를 적용하며 무제한 순회하지
않습니다. OS 테이블 조회는 별도 blocking 워커에서 수행하며 외부 DNS를 시험 조회하거나
NIC/route/DNS 설정을 변경하지 않습니다. DNS는 현재 공유 자원 생성 시 선택합니다.

시험의 Dial 주입은 모든 목적지를 테스트가 만든127.0.0.1 소켓으로 치환합니다.
실제 두 smoltcp 인스턴스와 NCM CRC/조각을 거친 TCP64KiB·half-close, UDP1472B 및
DNS 원래 endpoint 왕복, 잘못된 checksum/조각/범위·슬롯 한도/만료/재사용,
유한 큐와 idle 소켓 종료를 검증했습니다. 모의 MCU에서는 DNS 준비 실패의 기존
링크 유지, host/emulation 상호 전환·같은 모드 멱등성과 DNS 옵션 차이를 확인했습니다.
외부 인터넷 요청·사용자 NIC 변경·실제 타깃 트래픽 시험은 수행하지 않았습니다.

IPv6 DNS 추가 시험은 원래 Dial 치환 없이 앱 소유 `[::1]` UDP/TCP 서버로 실제 연결합니다.
IPv4 NCM 클라이언트의 DNS 질문/응답·TCP 길이 prefix/half-close와 가상 .1:53 source
복원을 확인했습니다. Win32 버퍼 fixture는 IPv6-only/혼합 순서, scoped link-local·
인터페이스 ID 보완·공유 목록, 잘못된 주소/포인터/길이·순환을 검사합니다.
현재 Windows에서 읽기 전용 실조회도 별도로 통과했으며 출력은 family/범위 유무뿐입니다.
실조회에서는 IPv4가 선택됐습니다. 실제 IPv6-only PC/타깃 NCM 실기는 아직 별도입니다.

```powershell
$env:MINIKMB_DNS_INSPECT = '1'
cargo test --lib windows_read_only_system_dns_discovery -- --ignored --nocapture
Remove-Item Env:MINIKMB_DNS_INSPECT
```

이 명시적 시험은 어댑터 테이블만 읽고 DNS 패킷을 발신하지 않습니다. 일반 suite에서는
네트워크 구성에 의존하지 않도록 ignore합니다. IP 주소·인터페이스 이름을 출력하지 않습니다.

## 타깃 SYN 스캐너

```json
{"target":"192.168.127.100","start":1,"end":1024,"timeout_ms":1000}
```

현재 active 채널의 NCM이 ON이고 link_up일 때만 시작합니다. canonical IPv4 /24 타깃
하나와1..65535 범위,250..5000ms 대기 시간을 허용합니다. 서버 .1/네트워크/브로드캐스트,
서브넷 밖 주소·UID/여러 대상·PC 인터페이스 지정을 거부합니다. 시작은 자동 실행하지 않습니다.

2초 한도의 ARP 조회로 타깃 MAC을 확인한 뒤 최대8개 SYN 프로브를 보냅니다. 응답의
MAC/IP/포트/checksum·ACK 순서 번호를 확인해 SYN+ACK는 열림, RST+ACK는 닫힘,
기한 경과는 응답 없음으로 구분합니다. 열린 포트에는 RST+ACK를 반환해 연결을
완성하지 않습니다. ARP 실패는 스캔 오류이며 미전송 포트를 응답 없음으로 세지 않습니다.
8개 슬롯·32개 공용 출력 큐를 쓰고 큐가 찼을 때 추가 프로브를 만들지 않습니다.

`network_get_status.scan`에 running/phase/target/start/end/timeout_ms·완료/전체·open·
closed/filtered/pending/error를 반환합니다. 중지 시 완료된 결과를 유지하며 남은
포트를 임의 합산하지 않습니다. 새 시작은 이전 범위를 대체하고 새 난수 순서 번호를
사용합니다. 서버 소켓/PC 라우팅/외부 네트워크를 스캔하는 코드는 사용하지 않습니다.

중지·키/클라이언트 종료·NCM OFF·채널 전환은 원래 스캔 수명을 취소합니다. 출력
큐에 있는 프로브뿐 아니라 MCU 워커에 대기 중인 요청에도 원래 키/클라이언트·
스캔 수명·UID/세대/관리 세션을 검사합니다. 이미 보낸 패킷을 되돌리거나 재시도하지
않습니다. 취소된 프로브 때문에 NCM/호스트 공유 전체를 끄지는 않습니다.
UI의 포트 스캔 탭에서 범위/진행/결과·명시적 시작/중지를 제공합니다. 창을 닫으면
그 창이 시작한 스캔은 취소되고 완성된 NCM 링크는 유지됩니다.

자동 시험은 메모리 내 smoltcp 타깃의 열림/닫힘/무응답, 잘못된 ACK/checksum/MAC,
범위·ARP 실패·슬롯/큐 한도, 모의 MCU의 대기 중 중지/키 폐기·다른 UID 전환과
인증 REST 시작→MCP 중지 경로를 다룹니다. 실제 타깃 스캔은 수행하지 않았습니다.

## 새 loopback TCP/UDP 포워딩

```json
{"rules":[{"id":"ssh","protocol":"tcp","listen_port":18022,"target_ip":"192.168.127.100","target_port":22}]}
```

로컬 수신 주소는127.0.0.1로 고정합니다. 기존 PC 서비스·NIC·라우트·방화벽을
변경하거나 다른 보드로 포워딩하지 않습니다. 각 규칙은 고유한1..64 ASCII 영숫자/
밑줄/하이픈 ID, tcp/udp,1..65535 로컬/타깃 포트와 현재 NCM /24의 canonical IPv4
주소를 사용합니다. 같은 protocol/listen_port와 ID 중복은 거부하며 TCP/UDP는 같은
숫자의 포트를 별도로 쓸 수 있습니다. 일반 제어처럼 UID/인터페이스 인자를 받지 않습니다.

configure는 중지 상태에서 규칙만 적용합니다. 빈 배열은 목록 삭제이며 적용해도
포트를 열지 않습니다. UID별 최대64개 채널을 `preferences.json`에 원자 저장하고 NCM
OFF/ON이나 앱 재실행에서 자동으로 시작하지 않습니다. [설정 화면](PREFERENCES.md)에서
Electron 규칙을 검토해 가져옵니다. 실행 중 자원에는 다음 명시적 시작 때 적용하며
실제 시작에서는 현재 NCM 서브넷을 다시 검증합니다.
start는 NCM ON/link_up 후1..32개 규칙을 검증합니다. 유효한 시작은 기존 포워딩을
중지·완전히 정리한 뒤 새 소켓을 준비합니다. 새 bind 실패 시 이미 연 새 소켓을 모두
닫으며 이전 포워딩은 OFF로 남습니다. 잘못된 규칙은 기존 가동 자원을 건드리지 않습니다.

새 TCP 연결·UDP 클라이언트는 호스트 공유와 같은32개 RAII 연결 예산을 예약합니다.
스택 수락을 기다리는 연결과 종료 중 I/O도 포함합니다. TCP는4KiB 조각·방향별8개 큐·
16KiB 스택 버퍼·양방향 half-close와 타깃 connect15초/idle120초를 적용합니다.
UDP는 클라이언트별8개 큐/1472B와 idle30초를 사용하며 정상 idle 만료는 오류로
표시하지 않습니다. 내부 발신 포트는47000..65535에서 순환하고 종료 후60초 격리합니다.
닫힌 포트의 입력을 거부하며 격리 시간에는 늦은 응답이 새 로컬 클라이언트로 넘어가지
않게 합니다.

`network_get_status.forwarding`은 running/cleanup_required/rules/connections/dropped/error를,
`flow_usage`는 스택 수락 대기와 종료 중 I/O를 포함한 전체 예약 수를 반환합니다.
실행 중/실패한 수신 자원이 남아 있으면 cleanup_required이며 중지를 통해 정리합니다.
완성된 포워딩은 앱 소유로 창/생성 키/클라이언트 종료 후에도 유지합니다. 새 제어 요청의
권한은 폐기되며, 수락 중 요청의 키/클라이언트 종료는 원래 자원을 정리합니다.

중지/채널 전환/NCM OFF는 수신기·연결·대기 패킷의 원래 UID/세대/세션에 고정된
취소 수명을 먼저 닫습니다. 이미 MCU 워커에 대기 중인 전송도 실행 직전에 검사합니다.
수신 태스크와 모든 진행/종료 중 I/O의 종료를 기다린 뒤 정리 응답을 반환합니다.
NCM 전체 종료는 호스트 공유 소켓도 기다린 뒤 원래 MCU의 OFF를 확인합니다.

네트워크 창의 포워딩 탭에서 규칙 추가/삭제·적용/불러오기·시작/중지와 연결/드롭을
표시합니다. 모의 MCU·메모리 내 타깃 및 앱 소유 loopback 소켓으로 TCP64KiB/half-close,
서로 다른 UDP 클라이언트의1472B 왕복·늦은 응답 거부·큐/32연결 제한·bind 롤백,
중지 후 실제 포트 재사용·키/클라이언트 종료·채널 전환·인증 REST 시작→MCP 중지를
검증했습니다. 실제 타깃으로의 포워딩·성능은 시험하지 않았습니다.
