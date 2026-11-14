# Protocol modules: adding a decoder

A protocol module is one Rust file with a type that implements `protocols::Protocol`, and one line that registers it.
Everything around the decoder comes from that one type:

| What | Where it shows | Hook (default) |
|---|---|---|
| name, stable key | Decode > Protocol, event table, decoder label, saved setups, SCPI setups | `name`, `key` (required) |
| source rows | Decode menu: one row per role, named after it ("SDA", "CC", "Data" ...) | `roles` (one "Source") |
| differential input | the "B line (A-B)" row for an RS-485 style pair | `diff_ok` (off) |
| options | Decode menu rows with a list of choices, saved in setups (`dec_opt`) | `options` (none) |
| the decoder | trace band, event table, Events pane, protocol trigger, CSV export | `decode` (required) |
| field text | the labels on the trace and in the event table | `text` (the generic text of each kind) |
| protocol trigger | Trigger > Bus: what "Frame", "ID/Addr", "Data", "Error" mean | `trigger_match` (the generic rule) |
| trigger start value | what Trigger > Bus looks for when it is switched on | `trigger_default` (Frame) |
| trigger timing | sampling (a twentieth of a bit) and the search window | `bit_time` (required), `frame_span` (200 bits) |
| hardware edge | the edge the hardware trigger arms on in front of the protocol trigger | `start_edge` (falling) |
| first settings | channels and threshold when the protocol is chosen | `setup` (CH3 / CH4, 1.65 V) |
| demo traffic | the host build's simulated signals, on the channels of `setup` | `demo` (none) |
| own view | Display > Second pane > Bus view: a status line and columns of rows, with a memory across records | `view` (none) |

Only `key`, `name`, `bit_time` and `decode` are required. The defaults fit a single-line serial protocol on CH3.

## Steps

1. **The decoder** goes into `oos-dsp` when it is more than a few lines: a function from `&[f32]` samples (volts, fixed
   `dt`) to `Vec<oos_dsp::decode::Packet>`, and a generator for its tests in the same file (`decode_ext*.rs` show the
   pattern: `pub mod gen` with a function that returns an `oos_dsp::sim::Logic`). Packet times are fractional sample
   indices; use `Kind::Field("NAME")` for protocol-specific fields, `ok: false` for a failed check.
2. **The module**: copy `ui/src/protocols/example.rs` to `ui/src/protocols/<name>.rs`. The example uses every hook.
   Small modules such as `dmx512.rs` or `usbpd.rs` show how little a plain protocol needs; `dcc.rs` is a complete one
   with labels, a bipolar demo and a view that collects the state of a model railway over the records.
3. **Register it** in `ui/src/protocols/mod.rs`: `mod <name>;` and one entry in `MODULES`. The position in the list is
   the position in the Decode menu, after the built-in protocols.
4. **Test**: `(cd ui && cargo test)`. Every module is checked automatically: key and name unique, option defaults in
   range, and, when it has demo traffic, that it decodes its own demo without errors (`protocols::tests`). The Decode
   menu of every protocol goes through the layout lints. Add the module's own tests next to it, and add it to the
   list in `extended_decoders_decode_their_demo` (`ui/src/ui.rs`) with a time base that shows a whole frame and a field
   that must appear.
5. **Try it** on the host: `cargo run` in `ui/` (see the README), Decode > Protocol, decoder on. With demo traffic the
   signals come up on their channels by themselves.

## The view and its memory

`view` is called once for every new record that the Bus view looks at (a few times a second), with that record's
packets. To show more than one record holds, for example every device seen on a bus, keep the state in `mem`:
`protocols::memory::<T>(mem)` gives the module's own type, made with `T::default()` the first time. The scope empties
the memory when the decoder changes protocol or is switched off, and on Clear. Return `None` for a protocol without a
view: the pane then says so. A module whose `view` gives something for no packets gets the pane opened when it is
chosen.

## Rules

- **The key never changes** once released: saved setups refer to it (`dec_proto = <key>`). Letters and digits only.
- **Demo traffic with idle time.** Repeat the traffic with an idle gap of at least one frame: a record may start in
  the middle of a frame, and with too short a gap a decoder that started wrong stays wrong in every repeat.
- **Say what was tested.** Most decoders have only seen their own generator. Write in the module's doc comment whether
  it has decoded a real signal, and from which device.
- Text, comments and names in English; no code from other firmware (see CONTRIBUTING.md).

## The built-in protocols

UART, I2C, SPI, CAN and the other protocols that were there before this interface are still wired by hand through
`model::Proto` (`trig.rs`, `ui.rs`, `wave.rs`). DMX512, S/PDIF and USB PD already moved into modules, DCC came as one; the others
follow one at a time. Their saved setups keep working because a module's key is the old name (`Dmx512`, `Spdif`,
`UsbPd`).
