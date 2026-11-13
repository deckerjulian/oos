# The GPU picture (System > Graphics = GPU)

The RK3399 has a Mali-T860 MP4. With System > Graphics = GPU, oos-ui lets it draw the screen: the CPU only records
what is to be drawn, in two layers (under and over the traces), and leaves the traces as phosphor maps; the GPU draws
the layers from those commands, adds the traces and scales the picture for a monitor. GPU is the default, CPU the
other choice; it is a setting of the scope (`graphics` in `/oos/system.conf`, docs/network.md). Where the GPU does not
start, the CPU draws (said on screen only when the setting was just changed).

## What it is good for, measured on a DHO924S

Panel, Run, four channels, 30 frames a second in every column (2026-11-12, `:SYSTem:STATe?` and the load per thread):

| | CPU | GPU, layers as pixels (first version) | GPU, layers as commands |
|---|---|---|---|
| frame loop (its core) | 53 % | 44 % | 35 % |
| oos-gpu | – | 12 % | 8.5 % |
| drawing on the CPU per frame | 6.9 ms + 1.6 ms present | 7.3 ms | 3.5 to 4 ms |
| the GPU per frame | – | about 9 ms | 9 to 13 ms |

| | CPU | GPU |
|---|---|---|
| HDMI Mirror / Waveform | the panel picture scaled on the CPU, nearest pixel | scaled by the GPU from what it already has, filtered, no CPU work |
| HDMI Layout | the screen drawn again at the monitor's size on the CPU | drawn by the GPU at the monitor's size, the traces stretched and smoothed |

The GPU itself is no faster than the CPU at the traces (the CPU skips every empty pixel of the phosphor maps, the GPU
shades all of them); what it saves is the CPU's drawing of everything else, and the monitor's picture.

With a 1920x1080 monitor, four channels, Run, a menu open (2026-11-13, frames a second / the frame loop's core):

| | Mirror | Waveform | Layout |
|---|---|---|---|
| CPU | 30 / 51 % | 30 / 54 % | 26 / 70 % |
| GPU | 30 / 22 % | 30 / 29 % | 30 / 28 % |

The helper's GPU time per frame there: the panel 8 to 10 ms, the monitor 12 to 15 ms. Before the traces' two steps
(below), the monitor's traces alone took 36 to 41 ms and the GPU fell behind the CPU (25 frames a second).

## How it works

- **oos-ui** (`ui/src/ui.rs`, `Ui::render_layers`): the same drawing code as always, only into two canvases that
  record (`gfx::Canvas::record`): under the traces (background, graticule, masks) and over them (everything else).
  Each drawing call becomes commands (`gfx::Cmd`, 24 bytes): a rectangle with a colour and alpha, a glyph (its alpha
  bytes in a glyph atlas the helper keeps), an image, a rounded corner, an anti-aliased line. The traces become jobs
  (`TraceJob`: the window, the maps of the shown channels, the brightness tables the CPU would use).
  `OOS_GPU_PIXELS=1` draws the layers on the CPU and sends them as pixels instead, as the first version did
  (premultiplied alpha in the top byte, `gfx::Canvas::layer`).
- **The commands give the CPU's pixels**: the helper's shader computes each pixel's coverage the way `gfx.rs` does
  (the corners' 16 samples, the line's 16.16 steps, the glyphs' alphas). A test plays the commands back on the CPU
  the same way and compares them with drawing directly, on every kind of screen
  (`recorded_layers_replay_to_the_same_pixels`).
- **The traces in two steps**: first, at the size of the maps, each cell as the index into its channel's LUT plus 1
  (0: empty), the four channels in one half-float texture; then in the target one read of it per pixel, the LUT
  after it. Stretched for a monitor, the GPU's filter mixes the indices (the LUT after the mixing keeps thin traces
  bright); unstretched, the panel's pixels are exactly the CPU's. Colour grade the same way with the summed maps.
- **Two sets of textures**: the panel's frames and the monitor's keep their own layers, maps and indices in the
  helper (`set` in the frame), so each uploads only the rows that changed since its own last frame. Mirror takes the
  panel's set (`reuse`).
- **oos-gpu** (`gpu/oos-gpu.c`): a separate process, because oos-ui is one static musl binary and Mesa needs glibc.
  It draws each layer from its commands into a texture: one indexed draw, two triangles per command (Panfrost on
  this Mali took 14 us per instance of an instanced draw, 30 ms for one layer). A layer whose commands and images
  are the same as last time stays as it is (a hash of them; mostly the layer under the traces). Then it draws into
  the display's scan-out buffers, which it gets as dma-bufs: the layer under the traces, the traces of each window in
  one pass (all channels, channel 4 first, as the CPU), the layer over them. For the web page, screenshots and SCPI it
  reads the picture back, only when someone wants it. `OOS_GPU_LOG=1`: the GPU time of the layers on stderr.
- **The protocol** (`ui/src/gpu.rs`): a Unix socket on the helper's stdin and one shared memory arena. Fixed C
  structs (`Hello`, `Target`, `Frame`, `Done`, `Cmd`), their sizes checked by a test on both sides. Each output keeps
  its blocks in its half of the arena; of the maps only rows that changed since the last frame are copied and
  uploaded. The commands, images and new glyphs lie at the top of the half.
- **Pipelined**: while the GPU puts frame N together, the CPU draws frame N+1; frame N is shown when it is done
  (three scan-out buffers per output). A screenshot waits for its own frame.
- **Clock**: the Mali's load governor only sees the short work of each frame and stays at 200 MHz (an 8 ms frame);
  while the helper runs, its lowest step is 400 MHz, which runs at the same voltage.
- **Fallback**: no Panfrost render node, a helper that does not start, or any failed frame: oos-ui says so and draws
  on the CPU until the setting is changed again.

## What the image has for it

- the kernel with Panfrost (`kernel/config`) and the GPU node of the device tree enabled for it
  (`kernel/dts/rk3399-rigol-dho900.dts`: `arm,mali-t860`, `mali-supply`, steps up to 600 MHz);
- `rootfs/out/gpu/`, built by `gpu/build.sh`: the helper, Mesa 26.0.8 with only Panfrost (EGL, GLES 3, GBM; built in
  two stages, LLVM only at build time), libdrm, zlib, libstdc++ and glibc with its loader. About 26 MB, 8 MB of the
  compressed image.

## Testing without a scope

`gpu/test-host.sh` runs every script of `ui/tests` in an arm64 container twice, on the CPU and through the helper on
Mesa's software renderer, and `gpu/compare.py` compares the pictures: all 92 agree to within 4 of 255 (the GPU blends
in floating point; two single pixels of a corner over colour-graded traces are 4 off, all else within 3). With
`OOS_GPU_HDMI=1920x1080 OOS_GPU_HDMI_MODE=Mirror|Waveform|Layout` the script runner also writes the monitor's picture
of every shot; commands and pixel layers (`OOS_GPU_PIXELS=1`) give the same monitor pictures to within 2.

## Known limits

- The GPU and the VPU are switched on by the device tree of 0.1.8; 0.1.7 has the code and draws on the CPU. 0.1.7
  starts the update to 0.1.8 by kexec with the new tree (docs/update.md, "Another device tree"); 0.1.6 cannot, so it
  goes to 0.1.7 first. The tree was installed on the unit on 2026-11-12 and started cold: the GPU, the VPU and the
  trial confirmation came up.
- Tried with a 1920x1080 monitor on a DHO924S; larger monitors (2560x1440, 4K) cost the GPU more per frame in
  proportion to their pixels and are not measured.
- With the logic analyser on and HDMI Layout, the LA columns are worked out again for every monitor frame.
- The grid's dots are single pixels, about 2000 commands; the GPU draws them only when the layer under the traces
  changes.
