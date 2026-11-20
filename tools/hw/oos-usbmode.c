// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

// oos-usbmode: switch a USB WLAN/Bluetooth stick out of its driver-CD mode (static aarch64 binary).
//
// Realtek combo sticks (TP-Link Archer TX10UB Nano and others) first show up as a mass-storage "DISK" with the Windows
// driver, USB id 0bda:1a2b. The kernel ignores that mode (usb-storage, unusual_devs.h, US_FL_IGNORE_DEVICE) and leaves
// the switch to user space. A SCSI START STOP UNIT with the eject bit, sent in a bulk-only CBW, makes the stick
// re-enumerate with its real ids (WLAN and Bluetooth interfaces). This is what usb_modeswitch's StandardEject does.
//
//   oos-usbmode            switch every stick in a known CD mode; prints one line per stick, exit 0 also with none
//   oos-usbmode list       only print the sticks in a known CD mode
//
// Nothing else is sent; the stick's flash is not written. Unplugging it brings the CD mode back.
//
// Build: aarch64 gcc -static -O2 -o oos-usbmode oos-usbmode.c
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/usbdevice_fs.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>

#define SYS "/sys/bus/usb/devices"

// the driver-CD modes this tool switches: vendor, product
static const struct { unsigned v, p; } cd_modes[] = {
	{ 0x0bda, 0x1a2b }, // Realtek "DISK" (RTL8851BU, RTL8852BU, RTL8821CU ... combo sticks)
};

static int read_hex(const char *dir, const char *name, unsigned *out) {
	char path[512], buf[32];
	snprintf(path, sizeof path, "%s/%s", dir, name);
	FILE *f = fopen(path, "r");
	if (!f) return -1;
	int ok = fgets(buf, sizeof buf, f) != NULL;
	fclose(f);
	if (!ok) return -1;
	*out = (unsigned)strtoul(buf, NULL, 16);
	return 0;
}

static int read_dec(const char *dir, const char *name, unsigned *out) {
	char path[512], buf[32];
	snprintf(path, sizeof path, "%s/%s", dir, name);
	FILE *f = fopen(path, "r");
	if (!f) return -1;
	int ok = fgets(buf, sizeof buf, f) != NULL;
	fclose(f);
	if (!ok) return -1;
	*out = (unsigned)strtoul(buf, NULL, 10);
	return 0;
}

static int read_str(const char *dir, const char *name, char *out, size_t n) {
	char path[512];
	snprintf(path, sizeof path, "%s/%s", dir, name);
	FILE *f = fopen(path, "r");
	if (!f) return -1;
	int ok = fgets(out, (int)n, f) != NULL;
	fclose(f);
	if (!ok) return -1;
	out[strcspn(out, "\n")] = 0;
	return 0;
}

// the bulk endpoints of interface 0 (`<dev>:1.0/ep_XX`), from sysfs
static int bulk_eps(const char *dev, unsigned *ep_out, unsigned *ep_in) {
	char ifdir[512];
	snprintf(ifdir, sizeof ifdir, "%s/%s:1.0", SYS, dev);
	DIR *d = opendir(ifdir);
	if (!d) return -1;
	struct dirent *e;
	*ep_out = *ep_in = 0;
	while ((e = readdir(d))) {
		if (strncmp(e->d_name, "ep_", 3)) continue;
		char ep[800], type[16], dir[8];
		unsigned addr;
		snprintf(ep, sizeof ep, "%s/%s", ifdir, e->d_name);
		if (read_str(ep, "type", type, sizeof type) || strcmp(type, "Bulk")) continue;
		if (read_str(ep, "direction", dir, sizeof dir) || read_hex(ep, "bEndpointAddress", &addr)) continue;
		if (!strcmp(dir, "out")) *ep_out = addr;
		else if (!strcmp(dir, "in")) *ep_in = addr;
	}
	closedir(d);
	return *ep_out && *ep_in ? 0 : -1;
}

static int bulk(int fd, unsigned ep, void *data, unsigned len) {
	struct usbdevfs_bulktransfer b = { .ep = ep, .len = len, .timeout = 1000, .data = data };
	return ioctl(fd, USBDEVFS_BULK, &b);
}

// one SCSI command without data in a bulk-only CBW, then its CSW (errors there are fine: the stick may be gone)
static int scsi(int fd, unsigned ep_out, unsigned ep_in, uint32_t tag, const uint8_t *cdb, unsigned cdb_len) {
	uint8_t cbw[31] = { 'U', 'S', 'B', 'C' };
	memcpy(cbw + 4, &tag, 4); // little endian on aarch64
	cbw[14] = (uint8_t)cdb_len;
	memcpy(cbw + 15, cdb, cdb_len);
	if (bulk(fd, ep_out, cbw, sizeof cbw) != (int)sizeof cbw) return -1;
	uint8_t csw[13];
	(void)bulk(fd, ep_in, csw, sizeof csw);
	return 0;
}

static int eject(const char *dev, unsigned bus, unsigned num) {
	unsigned ep_out, ep_in;
	if (bulk_eps(dev, &ep_out, &ep_in)) {
		fprintf(stderr, "oos-usbmode: %s: no bulk endpoints\n", dev);
		return -1;
	}
	char node[64];
	snprintf(node, sizeof node, "/dev/bus/usb/%03u/%03u", bus, num);
	int fd = open(node, O_RDWR);
	if (fd < 0) {
		fprintf(stderr, "oos-usbmode: %s: %s\n", node, strerror(errno));
		return -1;
	}
	unsigned intf = 0;
	if (ioctl(fd, USBDEVFS_CLAIMINTERFACE, &intf) < 0) {
		fprintf(stderr, "oos-usbmode: %s: claim: %s\n", node, strerror(errno));
		close(fd);
		return -1;
	}
	static const uint8_t allow_removal[6] = { 0x1e, 0, 0, 0, 0, 0 }; // PREVENT ALLOW MEDIUM REMOVAL: allow
	static const uint8_t start_stop_eject[6] = { 0x1b, 0, 0, 0, 0x02, 0 }; // START STOP UNIT: LoEj=1, Start=0
	int r = scsi(fd, ep_out, ep_in, 0x4f4f5301, allow_removal, sizeof allow_removal);
	if (r == 0) r = scsi(fd, ep_out, ep_in, 0x4f4f5302, start_stop_eject, sizeof start_stop_eject);
	(void)ioctl(fd, USBDEVFS_RELEASEINTERFACE, &intf);
	close(fd);
	if (r) fprintf(stderr, "oos-usbmode: %s: eject not sent: %s\n", dev, strerror(errno));
	return r;
}

int main(int argc, char **argv) {
	int list = argc > 1 && !strcmp(argv[1], "list");
	if (argc > 1 && !list) {
		fprintf(stderr, "usage: oos-usbmode [list]\n");
		return 2;
	}
	DIR *d = opendir(SYS);
	if (!d) return 0;
	int fail = 0;
	struct dirent *e;
	while ((e = readdir(d))) {
		if (e->d_name[0] == '.' || strchr(e->d_name, ':')) continue;
		char dir[512];
		unsigned v, p, bus, num;
		snprintf(dir, sizeof dir, "%s/%s", SYS, e->d_name);
		if (read_hex(dir, "idVendor", &v) || read_hex(dir, "idProduct", &p)) continue;
		if (read_dec(dir, "busnum", &bus) || read_dec(dir, "devnum", &num)) continue;
		for (size_t i = 0; i < sizeof cd_modes / sizeof cd_modes[0]; i++) {
			if (cd_modes[i].v != v || cd_modes[i].p != p) continue;
			if (list) {
				printf("%s %04x:%04x driver-CD mode\n", e->d_name, v, p);
			} else if (eject(e->d_name, bus, num) == 0) {
				printf("oos-usbmode: %s %04x:%04x switched out of its driver-CD mode\n", e->d_name, v, p);
			} else {
				fail = 1;
			}
		}
	}
	closedir(d);
	return fail;
}
