# Loaded waveforms, zoom and automatic analysis

Load file adds a reference alongside live channels without replacing measurements,
run state or the timebase. Take over and Offline waveforms enter explicit stored-
record sessions; Live restores acquisition.
Select the reference pill to focus the vertical controls, cursors and automatic
Eye action. Run, Stop and Single remain the acquisition/replay controls.

## Math and decoding

Open Menu > Math, choose an operation, then choose loaded R sources for A/B.
Earlier Math channels are also available without allowing circular dependencies.
Formulas accept named references and earlier Math channels, for example
`abs(R1-R2)` or `M1+R3`. Setup files retain those selections. FFT uses the source's
native sample interval. Large computations run in one bounded background worker;
the display reports Computing while the current result is pending.

Arithmetic aligns sources by physical time over their common overlap. Its output
uses the slowest participating interval, so a slower source does not gain
resolution through interpolation. Empty/non-overlapping sources and forward Math
dependencies produce an explicit status. Convolution uses a bounded direct kernel;
very large kernels are rejected explicitly rather than freezing the sole worker.

Open Menu > Decode, choose a protocol and assign loaded references to its named
roles. Roles may mix acquired and reference data. Different intervals/origins are
aligned before decoding; packet labels, table/export, SCPI and Search share the
same physical times. Long records use a bounded background worker, with stale
results rejected after sources/settings change.

C-PHY adds **single-trio, six-wirestate** A/B/C electrical decoding. It exposes
symbols and, after a qualified preamble/SYNC, mapped 16-bit words and POST. A
capture starting mid-burst reports symbols rather than guessing word alignment.
It does not interpret CSI-2/DSI packets, merge multiple trios or implement the newer
18-wirestate coding mode. Min gap is the minimum separation of adjacent wire
levels; the initial setting is 20 mV. Symbol rate defaults to Auto; a known rate
such as 2.25 Gsym/s can be set in the decoder menu or through `:DEC:CPHY:RATE`.
The implementation follows the published
[MIPI mapper](https://www.mipi.org/sites/default/files/MIPI-DevCon-2016-Implementing-MIPI-C-PHY.pdf)
and [state/symbol explanation](https://www.mipi.org/sites/default/files/MIPI-C-PHY-Introduction-From-Basic-to-Implementation.pdf).

## Zoom and memory

Tap the corner magnifier to open/close the overview/detail view. The overview
retains the main timebase and overlaid sources; detail may use Overlay or Stacked.
Move/resize the overview selection to position the detail window. Detail dragging
moves waveform content naturally. Plain mouse wheel pans horizontally. Ctrl plus
wheel adjusts vertical display magnification in zoom detail, or the selected
source's vertical scale outside zoom. H controls/box/pinch provide horizontal
zoom without overloading plain scrolling. Main/overview gestures affect the main
timebase.

The compact zoom row provides H-/H+, V-/V+, Box, Reset and Close. V controls are
display magnification and preserve physical source settings. Box remains selected
for repeated rectangles; a plain plot tap, a menu or Escape cancels it. Reset
restores the detail to its overview; Close removes detail and keeps explicit main
timebase changes. This follows the interaction structure in the official
[TekScope help](https://download.tek.com/manual/TekScope-Help-EN-US-077-1699-08_077169908.pdf).

Right-click or hold the plot and choose Fit record to recover the entire selected
reference or retained acquisition after navigating off-view. Reference fitting is
a display operation. Hardware Autoset configures live ADC channels only; in a
reference-only/SIM workspace it leaves the record unchanged and directs you to
Fit record. Mixed ADC/reference Autoset does not change reference scales/data.

Fixed memory depth retains native acquisition samples separately from the small
interpolated display record. Panning or changing time scale while stopped rebuilds
the view from that retained memory without taking a new capture or changing its
accepted trigger. The RAW waveform interface exposes native memory, including
segments outside the initial screen. A frame outside the view is retained so
navigation can return to it. Deep desktop demo captures are prepared off-thread.

Screen measurements follow the physical visible interval while stopped; Full
record stays fixed on pan. Long reference/acquired gates run in a bounded worker.
The six basic full-frame quantities can use summaries prepared during acquisition.
Reference files retain their own interval and are not limited by ADC bandwidth.

AC-coupled analog trigger sources hide the absolute voltage-level marker. Moving
a DC trigger level shows a thin horizontal guide; loaded references retain a
meaningful absolute level regardless of an unrelated channel's AC setting.

## One-tap Eye/Jitter

Choose Auto Eye/Jitter in the analysis menu/settings. The action
uses the selected source, clears stale companion/notch settings, estimates NRZ
rate/threshold/hysteresis, opens the full Eye page with optional Side/Under panes, TIE trend, histogram,
spectrum and bathtub plots, and closes setup. Long immutable records run in a
background worker. Fresh accepted live/SIM captures accumulate; panning a stopped
record does not manufacture new captures.

Flat data, too few edges and inadequate sampling produce explicit missing/limited
results. Automatic NRZ recovery is an estimate: a PAM/C-PHY rail is not a validated
binary eye or a decoded symbol-rate measurement. Explicit rate/clock controls
remain available. Model-derived BER/jitter quantities are distinguished from
measured values; this is not a claim of DPOJET compliance equivalence.

SSH provisioning and host-key/public-key management are described in
[SSH access](ssh.md). Desktop OOS does not start an SSH server.
