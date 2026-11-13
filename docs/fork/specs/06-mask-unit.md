# Pass/fail mask unit (0x1190..0x11dc, 0x4020)

Status: specification from static analysis. Short form. Implemented opt-in (`mask.hw = on`), see `../hwmask.md`.

## 1. What the stock firmware does

### 1.1 Registers

| register | bits | meaning | stock function |
|---|---|---|---|
| 0x1190 | 23:20 | lane/channel select, one-hot (2-channel mode maps the source to lane pair 1 or 2) | `SetMaskChanEn`, `SetMaskIndexEn` |
| 0x1190 | 28 | result latch: write 0, 1, 0 before reading the counters | `queryMaskReport`, `SetResultRd` |
| 0x1190 | 30:29 | clear counters: set both, then clear | `ClearSum` |
| 0x1190 | 30, 29, 28 | also set by `SetMaskCtrl(a, b, c, sel)` | |
| 0x1194 | 7:0 | lower limit (8-bit screen code) | `SetMaskTab`, `setMaskRule` |
| 0x1194 | 15:8 | upper limit | |
| 0x1194 | 25:16 | column index 0..999 | |
| 0x1194 | 26, 27 | lower / upper limit enable | |
| 0x1194 | 31:28 | table select (all ones in `setMaskRule`) | |
| 0x119c | 23:2, 29, 30, 31 | AUX OUT pulse on fail/pass: width, select, polarity, enable | `SetMaskOutput` |
| 0x11a0 / 0x11a4 | | frames tested, 64-bit (low, high) | `GetMaskFrmCnt` |
| 0x11b0/b4, b8/bc, c0/c4, c8/cc | | fail counters 1..4, 64-bit, 48 bits used | `GetMaskCrossCnt` |
| 0x1184 | 31, 30, 27:0 | mask compression, layout like 0x1170 | `SetMaskCompress` |
| 0x11d0, 0x11d8, 0x11dc | | zone trigger control and tables | `SetZoneTrigCtrl`, `SetZoneTab1/2` |
| 0x4020 | 15 | mask enable | `SetMaskEn` |
| 0x4020 | 14, 13 | save on pass / save on fail | |
| 0x4020 | 12 | stop on fail | `SetMaskMaskPfStop` |
| 0x4020 | 1 | zone trigger | `SetMaskZoneTrig` |

### 1.2 Sequences

- Rule load (`DrvMask_setMaskRule`): the API passes upper and lower limit arrays of 1,000 columns in 1/256 screen
  units. The driver shifts them right by 8 (8-bit screen code) and writes 1,000 words to 0x1194: lower, upper, column,
  both enables, table select.
- Enable (`DrvMask_Run`): store, reconfigure (`RunConfig`, includes stop and average clear), then
  `DrvMask_applyCfg` selects the source lane in 0x1190 [23:20].
- Per batch (`CDrvScope::run`): if the mask is on, `DrvMask_CheckErr`: latch (0x1190 bit 28 pulse), read fail counter 1
  and frames tested. If the fail count rose and "stop on fail" is set: 0x4020 bit 15 = 0 and STOP.
- Reset (`DrvMask_setMaskRst`): 0x1190 bits 30:29 pulse.
- The driver always reads counter 1 and relies on the lane select.

## 2. What OOS does today

- Software mask test on every record that reaches the UI: `ui/src/ui.rs:2048` `mask_test`, rules from
  `dsp/src/mask.rs:68`. Records that are not transferred are not tested.
- Handover value 0x4020 = 0x2001 has bit 13 set (save on fail) from the stock mask functions; OOS writes bit 0 only.

## 3. Proposed change

- `hal/src/dho/mask.rs`: `load(rule: &[(u8, u8); 1000])`, `enable(sh, on, stop_on_fail)`, `select(mask, src)`,
  `clear()`, `latch()`, and a reader returning (tested, failed).
- Map the OOS mask (`dsp/src/mask.rs`) to 1,000 screen columns and 8-bit codes from the vertical scale.
- Compression 0x1184 = the same k as 0x1170 so 1,000 columns cover the screen.
- UI: hardware counts when the source is a hardware channel; software test stays for math/ref sources.
- Gain: no per-record compare on the CPU; test coverage of every played frame, possibly every acquired frame.

## 4. Host test plan

- Write-sequence test: rule load is 1,000 writes with the field layout above, column order 0..999.
- Mapping test: a software mask and its 8-bit hardware table agree on pass/fail for synthetic traces (model of the
  comparison in `tools/maskmodel.py`).
- Counter assembly: 64-bit low/high, 48-bit mask.

## 5. Hardware test checklist

1. Mask around a 1 kHz square, then add a glitch: fail counter rises, tested counter rises per frame.
2. Does the tested count follow triggers (acquisition side) or plays (transfer side)?
3. Stop on fail via 0x4020 bit 12 without software involvement.
4. Lane select in 1, 2 and 4 channel modes.
5. AUX OUT pulse on fail (0x119c) with a second scope (`docs/fork/auxout.md`).

## 6. Confidence and open questions

Register fields high (direct bit packing). Semantics of 0x1190 bits 30..28 in `SetMaskCtrl`, the vertical mapping of
the 8-bit code, and where the unit sits (acquisition or play chain) are unknown. Counters 2..4 are never read by the
stock driver.
