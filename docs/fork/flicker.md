# Trace flicker at minimum persistence (2026-11-13)

Report: at CH1 100 mV/div, 500 us/div, Auto sweep, 100 k points, the 1 kHz probe-comp trace flickers; less or
not at other time bases. Build d9800d9, `acq.pipeline = batch`, `graphics = gpu`. `:SYSTem:STATe?`: 30 fps,
about 80 frames acquired per second, so 2 to 3 frames per picture, some pictures none.

## Cause

A picture at minimum persistence holds the frames that arrived since the last screen refresh. On the hardware
source they reach the phosphor in two ways (`Ui::step`):

- every frame but the newest: the fast path, `drawpipe::draw_frame` -> `fastdraw::deposit`, one min/max span per
  screen column, weight 24 per pixel per frame;
- the newest: `Source::acquire` makes it a 3600-point record, drawn with `Phosphor::draw_ex`. A flat trace has
  about 3.6 record points per column, so it adds about 3.6 x 24 per pixel.

The grade of the picture (`picture_ref`, the earlier flicker fix) assumes frames of equal weight: the brightest
pixel per frame times the frames in the picture. With one record frame and k deposited frames the brightest pixel is
about 86 + 24 k, so the brightness per frame is 86 (k = 0), 55 (k = 1), 45 (k = 2). The per-frame reference
follows a higher value at once and a lower one slowly, so after a picture without deposited frames the next ones are
graded about twice too dark. The mix changes with the batch sizes, hence the flicker. At short time bases a picture
holds many frames and the one record frame matters little; at 500 us/div with 2 to 3 frames it dominates. The GPU
draws the maps it is given (the same LUT from `vmax`), so CPU and GPU show the same.

## Fix (branch fix/flicker-weight)

When the fast path applies and the record has its frame, the newest frame is drawn with `draw_frame` too. Every
frame of a picture then has the same weight, and `picture_ref` grades them alike. Other cases (zoom, Z input,
averaging, sparse records without a frame) keep `draw_ex`; there no frame goes through the fast path.

Host test `hardware_pictures_look_the_same_whatever_the_frames_per_picture`: a hardware source fed 1 to 5 equal
frames per picture; every picture's brightness within 3 of 255 of the first. Without the fix: off by 255.

## Verification on the unit

Build 20261113-165356 (dho804 6223967): the user confirmed by eye that the flicker is gone at the reported settings
(2026-11-13).

At the reported settings: screenshots in a row (`:DISPlay:DATA?`, only the changed ones count), the number of trace
pixels and their mean red must stay within a few percent; by eye the trace is steady.
