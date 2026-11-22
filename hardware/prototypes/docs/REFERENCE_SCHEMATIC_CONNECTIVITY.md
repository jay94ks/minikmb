# 레퍼런스 회로 연결관계 분석

이 문서는 `future` 라이브러리 대상 IC들의 레퍼런스 회로를 기능 블록과 net 흐름 기준으로 분석한 결과입니다. 부품 목록은 [`bom/REFERENCE_BOM.md`](../bom/REFERENCE_BOM.md), 패키지별 핀 번호는 [`pinouts`](../pinouts/)를 함께 참조하십시오.

## 판독 기준

- **확정**: 회로도 선과 net label을 직접 추적할 수 있는 연결
- **기능상 확정**: 핀 설명과 응용 회로가 일치하지만 별도 net label 또는 RefDes가 생략된 연결
- **추정 금지/TBD**: 공개 문서에 회로가 없거나 도면 해상도로 확정할 수 없는 연결
- 신호 방향은 IC를 기준으로 표기합니다. 예: `PWREN -> 외부 power switch`, `OVCUR <- 외부 power switch`.
- 커넥터 shield와 회로 GND의 결합 방식은 원문이 직접 연결한 경우에만 동일 net으로 적었습니다.

## 전체 기능 관계

```text
HDMI source
    |
    v
MS2130 HDMI RX ----> USB 3.x UVC/UAC device ----> USB host
    |                         |
    +---- SPI configuration --+

USB host ----> GL3523 upstream port P0
                    |
                    +---- downstream P1
                    +---- downstream P2
                    +---- downstream P3
                    +---- downstream P4

USB-C source + adapter power
    |              |
    +---- LDR6023SS power/CC/HPD control ----> 5 V / 3.3 V / 1.2 V rails
    |
    +---- PS171 DisplayPort Alt Mode receiver ----> HDMI output

TPS2553: 5 V source ----> protected/current-limited 5 V load or USB port
MP2315: 4.5-24 V ----> synchronous buck ----> 5 V evaluation output
MP2482: 4.5-30 V ----> asynchronous buck ----> 3.3 V evaluation output
```

---

## 1. MacroSilicon MS2130

출처: [MS2130 데이터시트](../references/MS2130-datasheet.pdf), 14쪽 전형 응용 회로.

### 1.1 기능 블록 흐름

```text
HDMI-A J1
  TMDS D2/D1/D0/CLK
        |
        +--> U5/U6 ESD arrays --> MS2130 HDRX2/1/0/CLK

  DDC SCL/SDA --------------------> MS2130 RXDDCSCL/RXDDCSDA
  +5 V -------- R6 1 k ----------> MS2130 HDRXDET
  HPD <--------- R7 1 k ---------- MS2130 HDRXHPD

MS2130 USB 3.x device
  SSTX +/- --> C1/C2 AC coupling --> U2 ESD --> CN1 SSTX +/-
  SSRX +/- <------------------------ U2 ESD <-- CN1 SSRX +/-
  DP/DM    <------------------------ U1 ESD <-> CN1 D+/D-

MS2130 SPI master <----> U4 64-KByte SPI flash
MS2130 XTIN/XTOUT <----> X1 24 MHz + R3/C4/C6
```

### 1.2 USB 3.x 연결

| 경로 | 커넥터 측 | 보호/결합 | MS2130 측 | 판정 |
|---|---|---|---|---|
| USB 2.0 D- | CN1 `D-` | U1 ESD | `DM`, pin 3 | 확정 |
| USB 2.0 D+ | CN1 `D+` | U1 ESD | `DP`, pin 4 | 확정 |
| SuperSpeed TX- | CN1 `SSTX-` | U2 ESD, C2 100 nF 직렬 | `SSTXM`, pin 9 | 확정 |
| SuperSpeed TX+ | CN1 `SSTX+` | U2 ESD, C1 100 nF 직렬 | `SSTXP`, pin 11 | 확정 |
| SuperSpeed RX- | CN1 `SSRX-` | U2 ESD, DC 연결 | `SSRXM`, pin 13 | 확정 |
| SuperSpeed RX+ | CN1 `SSRX+` | U2 ESD, DC 연결 | `SSRXP`, pin 14 | 확정 |
| VBUS | CN1 VBUS | U1의 VBUS 기준 입력에도 연결 | `VBUS` net; 칩 전원 rail과는 별도 | 확정 |
| GND/shield | CN1 GND와 shell | GND plane | 칩 GND/EP | 확정 |

TX AC coupling capacitor는 MS2130 쪽과 connector/ESD 쪽 사이에 직렬로 들어갑니다. RX pair에는 회로도상 별도 AC coupling capacitor가 없습니다.

### 1.3 선택적 USB plug detection

```text
USB VBUS 5 V -- R8 24 k --+--> GPIO0, pin 42
                           |
                         R9 47 k
                           |
                          GND
```

이 회로는 5 V를 약 3.31 V로 분압합니다. 데이터시트 주석에 따라 MS2130의 3.3/2.5/1.1 V rail이 켜져 있는데 USB 신호 케이블이 host에 연결되지 않을 수 있는 설계에서만 필요하며, 그 외에는 NC 처리할 수 있습니다.

### 1.4 HDMI RX 연결

| HDMI J1 | 중간 회로 | MS2130 | 비고 |
|---|---|---|---|
| D2+/D2- | U6 ESD | `HDRX2P/N`, pins 62/61 | 100 Ω differential routing 대상 |
| D1+/D1- | U6 ESD | `HDRX1P/N`, pins 58/57 | 동일 |
| D0+/D0- | U5 ESD | `HDRX0P/N`, pins 54/53 | 동일 |
| CK+/CK- | U5 ESD | `HDRXCP/CN`, pins 49/46 | 동일. pin 50은 AVSS_4이므로 CLK-에 연결하면 안 됨 |
| DDC CLK | R4 47 k pull-up to `HDRX5V` | `RXDDCSCL`, pin 26 | 5 V tolerant input |
| DDC DATA | R5 47 k pull-up to `HDRX5V` | `RXDDCSDA`, pin 27 | 5 V tolerant bidirectional |
| +5 V | R6 1 k 직렬 | `HDRXDET`, pin 28 | HDMI source 존재 감지 |
| HPD | R7 1 k 직렬 | `HDRXHPD`, pin 29 | 칩에서 HDMI source 방향 출력 |
| +5 V rail | C5 100 nF to GND | `HDRX5V` | DDC pull-up rail decoupling |

`HDRX_REXT`, pin 45는 R1 1.6 kΩ/1%로 GND에 연결하며 pin 가까이 배치합니다. HDMI shield pins도 원문에서는 GND에 직접 연결되어 있습니다.

### 1.5 SPI flash

| MS2130 | Flash U4 | 추가 연결 |
|---|---|---|
| `SPICS`, pin 38 | `/CS`, pin 1 | active-low chip select |
| `SPIMISO`, pin 37 | `DO/IO1`, pin 2 | flash -> MS2130 |
| `SPIMOSI`, pin 35 | `DI/IO0`, pin 5 | MS2130 -> flash |
| `SPICLK`, pin 36 | `CLK`, pin 6 | clock |
| `DVDD33_Flash` | `/WP`, pin 3; `/HOLD`, pin 7; VCC pin 8 | 세 핀 모두 3.3 V |
| GND | pin 4 | common ground |

별도 pull-up은 그려져 있지 않고 `/WP`와 `/HOLD`가 rail에 직접 연결됩니다.

### 1.6 Clock와 bias

```text
XTIN pin 21 ----+---- X1 24 MHz ----+---- XTOUT pin 22
                |                   |
              C4 22 pF            C6 22 pF
                |                   |
               GND                 GND

R3 1 MΩ: XTIN과 XTOUT 사이 병렬
R2 200 Ω: USB_REXT pin 7에서 GND
R1 1.6 kΩ: HDRX_REXT pin 45에서 GND
```

### 1.7 전원 rail

레퍼런스 회로는 regulator 자체를 그리지 않고 이미 생성된 rail을 MS2130에 공급합니다.

| Rail | 연결 핀 |
|---|---|
| `DVDD33` | 1, 16, 31, 33, 41 |
| `AVDD33_USB` | 2, 5, 8 |
| `AVDD11_USB` | 6, 10, 12 |
| `DVDD11` | 18, 39 |
| `AVDD25` | 20, 23, 47 |
| `AVDD11` | 24, 51, 55, 59 |
| `AVDD33` | 48, 52, 56, 60 |
| GND/EP | 15, 17, 19, 25, 30, 32, 34, 40, 50, 64, exposed pad 65 |

각 rail에는 보통 100 nF를 전원 pin 가까이 배치하라는 지시가 있지만 RefDes와 정확한 수량은 제시되지 않습니다. 서로 다른 1.1 V/2.5 V/3.3 V rail을 임의로 합치면 안 됩니다.

### 1.8 미사용/주의 핀

- `MCU_SEL`, pin 44와 `DVO_D7`, pin 63은 debug pin으로 기본 floating입니다.
- GPIO/SPI 겸용 핀은 flash를 사용할 때 SPI 기능으로 점유됩니다.
- HDMI와 USB differential pair는 ESD 보호 후 pair 순서와 P/N 극성을 그대로 유지해야 합니다.

---

## 2. Genesys Logic GL3523 OTY10/OTY30 QFN-76

출처: [GL3523 데이터시트 Rev. 1.70](../references/GL3523-datasheet-rev1.70.pdf).

공개 문서에는 완전한 reference schematic이 없습니다. 다음 연결은 pin table과 Fig. 4.5-4.11에서 확정되는 범위이며, USB connector, AC coupling, ESD, port power switch의 실제 부품 연결은 별도 Genesys reference schematic이 필요합니다.

### 2.1 USB topology

```text
Upstream connector
  USB 2.0 D+/D- ----------> DP_P0 / DM_P0
  USB 3.x TX from host ---> RXN1_P0 / RXP1_P0
  USB 3.x RX to host <----- TXN1_P0 / TXP1_P0

GL3523 hub core
  +--> downstream P1: DP/DM, TXP/N, RXP/N
  +--> downstream P2: DP/DM, TXP/N, RXP/N
  +--> downstream P3: DP/DM, TXP/N, RXP/N
  +--> downstream P4: DP/DM, TXP/N, RXP/N
```

`P0`는 upstream, `P1`-`P4`는 downstream으로 해석됩니다. USB 3.x 명칭은 칩 기준 방향이므로 host의 TX가 칩 RX에, 칩 TX가 host RX에 연결됩니다.

### 2.2 내부 regulator와 rail

```text
5 V input
  +--> V5 ------------------------> internal 5 V-to-3.3 V LDO --> V33 output
  |
  +--> VDDP --> internal switch --> SW --> external L --> 1.2 V rail
                      VSSP --> GND              |
                                                  +--> FB sense/divider

1.2 V rail --> DVDD12, AVDD12, VP12_P0..P4
3.3 V rail --> DVDD33, AVDD33_1..3 and I/O rail
```

| 핀 | 연결 규칙 |
|---|---|
| `V5`, pin 43 | 5 V source 입력 |
| `V33`, pin 44 | 내부 LDO의 3.3 V 출력; rail decoupling 필요 |
| `VDDP`, pin 42 | embedded 5 V-to-1.2 V switcher 전용 5 V 입력 |
| `SW`, pin 41 | 외부 인덕터의 switch 쪽 |
| `FB`, pin 39 | 인덕터 뒤 1.2 V 출력 감지; adjustable version은 분압, fixed version은 출력에 직접 연결 |
| `VSSP`, pin 40 | switcher power ground |
| `RTERM`, pin 46 | 20 kΩ로 GND에 연결, 필수 |
| exposed pad 77 | GND |

문서는 5 V-to-3.3 V LDO 최대 250 mA, 5 V-to-1.2 V switcher 최대 0.8 A를 제시합니다. 외부 인덕터/분압/커패시터 값은 공개 schematic에서 확정되지 않습니다.

### 2.3 Reset/plug sensing

```text
Upstream VBUS 5 V -- R1 --+--> RESETJ
                           |
                          R2
                           |
                          GND
RESETJ ---------------- capacitor to GND

VBUS pin --------------------------> internal USB PHY reset input
internal 3.3 V power-good ----------> internal reset input
```

- `RESETJ`, pin 48은 active-low입니다.
- R1/R2 divider는 upstream VBUS drop과 plug/unplug를 감지하도록 threshold를 정합니다.
- 문서에는 R1/R2/C 값이 없고 별도 schematic을 참조하라고만 되어 있습니다.
- 내부 power-good 충족 약 40 µs 뒤 internal reset이 풀리며, 외부 reset은 내부 reset보다 더 오래 유지하라고 권장합니다.

### 2.4 PGANG dual-purpose pin

전원 투입 직후 `PGANG`, pin 53은 입력 strap으로 읽히고 약 50 ms 뒤 suspend 상태 출력으로 전환됩니다.

| 초기 strap | 연결 | mode | 50 ms 이후 역할 |
|---|---|---|---|
| 0 | 100 kΩ 이상 pull-down to GND | individual port power | suspend indicator output |
| 1 | 100 kΩ 이상 pull-up to 3.3 V | gang power | suspend indicator output |

같은 node에 suspend LED를 둘 수 있지만 LED 극성과 최대 2.5 mA 조건을 지켜야 합니다. strap 저항과 LED 부하가 초기 logic level을 훼손하지 않도록 검증해야 합니다.

### 2.5 Port power switch interface

```text
GL3523 PWREN1J..PWREN4J ----> external USB power-switch EN pins
GL3523 OVCUR1J..OVCUR4J <---- external USB power-switch fault/OC pins
```

- `AMBER2=0`: low-active power switch enable
- `AMBER2=1`: high-active power switch enable. 이 경우 PWREN1-4에 외부 pull-down이 필요하다고 문서가 명시합니다.
- TPS2553 같은 active-high EN/open-drain fault switch를 사용할 경우 `AMBER2=1` 쪽 논리가 자연스럽지만, 실제 Genesys reference schematic의 pull-up/pull-down과 fault polarity를 확인해야 합니다.

### 2.6 PSELF, LEDs, SPI/SMBus, clock

- `PSELF=1`이면 self-powered, `PSELF=0`이면 bus-powered로 strap합니다. QFN-76 pin table에 PSELF가 직접 보이지 않아 multifunction pin/variant별 mapping은 별도 schematic 확인이 필요합니다.
- `PGREEN_P1..P4`, `PAMBER_P1..P4` 출력은 외부 직렬 저항과 LED를 거쳐 DGND로 연결합니다.
- SPI interface는 `P_SPI_CZ`, `P_SPI_CK`, `P_SPI_DO`, `P_SPI_DI` 네 선으로 serial configuration memory에 연결합니다. 명칭의 DI/DO 방향은 데이터시트에서 칩 기준 설명을 다시 확인해야 합니다.
- SMBus mode에서는 `OVCUR3J=SMD`, `OVCUR4J=SMC`로 겸용됩니다. RESETJ가 low인 동안 register를 쓰고 reset 해제 약 1 ms 전에 설정을 완료하는 timing이 제시됩니다.
- `X1/X2`, pins 57/56에는 25 MHz crystal을 연결합니다. 외부 3.3 V clock 사용 시 X1에 넣고 X2는 open으로 둡니다.
- `CHIPEN`, pin 45는 내부 pull-up이며 low=standby, high=normal입니다.

### 2.7 GL3523에서 아직 확정할 수 없는 연결

1. SuperSpeed TX AC coupling capacitor의 위치와 값
2. 각 USB port ESD/common-mode choke 구성
3. 1.2 V switcher 인덕터, 출력 capacitor, FB divider 값
4. RESETJ divider/RC 값
5. 각 downstream port power switch의 정확한 enable/fault pull resistor
6. EEPROM/SPI flash 품번과 strap 저항 조합

---

## 3. Texas Instruments TPS2553

출처: [TPS2553 데이터시트 Rev. F](../references/TPS2553-datasheet-revF.pdf), Fig. 24, 27, 28, 29.

### 3.1 패키지별 pin mapping

| 기능 | DBV SOT-23-6 | DRV WSON-6 |
|---|---:|---:|
| IN | 1 | 6 |
| GND | 2 | 5 |
| EN | 3 | 4 |
| `/FAULT` | 4 | 3 |
| ILIM | 5 | 2 |
| OUT | 6 | 1 |
| exposed PowerPAD | 없음 | 7, GND 연결 |

모든 응용 회로의 기본 전력 경로는 `source -> IN -> internal current-limited MOSFET -> OUT -> load`입니다. USB data pair는 TPS2553을 통과하지 않고 별도 경로로 connector에 연결됩니다.

### 3.2 공통 연결

```text
5 V source ----+---- IN [TPS2553] OUT ---- protected 5 V load
               |                         |
             CIN 0.1 µF                COUT/CLOAD
               |                         |
              GND                       GND

/FAULT ---- pull-up resistor ---- logic rail/source
EN <---------------------------- control logic
ILIM ---- programming resistor ---- GND
GND/PowerPAD ---------------------- GND plane
```

- `/FAULT`는 open-drain active-low이므로 pull-up 없이는 high level을 만들 수 없습니다.
- `ILIM`은 외부 logic로 직접 구동하면 안 되고 resistor network로만 GND와 연결합니다.
- 0.1 µF 이상 ceramic CIN은 IN-GND 바로 옆에 둡니다.

### 3.3 Two-level current limit - Fig. 24

```text
ILIM --+-- R1 210 kΩ ---------------------- GND
       |
       +-- R2 22.1 kΩ -- drain Q1 2N7002
                             source -------- GND
                             gate <--------- Current Limit Control
```

- Q1 OFF: ILIM-GND는 R1 210 kΩ만 사용하므로 낮은 current limit.
- Q1 ON: R2가 R1과 병렬이 되어 약 20 kΩ이 되므로 높은 current limit.
- EN은 별도 control signal을 받고 `/FAULT`는 RFAULT 100 kΩ로 input rail에 pull-up됩니다.

### 3.4 Auto-retry - Fig. 27

```text
Input rail -- RFAULT 100 kΩ --+-- /FAULT
                               +-- EN
                               |
                             CRETRY 0.1 µF
                               |
                              GND
```

정상일 때 RFAULT가 EN을 high로 유지합니다. 과전류 시 `/FAULT`가 node를 low로 끌어 EN을 끄고, 칩이 꺼지면 `/FAULT`가 high-Z가 되어 CRETRY가 RFAULT를 통해 충전됩니다. EN threshold에 도달하면 다시 켜지며 fault가 남아 있으면 반복합니다. `RFAULT x CRETRY`가 off-time을 결정합니다.

Fig. 28은 외부 logic driver를 RFAULT 앞에 직렬로 넣어 수동 enable과 auto-retry를 동시에 구현합니다. 이 경우 driver output이 low이면 retry node도 low로 유지됩니다.

### 3.5 USB power switch - Fig. 29

```text
5 V USB input --> IN/OUT switch --> USB port VBUS
USB data -------------------------> USB port D+/D- (TPS2553 우회)
Control Signal -------------------> EN
Fault Signal <--------------------- /FAULT + 100 kΩ pull-up
ILIM -- 20 kΩ --------------------> GND
USB port VBUS -- 120 µF ----------> GND
```

120 µF는 그림의 downstream-facing hub bypass 예입니다. 실제 USB 규격 버전과 port 수, inrush 조건에 맞게 다시 계산해야 하며, 20 kΩ 역시 목표 current limit에 맞춰 조정합니다.

---

## 4. LDR6023 Type-C Docking reference

출처: [LDR6023 데이터시트 Rev. 2.2](../references/LDR6023-datasheet-rev2.2.pdf), 9-10쪽.

이 회로는 LDR6023 단독 회로가 아니라 power mux, 5 V/3.3 V/1.2 V regulators, PS171 DisplayPort-to-HDMI bridge, connectors를 포함한 docking reference입니다. U6은 `LDR6023SS`, SSOP-16입니다.

### 4.1 전원 전체 흐름

```text
Notebook/book-side USB_VBUS ---- Q1 AO4805 --+
                                                +--> VBUS-IN
Adapter USB-C1 VBUS ----------- Q3 AO4407 --+       |
                                            |       +--> U5 TD1583 buck --> PP500 (5 V)
                         Q4 AO6800 gate control      |
                         ^                           +--> U4 XC6206 --> P3V3
                         |                           |
                  VBUS-EN / VADP-EN                  +--> U3 FP6161 --> P1V2
```

Q1/Q3/Q4, D2, R17-R22는 두 VBUS source의 역류 방지/선택을 위한 MOSFET gate-control network로 해석됩니다. 원문은 동작 순서를 설명하지 않으므로 두 enable을 동시에 활성화해도 안전하다고 가정하면 안 됩니다.

### 4.2 USB-C1 adapter connector와 감지

- J3의 VBUS pins는 `USB-C1-VBUS`로 묶이고 ferrite bead FB1을 거칩니다.
- CC1/CC2와 SBU1/SBU2는 LDR6023 주변 net으로 나뉘며 일부 선에 3.3 V TVS와 100 Ω/220 pF filtering이 있습니다.
- VBUS 감지 분압은 다음과 같습니다.

```text
USB-C1-VBUS -- R35 36 kΩ --+--> USB-C1-VBUS-DET --> U6 VDET0, pin 11
                            |
                          R36 12 kΩ
                            |
                           GND

VDET0 node -- C43 1 nF --> GND
```

분압비는 1/4이므로 5 V VBUS에서 약 1.25 V입니다.

### 4.3 LDR6023SS U6 주요 연결

| U6 pin | 신호 | 회로 연결 |
|---:|---|---|
| 1 | VSS | GND |
| 2 | VBEN | `VBUS-EN`; power-path MOSFET control network로 이동 |
| 3 | HPD | `DP_HPD`; PS171의 DP HPD와 연결 |
| 4 | LENS | `VPP` net; 원문 기능 설명 부족 |
| 5 | POL | adapter polarity/enable 관련 network; 도면 net label 중첩으로 재확인 필요 |
| 7 | C0CC2 | USB-C0 CC2 계열, 100 Ω filter network |
| 8 | C0CC1 | USB-C0 CC1 계열, 100 Ω/220 pF filter network |
| 11 | VDET0 | `USB-C1-VBUS-DET`, C43 1 nF to GND |
| 12 | C1CC2 | R32 100 Ω를 통해 `USB-C1-CC2` |
| 13 | C1CC1 | R30 100 Ω를 통해 `USB-C1-CC1` |
| 16 | VDD | R29 10 Ω를 통해 PP500; C40 10 µF와 C41 22 µF로 GND decoupling |

pins 6, 9, 10, 14의 RSV와 pin 15 PDLDO는 원문 회로에서 유효한 외부 연결을 확정하기 어렵습니다. RSV pin을 임의로 rail이나 GND에 연결하지 말고 SS 변형의 최신 제조사 지침을 확인해야 합니다.

### 4.4 5 V buck: TD1583 U5

```text
VBUS-IN --> VIN
           |
          C38/C39 47 µF to GND

SW --> L6 10 µH --> PP500 5 V
 |                  |
 D3 SS34 to GND    C32/C33 47 µF to GND

PP500 -- R25 11 k --+--> FB
                     |
                   R28 3.4 k
                     |
                    GND

EN: R26 100 k / R27 100 k network to VBUS-IN/GND
```

C31 330 pF와 R23 10 Ω는 feedback/고주파 보상 경로입니다. 정확한 series/parallel 위치는 PCB 작성 시 원문을 확대해 재대조해야 합니다.

### 4.5 3.3 V LDO와 1.2 V buck

```text
PP500 -- R24 10 Ω --> U4 XC6206 VIN --> OUT --> P3V3
        C34/C35                         C36/C37
          |                               |
         GND                             GND

PP500 --> U3 FP6161 VIN
PP500 --> U3 EN
SW --> L3 2.2 µH --> P1V2
P1V2 -- R15 51 k --+--> FB
                    |
                  R16 51 k
                    |
                   GND
```

C18은 PP500 input, C17은 P1V2 output bulk capacitor이며 C16 100 pF는 feedback의 고주파 보상/전달 경로입니다.

### 4.6 PS171 USB-C/DP Alt Mode to HDMI 경로

```text
USB-C J2 high-speed lane pairs
        |
        +--> PS171 DRX0/DRX1/DRX2/DRX3
                     |
                     +--> OUT_D2/D1/D0/CLK
                              |
                              +--> HDMI J1 TMDS pairs

USB-C AUX pair -- C5/C6 100 nF coupling --> PS171 AUXP/AUXN
LDR6023 DP_HPD ---------------------------> PS171 DP_HPD
PS171 HDMI_HPD <--------------------------- HDMI J1 HPD
PS171 DDC_SCL/SDA <-----------------------> HDMI J1 SCL/SDA
```

HDMI +5 V는 PP500에서 D1 1N5818W를 거쳐 공급됩니다. R7/R8 2 kΩ는 DDC pull-up, R10 100 kΩ는 HPD/bias network입니다. `DP_HPD`는 R13 0 Ω link를 통해 PS171에 들어갑니다.

### 4.7 PS171 configuration, clock, reset

- W25X40 U1은 `IROMS/IROMQ/IROMW/IROMC/IROMD` bus로 PS171에 연결됩니다.
- flash `/CS`, `/HOLD`, VCC에는 4.7 kΩ pull-up network가 있고, `WP` 계열에는 10 kΩ pull-down이 있습니다.
- X1 27 MHz는 `XTLI/XTLO` 사이, C1/C2 20 pF는 각 crystal terminal에서 GND입니다.
- `RST`는 R6 10 kΩ로 VDD33에 pull-up되고 C4 1 µF로 GND에 연결됩니다.
- `REXT`는 R5 4.99 kΩ/1%로 GND에 연결됩니다.
- AUXDC_P/N에는 R11/R12 1 MΩ bias와 C5/C6 100 nF AC coupling이 있습니다.

### 4.8 PS171 전원 필터 tree

| Source | bead | PS171 rail | input/output capacitors |
|---|---|---|---|
| P3V3 | L1 FB300 | VDD33 | C7/C8 4.7 µF, C9/C10 100 nF |
| P1V2 | L2 FB300 | VDD12 | C11/C12 4.7 µF, C13/C14/C15 100 nF |
| P1V2 | L4 FB300 | VDDA12 | C19/C20 4.7 µF, C21 100 nF |
| P1V2 | L5 FB300 | VDDRX | C22/C23 4.7 µF, C24/C25 100 nF |

각 bead 앞뒤의 capacitor는 local return path를 짧게 구성해야 하며, 네 rail은 서로 다른 noise domain으로 유지합니다.

### 4.9 USB data/power connector J4

도면의 J4 `USB3-1` 심볼은 실제 표시상 VBUS/D-/D+/GND 네 선만 사용합니다.

- VBUS는 `PP500`에서 공급되고 C42 47 µF/10 V로 GND에 bypass됩니다.
- D-/D+는 `USB-C0_N/P` net을 통해 J2 Type-C의 USB 2.0 D-/D+에 연결됩니다.
- SuperSpeed pair가 J4까지 이어진 연결은 이 도면에 없습니다. 이름만 보고 완전한 USB 3.x port로 간주하면 안 됩니다.

### 4.10 LDR reference의 불확실성

1. 데이터시트 자체가 application section의 정확성/완전성을 보증하지 않는다고 명시합니다.
2. U6은 LDR6023SS이므로 QFN-32 LDR6023Q pin 번호와 직접 대응하지 않습니다.
3. U6 pin 4/5, RSV/PDLDO 처리와 일부 resistor RefDes는 원본 CAD/netlist 또는 최신 SS datasheet로 재확인해야 합니다.
4. power-path MOSFET의 body-diode 방향, simultaneous enable 조건, dead-time은 별도 검증이 필요합니다.

---

## 5. MPS MP2315 - EV2315-J-00A

출처: [MPS EV2315-J-00A 문서](https://www.monolithicpower.com/en/documentview/productdocument/index/version/2/document_type/Datasheet/lang/EN/sku/EV2315-J-00A/document_id/2760/). 이 보드는 4.5-24 V 입력에서 5 V를 만드는 synchronous buck입니다.

### 5.1 Power stage

```text
VIN --+--> IN pin 2 [MP2315]
      |
      +--> C1 22 µF --> GND
      +--> C1A NS
      +--> C8 0.1 µF --> GND

SW pin 3 --> L1 4.9 µH --> VOUT 5 V
                               |
                               +--> C2 22 µF --> GND
                               +--> C2A 22 µF --> GND
```

MP2315는 synchronous converter이므로 MP2482와 달리 외부 catch diode가 없습니다.

### 5.2 Bootstrap와 bias

- `BST`, pin 5에서 `SW`, pin 3 사이에 R4 20 Ω와 C4 0.1 µF bootstrap network가 연결됩니다.
- `VCC`, pin 7은 내부 regulator 출력이며 C6 0.1 µF로 GND에 decouple합니다.
- `GND`, pin 4는 input/output capacitor return과 같은 power ground로 연결합니다.

### 5.3 Feedback

```text
VOUT -- R1 40.2 kΩ --+--> FB pin 8
                      |
                    R2 7.68 kΩ
                      |
                     GND
```

기준전압 0.8 V를 적용하면 `VOUT ≈ 0.8 x (1 + 40.2/7.68) = 4.99 V`입니다. C3 15 pF는 upper feedback 경로의 feed-forward/고주파 보상용입니다. R3 0 Ω, R9 20 kΩ, R10 10 Ω는 EVB의 sense/test/보상 연결에 포함되며 PCB를 복제할 때는 원문 schematic net 위치를 그대로 대조해야 합니다.

### 5.4 AAM와 enable

- `AAM`, pin 1은 R5 100 kΩ, R8 75 kΩ, C5 1 nF network로 light-load AAM threshold를 설정합니다.
- `EN/SYNC`, pin 6은 board의 enable/synchronization test point로 연결됩니다. R6/R7은 NS option입니다.
- EN에 외부 200 kHz-2 MHz clock을 넣으면 switching synchronization 입력으로 사용됩니다.

### 5.5 원문 확인 사항

- EVB BOM의 C3 값은 15 pF인데 기재 Murata 품번은 39 pF로 해석되는 문자열입니다.
- BOM 문서 제목 일부는 5 V/3 A 사양인 반면 schematic 그림에는 5 V/2 A 표기가 보이는 판본이 있어 목표 부하를 최신 문서로 재확인해야 합니다.

---

## 6. MPS MP2482 - EV2482DN-00A

출처: [MPS EV2482DN-00A 문서](https://www.monolithicpower.com/media/document/EV2482DN-00A_r1.0.pdf). 이 보드는 4.5-30 V 입력에서 3.3 V/5 A를 만드는 asynchronous buck입니다.

### 6.1 Power stage

```text
VIN --+--> IN pin 8 [MP2482]
      +--> C1A/C1B/C1C each 4.7 µF --> GND
      +--> C6 0.1 µF ----------------> GND

SW pin 1 --+--> L1 4.7 µH --> VOUT 3.3 V
           |                    |
           +--> B540C ----------+--> C2A/C2B 47 µF --> GND
                  to GND
```

B540C는 SW가 low로 전환되는 동안 inductor current의 freewheel 경로를 제공합니다. 원문 schematic은 `D3`, BOM은 `D1`로 표기하므로 RefDes가 불일치합니다.

### 6.2 Bootstrap, VCC, ground

- `BST`, pin 2와 `SW`, pin 1 사이에 C3 100 nF와 R7 10 Ω bootstrap network가 있습니다.
- `VCC`, pin 3은 C4 1 µF로 GND에 decouple합니다.
- `GND`, pin 4와 MP2482DN exposed pad는 ground plane에 연결합니다.
- C4 정격은 schematic 16 V, BOM 10 V로 서로 다릅니다.

### 6.3 Feedback와 output voltage

```text
VOUT -- R1 40.2 kΩ --+--> FB pin 5
                      |
                    R2 13 kΩ
                      |
                     GND
```

기준전압 0.81 V를 적용하면 `VOUT ≈ 0.81 x (1 + 40.2/13) = 3.31 V`입니다. C7 82 pF와 R8 0 Ω는 feedback의 feed-forward/sense option으로 들어가며, layout에서는 FB trace를 SW node와 inductor에서 떨어뜨려야 합니다.

### 6.4 Enable와 power-good

```text
VIN -- R3 100 kΩ --+
                     +--> EN/SYNC pin 7
GND -- R5 100 kΩ ---+
                     +--> C5 0.1 µF --> GND
External EN -- R6 0 Ω
Optional R4 = NS

PG pin 6 --> PGOOD test/output; open-drain이므로 사용하는 logic rail에 pull-up 필요
```

R3/R5는 기본 enable threshold를 만들고 R6는 외부 EN 입력 link입니다. 외부 clock을 EN/SYNC에 적용하면 datasheet 범위 내에서 동기화할 수 있습니다.

---

## PCB/netlist 작성 전 최종 확인 목록

1. 각 symbol의 package variant가 회로도와 같은지 확인: 특히 LDR6023SS와 LDR6023Q, TPS2553 DBV/DRV, MP2482 DN/DS.
2. connector 관점과 IC 관점의 USB 3.x TX/RX 명칭을 뒤집어 연결하지 말 것.
3. HDMI/USB high-speed pair는 P/N, lane 번호, AC coupling 위치, ESD 방향을 netlist 단계에서 검증할 것.
4. MS2130의 여러 1.1 V/2.5 V/3.3 V rail과 PS171의 VDD12/VDDA12/VDDRX rail을 임의로 단일 bead 뒤에 합치지 말 것.
5. open-drain 신호인 TPS2553 `/FAULT`, MP2482 `PG`, USB hub over-current 신호의 pull-up rail과 polarity를 확인할 것.
6. GL3523은 별도 Genesys reference schematic 없이는 production netlist를 확정하지 말 것.
7. LDR power mux는 MOSFET body diode와 enable sequencing을 SPICE 또는 bench test로 검증할 것.
8. MPS EVB 문서의 값/품번/RefDes 불일치는 발주 전에 회로, BOM, PCB silk 세 자료를 교차 확인할 것.
