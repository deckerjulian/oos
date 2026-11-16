# SPDX-License-Identifier: GPL-2.0-or-later

"""Offline tests of tools/mho-check.py: the parts that decide what is read and how it is judged. No adb, no scope."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


mc = load("oos_mho_check", ROOT / "tools/mho-check.py")


class ReadOnly(unittest.TestCase):
    def test_every_read_gives_the_tool_exactly_one_address(self):
        # pcie2dev writes when it gets two arguments: the loop must hand it one quoted word per call
        cmd = mc.remote_read_command([0x4, 0x4008, 0x20004])
        self.assertEqual(cmd, 'for a in 0x4 0x4008 0x20004; do /rigol/tools/pcie2dev "$a"; done')
        for bad in (-4, 0x4002, 1 << 24, "0x4008 0x1", 4.0):
            with self.assertRaises(ValueError):
                mc.remote_read_command([bad])

    def test_the_lists_hold_only_aligned_addresses_and_no_unique_ids(self):
        addrs = [a for a, _ in mc.STATIC_REGS] + mc.SNAP_REGS
        mc.remote_read_command(addrs)
        # the FPGA DNA would identify the unit; the DHO's registers 0x4000 and 0x1190 are not decoded on the MHO
        for never in (0x4044, 0x4048, 0x4000, 0x1190, 0x2000):
            self.assertNotIn(never, addrs)

    def test_the_serial_number_is_left_out(self):
        self.assertEqual(mc.idn_without_serial("RIGOL TECHNOLOGIES,MHO954,MHO9A0000001,00.01.00.00.26"),
                         "RIGOL TECHNOLOGIES,MHO954,<serial>,00.01.00.00.26")


class Parsing(unittest.TestCase):
    def test_reads(self):
        out = "Memory map to 0x7f00000000\noffset = 0x4008\nRead(0x4008) = #0x40001#\nRead(0x4) = #0xE2072115#\n"
        self.assertEqual(mc.parse_reads(out), {0x4008: 0x40001, 0x4: 0xE2072115})

    def test_version_table(self):
        t = mc.parse_versions("0x1@sparrow3_K160T.bit\r\n0x2@sparrow3_RT688.bit\n\n0xa@sparrow3_K160T_LR.bit\n")
        self.assertEqual(t, {1: "sparrow3_K160T.bit", 2: "sparrow3_RT688.bit", 10: "sparrow3_K160T_LR.bit"})
        self.assertTrue(all(mc.IMAGES[c] == n for c, n in t.items()))

    def test_bar_sizes(self):
        res = ("0x00000000fa000000 0x00000000faffffff 0x0000000000040200\n"
               "0x00000000fb000000 0x00000000fb00ffff 0x0000000000040200\n"
               "0x0000000000000000 0x0000000000000000 0x0000000000000000\n")
        self.assertEqual(mc.bar_sizes(res), [16 << 20, 64 << 10])


class Judging(unittest.TestCase):
    def test_adc_bridge_word(self):
        self.assertTrue(mc.adc_bridge_word(0x0112BEEF)[0])     # ADC 0, write, strobe gone
        self.assertTrue(mc.adc_bridge_word(0x02850000)[0])     # ADC 1, read
        self.assertFalse(mc.adc_bridge_word(0x0512BEEF)[0])    # strobe still set
        self.assertFalse(mc.adc_bridge_word(0x0312BEEF)[0])    # both ADCs
        self.assertIsNone(mc.adc_bridge_word(0)[0])

    def test_preamble(self):
        ok, _ = mc.preamble_check("1,0,1000,1,1e-9,-5e-7,0,1.333333e-05,0,32768", 0.1)
        self.assertTrue(ok)
        ok, text = mc.preamble_check("1,0,1000,1,1e-9,-5e-7,0,4e-3,0,128", 1.0)
        self.assertFalse(ok)
        self.assertIn("250.0 codes", text)
        self.assertFalse(mc.preamble_check("0,0,1000,1,1e-9,0,0,4e-3,0,128", 1.0)[0])
        self.assertFalse(mc.preamble_check("garbage", 1.0)[0])

    def test_fpga_done_is_judged_after_the_load(self):
        # the driver logs DONE when it requests the line, before the image is loaded
        log = ("[    9.594720] finish request gpio PIN_DONE voltage:0\n"
               "[   18.070367] pci 0000:01:00.0: [10ee:7024] type 00 class 0x070001\n")
        status, text = mc.fpga_done(log)
        self.assertEqual(status, "PASS")
        self.assertIn("18.070367", text)
        self.assertEqual(mc.fpga_done("[ 9.5] finish request gpio PIN_DONE voltage:0\n")[0], "FAIL")
        self.assertEqual(mc.fpga_done("[ 9.5] get PIN_DONE voltage:1\n")[0], "PASS")
        self.assertEqual(mc.fpga_done("")[0], "INFO")

    def test_the_depth_is_written_back_as_the_scope_takes_it(self):
        # the query answers the number, the command takes the menu word (the number is refused with -222)
        self.assertEqual(mc.depth_word("1.0000E+04"), "10k")
        self.assertEqual(mc.depth_word("1.0000E+03"), "1k")
        self.assertEqual(mc.depth_word("1.0000E+06"), "1M")
        self.assertEqual(mc.depth_word("2.5000E+07"), "25M")
        self.assertEqual(mc.depth_word("1.0000E+08"), "100M")
        self.assertEqual(mc.depth_word("AUTO"), "AUTO")
        self.assertEqual(mc.depth_word("1500"), "1500")
        self.assertEqual(mc.depth_word("x"), "x")

    def test_settings_compare_as_numbers_or_words(self):
        self.assertTrue(mc.same_setting("2.000000E-6", "2e-06"))
        self.assertTrue(mc.same_setting("CHAN1", "chan1"))
        self.assertFalse(mc.same_setting("1.0000E+06", "1.0000E+04"))
        self.assertIn(":ACQuire:MDEPth", mc.RESTORE)
        self.assertLess(mc.RESTORE.index(":MASK:OPERate"), mc.RESTORE.index(":MASK:ENABle"))

    def test_map_sets_back_everything_it_changes(self):
        for fifty in (False, True):
            back = set(mc.map_restore_list(fifty))
            for label, commands in mc.map_steps(fifty):
                for c in commands:
                    head = c.split()[0]
                    self.assertTrue(head in back or head == ":RUN", f"{label}: {c} is not set back")
            order = mc.map_restore_list(fifty)
            self.assertLess(order.index(":CHANnel1:SCALe"), order.index(":CHANnel1:OFFSet"))
            self.assertLess(order.index(":CHANnel1:SCALe"), order.index(":TRIGger:EDGE:LEVel"))
        self.assertNotIn(":CHANnel1:IMPedance", mc.map_restore_list(False))
        self.assertLess(mc.map_restore_list(True).index(":CHANnel1:IMPedance"),
                        mc.map_restore_list(True).index(":CHANnel1:SCALe"))
        self.assertFalse(any("IMPedance" in c for _, cs in mc.map_steps(False) for c in cs), "50 ohm only on request")

    def test_map_reads_only_configuration_registers(self):
        addrs = [a for a, _ in mc.MAP_REGS]
        mc.remote_read_command(addrs + [mc.ADC_SPI_WORD])
        self.assertEqual(len(addrs), len(set(addrs)))
        for never in (0x4044, 0x4048, 0x4000, 0x1190, 0x2000, 0x3004, 0x4034, 0x4100):
            self.assertNotIn(never, addrs)

    def test_gpio_lines_changes_and_log(self):
        g = mc.gpio_lines("GPIOs 32-63, platform/ff730000.gpio, gpio1:\n"
                          " gpio-32  (fpga_pwr            ) out hi\n gpio-120 (sysfs     ) in  lo\n")
        self.assertEqual(g, {32: "(fpga_pwr) out hi", 120: "(sysfs) in lo"})
        self.assertEqual(mc.changes(g, {32: "(fpga_pwr) out lo", 120: "(sysfs) in lo"}),
                         ["32: (fpga_pwr) out hi -> (fpga_pwr) out lo"])
        new, last = mc.log_after("[   10.5] a\n[   12.0] eth0 aa:bb:cc:dd:ee:ff up\n", 11.0)
        self.assertEqual((new, last), (["[   12.0] eth0 <mac> up"], 12.0))

    def test_channel_mask(self):
        self.assertEqual(mc.channel_mask(0x00420003), 0x3)


if __name__ == "__main__":
    unittest.main()
