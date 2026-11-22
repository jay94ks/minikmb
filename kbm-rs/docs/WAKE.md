# PC 깨우기: 명시적 WOL

2026-11-21. 기존 Electron의 `wake-on-lan.ts`/`wake.ts` 동작을 Rust 서비스로
이관했습니다. 상단 **PC 깨우기**, REST, MCP가 같은 설정·검증·발신을 사용합니다.

## 사용과 결과

1. 활성 채널의 PC 깨우기 창에서 **WOL 사용**, 타깃 유선 LAN MAC, 관리 PC LAN IPv4를
   지정하고 저장합니다. 값은 UID별 `preferences.json`에 보관합니다.
2. **WOL 보내기**를 누르면 저장된 설정으로 한 번 발신합니다. 수정한 미저장 설정으로는
   발신하지 않습니다. 설정 저장/가져오기·채널 선택·앱 재실행은 자동 발신하지 않습니다.
3. code128은 패킷을 OS에 전달한 결과이며 기상 확인이 아닙니다. 타깃의 LAN/BIOS·전원
   상태·네트워크 경로는 별도 조건입니다. 자동 재시도나 키 입력으로 대체하지 않습니다.

| code | 의미 |
|---|---|
| 128 | `SentUnconfirmed`: 발신, 지원 여부·실제 기상 미확인 |
| 129 | `IgnoredDisabled`: 설정이 비활성이라 발신하지 않음 |
| 130 | `IgnoredUnsupported`: 현재 시스템의 UDP 브로드캐스트 미지원 |

MAC은 raw12자리/콜론/하이픈을 지원하고 앞뒤 공백을 제거한 소문자 콜론 형식으로
저장합니다. 전부0·멀티캐스트·길이/혼합 구분자 오류는 거부합니다. LAN은 현재 연결된
IPv4 주소를 명시적으로 선택합니다. 전송 때 목록을 다시 읽어 정확히 하나와 일치해야
하며 없는/모호한 주소는 `WOL_ADAPTER_MISSING`으로 알립니다.

Windows의 읽기 전용
[`GetAdaptersAddresses`](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getadaptersaddresses)로
IPv4·연결 상태·prefix를 조회합니다. 크기 변경을 최대3회/1MiB로 제한해 재조회하고
loopback·비가동·브로드캐스트가 없는 /31·/32를 제외합니다. IPv4/prefix에서 directed
broadcast를 계산합니다. 지정한 LAN IPv4의 임시 UDP 소켓을 만들고 브로드캐스트를
허용한 뒤 그 subnet의 **UDP9**로 `FF×6 + MAC×16`, 총102B를 한 번 보냅니다.
쓰기 대기는2초이며 임의 목적지·포트·DNS/default route 자동 선택을 받지 않습니다.
소켓은 작업 종료 시 닫힙니다. NIC/route/firewall/ICS 설정이나 USB/NCM을 변경하지 않습니다.

## REST·MCP

모든 명령은 UID 인자를 받지 않고 현재 active에 고정됩니다. 없으면
`NO_ACTIVE_CHANNEL`입니다. 키·클라이언트·원래 UID/세대와 앱 종료 상태를 실행 직전,
그리고 실제 발신 직전에 다시 확인합니다. 최대2개 작업만 허용하며 초과 시
`QUEUE_FULL`입니다. UI 창 닫기·채널 전환·키 폐기 시 아직 발신하지 않은 작업은 취소합니다.
이미 OS에 전달된 패킷을 취소되었다고 표시하거나 재발신하지 않습니다.

| MCP 도구 | REST | 인자 |
|---|---|---|
| `wake_get_status` | GET `/api/v1/active/wake` | 없음 |
| `wake_list_adapters` | GET `/api/v1/active/wake/adapters` | 없음 |
| `wake_configure` | POST `/api/v1/active/wake/configure` | 아래 세 필드 |
| `wake_send` | POST `/api/v1/active/wake/send` | `{}` |

```json
{"enabled":true,"mac":"02:aa:bb:cc:dd:ee","local_address":"192.168.1.4"}
```

GET의 query와 일반 명령의 UID/주소/포트 덮어쓰기는 거부합니다. 상태는 저장된 config와
`usb_wake_supported:false`, `system_control_supported:false`를 반환합니다.
현재 STM32 펌웨어에는 두 기능의 wire 명령이 없으므로 UI/API도 이를 지원한다고
표시하지 않습니다. WOL 결과에는 원래 UID/세대·code/message·`wake_confirmed:false`가
들어갑니다. API 키 원문·MAC/패킷 본문을 로그에 기록하는 기능은 추가하지 않았습니다.

## 검증

4개 모의 전송 시험으로102B 배치·MAC 정규화/거부·prefix 계산·무자동 발신·채널별 격리,
비활성/미지원/없는 어댑터, 발신 직전 채널 전환/키 폐기, UID/목적지 덮어쓰기와 작업
한도를 확인했습니다. REST/MCP 인증 통합 시험은 **비활성 설정/발신**만 사용해129를
확인합니다. 실제 WOL·NIC 변경·타깃 PC 기상 시험은 수행하지 않았습니다. 네이티브
발신의2초 timeout·LAN 목록/선택기의 실제 환경 및 실제 기상 수용 시험은 남아 있습니다.
