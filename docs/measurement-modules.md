# Measurement modules: adding measurements

A measurement module is a Rust type that implements `measures::MeasModule`, and one line that registers it. Everything
around its measurements comes from that one type:

| What | Where it shows | Hook (default) |
|---|---|---|
| the measurements | tiles on the measurement page, Selected list, strip, statistics, logger columns, setups (`cat` rows), SCPI (`CAT<id>`) | `defs` (required) |
| stable module key | tests and docs | `key` (required) |
| the values | computed on the catalog worker, never on the render thread; cached per source, gate and settings | `compute` (no values) |
| settings the values depend on | a change computes them again (BER, line frequency, core geometry ...) | `settings` (none) |
| extra sources, checks | sources outside the row (a fourth probe), or why a row cannot be measured ("three distinct phases") | `sources` (none) |
| quality line | under the description while a value computes | `pending_quality` ("Derived from scaled source roles") |
| missing value | the reason shown when a value comes out empty | `missing_reason` (none) |

Each measurement is a `Def`:

| Field | Meaning |
|---|---|
| `id` | stable number, unique over all modules, **never reused**: setups and SCPI store it |
| `name`, `blurb` | tile name and one sentence on what it measures (ends with a full stop) |
| `mark`, `unit` | drawn sign of the tile; SI unit of the value ("" for a count or a ratio) |
| `roles` | source slots it reads: 0 = A, 1 = B, 2 = C, 3 = D |
| `places` | category and group of its tile(s); one measurement may sit in several categories |

`measures::module(...)` builds one in a line. The categories (Basic, Two sources, Serial & PAM, Jitter & eye, Power,
Motor drive, Power rails, Double pulse) are in `measures::CATEGORIES`, each with the labels of its source rows and the
order of its groups. A group a module names that is not listed there follows the listed ones. Within a group, tiles
come in the order of `MODULES` and then of each module's `defs`.

## Steps

1. **The signal processing** goes into `oos-dsp` when it is more than a few lines: a function from `&[f32]` samples
   (fixed `dt`) to the numbers, with its tests there.
2. **The module**: copy `ui/src/measures/example.rs` to `ui/src/measures/<name>.rs`. The example uses every hook. Small
   modules such as `serial.rs` or `rails.rs` show a plain one; `jitter.rs` computes one eye diagram for 26 measurements
   and returns a quality line from its fit; `motor.rs` checks its sources and reads two more from its page.
3. **Register it** in `ui/src/measures/mod.rs`: `pub mod <name>;` and one entry in `MODULES`.
4. **Test**: `(cd ui && cargo test)`. Every module is checked automatically: numbers and names unique, places and roles
   valid, descriptions present, the stored numbers exactly the ones the setup reader takes, and every module computes
   a value (or NaN) for each of its measurements on a plain square wave without panicking (`measures::tests`). Add the
   module's own tests next to it: known inputs, known numbers.
5. **What is around it**: `ui/src/measures/mod.rs::every_measurement_is_complete` names what a measurement still
   lacks. Its line in `ui/tests/golden/measurement-ids.txt` (`OOS_BLESS=1 cargo test --locked -q measures::tests` in
   `ui/` adds it; the list only gains lines), and its group in the row of its category in the table of
   [reference-analysis.md](reference-analysis.md).
6. **Try it** on the host: `cargo run` in `ui/` (or `--serve` and the web remote), Measure, the category, tap the tile.

## How a value is computed

When a tile is on, the scope takes an immutable snapshot of the sources the measurement reads (its roles plus what
`sources` adds), within the measurement gate (Screen or Full record), and hands it to the worker thread. There the
sources are put on one time grid: the slowest source's sample spacing over the time all of them cover
(`Ctx::t0`, `Ctx::dt`); `Ctx::wave` and `Ctx::align` give the samples. `compute` gets every measurement of the module
that reads the same roles as the one asked for, so measurements that share an expensive intermediate are computed
together: put them in one module. The answer is cached until a source, the gate, the module's `settings` or the
display epoch changes; statistics count each accepted capture once.

`compute` runs in the background and a panic in it is caught (the value reads "Catalog analysis failed"), but it must
still return in reasonable time: the scope bounds the aligned data to 32 M points and 256 MiB.

## The standard measurements

The 60 standard measurements (`oos_dsp::measure::Meas`) and the 17 two-source variants (delay, phase, skew, setup and
hold) are computed by the scope itself, with their own statistics, per-measurement settings and SCPI names.
`measures/standard.rs` and `measures/pairs.rs` only place them on the page (`Kind::Standard`, `Kind::Pair`); their
numbers are not stored. A new measurement should come as a module.

## Rules

- **The id never changes** once released, and is never given to another measurement.
- **NaN, not zero**, when there is no value: the page shows "----" and `missing_reason`.
- **Say what was tested.** Write in the module's doc comment against what the numbers were checked (a reference
  instrument, a datasheet, a closed-form signal).
- Text, comments and names in English; no code from other firmware (see CONTRIBUTING.md).
