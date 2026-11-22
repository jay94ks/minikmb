# Sources and design decisions

확인일: 2026-10-22

## Attribution for supplier-library-derived data

MS2130과 GL3523의 공급사 land pattern 및 핀 데이터 교차 검증에는 **JLCEDA/EasyEDA Official Library** 자료를 사용했습니다. 원자료의 요구에 따라 다음 출처를 명시합니다.

- JLCEDA: https://lceda.cn/
- EasyEDA: https://easyeda.com/
- MS2130 official-library API snapshot: `easyeda-ms2130.json` (product C5905137)
- GL3523 official-library API snapshot: `easyeda-gl3523.json` (product C390630)

생성된 KiCad 파일은 사람이 읽을 수 있도록 다시 작성했으며 원본 JSON snapshot은 추적/재검증용으로 보관했습니다.

## MS2130

- Local research copy: `MS2130-datasheet.pdf`
- LCSC datasheet mirror used for the local copy: https://datasheet.lcsc.com/datasheet/pdf/5e5b81537e2ff3b4c2bdcbedb88db519.pdf?productCode=C5905137
- LCSC product C5905137: https://www.lcsc.com/product-detail/C5905137.html
- EasyEDA official-library endpoint: https://easyeda.com/api/products/C5905137/components?version=6.5.22
- Package: QFN-64-EP, 9×9 mm, 0.50 mm pitch. Datasheet nominal EP is 5.7×5.7 mm. Supplier land pattern uses 0.28×0.665 mm perimeter pads.
- Datasheet names `HDRX*` are used instead of some supplier-symbol `HDMIRX*` aliases.

## GL3523

- Manufacturer datasheet Rev.1.70: https://www.genesyslogic.com.tw/upload/prodAttach/GL3523%20Datasheet%20Rev.1.70.pdf
- Local research copy: `GL3523-datasheet-rev1.70.pdf`
- EasyEDA official-library endpoint: https://easyeda.com/api/products/C390630/components?version=6.5.22
- Selected variant: OTY10/OTY30, QFN-76-EP, 9×9 mm, 0.40 mm pitch. The datasheet also lists incompatible QFN-64/QFN-48 variants.
- Supplier land pattern uses 0.20×0.80 mm perimeter pads and a 5.6×5.6 mm land for the exposed pad. The package drawing permits a broader exposed-pad dimension range; the supplier land size is retained here.

## TPS2553

- TI product page: https://www.ti.com/product/TPS2553
- TI datasheet Rev.F: https://www.ti.com/lit/ds/symlink/tps2553.pdf
- Local research copy: `TPS2553-datasheet-revF.pdf`
- Both package pinouts are supplied because DBV SOT-23-6 and DRV WSON-6 use different numbering.
- DBV land pattern follows TI's 1.1×0.6 mm pads, 0.95 mm pitch, 2.6 mm row-center spacing.
- DRV land pattern follows TI's 0.45×0.30 mm signal pads, 0.65 mm pitch, 1.95 mm row-center spacing, and 1.0×1.6 mm PowerPAD. Paste apertures implement TI's two-window example.

## LDR6023

- Manufacturer document mirrored on TI E2E, LDR6023 Rev.2.2: https://e2e.ti.com/cfs-file/__key/communityserver-discussions-components-files/196/LDR6023S_5F00_Legendary_5F00_IC.pdf
- Local research copy: `LDR6023-datasheet-rev2.2.pdf`
- Selected variant: original LDR6023Q QFN-32, 5×5 mm, 0.50 mm pitch, nominal lead width 0.23 mm, lead length 0.4 mm, exposed pad 3.4×3.4 mm.
- Land pads are 0.25×0.70 mm to provide heel/toe allowance around the package lead.
- Datasheet discrepancy: pin 8 C01M is marked type `G`, while its description says “C0 Port CC1 Voltage Detect.” The symbol uses `input`; confirm against the exact silicon/FAE information before release.

## MP2315

- MPS product page: https://www.monolithicpower.com/en/mp2315.html
- MPS datasheet Rev.1.01: https://www.monolithicpower.com/pub/media/document/MP2315_r1.01.pdf
- Package: TSOT23-8 / JEDEC MO-193 variation BA. Pin 1 is lower-left when the top mark is read left-to-right.
- MPS marks MP2315 as NRFND and currently points new designs to MP2393.

## MP2482

- MPS product page: https://www.monolithicpower.com/en/mp2482.html
- MPS datasheet Rev.1.4: https://www.monolithicpower.com/en/documentview/productdocument/index/version/2/document_type/Datasheet/lang/EN/sku/MP2482
- DN: SOIC-8E exposed-pad package. Official recommended land pattern uses 1.60×0.61 mm leads, 1.27 mm pitch, 5.40 mm row-center spacing, and a 2.62 mm (across the lead rows) × 3.51 mm (along the pin-pitch direction) center land.
- DS: legacy SOIC-8 without exposed pad, with the same perimeter lead land pattern.
- MPS marks MP2482 as NRFND. The currently listed active ordering codes on the product page are DN exposed-pad codes.

## Scope of verification

Package drawings and relevant pinout pages were visually rendered and inspected. Generated files are additionally checked by `scripts/validate_kicad_libraries.py`. A KiCad GUI/import check remains recommended because `kicad-cli` is not installed on this host.
