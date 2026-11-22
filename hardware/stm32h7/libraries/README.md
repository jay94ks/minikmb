# CAD library sources

CH482X and GL3510-OSY52 custom symbols/footprints were authored from the manufacturer package/pin drawings for this project. They are included as project-relative libraries. Schematic symbols are also embedded in the schematic and footprints in the board.

- CH482X: WCH, [manufacturer document mirrored by supplier](https://cdn.icstop.com/upload/pdfs/38/41/384110180368ca2f616c00f531f5e09c.pdf), JLC/LCSC C2913011.
- GL3510-OSY52: Genesys Logic, [datasheet source](https://datasheet.lcsc.com/datasheet/pdf/4321c9a7aefd52779b485a0872567b52.pdf?productCode=C7501408), JLC/LCSC C7501408.
- Standard symbols and footprint geometry used in the project come from KiCad libraries. Their [CC-BY-SA-4.0 license and exception](https://gitlab.com/kicad/libraries/kicad-footprints/-/blob/master/LICENSE.md) are distinct from MiniKMB's MIT license.
- Earlier supplier-library-derived MS2130/GL3523 data are attributed to JLCEDA/EasyEDA in `../../prototypes/references/SOURCES.md`. That attribution does not turn the actual current hub into a GL3523.

Pin names, manufacturer drawings and supplier reference data retain their original rights. Embedded/derived third-party geometry is not exclusively claimed as MiniKMB-authored material.

The exact used KiCad 10 standard footprint subset is included locally with hashes in standard-footprints.json. Optional standard 3D models still use the KiCad installation model paths; they are not needed to open the schematic or manufacture the PCB.
