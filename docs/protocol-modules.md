# Protocol modules: adding a decoder

A protocol decoder is a module in `oos-dsp` and one in `oos-ui`. In `oos-dsp`, next to the decoder in the file of its
family, an `oos_dsp::protocols::ModuleInfo` says what it is: its stable key, its name, its family, the lines it takes,
whether the main one may be a differential pair, how the decoder is called with the options and shared rows, its bit
time, and what the protocol trigger finds in its packets and starts with; `oos_dsp::protocols::MODULES` lists them.
In `oos-ui`, a type that implements `protocols::Protocol` names that info and brings what the scope shows of it (menu
rows, labels, the first settings, demo traffic, a view), with one line that registers it:

| What | Where it shows | Hook (default) |
|---|---|---|
| name, stable key | Decode > Protocol, event table, decoder label, saved setups, SCPI setups | `info` (required): `name`, `key` of its `ModuleInfo` |
| source rows | Decode menu: one row per role, named after it ("SDA", "CC", "Data" ...) | `info`: `roles` (one "Source") |
| differential input | the "B line (A-B)" row for an RS-485 style pair | `info`: `diff_ok` (off) |
| shared rows | Decode menu rows several protocols use (Baud, Bits, Parity, SPI mode, CAN rate, USB speed ...), saved under their own setup keys; a decoder reads their values (`Shared`) | `shared` (none) |
| options | Decode menu rows with a list of choices, saved in setups (`dec_opt`) | `options` (none) |
| the decoder | trace band, event table, Events pane, protocol trigger, CSV export | `info`: `decoder` (required) |
| field text | the labels on the trace and in the event table | `text` (the generic text of each kind) |
| protocol trigger | Trigger > Bus: what "Frame", "ID/Addr", "Data", "Error" mean | `info`: `trigger`, the protocol's own rule (none: `ModuleInfo::trigger_match` uses the generic rule of `dsp/src/protocols.rs`) |
| trigger start value | what Trigger > Bus looks for when it is switched on | `info`: `trigger_default` (Frame) |
| trigger timing | sampling (a twentieth of a bit) and the search window | `info`: `bit` (required), `span` (200 bits) |
| hardware edge | the edge the hardware trigger arms on in front of the protocol trigger, and on which role's line | `start_edge` (falling), `start_role` (the main line) |
| first settings | channels, LA lines, threshold, shared-row values and the demo's trigger when the protocol is chosen | `setup` (CH3 / CH4, 1.65 V) |
| old setups | how a setup saved before the protocol was a module maps onto its options | `legacy` (none) |
| demo traffic | the host build's simulated signals, on the channels of `setup` | `demo` (none) |
| own view | Display > Second pane > Bus view: a status line and columns of rows, with a memory across records | `view` (none) |

Only `info` is required, and in it the `decoder` and the `bit` time
(`dsp/src/protocols.rs::every_module_has_its_decoder_and_bit_time`). The defaults fit a single-line serial protocol on
CH3.

## Steps

1. **The decoder** goes into `oos-dsp` when it is more than a few lines: a function from `&[f32]` samples (volts, fixed
   `dt`) to `Vec<oos_dsp::decode::Packet>`, in the file of its protocol family under `dsp/src/decode/` (a new family
   gets its own file there), and a generator for its tests in `dsp/src/decode/gen/` (`decode::gen` shows the pattern:
   a function that returns an `oos_dsp::sim::Logic`); its tests go into the family's module of `dsp/tests/decode/`
   (`dsp/tests/decode/main.rs::every_decoder_family_has_its_tests` fails for a family without one). Packet times are
   fractional sample indices; use `Kind::Field("NAME")` for protocol-specific fields, `ok: false` for a failed check.
   Next to the decoder goes its `ModuleInfo` (`pub static`, made with `ModuleInfo::new(key, name, family)`): the call
   of the decoder on its `Lines` with `.decoder(|l| ...)` (option rows as indices in `l.opt`, shared rows in `l.sh`,
   the tables an option chooses from next to the decoder), the bit time with `.bit`, a trigger rule of its own with
   `.trigger(rule)` and a test of it on decoded traffic, the start value with `.starts_on`; and at the end of `MODULES`
   in `dsp/src/protocols.rs` its line.
2. **The module**: copy `ui/src/protocols/example.rs` to `ui/src/protocols/<name>.rs`. The example uses every hook;
   it is the one module that keeps its `ModuleInfo` in its own file, since it is compiled in the tests only.
   Small modules such as `dmx512.rs` or `usbpd.rs` show how little a plain protocol needs; `dcc.rs` is a complete one
   with labels, a bipolar demo and a view that collects the state of a model railway over the records.
3. **Register it** in `ui/src/protocols/mod.rs`: `mod <name>;` and one entry in `MODULES`, both under
   `#[cfg(feature = "dec-<name>")]`, and the feature in `ui/Cargo.toml` (`dec-<name> = []`, and in the `decoders`
   list so that it is on by default). The position in the list is
   the position in the Decode menu, after the seven core protocols, which keep the first places.
4. **Test**: `(cd ui && cargo test)`. Every module is checked automatically: key and name unique, option defaults in
   range, and, when it has demo traffic, that it decodes its own demo without errors
   (`ui/src/protocols/mod.rs::modules_decode_their_demo`). The Decode menu of every protocol goes through the layout
   lints. Add the module's own tests next to it, and with demo traffic its case in
   `ui/src/ui/tests.rs::extended_decoders_decode_their_demo`, with a time base that shows a whole frame and a field
   that must appear.
5. **What is around it**: `ui/src/protocols/mod.rs::every_decoder_is_complete` names what a module still lacks. Its
   key in `ui/tests/golden/decoder-keys.txt` (`OOS_BLESS=1 cargo test --locked -q protocols::tests` in `ui/` adds the
   line), its name under Protocol decoders in `wiki/Features.md` and the number of decoders there and in `README.md`,
   demo traffic or its entry in `NO_DEMO` with the reason, and the feature of its file.
6. **Try it** on the host: `cargo run` in `ui/` ([wiki/Building.md](../wiki/Building.md#try-it-without-a-scope)),
   Decode > Protocol, decoder on. With demo traffic the signals come up on their channels by themselves.

## Leaving decoders out of a build

Every module file except `core.rs` is a Cargo feature, `dec-<file>`; all are on by default (`decoders`). A build with
fewer decoders sets one of two variables for `ui/build.sh` or `boot/build-dev-image.sh`:

```sh
OOS_DECODERS="usb dcc" ui/build.sh               # only these, besides the core ones
OOS_DECODERS="" boot/build-dev-image.sh          # the core ones only
OOS_DECODERS_OFF="avionics vehicle" ui/build.sh  # all but these
ui/tools/decoder-features.sh list                # the names
```

With cargo directly: `cargo build --no-default-features --features dec-usb,dec-dcc`. The seven core protocols are
always in: the hardware trigger types and SCPI name them. A setup that names a decoder the build does not have loads
with the decoder off and says which line it left out. With fewer decoders, the tests that need every one skip
themselves; CI builds the core-only set and every optional decoder alone.

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

## The core protocols

Every decoder is a module, the core ones too: UART, I2C, SPI, CAN, LIN, Parallel and C-PHY are in
`ui/src/protocols/core.rs` and take the first places in `MODULES` (their order is fixed, saved setups and SCPI refer
to it). Their decoders and `ModuleInfo`s are in `oos-dsp`; the values several of them share (baud rate, bits, parity
...) are shared rows, so a setup keeps one baud rate when you switch between UART and LIN. Setups saved before a protocol became a
module keep working: its key is the old name, and `legacy` maps the old setup keys onto its options.
