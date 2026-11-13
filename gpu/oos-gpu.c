// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later
//
// oos-gpu: the GPU half of oos-ui's picture (Display > Graphics = GPU).
//
// oos-ui describes the screen in two layers: what lies under the traces (background, graticule, masks) and what lies
// over them (labels, cursors, bars, menus). Each layer comes as a list of drawing commands (rectangles, glyphs, line
// and corner coverage, images: ui/src/gfx.rs, `Cmd`) that this program draws on the GPU, or as pixels drawn on the
// CPU. The traces themselves stay as phosphor maps (hit counts per pixel). This program puts the three together on
// the GPU, straight into the buffers the display shows (dma-bufs of oos-ui's scan-out buffers): no copy of the picture
// by the CPU, and a monitor gets it scaled with filtering. When asked, it reads the picture back for the web page,
// screenshots and SCPI.
//
// It is a separate process because oos-ui is one static musl binary and Mesa needs glibc; if it fails, oos-ui goes
// back to drawing on the CPU. The protocol (one Unix socket on stdin, one shared memory arena) is in gpu/README.md
// and ui/src/gpu.rs; both sides check `VERSION`.
//
//   oos-gpu                 socket on fd 0 (started by oos-ui)
//   OOS_GPU_SOFT=1 oos-gpu  no GPU needed: Mesa's software renderer (host tests in a container)
#define _GNU_SOURCE
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES3/gl3.h>
#include <GLES2/gl2ext.h>
#include <errno.h>
#include <signal.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#define MAGIC 0x47534f4fu /* "OOSG" */
#define VERSION 2
#define MAX_TARGETS 8
#define MAX_QUADS 4
#define MAX_JOBS 16
#define NCH 4
#define NONE 0xffffffffu
#define FOURCC_XRGB8888 0x34325258u /* 'XR24' */

enum { K_HELLO = 1, K_TARGET = 2, K_DROP = 3, K_FRAME = 4, K_DONE = 5 };

struct hello {
	uint32_t magic, version, kind, pad;
	uint64_t arena_size;
};

struct target {
	uint32_t magic, kind, index;
	int32_t w, h, pitch;
	uint32_t fourcc;
	/* 0: a dma-buf in the fd that comes with it, 1: memory only (tests: read back always) */
	uint32_t memory;
};

struct drop {
	uint32_t magic, kind, index, pad;
};

/* A rectangle of a layer pair (crop) put into a rectangle of the target. */
struct quad {
	uint32_t under_off, over_off; /* arena offsets; NONE: no layer */
	int32_t lw, lh;               /* layer size (rows of lw * 4 bytes) */
	int32_t sx, sy, sw, sh;       /* crop in the layer */
	int32_t dx, dy, dw, dh;       /* place in the target */
	uint32_t filter;              /* 0 nearest, 1 linear (layers and maps) */
	uint32_t first_job, n_jobs;   /* this quad's jobs, in layer coordinates */
	uint32_t reuse;               /* 1: layers, maps and tables as the panel's last frame (set 0) uploaded them */
	int32_t under_y0, under_y1;   /* rows that changed since the last frame from these arena bytes */
	int32_t over_y0, over_y1;
	/* NONE: the layer is pixels; else it is that many commands (`struct cmd`) at its offset, drawn here */
	uint32_t under_n, over_n;
	uint32_t img_off, img_rows; /* the pixels of the image commands of both layers: rows of ATLAS_W xrgb words */
};

/* A drawing command of a layer (ui/src/gfx.rs, `Cmd`): the pixels x0..x1, y0..y1 covered by col (0xaarrggbb) the
 * way the CPU covers them, by kind (low byte; the high byte is a corner's mirroring). */
struct cmd {
	int16_t x0, y0, x1, y1;
	int16_t ax, ay, aw;
	uint16_t kind;
	uint32_t off, col;
};
#define ATLAS_W 4096
#define ATLAS_H 512

/* A plot window's traces: up to four maps of mw x mh 16-bit counts shown in x, y, w, h. */
struct job {
	int32_t x, y, w, h;
	int32_t mw, mh;
	uint32_t map_off[NCH]; /* NONE: channel not drawn */
	uint32_t graded;       /* 0: per channel colours, 1: colour grade of the sum */
	uint32_t lut_off;      /* 4 x 1024 alphas (u8), or 1024 colours (u32 xrgb) when graded */
	uint32_t shift[NCH];   /* count >> shift indexes the LUT (graded: shift[0]) */
	uint32_t col[NCH];     /* channel colours, xrgb */
	int32_t map_y0[NCH], map_y1[NCH]; /* rows of each map that changed (as for the layers) */
};

struct frame {
	uint32_t magic, kind, target, clear; /* clear: 0, or 0x01rrggbb to clear in that colour first */
	uint32_t n_quads, n_jobs;
	uint32_t readback, readback_off; /* read the target back into the arena as xrgb rows of tw */
	/* glyph alphas new to the atlas: rows of ATLAS_W bytes from atlas_off, to its row atlas_row */
	uint32_t atlas_off, atlas_rows, atlas_row;
	/* the set of textures: 0 the panel's, 1 the monitor's. Each keeps its layers, maps and LUT indices from its last
	 * frame, so only changed rows are uploaded and unchanged layers are not drawn again; with one set for both, every
	 * monitor frame replaced the panel's textures and the other way round. A quad with `reuse` takes set 0. */
	uint32_t set;
	struct quad q[MAX_QUADS];
	struct job j[MAX_JOBS];
};

struct done {
	uint32_t magic, kind, status, pad;
	float ms_upload, ms_draw, ms_read, pad2;
};

static uint8_t *arena;
static uint64_t arena_size;
static int sock = 0;

static double now_ms(void) {
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return t.tv_sec * 1e3 + t.tv_nsec * 1e-6;
}

static void say(const char *m) {
	fprintf(stderr, "oos-gpu: %s\n", m);
}

/* One message and the descriptor that may come with it. */
static int recv_msg(void *buf, size_t len, int *fd) {
	struct iovec io = {buf, len};
	char ctl[CMSG_SPACE(sizeof(int))];
	struct msghdr m = {0};
	m.msg_iov = &io;
	m.msg_iovlen = 1;
	m.msg_control = ctl;
	m.msg_controllen = sizeof ctl;
	ssize_t n = recvmsg(sock, &m, MSG_CMSG_CLOEXEC);
	if (n <= 0)
		return -1;
	if (fd) {
		*fd = -1;
		for (struct cmsghdr *c = CMSG_FIRSTHDR(&m); c; c = CMSG_NXTHDR(&m, c))
			if (c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_RIGHTS)
				memcpy(fd, CMSG_DATA(c), sizeof(int));
	}
	return (int)n;
}

static int send_done(uint32_t status, float up, float draw, float rd) {
	struct done d = {MAGIC, K_DONE, status, 0, up, draw, rd, 0};
	return write(sock, &d, sizeof d) == sizeof d ? 0 : -1;
}

/* ------------------------------------------------------------------------------------------------ GL */

static EGLDisplay dpy;
static PFNEGLCREATEIMAGEKHRPROC create_image;
static PFNEGLDESTROYIMAGEKHRPROC destroy_image;
static PFNGLEGLIMAGETARGETRENDERBUFFERSTORAGEOESPROC image_rb;

struct tgt {
	int used, w, h, memory;
	EGLImageKHR img;
	GLuint rb, fb;
};
static struct tgt tgts[MAX_TARGETS];

static GLuint prog_layer, prog_trace, prog_grade, prog_index, prog_gpre;
/* the read-back copy of a scan-out target */
static GLuint rb_read, fb_read;
static int rb_w, rb_h;
static GLint u_layer_tex, u_layer_src, u_layer_dst, u_layer_tsz, u_layer_opaque;
static GLint u_tr_ix, u_tr_lut, u_tr_col, u_tr_job, u_tr_msz, u_tr_lin, u_tr_under, u_tr_jl, u_tr_lsz;
static GLint u_gr_ix, u_gr_lut, u_gr_job, u_gr_msz, u_gr_lin;
static GLint u_al_maps[NCH], u_al_on, u_al_shift, u_al_dst;
static GLint u_gp_maps[NCH], u_gp_n, u_gp_shift, u_gp_dst;
static GLuint vao;

/* textures kept between frames, grown when a size changes */
struct tex {
	GLuint id, fb;
	int w, h;
	GLenum ifmt;
	/* the arena bytes it holds a copy of (NONE: not filled yet): only then a band of changed rows is enough */
	uint32_t from;
	/* drawn from commands: a hash of them and their images (0: not), the same again needs no drawing */
	uint64_t drawn;
};
#define SETS 2
static struct tex layer_tex[SETS][MAX_QUADS][2], map_tex[SETS][MAX_JOBS][NCH], lut_tex[SETS][MAX_JOBS];
/* each job's LUT indices at the size of its maps, worked out from them (FS_INDEX, FS_GRADE_PRE) */
static struct tex pre_tex[SETS][MAX_JOBS];
/* the glyph alphas of all fonts so far (R8, ATLAS_W wide) and the pixels of a frame's images (RGBA8) */
static struct tex atlas_tex, img_tex;
static GLuint prog_cmd, vao_cmd, vbo_cmd, ibo_cmd;
/* a command's rectangle as four vertices, each with the command's values (Panfrost on the Midgard GPU took 14 us
 * per instance of an instanced draw; four vertices of an indexed one cost next to nothing) */
struct cmd_vtx {
	float x, y;
	int16_t ax, ay, aw;
	uint16_t kind;
	uint32_t off, col;
};
static struct cmd_vtx *cmd_v;
static uint32_t cmd_cap, cmd_icap;
static GLint u_cmd_lsz, u_cmd_atlas, u_cmd_img;

static GLuint compile(GLenum t, const char *src) {
	GLuint s = glCreateShader(t);
	glShaderSource(s, 1, &src, NULL);
	glCompileShader(s);
	GLint ok;
	glGetShaderiv(s, GL_COMPILE_STATUS, &ok);
	if (!ok) {
		char log[2048];
		glGetShaderInfoLog(s, sizeof log, NULL, log);
		fprintf(stderr, "oos-gpu: shader: %s\n", log);
		exit(2);
	}
	return s;
}

static GLuint link_vs(const char *vs, const char *fs);
static GLuint link_prog(const char *fs) {
	/* the rectangle `dst` (x, y, w, h in target pixels) as two triangles. GL's y runs up, but row 0 of a scan-out
	 * buffer in memory is the top of the screen: so y is used as it is, and the picture lies top row first in
	 * memory, as the display and the read-back want it. */
	static const char *vs = "#version 300 es\n"
				"uniform vec4 dst; uniform vec2 tsz; out vec2 px;\n"
				"void main(){ vec2 c = vec2(float(gl_VertexID & 1), float(gl_VertexID >> 1));"
				" px = c;"
				" vec2 p = (dst.xy + c * dst.zw) / tsz * 2.0 - 1.0;"
				" gl_Position = vec4(p.x, p.y, 0.0, 1.0); }\n";
	return link_vs(vs, fs);
}

static GLuint link_vs(const char *vs, const char *fs) {
	GLuint p = glCreateProgram();
	glAttachShader(p, compile(GL_VERTEX_SHADER, vs));
	glAttachShader(p, compile(GL_FRAGMENT_SHADER, fs));
	glLinkProgram(p);
	GLint ok;
	glGetProgramiv(p, GL_LINK_STATUS, &ok);
	if (!ok) {
		say("link failed");
		exit(2);
	}
	return p;
}

/* A layer, cropped: px runs 0..1 over the destination rectangle. */
/* The layers arrive as oos-ui keeps them: bytes b, g, r, a (xrgb words); the shader puts the channels in order. */
static const char *FS_LAYER = "#version 300 es\n"
			      "precision highp float;\n"
			      "uniform sampler2D tex; uniform vec4 src; uniform vec2 lsz; uniform int opaque;\n"
			      "in vec2 px; out vec4 o;\n"
			      "void main(){ vec2 uv = (src.xy + px * src.zw) / lsz; vec4 c = texture(tex, uv).bgra;"
			      " if (opaque == 1) c.a = 1.0; o = c; }\n";

/* A job's traces in two steps. First, at the size of its maps, each channel's cell as the index into its LUT plus 1
 * (0: empty), all four channels in one RGBA16F texture (FS_INDEX). Then in the target: the layer under them, then
 * channel 4 down to 1, each its LUT's alpha over its colour (the CPU's `draw_traces`), opaque (FS_TRACE). Stretched
 * (a monitor) the GPU's own filter mixes the four nearest indices, one fetch for all channels, and the LUT comes after
 * the mixing, so a thin trace keeps its brightness; unstretched (the panel) the texel is taken as it is, which gives
 * the CPU's pixels. Mixing the counts in the shader instead took 16 fetches of integer textures per pixel: 36 to
 * 41 ms for the traces of a 1920x1080 monitor, against 5 ms of the panel. */
static const char *FS_INDEX = "#version 300 es\n"
			      "precision highp float; precision highp usampler2D;\n"
			      "uniform usampler2D m0, m1, m2, m3; uniform ivec4 on; uniform ivec4 shift;\n"
			      "out vec4 o;\n"
			      "float e(usampler2D m, int c){ uint v = texelFetch(m, ivec2(gl_FragCoord.xy), 0).r;"
			      " return v == 0u ? 0.0 : float(min(int(v) >> shift[c], 1023) + 1); }\n"
			      "void main(){ o = vec4(on.x == 1 ? e(m0, 0) : 0.0, on.y == 1 ? e(m1, 1) : 0.0,"
			      " on.z == 1 ? e(m2, 2) : 0.0, on.w == 1 ? e(m3, 3) : 0.0); }\n";
static const char *FS_TRACE = "#version 300 es\n"
			      "precision highp float;\n"
			      "uniform sampler2D under; uniform vec4 jl; uniform vec2 lsz; uniform highp sampler2D ix; uniform highp sampler2D lut;\n"
			      "uniform vec3 col[4]; uniform vec2 msz; uniform int lin;\n"
			      "in vec2 px; out vec4 o;\n"
			      "vec3 over(vec3 d, float e, int c){ if (e < 0.5) return d;"
			      " float a = texelFetch(lut, ivec2(min(int(e + 0.5) - 1, 1023), c), 0).r; return d * (1.0 - a) + col[c] * a; }\n"
			      "void main(){ vec3 d = texture(under, (jl.xy + px * jl.zw) / lsz).bgr;"
			      " vec4 e = lin == 1 ? texture(ix, px) : texelFetch(ix, clamp(ivec2(px * msz), ivec2(0), ivec2(msz) - 1), 0);"
			      " d = over(d, e.w, 3); d = over(d, e.z, 2); d = over(d, e.y, 1); d = over(d, e.x, 0);"
			      " o = vec4(d, 1.0); }\n";

/* Colour grade the same way: first, at the size of the maps, the sum of the shown maps as the index into the colour
 * LUT plus 1, then in the target that index's colour, opaque, where it is not empty. */
static const char *FS_GRADE_PRE = "#version 300 es\n"
				  "precision highp float; precision highp usampler2D;\n"
				  "uniform usampler2D m0, m1, m2, m3; uniform int n; uniform int shift;\n"
				  "out vec4 o;\n"
				  "void main(){ ivec2 c = ivec2(gl_FragCoord.xy); uint s = texelFetch(m0, c, 0).r;"
				  " if (n > 1) s += texelFetch(m1, c, 0).r; if (n > 2) s += texelFetch(m2, c, 0).r;"
				  " if (n > 3) s += texelFetch(m3, c, 0).r;"
				  " o = vec4(s == 0u ? 0.0 : float(min(int(s) >> shift, 1023) + 1), 0.0, 0.0, 0.0); }\n";
static const char *FS_GRADE = "#version 300 es\n"
			      "precision highp float;\n"
			      "uniform highp sampler2D ix; uniform highp sampler2D lut; uniform vec2 msz; uniform int lin;\n"
			      "in vec2 px; out vec4 o;\n"
			      "void main(){ float e = lin == 1 ? texture(ix, px).r : texelFetch(ix, clamp(ivec2(px * msz), ivec2(0), ivec2(msz) - 1), 0).r;"
			      " if (e < 0.5) discard;"
			      " o = vec4(texelFetch(lut, ivec2(min(int(e + 0.5) - 1, 1023), 0), 0).bgr, 1.0); }\n";

/* A layer from its commands, all in one instanced draw: each command a rectangle, its coverage per pixel as the
 * CPU's `Canvas` computes it (gfx.rs: `fill`, `blend`, `pixel`, `text`, `image`, `round`, `round_outline`, `line`).
 * The colour goes out as the layers lie in memory (b, g, r), the coverage as alpha: blended "over", a layer cleared
 * to transparent ends up premultiplied, as the CPU's. */
static const char *VS_CMD = "#version 300 es\n"
			    "layout(location = 0) in vec2 r; layout(location = 1) in ivec4 a; layout(location = 2) in uvec2 oc;\n"
			    "uniform vec2 lsz; flat out ivec4 va; flat out uvec2 voc;\n"
			    "void main(){ vec2 p = r / lsz * 2.0 - 1.0;"
			    " va = a; voc = oc; gl_Position = vec4(p, 0.0, 1.0); }\n";
static const char *FS_CMD = "#version 300 es\n"
			    "precision highp float; precision highp int;\n"
			    "uniform highp sampler2D atlas; uniform highp sampler2D img;\n"
			    "flat in ivec4 va; flat in uvec2 voc; out vec4 o;\n"
			    "int cov(int px, int py, int r){ if (r <= 0) return 0; int n = 0; float rr = float(r * r);"
			    " for (int sy = 0; sy < 4; sy++) for (int sx = 0; sx < 4; sx++) {"
			    "  float dx = float(px) + (float(sx) + 0.5) / 4.0 - float(r); float dy = float(py) + (float(sy) + 0.5) / 4.0 - float(r);"
			    "  if (dx * dx + dy * dy <= rr) n++; } return n * 255 / 16; }\n"
			    "ivec2 at(int i){ return ivec2(i & 4095, i >> 12); }\n"
			    "void main(){ ivec2 p = ivec2(gl_FragCoord.xy); int kind = va.w & 255, sub = va.w >> 8;"
			    " uint col = voc.y; int off = int(voc.x); int i = p.x - va.x, j = p.y - va.y;"
			    " vec3 c = vec3(float(col & 255u), float((col >> 8) & 255u), float((col >> 16) & 255u)) / 255.0;"
			    " int al = int(col >> 24);"
			    " if (kind == 1) { al = int(texelFetch(atlas, at(off + j * va.z + i), 0).r * 255.0 + 0.5); }"
			    " else if (kind == 2 || kind == 3) { vec4 t = texelFetch(img, at(off + j * va.z + i), 0);"
			    "  if (kind == 3 && t.rgb == vec3(0.0)) discard; c = t.rgb; al = 255; }"
			    " else if (kind == 4 || kind == 5) { if ((sub & 1) != 0) i = -i; if ((sub & 2) != 0) j = -j;"
			    "  al = kind == 4 ? cov(i, j, va.z) * al / 255 : max(cov(i, j, va.z) - cov(i - 1, j - 1, va.z - 1), 0); }"
			    " else if (kind == 6 || kind == 7) { int maj = kind == 7 ? p.y : p.x, mn = kind == 7 ? p.x : p.y;"
			    "  int yf = (va.y << 16) + (maj - va.x) * off; int yi = yf >> 16, fr = (yf >> 8) & 255;"
			    "  al = mn == yi ? 255 - fr : (mn == yi + 1 ? fr : 0); }"
			    " if (al <= 0) discard; o = vec4(c, float(min(al, 255)) / 255.0); }\n";

static void gl_setup(void) {
	prog_layer = link_prog(FS_LAYER);
	u_layer_tex = glGetUniformLocation(prog_layer, "tex");
	u_layer_src = glGetUniformLocation(prog_layer, "src");
	u_layer_dst = glGetUniformLocation(prog_layer, "dst");
	u_layer_tsz = glGetUniformLocation(prog_layer, "tsz");
	u_layer_opaque = glGetUniformLocation(prog_layer, "opaque");
	const char *tn[NCH] = {"m0", "m1", "m2", "m3"};
	prog_index = link_prog(FS_INDEX);
	for (int c = 0; c < NCH; c++)
		u_al_maps[c] = glGetUniformLocation(prog_index, tn[c]);
	u_al_on = glGetUniformLocation(prog_index, "on");
	u_al_shift = glGetUniformLocation(prog_index, "shift");
	u_al_dst = glGetUniformLocation(prog_index, "dst");
	prog_gpre = link_prog(FS_GRADE_PRE);
	for (int c = 0; c < NCH; c++)
		u_gp_maps[c] = glGetUniformLocation(prog_gpre, tn[c]);
	u_gp_n = glGetUniformLocation(prog_gpre, "n");
	u_gp_shift = glGetUniformLocation(prog_gpre, "shift");
	u_gp_dst = glGetUniformLocation(prog_gpre, "dst");
	prog_trace = link_prog(FS_TRACE);
	u_tr_ix = glGetUniformLocation(prog_trace, "ix");
	u_tr_lut = glGetUniformLocation(prog_trace, "lut");
	u_tr_col = glGetUniformLocation(prog_trace, "col");
	u_tr_under = glGetUniformLocation(prog_trace, "under");
	u_tr_jl = glGetUniformLocation(prog_trace, "jl");
	u_tr_lsz = glGetUniformLocation(prog_trace, "lsz");
	u_tr_job = glGetUniformLocation(prog_trace, "dst");
	u_tr_msz = glGetUniformLocation(prog_trace, "msz");
	u_tr_lin = glGetUniformLocation(prog_trace, "lin");
	prog_grade = link_prog(FS_GRADE);
	u_gr_ix = glGetUniformLocation(prog_grade, "ix");
	u_gr_lut = glGetUniformLocation(prog_grade, "lut");
	u_gr_job = glGetUniformLocation(prog_grade, "dst");
	u_gr_msz = glGetUniformLocation(prog_grade, "msz");
	u_gr_lin = glGetUniformLocation(prog_grade, "lin");
	prog_cmd = link_vs(VS_CMD, FS_CMD);
	u_cmd_lsz = glGetUniformLocation(prog_cmd, "lsz");
	u_cmd_atlas = glGetUniformLocation(prog_cmd, "atlas");
	u_cmd_img = glGetUniformLocation(prog_cmd, "img");
	glGenVertexArrays(1, &vao_cmd);
	glBindVertexArray(vao_cmd);
	glGenBuffers(1, &vbo_cmd);
	glBindBuffer(GL_ARRAY_BUFFER, vbo_cmd);
	glEnableVertexAttribArray(0);
	glEnableVertexAttribArray(1);
	glEnableVertexAttribArray(2);
	glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, sizeof(struct cmd_vtx), (void *)0);
	glVertexAttribIPointer(1, 4, GL_SHORT, sizeof(struct cmd_vtx), (void *)8);
	glVertexAttribIPointer(2, 2, GL_UNSIGNED_INT, sizeof(struct cmd_vtx), (void *)16);
	glGenBuffers(1, &ibo_cmd);
	glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, ibo_cmd);
	glGenVertexArrays(1, &vao);
	glBindVertexArray(vao);
	glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
	glPixelStorei(GL_PACK_ALIGNMENT, 1);
}

/* A texture of the size and format, kept when it already has them. */
static void tex_ensure(struct tex *t, int w, int h, GLenum ifmt) {
	if (t->id && t->w == w && t->h == h && t->ifmt == ifmt)
		return;
	t->from = NONE;
	t->drawn = 0;
	if (t->fb)
		glDeleteFramebuffers(1, &t->fb);
	t->fb = 0;
	if (t->id)
		glDeleteTextures(1, &t->id);
	glGenTextures(1, &t->id);
	glBindTexture(GL_TEXTURE_2D, t->id);
	glTexStorage2D(GL_TEXTURE_2D, 1, ifmt, w, h);
	glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
	glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
	glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
	glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
	t->w = w;
	t->h = h;
	t->ifmt = ifmt;
}

static int in_arena(uint64_t off, uint64_t len) {
	return off != NONE && off <= arena_size && len <= arena_size - off;
}

/* Rows y0..y1 of arena bytes at `off` (rows of `row` bytes) into a texture: all of it when it holds other bytes. */
static void upload_rows(struct tex *t, uint32_t off, int w, int h, GLenum ifmt, GLenum fmt, GLenum type, int row, int y0, int y1) {
	tex_ensure(t, w, h, ifmt);
	t->drawn = 0;
	glBindTexture(GL_TEXTURE_2D, t->id);
	if (t->from != off) {
		y0 = 0;
		y1 = h;
		t->from = off;
	}
	y0 = y0 < 0 ? 0 : y0;
	y1 = y1 > h ? h : y1;
	if (y1 > y0)
		glTexSubImage2D(GL_TEXTURE_2D, 0, 0, y0, w, y1 - y0, fmt, type, arena + off + (size_t)y0 * row);
}

/* xrgb rows (bytes b, g, r, x) as they are: the shaders read them as .bgra. */
static void upload_bgra(struct tex *t, uint32_t off, int w, int h, int y0, int y1) {
	upload_rows(t, off, w, h, GL_RGBA8, GL_RGBA, GL_UNSIGNED_BYTE, w * 4, y0, y1);
}

static void draw_rect(GLint u_dst, float x, float y, float w, float h) {
	glUniform4f(u_dst, x, y, w, h);
	glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
}

static void set_tsz(GLuint prog, float tw, float th) {
	glUniform2f(glGetUniformLocation(prog, "tsz"), tw, th);
}

/* A layer of lw x lh drawn from its n commands at `off` into `t`: the one under the traces on opaque black, the one
 * over them on transparent. */
static uint64_t hash(uint64_t h, const uint8_t *p, size_t n) {
	/* FNV-1a, eight bytes at a time */
	for (; n >= 8; n -= 8, p += 8) {
		uint64_t w;
		memcpy(&w, p, 8);
		h = (h ^ w) * 0x100000001b3ull;
	}
	for (; n; n--, p++)
		h = (h ^ *p) * 0x100000001b3ull;
	return h;
}

static int draw_cmds(struct tex *t, uint32_t off, uint32_t n, int lw, int lh, int over, uint64_t img_hash) {
	if (n > (1u << 22) || !in_arena(off, (uint64_t)n * sizeof(struct cmd)))
		return 20;
	tex_ensure(t, lw, lh, GL_RGBA8);
	t->from = NONE;
	/* the same commands on the same images as last time: the texture holds that picture already */
	uint64_t h = hash(img_hash ^ ((uint64_t)lw << 32 | (uint32_t)lh), arena + off, (size_t)n * sizeof(struct cmd)) | 1;
	if (t->drawn == h)
		return 0;
	t->drawn = h;
	if (!t->fb) {
		glGenFramebuffers(1, &t->fb);
		glBindFramebuffer(GL_FRAMEBUFFER, t->fb);
		glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, t->id, 0);
		if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE)
			return 21;
	}
	glBindFramebuffer(GL_FRAMEBUFFER, t->fb);
	glViewport(0, 0, lw, lh);
	glDisable(GL_SCISSOR_TEST);
	glClearColor(0, 0, 0, over ? 0 : 1);
	glClear(GL_COLOR_BUFFER_BIT);
	if (!n)
		return 0;
	glUseProgram(prog_cmd);
	glUniform2f(u_cmd_lsz, lw, lh);
	glActiveTexture(GL_TEXTURE0);
	glBindTexture(GL_TEXTURE_2D, atlas_tex.id);
	glUniform1i(u_cmd_atlas, 0);
	glActiveTexture(GL_TEXTURE1);
	glBindTexture(GL_TEXTURE_2D, img_tex.id);
	glUniform1i(u_cmd_img, 1);
	glEnable(GL_BLEND);
	glBlendFuncSeparate(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA, GL_ONE, GL_ONE_MINUS_SRC_ALPHA);
	glBindVertexArray(vao_cmd);
	glBindBuffer(GL_ARRAY_BUFFER, vbo_cmd);
	if (n > cmd_cap) {
		cmd_cap = n + n / 2;
		cmd_v = realloc(cmd_v, (size_t)cmd_cap * 4 * sizeof *cmd_v);
		if (!cmd_v)
			return 24;
	}
	/* the indices: two triangles per command, kept */
	if (n > cmd_icap) {
		cmd_icap = cmd_cap;
		uint32_t *ix = malloc((size_t)cmd_icap * 6 * sizeof *ix);
		if (!ix)
			return 24;
		for (uint32_t k = 0; k < cmd_icap; k++) {
			uint32_t b = k * 4;
			uint32_t q[6] = {b, b + 1, b + 2, b + 2, b + 1, b + 3};
			memcpy(ix + k * 6, q, sizeof q);
		}
		glBufferData(GL_ELEMENT_ARRAY_BUFFER, (GLsizeiptr)cmd_icap * 6 * sizeof *ix, ix, GL_STATIC_DRAW);
		free(ix);
	}
	const struct cmd *cs = (const struct cmd *)(arena + off);
	for (uint32_t k = 0; k < n; k++) {
		const struct cmd *c = &cs[k];
		struct cmd_vtx v = {0, 0, c->ax, c->ay, c->aw, c->kind, c->off, c->col};
		for (int e = 0; e < 4; e++) {
			v.x = (e & 1) ? c->x1 : c->x0;
			v.y = (e & 2) ? c->y1 : c->y0;
			cmd_v[k * 4 + e] = v;
		}
	}
	glBufferData(GL_ARRAY_BUFFER, (GLsizeiptr)n * 4 * sizeof *cmd_v, cmd_v, GL_STREAM_DRAW);
	/* OOS_GPU_LOG=1: the GPU time of the layers, every 60th frame on stderr (each then waits for the GPU) */
	static int logn = -1;
	if (logn < 0)
		logn = getenv("OOS_GPU_LOG") != NULL;
	double a = logn ? (glFinish(), now_ms()) : 0;
	glDrawElements(GL_TRIANGLES, (GLsizei)n * 6, GL_UNSIGNED_INT, 0);
	glBindVertexArray(vao);
	glDisable(GL_BLEND);
	if (logn) {
		static int k[2];
		glFinish();
		if (k[over]++ % 60 == 0)
			fprintf(stderr, "oos-gpu: %s layer %dx%d: %u commands, %.1f ms\n", over ? "over" : "under", lw, lh, n, now_ms() - a);
	}
	return 0;
}

/* The job's LUT indices from its maps just uploaded, into pre_tex[s][ji] at the maps' size. 0: done, else the frame's
 * error (a GPU that cannot draw into a half-float texture). */
static int prepass(int s, uint32_t ji, const struct job *j) {
	struct tex *p = &pre_tex[s][ji];
	tex_ensure(p, j->mw, j->mh, GL_RGBA16F);
	p->drawn = 0;
	if (!p->fb) {
		glGenFramebuffers(1, &p->fb);
		glBindFramebuffer(GL_FRAMEBUFFER, p->fb);
		glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, p->id, 0);
		if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) {
			say("cannot draw into a half-float texture");
			return 24;
		}
	} else
		glBindFramebuffer(GL_FRAMEBUFFER, p->fb);
	glViewport(0, 0, j->mw, j->mh);
	glDisable(GL_SCISSOR_TEST);
	glDisable(GL_BLEND);
	/* every pixel is written: the clear only spares the Mali loading the old content of each tile */
	glClearColor(0, 0, 0, 0);
	glClear(GL_COLOR_BUFFER_BIT);
	int first = -1, n = 0;
	for (int c = 0; c < NCH; c++)
		if (j->map_off[c] != NONE && first < 0)
			first = c;
	if (first < 0)
		return 0;
	GLuint prog = j->graded ? prog_gpre : prog_index;
	glUseProgram(prog);
	set_tsz(prog, j->mw, j->mh);
	if (j->graded) {
		for (int c = 0; c < NCH; c++) {
			if (j->map_off[c] == NONE)
				continue;
			glActiveTexture(GL_TEXTURE1 + n);
			glBindTexture(GL_TEXTURE_2D, map_tex[s][ji][c].id);
			glUniform1i(u_gp_maps[n], 1 + n);
			n++;
		}
		for (int c = n; c < NCH; c++)
			glUniform1i(u_gp_maps[c], 1);
		glUniform1i(u_gp_n, n);
		glUniform1i(u_gp_shift, j->shift[0]);
		draw_rect(u_gp_dst, 0, 0, j->mw, j->mh);
		return 0;
	}
	GLint on[NCH], sh[NCH];
	for (int c = 0; c < NCH; c++) {
		on[c] = j->map_off[c] != NONE;
		sh[c] = j->shift[c];
		/* an unused sampler still gets a complete texture of the same kind */
		glActiveTexture(GL_TEXTURE1 + c);
		glBindTexture(GL_TEXTURE_2D, map_tex[s][ji][on[c] ? c : first].id);
		glUniform1i(u_al_maps[c], 1 + c);
	}
	glUniform4iv(u_al_on, 1, on);
	glUniform4iv(u_al_shift, 1, sh);
	draw_rect(u_al_dst, 0, 0, j->mw, j->mh);
	return 0;
}

static int do_frame(const struct frame *f, struct done *d) {
	if (f->target >= MAX_TARGETS || !tgts[f->target].used || f->n_quads > MAX_QUADS || f->n_jobs > MAX_JOBS || f->set >= SETS)
		return 1;
	int fs = f->set;
	struct tgt *t = &tgts[f->target];
	float tw = t->w, th = t->h;
	double t0 = now_ms();
	if (!atlas_tex.id) {
		tex_ensure(&atlas_tex, ATLAS_W, ATLAS_H, GL_R8);
		tex_ensure(&img_tex, ATLAS_W, 64, GL_RGBA8);
	}
	if (f->atlas_rows) {
		if (f->atlas_row + f->atlas_rows > ATLAS_H || !in_arena(f->atlas_off, (uint64_t)f->atlas_rows * ATLAS_W))
			return 22;
		glBindTexture(GL_TEXTURE_2D, atlas_tex.id);
		glTexSubImage2D(GL_TEXTURE_2D, 0, 0, f->atlas_row, ATLAS_W, f->atlas_rows, GL_RED, GL_UNSIGNED_BYTE, arena + f->atlas_off);
	}
	/* uploads: layers, maps, LUTs */
	for (uint32_t qi = 0; qi < f->n_quads; qi++) {
		const struct quad *q = &f->q[qi];
		size_t ln = (size_t)q->lw * q->lh * 4;
		if (q->lw <= 0 || q->lh <= 0 || q->lw > 8192 || q->lh > 8192)
			return 2;
		if (q->first_job + q->n_jobs > f->n_jobs)
			return 4;
		if (q->reuse)
			continue;
		uint64_t img_hash = 0xcbf29ce484222325ull;
		if (q->img_rows) {
			if (q->img_rows > 2048 || !in_arena(q->img_off, (uint64_t)q->img_rows * ATLAS_W * 4))
				return 23;
			img_hash = hash(img_hash, arena + q->img_off, (size_t)q->img_rows * ATLAS_W * 4);
			if ((int)q->img_rows > img_tex.h)
				tex_ensure(&img_tex, ATLAS_W, (q->img_rows + 63) & ~63u, GL_RGBA8);
			/* the texture holds these pixels already */
			if (img_tex.drawn != img_hash) {
				glBindTexture(GL_TEXTURE_2D, img_tex.id);
				glTexSubImage2D(GL_TEXTURE_2D, 0, 0, 0, ATLAS_W, q->img_rows, GL_RGBA, GL_UNSIGNED_BYTE, arena + q->img_off);
				img_tex.drawn = img_hash;
			}
		}
		int st;
		if (q->under_off != NONE && q->under_n != NONE) {
			if ((st = draw_cmds(&layer_tex[fs][qi][0], q->under_off, q->under_n, q->lw, q->lh, 0, img_hash)))
				return st;
		} else if (q->under_off != NONE) {
			if (!in_arena(q->under_off, ln))
				return 3;
			upload_bgra(&layer_tex[fs][qi][0], q->under_off, q->lw, q->lh, q->under_y0, q->under_y1);
		}
		if (q->over_off != NONE && q->over_n != NONE) {
			if ((st = draw_cmds(&layer_tex[fs][qi][1], q->over_off, q->over_n, q->lw, q->lh, 1, img_hash)))
				return st;
		} else if (q->over_off != NONE) {
			if (!in_arena(q->over_off, ln))
				return 3;
			upload_bgra(&layer_tex[fs][qi][1], q->over_off, q->lw, q->lh, q->over_y0, q->over_y1);
		}
	}
	for (uint32_t ji = 0; ji < f->n_jobs; ji++) {
		const struct job *j = &f->j[ji];
		int reused = 0;
		for (uint32_t qi = 0; qi < f->n_quads; qi++)
			if (f->q[qi].reuse && ji >= f->q[qi].first_job && ji < f->q[qi].first_job + f->q[qi].n_jobs)
				reused = 1;
		if (reused)
			continue;
		if (j->mw <= 0 || j->mh <= 0 || j->mw > 8192 || j->mh > 8192)
			return 5;
		size_t mn = (size_t)j->mw * j->mh * 2;
		for (int c = 0; c < NCH; c++) {
			if (j->map_off[c] == NONE)
				continue;
			if (!in_arena(j->map_off[c], mn))
				return 6;
			upload_rows(&map_tex[fs][ji][c], j->map_off[c], j->mw, j->mh, GL_R16UI, GL_RED_INTEGER, GL_UNSIGNED_SHORT, j->mw * 2, j->map_y0[c], j->map_y1[c]);
		}
		if (j->graded) {
			if (!in_arena(j->lut_off, 1024 * 4))
				return 7;
			lut_tex[fs][ji].from = NONE;
			upload_bgra(&lut_tex[fs][ji], j->lut_off, 1024, 1, 0, 1);
		} else {
			if (!in_arena(j->lut_off, 1024 * NCH))
				return 7;
			tex_ensure(&lut_tex[fs][ji], 1024, NCH, GL_R8);
			glBindTexture(GL_TEXTURE_2D, lut_tex[fs][ji].id);
			glTexSubImage2D(GL_TEXTURE_2D, 0, 0, 0, 1024, NCH, GL_RED, GL_UNSIGNED_BYTE, arena + j->lut_off);
		}
		int st = prepass(fs, ji, j);
		if (st)
			return st;
	}
	double t1 = now_ms();
	glBindFramebuffer(GL_FRAMEBUFFER, t->fb);
	glViewport(0, 0, t->w, t->h);
	glDisable(GL_SCISSOR_TEST);
	/* always cleared: then the Mali does not load each tile's old content from the (uncached) scan-out buffer before
	 * drawing; a frame whose quads cover the target looks the same */
	uint32_t cc = f->clear ? f->clear : 0;
	glClearColor((cc >> 16 & 255) / 255.0f, (cc >> 8 & 255) / 255.0f, (cc & 255) / 255.0f, 1);
	glClear(GL_COLOR_BUFFER_BIT);
	for (uint32_t qi = 0; qi < f->n_quads; qi++) {
		const struct quad *q = &f->q[qi];
		GLint filt = q->filter ? GL_LINEAR : GL_NEAREST;
		int qs = q->reuse ? 0 : fs;
		/* scale from layer coordinates to the target */
		float kx = (float)q->dw / q->sw, ky = (float)q->dh / q->sh;
		/* everything of this quad stays inside its destination rectangle */
		glEnable(GL_SCISSOR_TEST);
		glScissor(q->dx, q->dy, q->dw, q->dh);
		/* 1. under the traces */
		if (q->under_off != NONE) {
			glDisable(GL_BLEND);
			glUseProgram(prog_layer);
			set_tsz(prog_layer, tw, th);
			glActiveTexture(GL_TEXTURE0);
			glBindTexture(GL_TEXTURE_2D, layer_tex[qs][qi][0].id);
			glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, filt);
			glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, filt);
			glUniform1i(u_layer_opaque, 1);
			glUniform1i(u_layer_tex, 0);
			glUniform4f(u_layer_src, q->sx, q->sy, q->sw, q->sh);
			glUniform2f(glGetUniformLocation(prog_layer, "lsz"), q->lw, q->lh);
			draw_rect(u_layer_dst, q->dx, q->dy, q->dw, q->dh);
		}
		/* 2. the traces, channel 4 first as on the CPU */
		for (uint32_t k = 0; k < q->n_jobs; k++) {
			uint32_t ji = q->first_job + k;
			const struct job *j = &f->j[ji];
			float x = q->dx + (j->x - q->sx) * kx, y = q->dy + (j->y - q->sy) * ky, w = j->w * kx, h = j->h * ky;
			int lin = q->filter && (w != j->mw || h != j->mh);
			int shown = 0;
			for (int c = 0; c < NCH; c++)
				shown |= j->map_off[c] != NONE;
			if (!shown || !pre_tex[qs][ji].id)
				continue;
			glActiveTexture(GL_TEXTURE1);
			glBindTexture(GL_TEXTURE_2D, pre_tex[qs][ji].id);
			glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, lin ? GL_LINEAR : GL_NEAREST);
			glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, lin ? GL_LINEAR : GL_NEAREST);
			glActiveTexture(GL_TEXTURE2);
			glBindTexture(GL_TEXTURE_2D, lut_tex[qs][ji].id);
			if (j->graded) {
				glDisable(GL_BLEND);
				glUseProgram(prog_grade);
				set_tsz(prog_grade, tw, th);
				glUniform1i(u_gr_ix, 1);
				glUniform1i(u_gr_lut, 2);
				glUniform2f(u_gr_msz, j->mw, j->mh);
				glUniform1i(u_gr_lin, lin);
				draw_rect(u_gr_job, x, y, w, h);
				continue;
			}
			if (q->under_off == NONE)
				continue;
			glDisable(GL_BLEND);
			glUseProgram(prog_trace);
			set_tsz(prog_trace, tw, th);
			glActiveTexture(GL_TEXTURE0);
			glBindTexture(GL_TEXTURE_2D, layer_tex[qs][qi][0].id);
			glUniform1i(u_tr_under, 0);
			glUniform4f(u_tr_jl, j->x, j->y, j->w, j->h);
			glUniform2f(u_tr_lsz, q->lw, q->lh);
			glUniform1i(u_tr_ix, 1);
			glUniform1i(u_tr_lut, 2);
			glUniform2f(u_tr_msz, j->mw, j->mh);
			glUniform1i(u_tr_lin, lin);
			GLfloat col[NCH * 3];
			for (int c = 0; c < NCH; c++) {
				col[c * 3] = (j->col[c] >> 16 & 255) / 255.0f;
				col[c * 3 + 1] = (j->col[c] >> 8 & 255) / 255.0f;
				col[c * 3 + 2] = (j->col[c] & 255) / 255.0f;
			}
			glUniform3fv(u_tr_col, NCH, col);
			draw_rect(u_tr_job, x, y, w, h);
		}
		/* 3. over the traces: premultiplied */
		if (q->over_off != NONE) {
			glEnable(GL_BLEND);
			glBlendFunc(GL_ONE, GL_ONE_MINUS_SRC_ALPHA);
			glUseProgram(prog_layer);
			set_tsz(prog_layer, tw, th);
			glActiveTexture(GL_TEXTURE0);
			glBindTexture(GL_TEXTURE_2D, layer_tex[qs][qi][1].id);
			glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, filt);
			glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, filt);
			glUniform1i(u_layer_opaque, 0);
			glUniform1i(u_layer_tex, 0);
			glUniform4f(u_layer_src, q->sx, q->sy, q->sw, q->sh);
			glUniform2f(glGetUniformLocation(prog_layer, "lsz"), q->lw, q->lh);
			draw_rect(u_layer_dst, q->dx, q->dy, q->dw, q->dh);
		}
		glDisable(GL_SCISSOR_TEST);
	}
	glFinish();
	double t2 = now_ms();
	if (f->readback || t->memory) {
		size_t n = (size_t)t->w * t->h * 4;
		if (!in_arena(f->readback_off, n))
			return 8;
		/* a scan-out buffer is uncached for the CPU: the GPU copies the picture into a buffer of its own first,
		 * which Mesa reads through a cached staging copy (a row-by-row read of the scan-out buffer took 150 ms) */
		GLuint rfb = t->fb;
		if (!t->memory) {
			if (!rb_read || rb_w != t->w || rb_h != t->h) {
				if (rb_read) {
					glDeleteFramebuffers(1, &fb_read);
					glDeleteRenderbuffers(1, &rb_read);
				}
				glGenRenderbuffers(1, &rb_read);
				glBindRenderbuffer(GL_RENDERBUFFER, rb_read);
				glRenderbufferStorage(GL_RENDERBUFFER, GL_RGBA8, t->w, t->h);
				glGenFramebuffers(1, &fb_read);
				glBindFramebuffer(GL_FRAMEBUFFER, fb_read);
				glFramebufferRenderbuffer(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_RENDERBUFFER, rb_read);
				rb_w = t->w;
				rb_h = t->h;
			}
			glBindFramebuffer(GL_READ_FRAMEBUFFER, t->fb);
			glBindFramebuffer(GL_DRAW_FRAMEBUFFER, fb_read);
			glBlitFramebuffer(0, 0, t->w, t->h, 0, 0, t->w, t->h, GL_COLOR_BUFFER_BIT, GL_NEAREST);
			rfb = fb_read;
		}
		glBindFramebuffer(GL_READ_FRAMEBUFFER, rfb);
		uint32_t *dst = (uint32_t *)(arena + f->readback_off);
		glReadPixels(0, 0, t->w, t->h, GL_RGBA, GL_UNSIGNED_BYTE, dst);
		/* bytes r, g, b, a -> the word 0x00rrggbb */
		for (size_t i = 0; i < (size_t)t->w * t->h; i++) {
			uint32_t v = dst[i];
			dst[i] = (v & 0xff) << 16 | (v & 0xff00) | (v >> 16 & 0xff);
		}
	}
	double t3 = now_ms();
	d->ms_upload = t1 - t0;
	d->ms_draw = t2 - t1;
	d->ms_read = t3 - t2;
	return glGetError() == GL_NO_ERROR ? 0 : 9;
}

static int add_target(const struct target *m, int fd) {
	if (m->index >= MAX_TARGETS || m->w <= 0 || m->h <= 0 || m->w > 8192 || m->h > 8192)
		return 1;
	struct tgt *t = &tgts[m->index];
	if (t->used) {
		glDeleteFramebuffers(1, &t->fb);
		glDeleteRenderbuffers(1, &t->rb);
		if (t->img)
			destroy_image(dpy, t->img);
		memset(t, 0, sizeof *t);
	}
	glGenRenderbuffers(1, &t->rb);
	glBindRenderbuffer(GL_RENDERBUFFER, t->rb);
	if (m->memory) {
		glRenderbufferStorage(GL_RENDERBUFFER, GL_RGBA8, m->w, m->h);
	} else {
		if (fd < 0 || m->fourcc != FOURCC_XRGB8888)
			return 2;
		EGLint at[] = {EGL_WIDTH, m->w, EGL_HEIGHT, m->h, EGL_LINUX_DRM_FOURCC_EXT, (EGLint)m->fourcc,
			       EGL_DMA_BUF_PLANE0_FD_EXT, fd, EGL_DMA_BUF_PLANE0_OFFSET_EXT, 0, EGL_DMA_BUF_PLANE0_PITCH_EXT, m->pitch, EGL_NONE};
		t->img = create_image(dpy, EGL_NO_CONTEXT, EGL_LINUX_DMA_BUF_EXT, NULL, at);
		close(fd);
		if (!t->img) {
			fprintf(stderr, "oos-gpu: dma-buf import of target %u failed (0x%x)\n", m->index, eglGetError());
			return 3;
		}
		image_rb(GL_RENDERBUFFER, t->img);
	}
	glGenFramebuffers(1, &t->fb);
	glBindFramebuffer(GL_FRAMEBUFFER, t->fb);
	glFramebufferRenderbuffer(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_RENDERBUFFER, t->rb);
	if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE)
		return 4;
	t->used = 1;
	t->w = m->w;
	t->h = m->h;
	t->memory = m->memory;
	return 0;
}

/* ------------------------------------------------------------------------------------------------ start */

/* The render node of the Mali (driver "panfrost"), by the name the kernel gives the driver. */
static int find_node(char *path, size_t len) {
	for (int i = 128; i < 136; i++) {
		char n[64];
		snprintf(n, sizeof n, "/sys/class/drm/renderD%d/device/driver", i);
		char drv[256];
		ssize_t k = readlink(n, drv, sizeof drv - 1);
		if (k <= 0)
			continue;
		drv[k] = 0;
		if (strstr(drv, "panfrost")) {
			snprintf(path, len, "/dev/dri/renderD%d", i);
			return 0;
		}
	}
	return -1;
}

static int egl_up(void) {
	PFNEGLGETPLATFORMDISPLAYEXTPROC gpd = (void *)eglGetProcAddress("eglGetPlatformDisplayEXT");
	if (!gpd)
		return -1;
	if (getenv("OOS_GPU_SOFT")) {
		dpy = gpd(EGL_PLATFORM_SURFACELESS_MESA, EGL_DEFAULT_DISPLAY, NULL);
	} else {
		char node[64];
		if (find_node(node, sizeof node)) {
			say("no Panfrost render node");
			return -1;
		}
		/* the device by its render node: EGL_EXT_platform_device */
		PFNEGLQUERYDEVICESEXTPROC qd = (void *)eglGetProcAddress("eglQueryDevicesEXT");
		PFNEGLQUERYDEVICESTRINGEXTPROC qs = (void *)eglGetProcAddress("eglQueryDeviceStringEXT");
		EGLDeviceEXT devs[8];
		EGLint n = 0;
		if (qd && qs && qd(8, devs, &n)) {
			for (int i = 0; i < n && !dpy; i++) {
				const char *f = qs(devs[i], EGL_DRM_RENDER_NODE_FILE_EXT);
				if (f && !strcmp(f, node))
					dpy = gpd(EGL_PLATFORM_DEVICE_EXT, devs[i], NULL);
			}
		}
		if (!dpy) {
			say("no EGL device for the render node");
			return -1;
		}
	}
	EGLint maj, min;
	if (!dpy || !eglInitialize(dpy, &maj, &min))
		return -1;
	eglBindAPI(EGL_OPENGL_ES_API);
	EGLint ca[] = {EGL_CONTEXT_MAJOR_VERSION, 3, EGL_NONE};
	EGLContext ctx = eglCreateContext(dpy, EGL_NO_CONFIG_KHR, EGL_NO_CONTEXT, ca);
	if (ctx == EGL_NO_CONTEXT || !eglMakeCurrent(dpy, EGL_NO_SURFACE, EGL_NO_SURFACE, ctx))
		return -1;
	create_image = (void *)eglGetProcAddress("eglCreateImageKHR");
	destroy_image = (void *)eglGetProcAddress("eglDestroyImageKHR");
	image_rb = (void *)eglGetProcAddress("glEGLImageTargetRenderbufferStorageOES");
	fprintf(stderr, "oos-gpu: %s, %s\n", glGetString(GL_RENDERER), glGetString(GL_VERSION));
	return 0;
}

int main(void) {
	signal(SIGPIPE, SIG_IGN);
	struct hello h;
	int afd = -1;
	if (recv_msg(&h, sizeof h, &afd) != sizeof h || h.magic != MAGIC || h.kind != K_HELLO || afd < 0) {
		say("no hello");
		return 1;
	}
	if (h.version != VERSION) {
		fprintf(stderr, "oos-gpu: protocol %u, this is %u\n", h.version, VERSION);
		send_done(100, 0, 0, 0);
		return 1;
	}
	arena_size = h.arena_size;
	arena = mmap(NULL, arena_size, PROT_READ | PROT_WRITE, MAP_SHARED, afd, 0);
	close(afd);
	if (arena == MAP_FAILED) {
		say("arena not mapped");
		send_done(101, 0, 0, 0);
		return 1;
	}
	if (egl_up()) {
		say("no GPU context");
		send_done(102, 0, 0, 0);
		return 1;
	}
	gl_setup();
	send_done(0, 0, 0, 0);
	/* the largest message is a frame */
	static union {
		uint32_t w[2];
		struct target t;
		struct drop d;
		struct frame f;
	} m;
	for (;;) {
		int fd = -1;
		int n = recv_msg(&m, sizeof m, &fd);
		if (n <= 0)
			return 0;
		if (n < 8 || m.w[0] != MAGIC) {
			say("bad message");
			return 1;
		}
		struct done d = {0};
		uint32_t st;
		switch (m.w[1]) {
		case K_TARGET:
			st = n == sizeof(struct target) ? (uint32_t)add_target(&m.t, fd) : 10;
			break;
		case K_DROP:
			st = 0;
			if (m.d.index < MAX_TARGETS && tgts[m.d.index].used) {
				struct tgt *t = &tgts[m.d.index];
				glDeleteFramebuffers(1, &t->fb);
				glDeleteRenderbuffers(1, &t->rb);
				if (t->img)
					destroy_image(dpy, t->img);
				memset(t, 0, sizeof *t);
			}
			break;
		case K_FRAME:
			st = n == sizeof(struct frame) ? (uint32_t)do_frame(&m.f, &d) : 11;
			break;
		default:
			st = 12;
		}
		if (fd >= 0 && m.w[1] != K_TARGET)
			close(fd);
		if (send_done(st, d.ms_upload, d.ms_draw, d.ms_read))
			return 0;
	}
}
