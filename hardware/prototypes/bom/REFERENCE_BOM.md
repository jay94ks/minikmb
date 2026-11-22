# 레퍼런스 회로 BOM 추출

이 문서는 `future` 라이브러리 대상 6종 IC의 데이터시트/평가보드 문서에 실린 레퍼런스 회로에서 부품을 추출한 결과입니다. 제조사가 정식 BOM 표를 제공한 MP2315와 MP2482를 제외하면, 아래 목록은 **회로도 기반 역산 BOM**입니다. 패키지·정격·제조사 품번이 회로도에 없는 부품은 발주 전에 반드시 별도로 선정해야 합니다.

각 부품이 어느 net과 기능 블록에 연결되는지는 [`docs/REFERENCE_SCHEMATIC_CONNECTIVITY.md`](../docs/REFERENCE_SCHEMATIC_CONNECTIVITY.md)를 참조하십시오.

표기 규칙:

- `NS`, `NC`, `DNP`: 미실장
- `104` = 100 nF, `105` = 1 µF, `106` = 10 µF, `102` = 1 nF로 해석
- `TBD`: 공개 문서에서 값 또는 품번을 확정할 수 없음
- 수량은 한 회로/한 보드 기준이며, 대안 회로는 동시에 합산하지 않음

## 추출 범위와 발주 가능성

| 대상 | 사용한 레퍼런스 | 결과 수준 | 바로 발주 가능한가? |
|---|---|---|---|
| MS2130 | 데이터시트 14쪽, 전형 응용 회로 | RefDes/값 추출 | 아니오 — 패키지와 ESD 품번 미제공 |
| GL3523 | 데이터시트 31~34쪽, 설정/연결 예 | 부분 목록 | 아니오 — 별도 reference schematic을 참조하라고만 기재 |
| TPS2553 | 데이터시트 Fig. 24, 27/28, 29 | 회로 변형별 목록 | 부분 가능 — 수동소자 패키지 미제공 |
| LDR6023 | 데이터시트 9~10쪽, Type-C Docking | RefDes/값 추출 | 아니오 — 다수 패키지/정격/품번 미제공 |
| MP2315 | EV2315-J-00A 정식 BOM | 제조사/품번 포함 | 대체로 가능 — 원문 불일치 1건 확인 필요 |
| MP2482 | EV2482DN-00A 정식 BOM | 제조사/품번 포함 | 대체로 가능 — 원문 RefDes/정격 불일치 확인 필요 |

## 1. MacroSilicon MS2130

출처: [MS2130 데이터시트](../references/MS2130-datasheet.pdf), 14쪽 전형 응용 회로.

| 수량 | RefDes | 값/부품 | 역할·비고 |
|---:|---|---|---|
| 1 | U7 | MS2130 | QFN-64, 9 × 9 mm |
| 1 | U4 | SPI Flash, 64 KByte | 정확한 주문 품번/패키지 미기재 |
| 1 | CN1 | USB 3.0 Male | 커넥터 품번 미기재 |
| 1 | J1 | HDMI-A Female | 커넥터 품번 미기재 |
| 1 | X1 | 24 MHz crystal | 부하 용량/ESR/패키지 미기재 |
| 2 | C1, C2 | 100 nF, 50 V | USB 3.x TX AC coupling |
| 2 | C4, C6 | 22 pF, 50 V | 24 MHz crystal load |
| 1 | C5 | 100 nF, 50 V | HDMI 측 필터/디커플링 |
| 1 | R1 | 1.6 kΩ, 1% | `HDRX_REXT`, pin 45 가까이 배치 |
| 1 | R2 | 200 Ω, 1% | `USB_REXT`, pin 7 가까이 배치 |
| 1 | R3 | 1 MΩ, 1% | crystal 병렬 바이어스 |
| 2 | R4, R5 | 47 kΩ, 5% | HDMI DDC/5 V 관련 pull-up |
| 2 | R6, R7 | 1 kΩ, 1% | HDMI HPD/5 V detect 경로 |
| 1 | R8 | 24 kΩ, 5% | USB plug detect 분압, 조건부 사용 |
| 1 | R9 | 47 kΩ, 5% | USB plug detect 분압, 조건부 사용 |
| 1 | U1 | USB 2.0 ESD array | 정확한 품번/패키지 미기재 |
| 1 | U2 | USB 3.x SuperSpeed 4-line ESD array | 정확한 품번/패키지 미기재 |
| 2 | U5, U6 | HDMI 4-line ESD array | 정확한 품번/패키지 미기재 |
| 설계별 | — | 100 nF 전원 디커플링 | 데이터시트는 각 전원 핀 가까이 배치하도록 지시하지만 RefDes/수량은 제시하지 않음 |

`R8/R9`은 MS2130의 전원 rail은 살아 있지만 USB 신호 케이블이 호스트에 연결되지 않을 수 있는 구성에서만 필요하다는 주석이 있습니다.

## 2. Genesys Logic GL3523

출처: [GL3523 데이터시트 Rev. 1.70](../references/GL3523-datasheet-rev1.70.pdf), 31~34쪽.

공개 데이터시트에는 전체 reference schematic이나 BOM이 없습니다. 아래는 핀 설명과 설정 예에서 확정되는 최소 외장 부품입니다.

| 수량 | 기능 RefDes | 값/부품 | 상태·비고 |
|---:|---|---|---|
| 1 | U1 | GL3523 OTY10/OTY30 | 이 프로젝트가 대상으로 한 QFN-76 변형 |
| 1 | X1 | 25 MHz crystal/oscillator | 주파수 허용오차 ±0.03%; 나머지 사양은 별도 선정 |
| 1 | RTERM | 20 kΩ | `RTERM`-GND, 필수 |
| 1 | LSW | TBD | 내부 switching regulator 출력 인덕터; 값 미제공 |
| 2 | RFB_TOP, RFB_BOT | TBD | 가변 출력 버전의 FB 분압; fixed 버전은 FB를 출력에 연결 |
| 2 | RRESET1, RRESET2 | TBD | RESETJ 외부 분압 예; 값은 schematic 참조라고만 기재 |
| 1 | CRESET | TBD | reset delay/filter 예; 값 미제공 |
| 1 | RPGANG | 100 kΩ | gang mode면 3.3 V pull-up, individual mode면 GND pull-down 중 하나만 실장 |
| 선택 | LED | 사용자 선정 | suspend/status LED 예; LED 전류 ≤ 2.5 mA |
| 선택 | RLED | TBD | LED 직렬 저항; 값 미제공 |

따라서 GL3523은 이 문서만으로 구매 BOM을 확정할 수 없습니다. Genesys의 별도 reference schematic/EVB BOM을 확보하면 전원 인덕터, FB 분압, reset RC, 각 포트 필터/ESD/커넥터를 보완해야 합니다.

## 3. Texas Instruments TPS2553

출처: [TPS2553 데이터시트 Rev. F](../references/TPS2553-datasheet-revF.pdf), Fig. 24, 27/28, 29.

세 회로는 서로 다른 적용 예입니다. 필요한 회로 하나를 선택해 사용합니다.

### 3.1 Two-level current limit — Fig. 24

| 수량 | RefDes/기능 | 값/부품 | 비고 |
|---:|---|---|---|
| 1 | U1 | TPS2553 | DBV 또는 DRV 패키지 선택 |
| 1 | CIN | 0.1 µF | 입력 bypass |
| 1 | RFAULT | 100 kΩ | `/FAULT` pull-up |
| 1 | R1 | 210 kΩ | 두 단계 ILIM 네트워크 |
| 1 | R2 | 22.1 kΩ | 두 단계 ILIM 네트워크 |
| 1 | Q1 | 2N7002 | 단계 전환용 NMOS |
| 설계별 | CLOAD, RLOAD | 부하 의존 | BOM 고정값 아님 |

### 3.2 Auto-retry — Fig. 27/28

| 수량 | RefDes/기능 | 값/부품 | 비고 |
|---:|---|---|---|
| 1 | U1 | TPS2553 | Fig. 28은 외부 logic driver도 추가 사용 |
| 1 | CIN | 0.1 µF | 입력 bypass |
| 1 | RFAULT | 100 kΩ | `/FAULT` pull-up |
| 1 | CRETRY | 0.1 µF | retry timing |
| 1 | RILIM | 20 kΩ | 예제값 |
| 설계별 | CLOAD, RLOAD | 부하 의존 | BOM 고정값 아님 |

### 3.3 USB power switch — Fig. 29

| 수량 | RefDes/기능 | 값/부품 | 비고 |
|---:|---|---|---|
| 1 | U1 | TPS2553 | 그림은 TPS2552/53 공용 표기 |
| 1 | CIN | 0.1 µF | 입력 bypass |
| 1 | RFAULT | 100 kΩ | `/FAULT` pull-up |
| 1 | RILIM | 20 kΩ | 회로도 예제값; 목표 전류에 맞춰 재계산 필요 |
| 1 | COUT | 120 µF | downstream-facing USB port 예 |
| 1 | JUSB | USB port connector | 정확한 품번 미기재 |

DRV(WSON) 사용 시 exposed PowerPAD를 GND에 연결해야 합니다.

## 4. Legendary LDR6023 Type-C Docking

출처: [LDR6023 데이터시트 Rev. 2.2](../references/LDR6023-datasheet-rev2.2.pdf), 9~10쪽.

중요: 이 레퍼런스 회로의 U6은 **LDR6023SS(SSOP-16)**입니다. 현재 `future`에 만든 `LDR6023Q` QFN-32 심볼/풋프린트를 그대로 대입하면 안 됩니다.

### 4.1 Power supply / USB PD module — 9쪽

| 수량 | RefDes | 값/부품 | 비고 |
|---:|---|---|---|
| 1 | U6 | LDR6023SS | SSOP-16 |
| 1 | J3 | USB-C receptacle | 정확한 품번 미기재 |
| 1 | FB1 | 30 Ω / 4 A, `GPDZ2012M300-4A0TF`로 판독 | 원문 해상도가 낮아 제조사 문자열 재확인 필요 |
| 2 | TVS1, TVS2 | 3.3 V TVS | CC/SBU 보호, 품번 미기재 |
| 1 | C40 | 10 µF (`106`) | LDR6023 주변 |
| 1 | C41 | 22 µF, 10 V | VBUS |
| 1 | C43 | 1 nF (`102`) | VBUS detect filter |
| 3 | C44, C45, C46 | 220 pF | CC/SBU filter |
| 1 | R29 | 10 Ω | 전원/신호 직렬 |
| 3 | R30, R32, R34 | 100 Ω | CC/SBU 직렬 |
| 1 | R31 | 10 kΩ | control pull resistor |
| 1 | R33 | NC | 미실장 |
| 1 | R35 | 36 kΩ | VBUS detect 분압 |
| 1 | R36 | 12 kΩ | VBUS detect 분압 |
| 1 | U5 | TD1583 | 5 V buck |
| 2 | C38, C39 | 47 µF, 25 V | TD1583 input |
| 2 | C32, C33 | 47 µF, 10 V | TD1583 output |
| 1 | C31 | 330 pF | TD1583 feedback compensation |
| 1 | L6 | 10 µH, 3 A | TD1583 power inductor |
| 1 | D3 | SS34 | TD1583 catch diode |
| 1 | R23 | 10 Ω | TD1583 |
| 1 | R25 | 11 kΩ, 1% | feedback |
| 1 | R28 | 3.4 kΩ, 1% | feedback |
| 2 | R26, R27 | 100 kΩ | enable/bias |
| 1 | U4 | XC6206 | LDO |
| 1 | C34 | 22 µF, 10 V | LDO input |
| 2 | C35, C36 | 10 µF (`106`) | LDO input/output 주변 |
| 1 | C37 | 1 µF (`105`) | LDO 주변 |
| 1 | R24 | 10 Ω | LDO input filter |
| 1 | U3 | FP6161 | 1.2 V buck |
| 2 | C17, C18 | 10 µF | FP6161 input/output |
| 1 | C16 | 100 pF | feedback compensation |
| 1 | L3 | 2.2 µH | power inductor |
| 2 | R15, R16 | 51 kΩ | feedback network |
| 1 | Q1 | AO4805 | dual MOSFET; Q1A/Q1B는 한 패키지 |
| 1 | Q3 | AO4407 | MOSFET |
| 1 | Q4 | AO6800 | dual MOSFET |
| 1 | D2 | SS34 | Schottky diode |
| 1 | C26 | 2.2 µF, 10 V | adapter path |
| 1 | R17 | 100 kΩ | adapter path |
| 4 | R18, R19, R20, R21 | 10 kΩ | gate/control bias |
| 1 | R22 | 0 Ω | link |

### 4.2 HDMI module — 10쪽

| 수량 | RefDes | 값/부품 | 비고 |
|---:|---|---|---|
| 1 | U1 | W25X40 | SPI flash |
| 1 | U2 | PS171 / PS171A | 도면에 두 표기가 함께 있어 주문 코드 확인 필요 |
| 1 | X1 | 27 MHz crystal | 부하/ESR/패키지 미기재 |
| 1 | J1 | HDMI_DB19 | HDMI receptacle |
| 1 | J2 | TYPE_C | USB-C connector |
| 1 | J4 | USB3-1 | USB 3.x connector |
| 1 | D1 | 1N5818W | Schottky diode |
| 2 | C1, C2 | 20 pF | crystal load |
| 15 | C3, C5, C6, C9, C10, C13, C14, C15, C21, C24, C25, C27, C28, C29, C30 | 100 nF (`104`) | digital/rail decoupling 및 lane coupling/filter |
| 1 | C4 | 1 µF | reset rail |
| 8 | C7, C8, C11, C12, C19, C20, C22, C23 | 4.7 µF | rail bulk decoupling |
| 1 | C42 | 47 µF, 10 V | USB VBUS bulk |
| 3 | R1, R2, R4 | 4.7 kΩ | flash/I²C pull-up |
| 3 | R3, R6, R9 | 10 kΩ | strap/reset/bias |
| 1 | R5 | 4.99 kΩ, 1% | PS171 REXT |
| 2 | R7, R8 | 2 kΩ | HDMI DDC |
| 1 | R10 | 100 kΩ | HPD/bias |
| 2 | R11, R12 | 1 MΩ | AUX bias |
| 1 | R13 | 0 Ω | link |
| 1 | R14 | NC | 미실장 |
| 4 | L1, L2, L4, L5 | FB300 | ferrite bead; DC 전류/패키지 미기재 |

데이터시트 자체가 application section의 정확성/완전성을 보증하지 않는다고 명시합니다. 특히 MOSFET, 인덕터, ferrite bead, TVS, 커넥터는 실제 전압·전류·ESD 요건으로 다시 검증해야 합니다.

## 5. MPS MP2315 — EV2315-J-00A 정식 BOM

출처: [MPS EV2315-J-00A 평가보드 문서](https://www.monolithicpower.com/en/documentview/productdocument/index/version/2/document_type/Datasheet/lang/EN/sku/EV2315-J-00A/document_id/2760/). 보드 사양은 4.5~24 V 입력, 5 V/3 A 출력입니다.

| 수량 | RefDes | 값 | 설명 | 패키지 | 제조사 | 제조사 품번 |
|---:|---|---|---|---|---|---|
| 1 | C1 | 22 µF | 25 V, 10%, X5R | 1206 | Murata | GRM31CR61E226KE15L |
| 0 | C1A, C7 | NS | 미실장 | 1206 | — | — |
| 2 | C2, C2A | 22 µF | 10 V, X7R | 1206 | Murata | GRM21BR60J226ME39L |
| 1 | C3 | 15 pF | 50 V, C0G | 0603 | Murata | GRM1885C1H390JA01D* |
| 3 | C4, C6, C8 | 0.1 µF | 16 V, X7R | 0603 | Murata | GRM188R71C104KA01D |
| 1 | C5 | 1 nF | 50 V, X7R | 0603 | Murata | GRM188R71H102KA01D |
| 1 | R1 | 40.2 kΩ | 1% thick film | 0603 | Yageo | 9C06031A4022FKHFT |
| 1 | R2 | 7.68 kΩ | 1% thick film | 0603 | Yageo | 미기재 |
| 1 | R3 | 0 Ω | 1% thick film | 0603 | Yageo | 9C06031A0R00JLHFT |
| 1 | R4 | 20 Ω | 5% thick film | 0603 | Yageo | 9C06031A20R0JLHFT |
| 1 | R5 | 100 kΩ | 1% thick film | 0603 | Any | — |
| 0 | R6, R7 | NS | 미실장 | — | — | — |
| 1 | R8 | 75 kΩ | 1% thick film | 0603 | Any | — |
| 1 | R9 | 20 kΩ | 1% thick film | 0603 | Any | — |
| 1 | R10 | 10 Ω | 1% thick film | 0603 | Yageo | 9C06031A10R0FKHFT |
| 1 | L1 | 4.9 µH | DCR 22 mΩ, Is 6 A | SMD | Würth | 744314490 |
| 1 | U1 | MP2315GJ | synchronous buck | TSOT23-8 | MPS | MP2315GJ |

\* 원문 BOM은 C3 값을 `15 pF`로 쓰지만 품번 `GRM1885C1H390JA01D`의 `390` 표기는 통상 39 pF에 해당합니다. 그대로 발주하지 말고 회로 동작점과 원문 schematic을 기준으로 MPS에 확인해야 합니다.

## 6. MPS MP2482 — EV2482DN-00A 정식 BOM

출처: [MPS EV2482DN-00A 평가보드 문서](https://www.monolithicpower.com/media/document/EV2482DN-00A_r1.0.pdf). 보드 사양은 4.5~30 V 입력, 3.3 V/5 A 출력입니다.

| 수량 | RefDes | 값 | 설명 | 패키지 | 제조사 | 제조사 품번 |
|---:|---|---|---|---|---|---|
| 3 | C1A, C1B, C1C | 4.7 µF | 50 V, X7R | 1210 | Murata | GRM32ER71H475KA88L |
| 2 | C2A, C2B | 47 µF | 10 V, X5R | 1210 | Murata | GRM32ER61A476KE20L |
| 3 | C3, C5, C6 | 0.1 µF | 50 V, X7R | 0603 | Murata | GRM188R71H104KA93D |
| 1 | C4 | 1 µF | 10 V, X5R* | 0603 | Murata | GRM185R61A105KE36 |
| 1 | C7 | 82 pF | 50 V, X7R | 0603 | Any | — |
| 1 | C8 | 22 µF | 6.3 V, X5R | 1210 | TDK | C3225X5R0J226M |
| 1 | R1 | 40.2 kΩ | 1% film | 0603 | Yageo | RC0603FR-0740k2L |
| 1 | R2 | 13 kΩ | 1% film | 0603 | Yageo | RC0603FR-0713kL |
| 1 | R3 | 100 kΩ | 5% film | 0603 | Any | — |
| 1 | R4 | NS | 미실장 | — | — | — |
| 1 | R5 | 100 kΩ | 5% film | 0603 | Any | — |
| 2 | R6, R8 | 0 Ω | 5% film | 0603 | Any | — |
| 1 | R7 | 10 Ω | 5% film | 0603 | Any | — |
| 1 | D1** | B540C | 40 V, 5 A diode | SMC | Diodes Inc.로 표기 | B540C |
| 0 | D2 | NS | 미실장 | — | — | — |
| 1 | L1 | 4.7 µH | 8.7 A inductor | SMD | Toko | D128C931AS-4R7M |
| 1 | U1 | MP2482DN | step-down regulator | SOIC-8E | MPS | MP2482DN |

\* 회로도는 C4를 `1 µF / 16 V`로 표시하지만 BOM 표는 `10 V`로 기재합니다. 실제 부품 `GRM185R61A105KE36`도 정격을 재확인해야 합니다.

\** 평가보드 schematic에서는 B540C가 `D3`로 보이지만 BOM 표에서는 `D1`로 기재됩니다. PCB RefDes를 기준으로 발주/실장하기 전에 원본 Gerber 또는 보드 실크와 대조해야 합니다.

## 발주 전 체크리스트

1. LDR6023은 `Q`와 `SS` 패키지를 혼용하지 말 것.
2. GL3523은 공개 데이터시트만으로 완전 BOM을 만들 수 없으므로 별도 reference schematic 확보 후 확정할 것.
3. MP2315 C3 값/품번, MP2482 C4 정격과 diode RefDes 불일치를 해결할 것.
4. MS2130/LDR6023의 ESD, ferrite bead, 인덕터, 커넥터는 실제 전압·전류·대역폭·삽입손실 기준으로 품번을 선정할 것.
5. 모든 스위칭 전원 인덕터의 포화전류, RMS 전류, DCR과 출력 커패시터의 DC-bias derating을 확인할 것.
