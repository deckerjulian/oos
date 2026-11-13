# Hardware UART trigger

The UART protocol trigger in the TPU's receiver. Opt-in: `trig.serial_hw = on` in system.conf, read once at start,
off by default. Code: `hal/src/dho/uarttrig.rs` (words, timing), `tpu::uart` (the write sequence),
`seq::Trig::Uart`, `ui/src/board/dho/hw.rs` (`hw_uart`, `uart_debug`). Register facts: private RE notes `docs/fpga.md`
"Serial trigger" (netlist simulation).

## Hardware

- 0x205c[4:0] = 4 puts the UART receiver's event on the trigger FSM.
- The receiver's line is event R's comparator: `source(R; cmp, 0, cmp, 0)` with the source's level-A comparator,
  pattern (0, 0, 2, invert); 0x204c bit 23 inverts the line. The threshold is the source's level A, written with the
  trigger level and its hysteresis. 0x208c gets the source's DC filter word.
- 0x2100: bit time (156.25 MHz clocks) [20:0], condition [22:21] (0 start, 1 data, 2 error), relation [25:23]
  (==, !=, >, <, in range, out of range), error type [27:26] (parity, framing, either), stop bits [29:28]
  (1, 2, 1.5), parity [31:30] (none, odd, even, present unchecked).
- 0x2104: data bits - 1 [2:0], min [10:3], max [18:11]. 0x221c [10:0]: bit time bits 31:21, non-zero only below about
  75 baud. The counter reads its results at 0x221c; its writes are 0x2200..0x2218. The UART writes 0x221c only, so the
  two do not share a written register.
- Event time after the start edge: start 0.6 bit, data in the last data bit (bits + 0.7), error at the stop sample
  (bits + parity + 1.7, the parity case inferred), plus 4 clocks (25.6 ns).

## What OOS does

- Trigger > Protocol with the UART decoder on a channel uses the receiver when it can: 1..8 data bits, not the LA as
  input, not RS-485 (`dec_diff`), and a condition the receiver has:

| UI condition | receiver |
|---|---|
| Frame | start condition |
| Data = value (fits the data bits) | data, relation ==, min = max = value |
| Error | error, either type |
| ID/Addr | software (UART has no ID) |

  Baud, data bits and parity come from the Decode menu; stop bits 1 and not inverted, as the software decoder.
  Relations, polarity, stop bits and the error type are menu settings (`serial-ui.md`).
- Everything else keeps the edge trigger on the line with the software decoder picking the frame.
- Written in the trigger block of `configure` with the other trigger types (the acquisition thread disarms first).
- The software trigger stays the reference: every record still goes through the decoder, and the screen's trigger
  point is the software one (the end of the matching frame's stop bit), searched near the hardware trigger. The
  hardware fires earlier in the frame (`uarttrig::event_s`), so `settings` shifts the record by `sw_offset_s`
  (8N1 start: 9.4 bit times) to keep the window centred on the software point.
- Fork-local `:SYSTem:DEBug:UART?`: `on|off,src,baud,bits,parity,cond,invert,clocks,r2100,r2104,r221c,mux,event_s,
  shift_s` for the current settings; `off,enabled=N` where the receiver does not apply.

## Bench set-up

Needs a 3.3 V USB-UART adapter (FTDI FT232R/FT231X, CP2102 or CH340, set to 3.3 V I/O), a PC with a serial terminal
or Python + pyserial, a 10x probe.

1. Adapter in the PC, find its port (`ls /dev/tty.usbserial*` on macOS, `/dev/ttyUSB0` on Linux).
2. Probe CH1 tip on the adapter's TXD pin, probe ground on the adapter's GND. Nothing else connected to the scope.
3. Line settings on the PC: 115200 baud, 8 data bits, no parity, 1 stop bit, no flow control.
4. A repeating test pattern, e.g. in Python:
   `import serial,time; s=serial.Serial(PORT,115200); [ (s.write(b"U\x41\x00\xff"), time.sleep(0.01)) for _ in iter(int,1) ]`
   (0x55, 0x41, 0x00, 0xff every 10 ms). For parity tests reopen with `parity=serial.PARITY_EVEN`.
5. Scope: CH1 1 V/div (10x), offset so the 0..3.3 V swing fills the screen, 20 us/div. Decode: UART on CH1,
   115200, 8 bits, parity none, threshold 1.65 V. Trigger: Protocol, Normal sweep. `trig.serial_hw = on` in
   system.conf, restart oos-ui.

## On-unit procedure

Note `:SYSTem:DEBug:UART?` (expect `on,src=CH1,baud=115200,...,clocks=1356,r2100=0x0000054c`) and the frame rate
(`:SYSTem:STATe?`) at every step.

1. Condition Frame: stable picture, every frame starts a trigger; the decoded byte at the trigger point changes
   between 0x55, 0x41, 0x00, 0xff from frame to frame. The trigger marker sits at the end of a stop bit, not 0.6 bit
   after the start edge (the shift). If it sits a constant few bit times off, record the offset: it corrects
   `event_bits`.
2. Data = 0x41: only frames with 0x41 at the trigger point; the waveform is identical from trigger to trigger.
   Data = 0x42 (not sent): no triggers (WAIT). Compare the trigger rate with `trig.serial_hw = off`: the hardware
   triggers once per pattern (every 10 ms, 100/s); the software path triggers fewer times per second (it waits for
   the record to hold the frame).
3. Error: no triggers with the clean pattern. Then send with even parity while the scope decodes "none" and 8 bits:
   the receiver sees the parity bit as a stop bit and fires on framing errors when the parity bit is 0. Switch the
   scope to parity even: no triggers; scope parity odd: triggers (parity errors).
4. Baud range: 9600 and 1000000 baud (the adapter's limits permitting), Frame condition: stable triggering. The debug
   line shows `clocks=16276` and `clocks=156`.
5. Counter coexistence: Measure > Counter on CH1 while the UART trigger runs: the counter shows the bit-level edge
   rate and keeps updating; the UART trigger keeps firing. Counter off again.
6. Inverted line: not offered by the UI (the software decoder has no idle-low setting). Skip.
7. `trig.serial_hw = off`, restart: `:SYSTem:DEBug:UART?` returns `off,...`, the protocol trigger behaves as before.

Record the results in `docs/fork/unit-dho804.md`.
- SPI and I2C: `hwserial.md`.
