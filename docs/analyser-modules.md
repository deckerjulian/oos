# Analyser modules: adding an analyser page

An analyser page is a view that replaces the waveform: spectrum, eye, power, Bode, bandwidth test, magnetic, inverter,
double pulse. Each one implements `ui::analysers::Analyser` next to its own code and has one line in `ANALYSERS`. The
scope asks the registry for everything around a page:

| What | Where it shows | Hook (default) |
|---|---|---|
| the view | what the plot shows (`AView`) | `view` (required) |
| feature id and name | System > Features (`features.off` in system.conf): leaving it out removes it from the start menu and stops it | `feature` (required) |
| name | the view bar | `name` (required) |
| settings menu | the view bar's Settings, and the start menu entry | `menu` (required) |
| start menu entries | Menu > Analysers | `start` (its menu) |
| overflow menu | the ⋮ button in the plot, before Settings and Exit | `more` (nothing) |
| SCPI | `:VIEW <keyword>` | `scpi` (none) |
| setups | `view = <AView>` | `stored` (yes) |
| record | whether it computes without an acquisition record (a loaded reference, a capture of its own) | `without_record` (no) |
| computing | from each new record (every captured sample of the window); `pane` when it runs beside the waveform | `update` (nothing) |
| drawing | into the plot | `draw` (required) |

## Steps

1. **The signal processing** goes into `oos-dsp`, with its tests there.
2. **The page**: a file in `ui/src/ui/` with its `update` and `draw`, its menu rows (`Item`s of its own `MenuId`, the
   way `magnetic.rs` or `dpt.rs` do it) and its results in `Ui::azr`. Its settings live in the model, so setups keep
   them. Add a variant to `AView` (and to `AView::ALL`) and one to `MenuId`.
3. **The module**: a type that implements `Analyser` at the end of that file. `magnetic.rs` (`MagneticPage`) is a
   complete small one; `bwtest.rs` shows a page that runs a procedure (not stored, not over SCPI, a second start menu
   entry); `analysis.rs` (`SpectrumPage`) a page that also runs in the pane beside the waveform.
4. **Register it** in `ui/src/ui/analysers.rs`: one entry in `ANALYSERS`. The position is the position in the start
   menu and in System > Features. Never change a feature id once released: system.conf stores it.
5. **Test**: `(cd ui && cargo test)`. Every page is checked the same way (`analysers::tests`): views, feature ids,
   names, menus and SCPI keywords unique; it computes and draws on the demo signals without panicking; the view bar
   names it; its overflow menu leads to its settings and back to the waveform; leaving it out in System > Features
   takes it out of the start menu; `:VIEW` reaches it and setups keep it when it says so. Add the page's own tests
   next to it.
6. **What is around it**: `ui/src/ui/analysers.rs::every_page_is_complete` names what a page still lacks. Its row in
   `PAGES` of that test (feature id, the word of its view in setups, its `:VIEW` keyword: none of them changes once
   released), the keyword in the `:VIEW` rows of `docs/scpi.md` and `wiki/SCPI-and-VXI-11.md`, its name under
   Analysers in `wiki/Features.md`, and its menu in the list of `menus_fit_at` (`ui/src/ui/lint/menu_layout.rs`), so
   the layout check opens it.

## What is not a module yet

The pages keep their menu rows as `Item`s and their state in `Ui`, as every other menu of the scope does; a page is
therefore a file of the `ui` module, not a stand-alone file like a protocol or measurement module. The registry takes
the wiring around a page (start menu, Features, view bar, SCPI, setups, the record, drawing) into one place.
