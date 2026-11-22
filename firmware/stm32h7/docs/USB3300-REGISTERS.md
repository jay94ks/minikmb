# USB3300 register map and initialization

대상은 MiniKMB의 USB3300-EZK 두 개다. 채널 0은 U5·Y1·J3, 채널 1은 U6·Y2·J4를 사용한다. 두 채널 모두 STM32H743의 OTG_HS에 8비트 ULPI로 연결된다. J9 관리 CDC는 별도의 OTG_FS다. 아래 표에는 USB3300의 공개 ULPI 주소 전체, POR 기본값, 채널 0에서 읽은 값을 담았다. 기준은 Microchip DS00001783D §6.1.4와 표 6-3이다. [USB3300 데이터시트](https://ww1.microchip.com/downloads/aemDocuments/documents/UNG/ProductDocuments/DataSheets/USB3300-Data-Sheet-DS00001783.pdf)

## 공개 레지스터 맵

주소와 값은 16진수이며 각 레지스터는 8비트다. `SET` 주소에 쓴 값에서 1인 비트만 설정되고, `CLEAR` 주소에 쓴 값에서 1인 비트만 해제된다. 별칭 주소를 읽으면 본 레지스터와 같은 값이 반환된다. 쓰기 시 예약 비트는 0을 유지한다.

| 레지스터 | 읽기 | 전체 쓰기 | SET | CLEAR | POR | J3 실패 상태 |
|---|---|---|---|---|---|---|
| Vendor ID Low | `0x00` | — | — | — | `0x24` | `0x24` |
| Vendor ID High | `0x01` | — | — | — | `0x04` | `0x04` |
| Product ID Low | `0x02` | — | — | — | `0x04` | `0x04` |
| Product ID High | `0x03` | — | — | — | `0x00` | `0x00` |
| Function Control | `0x04–0x06` | `0x04` | `0x05` | `0x06` | `0x41` | `0x45` |
| Interface Control | `0x07–0x09` | `0x07` | `0x08` | `0x09` | `0x00` | `0x00` |
| OTG Control | `0x0A–0x0C` | `0x0A` | `0x0B` | `0x0C` | `0x06` | `0x01` |
| USB Interrupt Enable Rising | `0x0D–0x0F` | `0x0D` | `0x0E` | `0x0F` | `0x1F` | `0x1F` |
| USB Interrupt Enable Falling | `0x10–0x12` | `0x10` | `0x11` | `0x12` | `0x1F` | `0x1F` |
| USB Interrupt Status | `0x13` | — | — | — | `0x00` | `0x14` |
| USB Interrupt Latch | `0x14`·읽으면 래치 해제 | — | — | — | `0x00` | 미계측 |
| Debug | `0x15` | — | — | — | `0x00` | `0x01` |
| Scratch | `0x16–0x18` | `0x16` | `0x17` | `0x18` | `0x00` | `0x00` |

ID는 low/high를 조합해 `0424:0004`다. ULPI에서 비공개인 주소는 접근 대상에서 제외한다. Carkit, Extended, Vendor 전용 영역(`0x30–0x3F` 포함)은 Microchip 시험용으로 예약돼 있다. §6.1.4.10의 제목에는 `0x13`에 auto-clear 표기가 있지만, 표 6-3 및 비트 본문은 `0x13`을 현재 상태, `0x14`를 읽을 때 이벤트가 지워지는 래치로 정의한다. 동일한 상태 비트 `0x13=0x14`는 J3의 별도 조회에서도 반복됐다. [USB3300 표 6-3 및 §6.1.4.10–16](https://ww1.microchip.com/downloads/aemDocuments/documents/UNG/ProductDocuments/DataSheets/USB3300-Data-Sheet-DS00001783.pdf#page=20)

### Function Control (`0x04`)

| 비트 | 필드 | POR | 동작 |
|---|---|---:|---|
| 1:0 | XcvrSelect | `01` | `00` HS, `01` FS, `10` LS, `11` FS 프리앰블을 붙이는 LS 전송 |
| 2 | TermSelect | 0 | 속도·OpMode·OTG 풀다운과 함께 D+/D− 종단 선택 |
| 4:3 | OpMode | `00` | `00` 정상, `01` 송신 구동 해제, `10` 비트 스터핑·NRZI 인코딩 해제, `11` 예약 |
| 5 | Reset | 0 | 1이면 트랜시버 리셋, 완료 후 자동으로 0. ULPI 레지스터는 보존 |
| 6 | SuspendM | 1 | 0이면 PHY 저전력 진입. 해당 모드에서 CLKOUT 정지. STP로 깨우고 DIR이 Low가 된 뒤 동기 통신 재개 |
| 7 | 예약 | 0 | 0 유지 |

### Interface Control (`0x07`)

| 비트 | 필드 | POR | 동작 |
|---|---|---:|---|
| 0 | 6-pin FsLsSerialMode | 0 | 1이면 6핀 FS/LS 직렬 모드, 종료 시 자동 해제 |
| 1 | 3-pin FsLsSerialMode | 0 | 1이면 3핀 FS/LS 직렬 모드, 종료 시 자동 해제 |
| 2 | 예약 | 0 | 0 유지 |
| 3 | ClockSuspendM | 0 | FS/LS 직렬 모드에서 1이면 CLKOUT 60 MHz 유지, 0이면 정지 |
| 4 | AutoResume | 0 | 호스트 모드에서 PHY가 재개 신호를 자동 송신할 수 있도록 함 |
| 5 | IndicatorComplement | 0 | EXTVBUS 입력 극성 반전 |
| 6 | IndicatorPassThru | 0 | 1이면 EXTVBUS를 내부 VBUS 비교기와 AND하지 않고 전달 |
| 7 | InterfaceProtectDisable | 0 | 1이면 기동 보호용 STP 내부 풀업을 해제 |

### OTG Control (`0x0A`)

| 비트 | 필드 | POR | 동작 |
|---|---|---:|---|
| 0 | IdPullup | 0 | ID 감지용 3.3 V 풀업 |
| 1 | DpPulldown | 1 | D+ 15 kΩ 풀다운 |
| 2 | DmPulldown | 1 | D− 15 kΩ 풀다운 |
| 3 | DischrgVbus | 0 | SRP용 VBUS 방전 저항 |
| 4 | ChrgVbus | 0 | SRP용 VBUS 충전 저항 |
| 5 | DrvVbus | 0 | 외부 VBUS 전원 스위치 제어 요청 |
| 6 | DrvVbusExternal | 0 | 별도의 외부 VBUS 제어 요청. 비트 5 또는 6이 1이면 CPEN High |
| 7 | UseExternalVbusIndicator | 0 | 1이면 EXTVBUS 선택, 0이면 내부 VbusValid 비교기 사용 |

### 인터럽트·Debug·Scratch

| 비트 | 인터럽트 신호 | Rising `0x0D` / Falling `0x10` | Status `0x13` / Latch `0x14` |
|---|---|---|---|
| 0 | HostDisconnect | 각각 상승·하강 통지 허용, POR 1. 호스트 모드용 | 현재 값 / 마스크를 통과한 이벤트 |
| 1 | VbusValid | 각 에지 통지 허용, POR 1 | 현재 값 / 이벤트 |
| 2 | SessValid | 각 에지 통지 허용, POR 1 | 현재 값 / 이벤트 |
| 3 | SessEnd | 각 에지 통지 허용, POR 1 | 현재 값 / 이벤트 |
| 4 | IdGnd | 각 에지 통지 허용, POR 1 | 현재 값 / 이벤트 |
| 7:5 | 예약 | 0 | 0 |

`0x13`의 신호 값은 비교기 입력에 따라 바뀐다. `0x14`는 읽으면 래치가 지워지므로 이벤트 진단에서 첫 값을 따로 보관한다. `0x15[1:0]`은 현재 `LineState[1:0]`이며 `[7:2]`는 예약이다. `0x16[7:0]`은 전체가 테스트용 저장 공간이다. ID 또는 Scratch 확인은 ULPI 레지스터 트랜잭션의 검증 범위다. USB 프레임 수신은 별도로 확인한다.

## 하드웨어 리셋과 ULPI 확인 순서

1. PHY에 3.3 V를 공급하고 XI/XO의 24 MHz 기준을 기동한다. 외부 오실레이터를 쓰는 구성은 24 MHz를 XI에 공급하고 XO를 개방한다. USB3300의 내부 PLL이 잠기면 동기 ULPI용 CLKOUT은 60 MHz다.
2. RESET을 High로 유지하면서 CLKOUT이 활성화될 때까지 기다린다. 데이터시트 최소 조건은 **활성 CLKOUT 한 주기보다 길게** RESET을 유지하는 것이다. 제안한 1 µs는 60 MHz에서 약 60주기의 설계 여유다. CLKOUT이 활성화된 상태에서 RESET을 Low로 내리고, 다른 PHY 디지털 입력은 이후 최소 2주기 동안 유지한다.
3. 링크가 데이터 버스를 아직 구동할 수 없으면 STP를 High로 유지한다. (DIR=0)으로 PHY가 버스를 넘긴 뒤 Link가 DATA에 Idle `0x00`을 구동할 수 있을 때 STP를 Low로 내린다. PHY가 저전력 상태에서 깨어날 때도 STP를 먼저 올리고 DIR이 내려올 때까지 유지한다.
4. 선택 검증에서 DIR이 Low인 상태에 Vendor ID Low `0x00`을 읽어 `0x24`를 확인한다. NXT는 개별 레지스터 명령의 바이트 핸드셰이크에 사용한다. Scratch 시험은 원래 값을 읽고 `0x55`·`0xAA`를 각각 써서 되읽은 뒤 원래 값을 복원한다. 비트 SET/CLEAR 별칭은 필요한 경우에만 시험한다. [USB3300 §6.1.5, §6.1.9–11, §6.3](https://ww1.microchip.com/downloads/aemDocuments/documents/UNG/ProductDocuments/DataSheets/USB3300-Data-Sheet-DS00001783.pdf)

### 모드 전환 예

| 단계 | Function Control | OTG Control | 의미 |
|---|---|---|---|
| 전원 인가 후 POR | `0x41` | `0x06` | FS 선택, TermSelect=0, SuspendM=1. POR의 Interface Control은 `0x00` |
| 표준 Device attach 또는 FS | `0x45` | `0x00` | Bus session과 Device 준비를 확인한 후 D+ 풀업, D+/D− 풀다운 해제, CPEN Low |
| MiniKMB에서 관측한 Device FS | `0x45` | `0x01` | 표준 FS 종단과 동일. ID 풀업만 추가로 켜진 상태 |
| 표준 Device HS chirp | `0x54` | `0x00` | HS 트랜시버·TermSelect=1·OpMode=10. 실제 Link가 핸드셰이크를 수행 |
| 표준 Device HS | `0x40` | `0x00` | HS 트랜시버, 정상 인코딩, HS 종단. 핸드셰이크 완료 후 |
| PHY 저전력으로 전환하는 예 | `0x05` | 종단 상태 유지 | FS 종단을 유지한 `0x45`에서 SuspendM을 0으로 해제 |
| Host FS 예 | `0x45` | `0x66` | 양쪽 풀다운과 CPEN 활성. 외부 VBUS 스위치가 전제. 비트 5 또는 6 중 하나만으로도 CPEN High |
| Host HS chirp / 정상 HS 예 | `0x50` / `0x40` | `0x66` | Host의 SE0 리셋과 chirp 절차에서 Link가 전환 |

위 값은 표 6-8의 트랜시버·종단 설정과 `SuspendM`을 합성한 예다. Host 행은 모드 비교용 예이며 현재 보드의 타깃 경로에는 적용하지 않는다. MiniKMB 타깃은 Device이며 CPEN 및 EXTVBUS가 미연결이다. 표준 Device는 내부 비교기의 `SessValid`로 세션을 판단한다. `VbusValid`는 더 높은 임계값을 쓰므로 Device attach 조건과 구별한다. [USB3300 표 6-8 및 §6.5.3–4](https://ww1.microchip.com/downloads/aemDocuments/documents/UNG/ProductDocuments/DataSheets/USB3300-Data-Sheet-DS00001783.pdf)

## 현재 펌웨어가 사용하는 조합

보드 시작 코드가 PB7(USB3300 RESET)을 Low로 구성한다. [InitUlpi](../Core/Src/usb/target_usb.c)에서 PC2_C(DIR)와 PC3_C(NXT)의 아날로그 스위치를 닫고, PC0(STP)을 push-pull High로 유지한다. ULPI DATA[7:0]과 DIR·NXT는 AF10, no-pull, Very High로 설정한다. PA5(CLKOUT)는 준비 검사 중에만 TIM2 ETR로 전환한다.

| 순서 | 검사와 동작 |
|---|---|
| 초기 CLKOUT 확인, 단계 4 | STP를 High로 유지한 상태에서 58.8–61.2 MHz를 세 번 연속 확인한다. |
| RESET 유지 중, 단계 9 | PB7을 High로 올리고 2 ms 뒤 같은 범위의 CLKOUT을 세 번 연속 확인한다. |
| RESET 해제 후, 단계 5 | PB7을 Low로 내리고 2 ms 뒤 CLKOUT 범위와 DIR Low를 함께 세 번 연속 확인한다. |
| OTG_HS 시작, 단계 2→3 | PA5를 AF10으로 돌리고 TIM2를 끈다. 준비가 완료되면 STM32의 ULPI·OTG_HS 클록을 활성화하고 OTG_HS를 RCC에서 리셋한다. PC0을 AF10으로 넘긴 뒤 TinyUSB의 rhport 1 Device를 시작한다. |

TIM2는 CLKOUT을 4분주해 센다. DWT 사이클 카운터로 약 1 ms의 표본 시간을 구하고 카운터 증가량을 Hz로 환산한다. 한 단계에 250 ms를 사용하며, 준비가 실패하면 타깃 태스크에서 250 ms 뒤 재시도한다. 이 계측값은 초기화 시점의 평균 주파수다. 런타임 CLKOUT·지터와 ULPI setup/hold는 별도 측정 항목이다.

보드 코드는 VBUS sensing을 켜고 TinyUSB에 포트를 넘긴다. DWC2는 8비트 SDR ULPI와 Device 모드를 선택하고 PHY의 부착·chirp·종단 전환을 제어한다. EXTVBUS 추가 제어와 FS/LS 직렬 모드 관련 코어 비트는 지운다. 현재 기본 타깃과 관리 스택은 PIO를 사용한다. 별도 DMA 시험 이미지는 검증 기록에서 프로필을 확인한다. [보드 TinyUSB 설정](../Core/Inc/tusb_config.h) · [DWC2 PHY 초기화](../Middlewares/Third_Party/TinyUSB/src/portable/synopsys/dwc2/dwc2_common.c)

| 프로필 | DCFG.DSPD | DCFG.XCVRDLY | 타깃 OTG_HS |
|---|---:|---:|---|
| 기본 Debug, MINIKMB_TARGET_HIGH_SPEED=0 | 01 | 0 | 외장 PHY에서 Full Speed. 120/240 MHz HCLK 실기에서 열거 후 TRDT=6 |
| Release, MINIKMB_TARGET_HIGH_SPEED=1 | 00 | 1 | High Speed 협상 허용. HS 열거 뒤 TRDT=9. FS로 협상되면 FS용 TRDT를 적용 |

두 프로필의 USB3300 동기 ULPI CLKOUT 기준은 60 MHz다. 타깃 코드가 MCU 설정으로 시작하면 공개 PHY 레지스터에 DWC2가 모드 변화에 필요한 공개 PHY 레지스터 명령을 보낸다. 채널 0에서 Function Control 0x45(FS)와 0x40(HS)을 별도 실기에서 읽었다. Vendor ID와 Scratch의 0x55·0xAA·0xFF·0x00 읽기 대조는 SWD 진단으로 수행했고 원래 Scratch 값 0x00으로 복원했다. RESET 핀과 Function Control Reset은 트랜시버를 리셋하며, 전체 POR 기본값으로 돌아가는 조건은 PHY 전원 재인가다. [타깃 프로필 헤더](../Core/Inc/target_features.h) · [STM32 RM0433 OTG_HS](https://www.st.com/resource/en/reference_manual/dm00314099-stm32h742-stm32h743753-and-stm32h750-value-line-advanced-armbased-32bit-mcus-stmicroelectronics.pdf) · [USB3300 §6.1.4.5 및 §6.4.2](https://ww1.microchip.com/downloads/aemDocuments/documents/UNG/ProductDocuments/DataSheets/USB3300-Data-Sheet-DS00001783.pdf)

