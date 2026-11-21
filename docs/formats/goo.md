# Elegoo GOO files: notes for the writer

Source: Elegoo's own published specification, "Goo Format Spec V1.2" (PDF in the
`elegooofficial/GOO` repository). The writer is written from that document only.

## Layout

1. **Header**, fields in this order (strings are fixed-length, padded with zeros):
   version (4, "V3.0"), magic (8: `07 00 00 00 44 4C 50 00`), software info (32),
   software version (24), file time (24), printer name (32), printer type (32),
   resin profile name (32), anti-aliasing level (u16), grey level (u16), blur level
   (u16), small preview (116 × 116 RGB565), `0D 0A`, big preview (290 × 290 RGB565),
   `0D 0A`, total layers (u32), X and Y resolution (u16 each), X and Y mirror (u8
   each), platform X, Y, Z (f32, mm), layer thickness (f32), common exposure (f32,
   s), exposure delay mode (u8: 1 static time, 0 turn-off time), turn-off time (f32),
   bottom before-lift, after-lift and after-retract times (f32), the same three for
   common layers, bottom exposure (f32), bottom layers (u32), then distances (mm)
   and speeds (mm/min), bottom first, for: lift, retract, second-stage lift,
   second-stage retract; bottom and common light PWM (u16, 0–255), advance mode
   (u8: 1 means per-layer values are used), print time (u32, s), volume (f32,
   mm³), weight (f32, g), price (f32), price unit (8), offset of the first layer
   (u32), grey-scale level (u8: 1 means 0x00–0xFF), transition layers (u16).
2. **Each layer**: pause flag (u16), pause Z (f32), layer Z (f32), exposure (f32),
   off time (f32), before-lift, after-lift and after-retract times (f32), lift
   distance and speed, second lift distance and speed, retract distance and speed,
   second retract distance and speed (f32), light PWM (u16), `0D 0A`; then the
   image's encoded size (u32), the image data, `0D 0A`.
3. **Ending**: `00 00 00 07 00 00 00 44 4C 50 00`.

## Image data

Starts with `0x55` and ends with an 8-bit checksum that does not cover the `0x55`.
Between them, chunks. The top two bits of a chunk's first byte give its kind:
`00` a run of 0x00, `11` a run of 0xFF, `01` a run of the grey value in the next
byte, `10` a run of a small difference from the previous pixel. For the run kinds,
the next two bits say how many more bytes extend the length: the length is those
bytes, most significant first, followed by the first byte's low four bits (so up to
28 bits). Difference chunks are optional for a writer; ours does not use them.

## Not stated in the specification; to confirm against a real file

- **Byte order** of the numbers (other readers treat it as big-endian).
- **The checksum**: assumed to be the bitwise complement of the byte sum of the
  chunks.
- Whether the **encoded size** counts the `0x55` and the checksum (assumed yes).
