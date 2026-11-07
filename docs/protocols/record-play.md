# Record -> play: why the read-out after an own arm stalls

Status: the fix of section 6.1 is verified on the unit and is what OOS runs (`scu::passthrough`, `scu::readout_play4`).
Register meanings marked "assumed" are inferred. Notation as in `acquisition-start.md` (`sN` = software copy of register N).

## 0. Result in short

1. Between "done" (0x4080 bit30) and `read()` a live frame needs only this: two stop sequences and one play sequence. No
   0x1xxx write except the SPU/TX reset pulses on 0x1000 inside the stop sequence (section 1).
2. A raw window read is a different path, the export path. It must start with the pass-through writes, which put the
   play-side processing chain into pass-through: zoom off, **compression off (0x1170 = 0x40000001)**, sinc off, linear
   interpolation off (section 2).
3. A live set-up at 200 us/div has **compression k = 4** (0x1170 = 0xc0000004, 2.5 M points -> 625,000 TX points). With
   k = 4, a window of 0x1100 = 4,584 memory points gives about 1,146 output points, but the TX length 0x11f0 asks for
   4,000. The FPGA sends exactly 0x11f0 points per frame and then EOP, so the frame never completes: the FSM stays busy
   (0x4034 = 0x1187, bits 0-2 = 7, bit13 = 0) and the xdma read times out without data (verified on the unit: the
   pass-through writes remove the stall).
4. A read-out therefore depends on the chain state left behind by whatever ran last. Interpolation makes *more* points than
   the window needs, so the TX still fills, and the data come out interpolated. Compression makes *fewer*, so it
   stalls (table 3.3). The cause is not the own record.
5. Fix (section 6): the pass-through writes, then the unchanged `readout_play4`.

## 1. Per-frame path between "done" and `read()`

### 1.1 Frame loop

- Once at start: 0x1010 bit31 = 0 (header on).
- Loop: read the next frame, then in RUN re-arm.

### 1.2 Reading one frame, analog path

| step | action |
|---|---|
| 1 | 0x4008 = s4008 \| bit23 (analog stream on) |
| 2 | 0x4008 = s4008 & ~bit24 (LA stream off) |
| 3 | request the frames (1.3) -> count; count < 1 means no frame (sleep 10 ms first if the time base is >= 1 ms/div) |
| 4 | len = display trace length; len < 8 means no frame |
| 5 | one read of `count * (Z + 16 + 2*ratio*len)` bytes, Z = 16 + 2*ratio*zoomlen with zoom on, else 0 |
| 6 | in scan mode only: 0x400c \|= bit25 |
| 7 | de-interleave: `len`, `count`, 8 header words per frame, `Z/2` words of zoom frame; no registers |

LA path: 0x4008 bit23 off, bit24 on, stop sequence, play-last pulse, FSM start (`la.md`).

RMS path (RMS measurement on, len == 1,000,000, memsel > 1 M): the pass-through writes, 0x1100 = 1000000 | 0x400,
0x1108 = 0, 0x11f0 = 1,000,000, play, read ratio x 2,000,000 bytes, stop sequence, horizontal block again, zoom enable
restored. This is the one raw window read inside a live loop. It uses the pass-through writes and restores the chain
with the horizontal block.

The read is one `read()` on `/dev/xdma0_c2h_0`. A short read or an error means no frame.

### 1.3 Requesting frames (RUN, SINGLE, record)

| # | access | register / value |
|---|---|---|
| r1 | read | 0x4080 (clear the shadow's bit30 first, so the shadow = read value); valid = bit30 |
| r2 | read | 0x4010 |
| r3 | read | 0x4014; count = rd4010 - rd4014 + 1 (bit31 set -> count = 0) |
| (r4) | read | 0x4034, in record mode and ultra acquisition only: (v & 0x2320) == 0 -> invalid |
| | | not valid -> no frame, retry after 9 ms |
| w1 | | **stop sequence** (1.4) |
| | | ring bookkeeping in software: slot s full, count stored, current slot = other slot |
| | | count = min(count, frames per batch) |
| w2 | | **stop sequence a second time** |
| | | SINGLE and raw export: count = 1 |
| w3 | | **play sequence**: mask, depth, count, first frame of the slot, last frame = first + count - 1, position 0 |

Play sequence: 0x4008 &= ~bit17; 0x4008 &= ~bit22; 0x4068 = position/6.4e6 (= 0); 0x406c = 0;
0x400c = (s & 0x89ffffff) | 0x30000000; 0x4010 = first; 0x4014 = first; 0x4018 = last; 0x400c &= ~bit28;
0x400c = (s & ~bit26) | 0x60000000; 0x4008 = (s & ~0xf) | mask; 0x4004 |= 1.

A live frame does **not** write 0x10e0. Raw export and record play first write the segment size of the slot.

### 1.4 Stop sequence, exactly

```
poll: read 0x4034, bit13; if 0: sleep 1 ms and again, at most 10 reads (~10 ms); continue anyway
stop off     0x4004 = s & 0x7ffffffe ; 0x4004 = s ; 0x4004 = s & 0x7ffffffb            sleep 1 us
stop on      0x4004 = s & 0x7ffffffe ; 0x4004 = s | 0x80000000 ; 0x4004 = s & 0x7ffffffb   sleep 1 us
info clear   0x4080 = s4080 & 0x7fffffff ; | 0x80000000 ; & 0x7fffffff   (s4080 = last READ value)
SPU reset    0x1000 = s | 1 ; s & ~1                                                   sleep 1 us
TX reset     0x1000 = s & ~0x10 ; s | 0x10 ; s & ~0x10                                 sleep 1 us
stop off     3 writes as above                                                         sleep 1 us
```

`oos-fpgad` writes everything back to back, without the sleeps, and acquisitions work on the unit.

### 1.5 Complete ordered list, done -> `read()` (RUN, 1 frame, slot 0)

`rd 0x4080` (done), `rd 0x4010`, `rd 0x4014`; stop sequence = `rd 0x4034` + 17 writes; stop sequence = `rd 0x4034` + 17
writes; play sequence = 12 writes; `read(count * (16 + 2*ratio*len))`. `readout_play4` = 7 window writes + 1 x stop
sequence + the same 12 writes. It is identical except for the window/header writes and the second stop sequence.

### 1.6 Read sizes

| path | TX register | `read()` bytes |
|---|---|---|
| live frame | 0x11f0 = roundup8(len) | count x (16 + 2 x ratio x len) [+ zoom frame] |
| raw export | 0x11f0 = w/ratio, w = roundup4(n) (w = 128 if roundup4(n)/ratio == 8) | 2 x w (header off: 0x1010 bit31 = 1) |
| RMS path | 0x11f0 = 1,000,000 | ratio x 2,000,000 |

## 2. The export path

The export path is the raw memory read in STOP: pass-through writes, export, return.

Pass-through writes:

| # | function | register | value |
|---|---|---|---|
| e1 | zoom off | 0x4008 | s4008 & ~bit25 |
| e2 | main compression off | 0x1170 | (s1170 & 0x30000000) \| 0x40000000 \| 1 |
| e3 | zoom compression off | 0x1174 | (s1174 & 0x30000000) \| 0x40000001 |
| e4 | main sinc off | 0x1130 | 0x00010101 (bit31 = enable = 0, factor 1 in bytes 0-2; no coefficient load) |
| e5 | zoom sinc off | 0x1134 | 0x00010101 |
| e6-e8 | main linear interpolation off | 0x1154, 0x1158, 0x115c | s1154 \| 1 (bit0 = 1 = main interp off), 0, 0xffffffff |
| e9-e11 | zoom linear interpolation off | 0x1154, 0x1160, 0x1164 | s1154 \| 2 (zoom interp off), 0, 0xffffffff |

`scu::passthrough` writes e2 to e11. OOS never sets the zoom bit, so e1 is not needed.

Export, analog: 0x4008 bit23 on, bit24 off; 0x1100 = n'/ratio + (0x48 | ratio << 9), 0x1108 = start/ratio;
0x11f0 = w/ratio; rewrite the delays 0x1120/0x1128 with the values of the horizontal block; 0x1010 bit31 = 1; write the
segment size of the slot to 0x10e0; one stop sequence; play sequence with count 1; `read(2*w)`. The frame comes from the
software's ring bookkeeping, not from 0x4010/0x4014.

TX info registers, index 0 to 5: 0x11f0, 0x11f4, 0x12c0, 0x12c4, 0x11fc, 0x12cc.

Return: 0x4008 bit23 on, bit24 off, 0x1010 bit31 = 0. This does **not** restore compress/sinc/interp/zoom. Only the
horizontal block of the next RUN does.

`readout_window` + `readout_play4` = this export without the delay rewrite and the 0x10e0 write, which only rewrite
unchanged values.

## 3. State the play path depends on

### 3.1 Play-side ("Tx/Proc") registers

Roles are assumed unless stated.

| register | live value (200 us/div, 1 ch) | export value | role |
|---|---|---|---|
| 0x1100 / 0x1108 | memsel + 0x248 + 4 = 0x2627ec / 0 (`horizontal-config.md` 4.1) | n + 0x248 / start | memory window of the play |
| 0x11f0 | roundup8(len) = 0x98968 (625,000) | roundup4(n)/ratio | TX points per channel per frame, then EOP |
| 0x11f4 | roundup8(zoom len) | (zoom off) | zoom TX length, only with 0x4008 bit25 |
| 0x11fc | 1000 | unchanged | unknown TX item |
| 0x1170 | 0xc0000004 (k = 4) | 0x40000001 | main compression (decimation) between memory and TX |
| 0x1174 | zoom compression | 0x40000001 | |
| 0x1130 / 0x1134 / 0x1138 | sinc with factor f; f = 1 at 200 us/div | 0x00010101 (main, zoom) | sinc interpolation; coefficient load 0x1140/0x1144 only if f > 1 |
| 0x1118 / 0x111c | sinc input length + 40 | unchanged | |
| 0x1154 (+0x1158..0x1164) | linear interp per main/zoom (bit0/bit1 = off), x2 bits 8/9 | bits 0,1 = 1, 0, 0xffffffff | linear interpolation (resampling) |
| 0x1120..0x112c, 0x131c, 0x10a0 | delays, wave-early word, filter control | delays rewritten with the same values | pipeline alignment |
| 0x4008 bit25 | zoom enable | 0 | second (zoom) frame per play |
| 0x1010 bit31 | 0, header on | 1 | 16-byte frame header |
| 0x1010 bit29 | 1 (SPU init) | | EOP/TLAST at frame end |
| 0x1010 bit28 | pulse, then rd 0x11e8 (roll only) | | |

Record-side registers that the play does not use (assumed): 0x10e4 (depth), 0x4050/0x4054 (pre/post), 0x10e0 (segment
size; the play uses it to locate frame N, unchanged), 0x404c, 0x10d0-0x10d8, 0x1148, 0x4020/0x4060/0x4064 (auto trigger),
TPU.

### 3.2 TX length model and the stall

The FPGA sends **exactly** one frame of the TX length with EOP. A longer read fails with "stopped half-way" (EIO), and
the rest of a frame comes with the next read (verified on the unit). Example: block 1 (4096 bytes) arrives and block 2
fails half-way, which fits an 8000-byte frame (4000-point TX length left in place). Frames are padded to 8 bytes in the
stream (`acquisition-start.md`, C2H read rules).

Arithmetic for a read-out at 200 us/div with the chain in live state: window 0x1100 = 4,000 + 0x248 = 4,584 points,
compression 4 -> about 1,146 points into the TX, TX length 4,000. The frame never reaches 4,000 points, so there is no EOP,
the FSM waits (bit13 = 0, bits 0-2 = 7), and the c2h read times out with nothing (xdma throws away a partial transfer on
timeout). A read with frame header has the same problem. Play-last and the play sequence differ only in the frame they
select.

### 3.3 Hand-over history against this model

All rows are verified on the unit, after a hand-over from the stock OS.

| state of the chain before the read-out | result |
|---|---|
| pass-through (the stock OS has just done a raw read) | correct raw data, word-exact |
| live, **linear interpolation 10x** (100 points, 50 kSa/s) | data come out 10x interpolated: more points than the window, the TX fills |
| live, compression 4 (2.5 M points at 200 us/div) | stall, 0x4034 = 0x1187, read timeout |
| live, compression 4, pass-through writes before the play | every own acquisition delivers its data |

## 4. 0x4034 bits

Meanings are assumed, inferred from observed values. Bits 6/7 and bit 13 are confirmed by the bisect in 6.3.

- (v & 0x2320) == 0: running.
- bit13: stopped.
- bit4 waiting, bits 6/7 triggered/auto, bit5 a further run state (`acquisition-start.md`); priority of the bits: 6, 7, 5, 4.
- Clear: write (last read & 0x3fffffff) | a<<31 | b<<30, then & 0x3fffffff. Bits 0-2 and 12 have no known use.

| value | bits | reading |
|---|---|---|
| 0x2000 | 13 | FSM idle, event flags cleared |
| 0x3180 | 13, 12, 8, 7 | idle after a record; 7/8/12 = sticky events of the record (nobody cleared them) |
| 0x31c0 | 13, 12, 8, 7, 6 | same plus bit6: stopped after a triggered record; the usual value before a play in RUN |
| 0x1187 | 12, 8, 7, 2, 1, 0 | FSM **busy** (bit13 = 0), state code 7 in bits 0-2 = play/transfer waiting (here: on the TX) |
| 0x11c7 | 12, 8, 7, 6, 2, 1, 0 | same busy state plus bit6; also seen in a normal RUN |
| 0x7 | 2, 1, 0 | busy right after a clear (seen in RUN) |

So the sticky bits do not block a play (a play works from 0x31c0). Bits 0-2 = 7 with bit13 = 0 is a normal transient
state while a frame is being sent. It only gets stuck if the frame cannot complete.

## 5. Done after 2.3 ms or 162 ms

- 2.3 ms is the **expected** time for 2.5 M points at 1.25 GSa/s: 1 ms pre-trigger fill + <= 1 ms to the next rising edge
  of the 1 kHz probe signal + 1 ms post-trigger. It is a real edge-triggered acquisition (verified on the unit, 6.3).
- 162 ms is a record without the TPU edge set-up: the auto trigger ends it (verified on the unit, 6.3). The record has no
  bit6.
- Busy values 0x11c7 / 0x1187 after a done are only the state after a failed play. The FSM stops by itself after a record
  (verified on the unit).
- No segment, continuous, roll or scan mode can come from the arm and play: both clear 0x4008 bit17/22, the arm sets
  0x400c exactly (0x22000000), and 0x4018 = 0x4014 gives one frame. Roll/scan set-up also clears 0x4008 bits 20/21/22/17
  and 0x400c bits 30/25/18/17.
- A memory burst is 10 points (`horizontal-config.md` 1.4).

## 6. Unit test of the read-out (concrete writes, in order)

Precondition: own arm done (0x4080 bit30), 0x4034 bit13 = 1. 0x1170/0x1174/0x1154 have no OOS shadow: the values below
assume bits 28-29 of 0x1170/0x1174 = 0 and 0x1154 upper bits = 0 (x2 interpolation off at 1.25 GSa/s).

### 6.0 Read-only check first

`rd 0x1170, 0x1174, 0x1130, 0x1154, 0x1100, 0x1108, 0x11f0, 0x10e0`. Some config registers do read back (0x10e0,
0x4010-0x4018, 0x4068), others do not (TPU). Whether 0x1170 reads back is not yet verified on hardware.

### 6.1 Test A: pass-through writes + `readout_play4` (own frame, raw, 8000 bytes)

Verified on the unit: every own acquisition delivers its 8000 bytes, 0x4034 stays 0x31c0.

| # | reg | value |
|---|---|---|
| 1 | 0x4008 | s4008 &= ~(1 << 25) (zoom off; no change for 0x800001; not written by OOS) |
| 2 | 0x1170 | 0x40000001 |
| 3 | 0x1174 | 0x40000001 |
| 4 | 0x1130 | 0x00010101 |
| 5 | 0x1134 | 0x00010101 |
| 6 | 0x1154 | 0x00000001 |
| 7 | 0x1158 | 0x00000000 |
| 8 | 0x115c | 0xffffffff |
| 9 | 0x1154 | 0x00000003 |
| 10 | 0x1160 | 0x00000000 |
| 11 | 0x1164 | 0xffffffff |
| 12-18 | | `readout_window`: 0x4008 \|= bit23, &= ~bit24; 0x1100 = 0x11e8; 0x1108 = 0x130b00; 0x11f0 = 0xfa0; 0x1010 = s1010 \| bit31; 0x400c = s400c & ~bit25 |
| (18a) | 0x10e0 | 0x40000 (same value, optional) |
| 19-35 | | stop sequence (17 writes) |
| 36-47 | | play sequence: 0x4008 &= ~bit17, &= ~bit22; 0x4068 = 0; 0x406c = 0; 0x400c = (s & 0x89ffffff) \| 0x30000000; 0x4010 = 0; 0x4014 = 0; 0x4018 = 0; 0x400c &= ~bit28; 0x400c = (s & ~bit26) \| 0x60000000; 0x4008 = (s & ~0xf) \| 1; 0x4004 \|= 1 |
| read | | 8000 bytes, no header; afterwards `rd 0x4034` (expect bit13 back) |

Writes 1-11 only touch the play-side chain. The next arm does not need them reset (assumed). OOS can leave the chain in
pass-through for good and always read raw windows.

### 6.2 Test B: live-mode frame

After done: 0x1170 = 0xc0000004 (only if test A changed it), 0x1100 = 0x2627ec, 0x1108 = 0, 0x11f0 = 0x98968,
0x1010 = s1010 & ~bit31, then two stop sequences and the 12 play writes, then **one read of 1,250,016 bytes** (16-byte
header + 625,000 words, each word = one of 4 compressed memory points).

This exact form is not yet verified on hardware. A compressed read of the whole record with 0x1100 = memsel + 0x248 works
on the unit (`seq::Settings::compress`, `acquisition-start.md`).

### 6.3 Configuration blocks before the arm

Bisect of the configuration blocks, each with a play after the arm and without the pass-through writes (verified on the
unit):

| blocks before the arm | done after | 0x4034 after done | after the play |
|---|---|---|---|
| none | 162 ms | 0x3180 | 0x1187, no data |
| auto trigger | 162 ms | 0x3180 | 0x1187, no data |
| TPU edge trigger | 2.8 ms | 0x31c0 | 0x11c7, no data |
| acquisition block (TPU set from the run before) | 3.4 ms | 0x31c0 | 0x11c7, no data |

So 0x31c0 means triggered and stopped. A pre-processing reset without a following horizontal block does no harm: the
full plan with the pass-through writes delivers frames (verified on the unit).

## 7. Small differences in `hal` worth knowing

1. `readout_window`: for roundup4(n)/ratio == 8 it writes 0x11f0 = 128. The exact form is 128/ratio with a read of 256
   bytes. This only matters for tiny windows.
2. `stop_sequence` writes 0x4080 from `sh.s4080`, which `plan` fixes before execution. The exact form uses the value last
   *read* from 0x4080. For done = 0x40000010 both agree, but after a play 0x4080 reads 0.
3. A live frame runs the stop sequence twice, with 1 us sleeps inside. `readout_play4` runs it once, back to back. This
   works on the unit.
