// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Trigger > Protocol beyond condition and value (`SerialTrig`, docs/fork/serial-ui.md): relation, limit and mask of
//! the value, UART polarity, stop bits and error type, SPI chip-select polarity and timeout framing, I2C direction,
//! address width and data bytes. The TPU's receivers take them (`trig.serial_hw = on`); the software trigger follows
//! as far as the decoders show it.

use super::*;

/// Index `i` of `n` moved by `d`, wrapping.
fn cyc(i: usize, n: usize, d: i32) -> usize {
    (i as i32 + d).rem_euclid(n as i32) as usize
}

/// SPI timeout steps (s): off, then 1-2-5 from 100 ns to 10 ms.
const TIMEOUTS: [f32; 17] = [0.0, 100e-9, 200e-9, 500e-9, 1e-6, 2e-6, 5e-6, 10e-6, 20e-6, 50e-6, 100e-6, 200e-6, 500e-6, 1e-3, 2e-3, 5e-3, 10e-3];

impl Ui {
    /// The settings items of the protocol trigger for the current protocol and condition.
    pub(super) fn serial_items(&self) -> Vec<Item> {
        use Item::*;
        let (m, s) = (&self.m, &self.m.ser);
        let mut v = Vec::new();
        let value = |v: &mut Vec<Item>, rel: Rel, mask: bool| {
            v.push(SerRel);
            if rel.uses_max() {
                v.push(SerMax);
            }
            if mask {
                v.push(SerMask);
            }
        };
        match (m.dec_proto, m.trig_bus) {
            (Proto::Uart, w) => {
                if w == BusWhat::Data {
                    value(&mut v, s.rel, false);
                }
                if w == BusWhat::Error {
                    v.push(UartErr);
                }
                v.extend([UartPol, UartStop]);
            }
            (Proto::Spi, w) => {
                if w == BusWhat::Data {
                    value(&mut v, s.rel, true);
                }
                v.push(if m.dec_cs.is_some() { SpiCs } else { SpiTimeout });
            }
            (Proto::I2c, BusWhat::Id) => {
                value(&mut v, s.addr_rel, false);
                v.extend([I2cAddrBits, I2cDir]);
            }
            (Proto::I2c, BusWhat::Data) => {
                value(&mut v, s.rel, true);
                v.extend([I2cBytes, I2cDir]);
            }
            _ => {}
        }
        v
    }

    /// The relation the value items edit: the address's for an I2C address, else the value's.
    fn rel_mut(&mut self) -> &mut Rel {
        if self.m.dec_proto == Proto::I2c && self.m.trig_bus == BusWhat::Id {
            &mut self.m.ser.addr_rel
        } else {
            &mut self.m.ser.rel
        }
    }

    pub(super) fn serial_value(&self, it: Item) -> String {
        let s = &self.m.ser;
        let i2c_addr = self.m.dec_proto == Proto::I2c && self.m.trig_bus == BusWhat::Id;
        match it {
            Item::SerRel => (if i2c_addr { s.addr_rel } else { s.rel }).name().into(),
            Item::SerMax | Item::SerMask => format!("0x{:X}", self.hex_value(it)),
            Item::UartPol => if s.uart_inv { "Inverted".into() } else { "Normal".into() },
            Item::UartStop => match s.uart_stop {
                StopBits::One => "1".into(),
                StopBits::OneHalf => "1.5".into(),
                StopBits::Two => "2".into(),
            },
            Item::UartErr => match s.uart_err {
                UartErr::Either => "Any".into(),
                UartErr::Parity => "Parity".into(),
                UartErr::Framing => "Framing".into(),
            },
            Item::SpiCs => if s.spi_cs_high { "Active high".into() } else { "Active low".into() },
            Item::SpiTimeout => if s.spi_timeout > 0.0 { si_short(s.spi_timeout as f64, "s") } else { "Off".into() },
            Item::I2cDir => format!("{:?}", s.dir),
            Item::I2cAddrBits => if s.ten_bit { "10-bit".into() } else { "7-bit".into() },
            Item::I2cBytes => if s.bytes == 0 { "Any".into() } else { s.bytes.to_string() },
            _ => String::new(),
        }
    }

    pub(super) fn serial_options(it: Item) -> Vec<String> {
        let v: &[&str] = match it {
            Item::SerRel => return RELS.iter().map(|r| r.name().to_string()).collect(),
            Item::UartPol => &["Normal", "Inverted"],
            Item::UartStop => &["1", "1.5", "2"],
            Item::UartErr => &["Any", "Parity", "Framing"],
            Item::SpiCs => &["Active low", "Active high"],
            Item::I2cDir => &["Either", "Write", "Read"],
            Item::I2cAddrBits => &["7-bit", "10-bit"],
            Item::I2cBytes => &["Any", "1", "2", "3", "4", "5"],
            _ => &[],
        };
        v.iter().map(|x| x.to_string()).collect()
    }

    pub(super) fn serial_adjust(&mut self, it: Item, d: i32) {
        let s = &mut self.m.ser;
        match it {
            Item::SerRel => {
                let r = self.rel_mut();
                let i = RELS.iter().position(|x| x == r).unwrap_or(0);
                *r = RELS[cyc(i, RELS.len(), d)];
            }
            Item::SerMax | Item::SerMask => {
                let v = (self.hex_value(it) as i64 + d as i64).clamp(0, BUS_VAL_MAX as i64) as u64;
                self.set_hex(it, v);
            }
            Item::UartPol => s.uart_inv = !s.uart_inv,
            Item::UartStop => {
                let i = STOP_BITS.iter().position(|x| *x == s.uart_stop).unwrap_or(0);
                s.uart_stop = STOP_BITS[cyc(i, STOP_BITS.len(), d)];
            }
            Item::UartErr => {
                let i = UART_ERRS.iter().position(|x| *x == s.uart_err).unwrap_or(0);
                s.uart_err = UART_ERRS[cyc(i, UART_ERRS.len(), d)];
            }
            Item::SpiCs => s.spi_cs_high = !s.spi_cs_high,
            Item::SpiTimeout => {
                let i = TIMEOUTS.iter().position(|&x| x >= s.spi_timeout).unwrap_or(0) as i32;
                s.spi_timeout = TIMEOUTS[(i + d).clamp(0, TIMEOUTS.len() as i32 - 1) as usize];
            }
            Item::I2cDir => {
                let i = I2C_DIRS.iter().position(|x| *x == s.dir).unwrap_or(0);
                s.dir = I2C_DIRS[cyc(i, I2C_DIRS.len(), d)];
            }
            Item::I2cAddrBits => s.ten_bit = !s.ten_bit,
            Item::I2cBytes => s.bytes = (s.bytes as i32 + d).clamp(0, 5) as u8,
            _ => {}
        }
    }

    /// Items edited on the hex keypad.
    pub(super) fn hex_item(it: Item) -> bool {
        matches!(it, Item::TrigBusVal | Item::SerMax | Item::SerMask)
    }

    /// The value a hex item shows: the condition value, the upper limit (the address's for an I2C address), the mask.
    pub(super) fn hex_value(&self, it: Item) -> u64 {
        let s = &self.m.ser;
        match it {
            Item::SerMax if self.m.dec_proto == Proto::I2c && self.m.trig_bus == BusWhat::Id => s.addr_max as u64,
            Item::SerMax => s.max,
            Item::SerMask => s.mask & BUS_VAL_MAX,
            _ => self.m.trig_bus_val,
        }
    }

    pub(super) fn set_hex(&mut self, it: Item, v: u64) {
        let v = v.min(BUS_VAL_MAX);
        let i2c_addr = self.m.dec_proto == Proto::I2c && self.m.trig_bus == BusWhat::Id;
        let s = &mut self.m.ser;
        match it {
            Item::SerMax if i2c_addr => s.addr_max = v.min(0x3ff) as u32,
            Item::SerMax => s.max = v,
            // all bits of the entry count, above it every bit
            Item::SerMask => s.mask = if v == BUS_VAL_MAX { u64::MAX } else { v },
            _ => self.m.trig_bus_val = v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui_for(p: Proto, what: BusWhat) -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.dec_proto = p;
        ui.set_trig_type(TrigType::Bus);
        ui.m.trig_bus = what;
        ui
    }

    #[test]
    fn items_follow_protocol_condition_and_relation() {
        use Item::*;
        let mut ui = ui_for(Proto::Uart, BusWhat::Data);
        assert_eq!(ui.serial_items(), [SerRel, UartPol, UartStop]);
        ui.select_option(SerRel, "In range");
        assert_eq!((ui.m.ser.rel, ui.serial_items()), (Rel::In, vec![SerRel, SerMax, UartPol, UartStop]));
        ui.m.trig_bus = BusWhat::Error;
        assert_eq!(ui.serial_items(), [UartErr, UartPol, UartStop]);
        let mut ui = ui_for(Proto::Spi, BusWhat::Data);
        ui.m.dec_cs = None;
        assert_eq!(ui.serial_items(), [SerRel, SerMask, SpiTimeout]);
        ui.m.dec_cs = Some(2);
        assert_eq!(ui.serial_items(), [SerRel, SerMask, SpiCs]);
        let mut ui = ui_for(Proto::I2c, BusWhat::Id);
        assert_eq!(ui.serial_items(), [SerRel, I2cAddrBits, I2cDir]);
        // the address has its own relation
        ui.select_option(SerRel, ">");
        assert_eq!((ui.m.ser.addr_rel, ui.m.ser.rel), (Rel::Gt, Rel::Eq));
        ui.m.trig_bus = BusWhat::Data;
        assert_eq!(ui.serial_items(), [SerRel, SerMask, I2cBytes, I2cDir]);
        // the menu shows them right after the value
        let t = ui.trigger_items();
        let k = t.iter().position(|&x| x == TrigBusVal).unwrap();
        assert_eq!(&t[k..k + 5], &[TrigBusVal, SerRel, SerMask, I2cBytes, I2cDir]);
    }

    #[test]
    fn choices_steps_and_hex_entry() {
        let mut ui = ui_for(Proto::Uart, BusWhat::Error);
        for (it, opt, check) in [
            (Item::UartPol, "Inverted", (|u: &Ui| u.m.ser.uart_inv) as fn(&Ui) -> bool),
            (Item::UartStop, "1.5", |u: &Ui| u.m.ser.uart_stop == StopBits::OneHalf),
            (Item::UartErr, "Framing", |u: &Ui| u.m.ser.uart_err == UartErr::Framing),
        ] {
            ui.select_option(it, opt);
            assert!(check(&ui) && ui.value(it) == opt, "{:?}", it);
        }
        let mut ui = ui_for(Proto::Spi, BusWhat::Data);
        ui.m.dec_cs = None;
        assert_eq!(ui.value(Item::SpiTimeout), "Off");
        ui.adjust(Item::SpiTimeout, 4);
        assert_eq!(ui.m.ser.spi_timeout, 1e-6);
        ui.adjust(Item::SpiTimeout, 100);
        assert_eq!(ui.m.ser.spi_timeout, 10e-3);
        // the keypad sets max and mask
        ui.set_hex(Item::SerMask, 0xf0);
        ui.set_hex(Item::SerMax, 0x7f);
        assert_eq!((ui.m.ser.mask, ui.m.ser.max, ui.value(Item::SerMask)), (0xf0, 0x7f, "0xF0".to_string()));
        let mut ui = ui_for(Proto::I2c, BusWhat::Id);
        ui.set_hex(Item::SerMax, 0x7ff);
        assert_eq!(ui.m.ser.addr_max, 0x3ff);
        ui.m.trig_bus = BusWhat::Data;
        ui.adjust(Item::I2cBytes, 9);
        assert_eq!(ui.m.ser.bytes, 5);
        ui.set_hex(Item::TrigBusVal, u64::MAX);
        assert_eq!(ui.m.trig_bus_val, BUS_VAL_MAX);
    }
}
