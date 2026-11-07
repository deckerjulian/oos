// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

// oos-hw: minimal bring-up tool for the DHO800/DHO900 under OOS (static aarch64 binary).
//
//   oos-hw fpga-boot [addr]     send "\rBOOT\r" + addr (big endian, default 0x400000) to the FPGA boot monitor
//                               on /dev/spidev1.0 (mode 0, 8 bit, 10 MHz) - exactly what Rigol's spi2boot does.
//                               No other monitor command exists in this tool (no flash erase/program).
//   oos-hw rd <off> [count]     read 32-bit FPGA registers from /dev/xdma0_bypass (BAR1, 32 KiB)
//   oos-hw wr <off> <value>     write one register (only offsets 0x1000-0x4fff, see docs/protocols/fpga-boot.md)
//   oos-hw pll-init             program the LMX2582 sample clock PLL (GPIO 3-wire, the stock 1.25 GHz table)
//
// Build: aarch64 gcc -static -O2 -o oos-hw oos-hw.c
#include <fcntl.h>
#include <linux/spi/spidev.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>

#include <linux/gpio.h>
#include <time.h>

#define BAR_SIZE 0x8000

static int die(const char *m) { perror(m); return 1; }

// ---- LMX2582 sample clock PLL, 3-wire bus on GPIOs (protocol of Rigol's spi2pll_lxm2582_gpio.ko) ----------------
// LE low; per byte, MSB first: SDIO = bit, SCLK low, wait, SCLK high, wait (data taken on the rising edge);
// SCLK low after each byte; after the word: LE high pulse (latch), LE low. One write = one 24-bit word.
// Register values: the TICSPro export used by the stock tool spi2pll_lxm2582 (1.25 GHz), highest register first,
// 50 ms apart (as Rigol's own tool), R0 (FCAL_EN) last. Values checked identical to Rigol's. See docs/protocols/pll.md.
static const uint8_t lmx2582_init[][3] = {
    {70,0x00,0x00},{69,0x00,0x00},{68,0x00,0x89},{64,0x00,0x77},{62,0x00,0x00},{61,0x00,0x01},{59,0x00,0x00},
    {48,0x03,0xfc},{47,0x00,0xdf},{46,0x0f,0x63},{45,0x00,0x00},{44,0x00,0x00},{43,0x00,0x00},{42,0x00,0x00},
    {41,0x00,0x04},{40,0x00,0x00},{39,0x82,0x04},{38,0x00,0x32},{37,0x40,0x00},{36,0x0c,0x21},{35,0x02,0x9b},
    {34,0xc3,0xea},{33,0x2a,0x0a},{32,0x21,0x0a},{31,0x06,0x01},{30,0x00,0x34},{29,0x00,0x84},{28,0x29,0x24},
    {25,0x00,0x00},{24,0x05,0x09},{23,0x88,0x42},{22,0x23,0x00},{20,0x01,0x2c},{19,0x09,0x65},{14,0x04,0x20},
    {13,0x40,0x00},{12,0x70,0x01},{11,0x00,0x18},{10,0x10,0xd8},{9,0x03,0x02},{8,0x10,0x84},{7,0x28,0xb2},
    {4,0x19,0x43},{2,0x05,0x00},{1,0x08,0x08},{0,0x22,0x1c},
};

static void usleep_ns(long ns) { struct timespec t = {0, ns}; nanosleep(&t, NULL); }

// find a GPIO line by its device-tree line name on /dev/gpiochip0..9
static int find_line(const char *name, int *chip_fd, unsigned *offset) {
    char path[32];
    for (int c = 0; c < 10; c++) {
        snprintf(path, sizeof path, "/dev/gpiochip%d", c);
        int fd = open(path, O_RDWR);
        if (fd < 0) continue;
        struct gpiochip_info ci;
        if (ioctl(fd, GPIO_GET_CHIPINFO_IOCTL, &ci) == 0) {
            for (unsigned l = 0; l < ci.lines; l++) {
                struct gpioline_info li = {.line_offset = l};
                if (ioctl(fd, GPIO_GET_LINEINFO_IOCTL, &li) == 0 && !strcmp(li.name, name)) {
                    *chip_fd = fd; *offset = l;
                    printf("%-9s = %s line %u%s\n", name, ci.label, l, (li.flags & GPIOLINE_FLAG_KERNEL) ? " (busy!)" : "");
                    return 0;
                }
            }
        }
        close(fd);
    }
    fprintf(stderr, "GPIO line '%s' not found\n", name);
    return -1;
}

static int line_out(const char *name) {
    int chip; unsigned off;
    if (find_line(name, &chip, &off)) return -1;
    struct gpiohandle_request rq = {.lineoffsets = {off}, .flags = GPIOHANDLE_REQUEST_OUTPUT, .lines = 1};
    rq.default_values[0] = 0;
    snprintf(rq.consumer_label, sizeof rq.consumer_label, "oos-hw");
    if (ioctl(chip, GPIO_GET_LINEHANDLE_IOCTL, &rq) < 0) { perror(name); return -1; }
    close(chip);
    return rq.fd;
}

static void set(int h, int v) {
    struct gpiohandle_data d = {.values = {v ? 1 : 0}};
    ioctl(h, GPIOHANDLE_SET_LINE_VALUES_IOCTL, &d);
    usleep_ns(1000);
}

static int pll_init(void) {
    int le = line_out("spi_le"), sclk = line_out("spi_sclk"), sdio = line_out("spi_sdio");
    if (le < 0 || sclk < 0 || sdio < 0) return 1;
    for (size_t w = 0; w < sizeof lmx2582_init / sizeof lmx2582_init[0]; w++) {
        set(le, 0);
        for (int b = 0; b < 3; b++) {
            uint8_t v = b == 0 ? lmx2582_init[w][0] & 0x7f : lmx2582_init[w][b];  // R/W bit 0 = write
            set(sclk, 0);
            for (int i = 7; i >= 0; i--) {
                set(sdio, (v >> i) & 1);
                set(sclk, 0);
                set(sclk, 1);
            }
            set(sclk, 0);
        }
        set(le, 1);
        set(le, 0);
        usleep(50000);   // Rigol's spi2pll_lxm2582 waits 50 ms between words
    }
    printf("LMX2582: %zu words written (1.25 GHz table)\n", sizeof lmx2582_init / sizeof lmx2582_init[0]);
    return 0;
}

static int fpga_boot(uint32_t addr) {
    int fd = open("/dev/spidev1.0", O_RDWR);
    if (fd < 0) return die("open /dev/spidev1.0");
    uint8_t mode = SPI_MODE_0, bits = 8;
    uint32_t speed = 10000000;
    if (ioctl(fd, SPI_IOC_WR_MODE, &mode) < 0) return die("SPI_IOC_WR_MODE");
    if (ioctl(fd, SPI_IOC_WR_BITS_PER_WORD, &bits) < 0) return die("SPI_IOC_WR_BITS_PER_WORD");
    if (ioctl(fd, SPI_IOC_WR_MAX_SPEED_HZ, &speed) < 0) return die("SPI_IOC_WR_MAX_SPEED_HZ");
    const uint8_t cmd[6] = {'\r', 'B', 'O', 'O', 'T', '\r'};
    uint8_t a[4] = {addr >> 24, addr >> 16, addr >> 8, addr};
    if (write(fd, cmd, sizeof cmd) != sizeof cmd) return die("write BOOT");
    if (write(fd, a, sizeof a) != sizeof a) return die("write addr");
    close(fd);
    printf("sent \\rBOOT\\r %02x %02x %02x %02x to the FPGA boot monitor\n", a[0], a[1], a[2], a[3]);
    return 0;
}

static volatile uint32_t *bar(int rw) {
    int fd = open("/dev/xdma0_bypass", rw ? O_RDWR | O_SYNC : O_RDONLY | O_SYNC);
    if (fd < 0) { perror("open /dev/xdma0_bypass"); exit(1); }
    void *p = mmap(NULL, BAR_SIZE, rw ? PROT_READ | PROT_WRITE : PROT_READ, MAP_SHARED, fd, 0);
    if (p == MAP_FAILED) { perror("mmap"); exit(1); }
    return (volatile uint32_t *)p;
}

// ---- AFE (4x RT1642), bit-banged SPI on the SPI2 pins + 4 chip selects (protocol of Rigol's spi2afe_gpio.ko) ------
// mode 0, MSB first, ~5 us half period. write: CS low, {reg, val>>8, val}, CS high.
// read: CS low, {reg|0x80}, then 16 bits sampled after each rising SCLK edge, CS high.
static int afe_sclk = -1, afe_mosi = -1, afe_miso = -1, afe_cs[4] = {-1, -1, -1, -1};

static int line_in(const char *name) {
    int chip; unsigned off;
    if (find_line(name, &chip, &off)) return -1;
    struct gpiohandle_request rq = {.lineoffsets = {off}, .flags = GPIOHANDLE_REQUEST_INPUT, .lines = 1};
    snprintf(rq.consumer_label, sizeof rq.consumer_label, "oos-hw");
    if (ioctl(chip, GPIO_GET_LINEHANDLE_IOCTL, &rq) < 0) { perror(name); return -1; }
    close(chip);
    return rq.fd;
}

static int line_out_hi(const char *name) {   // chip selects idle high
    int chip; unsigned off;
    if (find_line(name, &chip, &off)) return -1;
    struct gpiohandle_request rq = {.lineoffsets = {off}, .flags = GPIOHANDLE_REQUEST_OUTPUT, .lines = 1};
    rq.default_values[0] = 1;
    snprintf(rq.consumer_label, sizeof rq.consumer_label, "oos-hw");
    if (ioctl(chip, GPIO_GET_LINEHANDLE_IOCTL, &rq) < 0) { perror(name); return -1; }
    close(chip);
    return rq.fd;
}

static int afe_open(void) {
    if (afe_sclk >= 0) return 0;
    char n[16];
    afe_sclk = line_out("spi_afe_sclk");
    afe_mosi = line_out("spi_afe_mosi");
    afe_miso = line_in("spi_afe_miso");
    for (int c = 0; c < 4; c++) { snprintf(n, sizeof n, "spi_afe_cs_%d", c); afe_cs[c] = line_out_hi(n); }
    for (int c = 0; c < 4; c++) if (afe_cs[c] < 0) return -1;
    return (afe_sclk < 0 || afe_mosi < 0 || afe_miso < 0) ? -1 : 0;
}

static void afe_set(int h, int v) {
    struct gpiohandle_data d = {.values = {v ? 1 : 0}};
    ioctl(h, GPIOHANDLE_SET_LINE_VALUES_IOCTL, &d);
    usleep_ns(5000);
}

static int afe_get(int h) {
    struct gpiohandle_data d = {0};
    ioctl(h, GPIOHANDLE_GET_LINE_VALUES_IOCTL, &d);
    return d.values[0];
}

static void afe_byte_out(uint8_t v) {
    afe_set(afe_sclk, 0);
    for (int i = 7; i >= 0; i--) { afe_set(afe_mosi, (v >> i) & 1); afe_set(afe_sclk, 0); afe_set(afe_sclk, 1); }
    afe_set(afe_sclk, 0);
}

static uint8_t afe_byte_in(void) {
    uint8_t v = 0;
    afe_set(afe_sclk, 0);
    for (int i = 0; i < 8; i++) { afe_set(afe_sclk, 0); afe_set(afe_sclk, 1); v = (v << 1) | afe_get(afe_miso); }
    afe_set(afe_sclk, 0);
    return v;
}

static int afe_verbose = 1;
static void afe_write(int ch, uint8_t reg, uint16_t val) {
    afe_set(afe_cs[ch], 0);
    afe_byte_out(reg); afe_byte_out(val >> 8); afe_byte_out(val & 0xff);
    afe_set(afe_cs[ch], 1);
    if (afe_verbose) printf("  ch%d w 0x%02x <- 0x%04x\n", ch + 1, reg, val);
}

static uint16_t afe_read(int ch, uint8_t reg) {
    afe_set(afe_cs[ch], 0);
    afe_byte_out(reg | 0x80);
    uint16_t v = afe_byte_in() << 8;
    v |= afe_byte_in();
    afe_set(afe_cs[ch], 1);
    return v;
}

// shadow registers of one channel
struct afe_sh { uint16_t r06, r07, r27, r2a, r2b, e57, e60, e61, e62, e63, e64; };

// reads one efuse word bit by bit through regs 0x2a/0x2b and reg 0 bit 0
static int afe_efuse(int ch, struct afe_sh *s, int addr, uint16_t *out) {
    int idx = ((((addr & 0xf0) + 0x1c0) >> 1) & 0xf8) | (addr & 0xf);
    if (idx < 0x0f || idx > 0x14) return -1;
    s->r2b |= 0x100; afe_write(ch, 0x2b, s->r2b);
    s->r2b |= 0x200; afe_write(ch, 0x2b, s->r2b);
    s->r2a &= ~0x200; afe_write(ch, 0x2a, s->r2a);
    s->r2b = (s->r2b & ~0x70) | (((((addr << 4) + 0x10) >> 4) & 7) << 4); afe_write(ch, 0x2b, s->r2b);
    uint16_t res = 0;
    int bits = idx == 0x14 ? 5 : 16;
    for (int i = 0; i < bits; i++) {
        if (idx == 0x14) { s->r2a = (s->r2a & ~7) | i; afe_write(ch, 0x2a, s->r2a); }
        else { s->r2b = (s->r2b & ~0xf) | i; afe_write(ch, 0x2b, s->r2b); }
        s->r2b &= ~0x1000; afe_write(ch, 0x2b, s->r2b);
        s->r2a &= ~0x200; afe_write(ch, 0x2a, s->r2a);
        res |= (afe_read(ch, 0) & 1) << i;
    }
    *out = res;
    s->r2b &= ~0x100; afe_write(ch, 0x2b, s->r2b);
    s->r2b &= ~0x200; afe_write(ch, 0x2b, s->r2b);
    return 0;
}

// AFE init
static const uint8_t afe_tab_reg[24] = {0x20,0x21,0x22,0x23,0x24,0x25,0x26,0x27,0x28,0x29,0x2a,0x2b,
                                        0x2c,0x2d,0x2e,0x2f,0x30,0x31,0x32,0x33,0x34,0x35,0x36,0x37};
static const uint16_t afe_tab_val[24] = {0x0008,0x0a91,0x5a00,0x1000,0x0000,0x0000,0x3200,0x0044,0x0000,0x0282,0x3c40,0x4c00,
                                         0x000c,0x0000,0x1ff9,0x5980,0x0000,0x0000,0xc000,0x0088,0x0009,0x0000,0x0000,0xc000};
static int afe_init(int ch) {
    struct afe_sh s = {.r06 = 0x1d00, .r07 = 0x8440, .r27 = 0x44, .r2a = 0x3c40, .r2b = 0x4c00,
                       .e57 = 0x5980, .e60 = 0, .e61 = 0, .e62 = 0xc000, .e63 = 0x88, .e64 = 9};
    printf("AFE CH%d init\n", ch + 1);
    for (int i = 0; i < 24; i++) afe_write(ch, afe_tab_reg[i], afe_tab_val[i]);
    afe_write(ch, 0x06, s.r06);
    afe_write(ch, 0x07, s.r07);
    s.r27 |= 0x8080; afe_write(ch, 0x27, s.r27);
    afe_write(ch, 0x28, 0x8000);
    s.r2b |= 0x300; afe_write(ch, 0x2b, s.r2b);
    afe_efuse(ch, &s, 0x57, &s.e57);
    afe_efuse(ch, &s, 0x60, &s.e60);
    afe_efuse(ch, &s, 0x61, &s.e61);
    afe_efuse(ch, &s, 0x62, &s.e62);
    afe_efuse(ch, &s, 0x63, &s.e63);
    afe_efuse(ch, &s, 0x64, &s.e64);
    s.r2b &= ~0x300; afe_write(ch, 0x2b, s.r2b);
    afe_write(ch, 0x2f, s.e57 | 0xc);
    afe_write(ch, 0x30, s.e60 & ~0x70);
    afe_write(ch, 0x31, s.e61);
    afe_write(ch, 0x32, 0x8000 | (s.e62 & 0x3fff));
    afe_write(ch, 0x33, s.e63);
    afe_write(ch, 0x34, s.e64);
    printf("CH%d efuse: 57=%04x 60=%04x 61=%04x 62=%04x 63=%04x 64=%04x\n", ch + 1, s.e57, s.e60, s.e61, s.e62, s.e63, s.e64);
    return 0;
}

// ---- ADC (RT8847IQ) init through the FPGA's ADC SPI master, register 0x1034 ------------------------------------
// ADC init: for i in 0..103 (twice):
//   word = 0x1000000 | (addr[i] & 0x7f) << 16 | (mask[addr[i]] ^ init[i]) & 0xffff,
//   1 ms + 100 us before, 100 us after each write;
//   then reg 0x0f once more: 0x010f0201 (shadow 0x7a01 -> &~0xc00 ^0x7000).
static const uint32_t adc_init_words[104] = {
    0x01000005, 0x01010000, 0x01020000, 0x01030000, 0x01040080, 0x01058080,
    0x010601c3, 0x01070000, 0x01080000, 0x01097700, 0x010a8078, 0x010b0000,
    0x010c3fff, 0x010d520f, 0x010e0078, 0x010f0201, 0x01100000, 0x01110000,
    0x01120000, 0x01130000, 0x01140000, 0x01150000, 0x01160044, 0x01170000,
    0x01180000, 0x01190000, 0x011a0000, 0x011b0000, 0x011c0000, 0x011d0000,
    0x011e0044, 0x011f0000, 0x01200000, 0x01210000, 0x01220000, 0x01230000,
    0x01240000, 0x01250000, 0x01260044, 0x01270000, 0x01280000, 0x01290000,
    0x012a0000, 0x012b0000, 0x012c0000, 0x012d0000, 0x012e0044, 0x012f0000,
    0x01300000, 0x01310000, 0x01320000, 0x01330000, 0x01340000, 0x01350000,
    0x01360044, 0x01370000, 0x01380000, 0x01390000, 0x013a0000, 0x013b0000,
    0x013c0000, 0x013d0000, 0x013e0044, 0x013f0000, 0x01400000, 0x01410000,
    0x01420000, 0x01430000, 0x01440000, 0x01450000, 0x01460044, 0x01470000,
    0x01480000, 0x01490000, 0x014a0000, 0x014b0000, 0x014c0000, 0x014d0000,
    0x014e0044, 0x014f0000, 0x01500810, 0x01510010, 0x01520010, 0x01530010,
    0x01549460, 0x01550004, 0x01560007, 0x01570080, 0x01580000, 0x01590000,
    0x015a0000, 0x015b07de, 0x015c0000, 0x015d0000, 0x015e0000, 0x015f0800,
    0x01600008, 0x01616dff, 0x01620060, 0x01630000, 0x01640000, 0x01650000,
    0x01660001, 0x01671402
};

static int adc_init(void) {
    volatile uint32_t *b = bar(1);
    for (int pass = 0; pass < 2; pass++)
        for (int i = 0; i < 104; i++) {
            usleep(1000); usleep(100);
            b[0x1034 >> 2] = adc_init_words[i];
            usleep(100);
        }
    usleep(100);
    b[0x1034 >> 2] = 0x010f0201;
    usleep(100);
    printf("ADC RT8847IQ: 2 x 104 words + reg 0x0f written via 0x1034\n");
    return 0;
}

// ---- remaining init, values of the stock 1.04 system -----------------------------------------------------------
static void wr(volatile uint32_t *b, uint32_t off, uint32_t v) { b[off >> 2] = v; usleep(10); }

static int spu_init(void) {   // SPU init; 0x1014 = live value of the stock system in STOP (bits 23:20 = 0xf, select 1)
    volatile uint32_t *b = bar(1);
    wr(b, 0x1000, 0x1); wr(b, 0x1000, 0x0); wr(b, 0x1000, 0x0); wr(b, 0x1000, 0x10); wr(b, 0x1000, 0x0);
    wr(b, 0x105c, 0x55555555);
    wr(b, 0x1010, 0x20000000);
    wr(b, 0x1014, 0x14f01301);
    printf("SPU init done\n");
    return 0;
}

static int scu_init(void) {   // SCU init: stop bit set, then cleared together with bit 2
    volatile uint32_t *b = bar(1);
    wr(b, 0x4004, 0x80000000); wr(b, 0x4004, 0x0);
    printf("SCU init done\n");
    return 0;
}

static int ddr_cal(void) {    // DDR skip taps from /rigol/data/cal_ddr.hex of this unit
    volatile uint32_t *b = bar(1);
    wr(b, 0x1000, 0x8);                                    // memory reset bit on
    wr(b, 0x1470, 0x12); wr(b, 0x1474, 0x4d6); wr(b, 0x147c, 0x8a2);
    wr(b, 0x1484, 0x924); wr(b, 0x148c, 0x1ef); wr(b, 0x1494, 0x0);
    wr(b, 0x1000, 0x0);                                    // memory reset bit off
    printf("DDR skip taps 0x12 0x4d6 0x8a2 0x924 0x1ef 0x0 written\n");
    return 0;
}

// lane delays found by the stock system on this unit (log of 2026-11-02)
static int lane_delay[28] = {12,12,13,12,13,12,12,12,12,13,11,11,13,13,12,11,13,13,13,13,13,13,12,11,12,12,12,13};

static int adc_idelay(void) { // load the lane delays; select 1 is part of 0x1014
    volatile uint32_t *b = bar(1);
    for (int lane = 0; lane < 28; lane++) {
        uint32_t d = lane_delay[lane] & 0x1ff;
        uint32_t m = lane < 14 ? ((0x1000u << lane) & 0xffff000) | 0x10000000 : ((0x1000u << (lane - 14)) & 0xffff000) | 0x20000000;
        wr(b, 0x1018, d); wr(b, 0x1018, d | m); wr(b, 0x1018, (d | m) & 0xfff);
    }
    printf("ADC lane delays loaded (28 lanes)\n");
    return 0;
}

static int adc_sync(int d) {  // ADC sync delay: ADC reg 0x06 bits 10:8, base value 0x01c3
    volatile uint32_t *b = bar(1);
    uint32_t v = (0x01c3 & ~0x700) | ((d & 7) << 8);
    usleep(100); b[0x1034 >> 2] = 0x1060000 | v; usleep(100);
    printf("ADC reg 0x06 <- 0x%04x (sync delay %d)\n", v, d & 7);
    return 0;
}

static int adc_meas(int sel) { // ADC measure unit: mean of a core
    volatile uint32_t *b = bar(1);
    uint32_t sh = b[0x1040 >> 2];
    uint32_t cfg = (sh & 0x77fff0e0) | 0x10 | ((sel & 0xf) << 8);
    wr(b, 0x1040, cfg);
    wr(b, 0x1040, cfg | 0x80000000);   // second write: same config + start (bit 31)
    int done = 0;
    for (int i = 0; i < 6 && !done; i++) { if (b[0x1040 >> 2] & 0x40000000) done = 1; else usleep(10000); }
    sh = b[0x1040 >> 2];
    printf("measure %s (0x1040 = 0x%08x)\n", done ? "done" : "NOT done", sh);
    for (int core = 0; core < 8; core++) {
        wr(b, 0x1040, (sh & ~0xf00u) | (core << 8));
        uint32_t a = b[0x1044 >> 2];
        if (a == 0xeeeeeeee) { usleep(1000); a = b[0x1044 >> 2]; }
        uint32_t m = b[0x1048 >> 2];
        printf("  core %d: avg 0x%08x (%u)  hi 0x%05x  lo 0x%05x\n", core, a, a, (m >> 12) & 0xffff0, (m & 0xffff) << 4);
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: oos-hw fpga-boot [addr] | pll-init | rd <off> [count] | wr <off> <val>\n"); return 2; }
    if (!strcmp(argv[1], "pll-init"))
        return pll_init();
    if (!strcmp(argv[1], "adc-init"))
        return adc_init();
    if (!strcmp(argv[1], "spu-init")) return spu_init();
    if (!strcmp(argv[1], "scu-init")) return scu_init();
    if (!strcmp(argv[1], "ddr-cal")) return ddr_cal();
    if (!strcmp(argv[1], "adc-idelay")) return adc_idelay();
    if (!strcmp(argv[1], "adc-sync")) return adc_sync(argc > 2 ? atoi(argv[2]) : 0);
    if (!strcmp(argv[1], "adc-meas")) return adc_meas(argc > 2 ? atoi(argv[2]) : 0);
    if (!strncmp(argv[1], "afe-", 4)) {
        if (afe_open()) return 1;
        if (!strcmp(argv[1], "afe-rd") && argc > 3) {        // afe-rd <ch 1-4> <reg> [count]
            int ch = atoi(argv[2]) - 1, reg = strtoul(argv[3], NULL, 0), n = argc > 4 ? atoi(argv[4]) : 1;
            if (ch < 0 || ch > 3) return 2;
            for (int i = 0; i < n; i++) printf("ch%d r 0x%02x = 0x%04x\n", ch + 1, reg + i, afe_read(ch, reg + i));
            return 0;
        }
        if (!strcmp(argv[1], "afe-init") && argc > 2) {      // afe-init <ch 1-4|all> [-q]
            afe_verbose = !(argc > 3 && !strcmp(argv[3], "-q"));
            if (!strcmp(argv[2], "all")) { for (int c = 0; c < 4; c++) afe_init(c); return 0; }
            int ch = atoi(argv[2]) - 1;
            if (ch < 0 || ch > 3) return 2;
            return afe_init(ch);
        }
        fprintf(stderr, "afe-rd <ch> <reg> [n] | afe-init <ch|all> [-q]\n");
        return 2;
    }
    if (!strcmp(argv[1], "fpga-boot"))
        return fpga_boot(argc > 2 ? strtoul(argv[2], NULL, 0) : 0x400000);
    if (!strcmp(argv[1], "rd") && argc > 2) {
        uint32_t off = strtoul(argv[2], NULL, 0), n = argc > 3 ? strtoul(argv[3], NULL, 0) : 1;
        if (off % 4 || off + 4 * n > BAR_SIZE) { fprintf(stderr, "offset out of range\n"); return 2; }
        volatile uint32_t *b = bar(0);
        for (uint32_t i = 0; i < n; i++) printf("0x%04x = 0x%08x\n", off + 4 * i, b[(off >> 2) + i]);
        return 0;
    }
    if (!strcmp(argv[1], "wr") && argc > 3) {
        uint32_t off = strtoul(argv[2], NULL, 0), v = strtoul(argv[3], NULL, 0);
        if (off % 4 || off < 0x1000 || off >= 0x5000) { fprintf(stderr, "refusing offset 0x%x\n", off); return 2; }
        volatile uint32_t *b = bar(1);
        b[off >> 2] = v;
        printf("0x%04x <- 0x%08x\n", off, v);
        return 0;
    }
    fprintf(stderr, "unknown command\n");
    return 2;
}
