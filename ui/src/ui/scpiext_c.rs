// SPDX-License-Identifier: GPL-2.0-or-later
//! SCPI coverage extensions, tier 3: system date, time and NTP, the network, the remote services, the scope name,
//! backlight and display, session, and the commands that power off, restart, update, roll back, change the boot
//! mode or the clock: those need `CONFIRM` as an argument and are refused without it. See `scpiext.rs`.

use super::scpiext::*;
use super::*;
use crate::scpi::{e as en, kw, num, parse_bool};

pub(super) const T3: &[&str] = &[
    "SYSTem:DATE", "SYSTem:TIME", "SYSTem:TZONe", "SYSTem:NTP:ENABle", "SYSTem:NTP:SERVer", "SYSTem:NTP:STATus", "SYSTem:NTP:SYNC",
    "SYSTem:COMMunicate:LAN:IPADdress", "SYSTem:COMMunicate:LAN:MAC", "SYSTem:COMMunicate:LAN:STATus", "SYSTem:COMMunicate:LAN:RETRy10",
    "SYSTem:WLAN:STATus", "SYSTem:WLAN:SCAN", "SYSTem:WLAN:NETWorks", "SYSTem:WLAN:SAVed", "SYSTem:WLAN:FORGet",
    "SYSTem:REMote:SERVice:SCPI", "SYSTem:REMote:SERVice:VXI11", "SYSTem:REMote:SERVice:TELNet", "SYSTem:REMote:SERVice:WEB", "SYSTem:REMote:SERVice:MDNS",
    "SYSTem:NAME",
    "DISPlay:BACKlight", "DISPlay:HDMI", "DISPlay:TEXTsize", "DISPlay:LAYout",
    "SYSTem:LOCK", "SESSion", "SYSTem:INFO",
    "SYSTem:POWer:OFF", "SYSTem:POWer:RESTart", "SYSTem:UPDate:STATus", "SYSTem:UPDate:ROLLback", "SYSTem:UPDate:INSTall",
    "SYSTem:BOOT", "SYSTem:OVERclock", "SYSTem:STOCk",
];
pub(super) const T3B: &[&str] = &[
    "BUS<n>:SPI:MODE", "BUS<n>:CAN:BAUD", "BUS<n>:CAN:SIGNal", "BUS<n>:CAN:FD:BAUD", "BUS<n>:CAN:FD:ABAUd", "BUS<n>:SENT:TICK", "BUS<n>:SENT:NIBBles",
    "BUS<n>:USB:SPEed", "BUS<n>:OPTion1", "BUS<n>:OPTion2", "BUS<n>:OPTion3", "BUS<n>:OPTion4",
    "BUS:LANE:STATe", "BUS:LANE:LABel", "BUS:LANE:POSition", "BUS:LANE:CLOCk:SOURce", "BUS:LANE:CLOCk:THReshold", "BUS:LANE:DATA:SOURce",
    "BUS:LANE:DATA:THReshold", "BUS:LANE:RW", "DECode:SAVE",
    "LA:DIGital<n>:PATTern", "LA:POD<n>:PRESet", "LA:SIZE",
    "ZONE<n>:STATe", "ZONE<n>:SOURce", "ZONE<n>:CONDition", "ZONE<n>:BOX", "ZONE<n>:POINts", "ZONE<n>:CLEar",
    "SOURce:ARB:LOAD", "SOURce<n>:ARB:LOAD",
    "MEASure:CATalog:ITEM", "MEASure:CATalog:SOURceC", "MEASure:CATalog:SOURceD",
    "CHANnel:ACTive", "SYSTem:QUICk", "DISPlay:LOWer", "DISPlay:TRENd1", "DISPlay:TRENd2",
];

/// Splits the `CONFIRM` token off an argument list: the rest, and whether it was there.
fn confirm(arg: &str) -> (String, bool) {
    let parts: Vec<&str> = arg.split(',').map(str::trim).collect();
    let yes = parts.iter().any(|p| p.eq_ignore_ascii_case("CONFIRM"));
    (parts.into_iter().filter(|p| !p.eq_ignore_ascii_case("CONFIRM")).collect::<Vec<_>>().join(","), yes)
}

fn needs_confirm(what: &str) -> String { conflict(&format!("{} needs the argument CONFIRM", what)) }

impl Ui {
    pub(super) fn ext_c(&mut self, key: &str, _idx: Option<usize>, q: bool, arg: &str) -> Option<Reply> {
        let action = |q: bool| if q { Err(undefined(key)) } else { DONE };
        Some(match key {
            // ---- date, time, NTP: the keyboard's "Set the clock" and the NTP rows of System > Date and time
            "SYSTem:DATE" | "SYSTem:TIME" => {
                let now = self.local_now();
                let date = key.ends_with("DATE");
                if q { return Some(t(if date { format!("{:04},{:02},{:02}", now.y, now.mo, now.d) } else { format!("{:02},{:02},{:02}", now.h, now.mi, now.s) })); }
                let v: Vec<u32> = arg.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                if v.len() != 3 || arg.split(',').count() != 3 { return Some(Err(bad(arg))); }
                let text = if date { format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", v[0], v[1], v[2], now.h, now.mi, now.s) } else { format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", now.y, now.mo, now.d, v[0], v[1], v[2]) };
                if self.time_osk_done(&wlan::OskFor::DateTime, &text) { DONE } else { Err(bad(arg)) }
            }
            "SYSTem:TZONe" => {
                if q { return Some(t(self.sys.tz.clone())); }
                if self.time_osk_done(&wlan::OskFor::Tz, text_arg(arg).as_str()) { DONE } else { Err(bad(arg)) }
            }
            "SYSTem:NTP:ENABle" => return Some(self.sel(Item::NtpOn, q, arg)),
            "SYSTem:NTP:SERVer" => {
                if q { return Some(t(self.sys.ntp_server.clone())); }
                let a = text_arg(arg);
                if a.eq_ignore_ascii_case(crate::sysconf::DHCP) { self.set_server(crate::sysconf::DHCP); DONE }
                else if self.time_osk_done(&wlan::OskFor::NtpServer, &a) { DONE } else { Err(bad(arg)) }
            }
            "SYSTem:NTP:STATus" => if q { t(self.ntp_status()) } else { Err(undefined(key)) },
            "SYSTem:NTP:SYNC" => { self.time_activate(Item::NtpSync); action(q) }
            // ---- network
            "SYSTem:COMMunicate:LAN:IPADdress" => {
                if !q { return Some(Err(undefined(key))); }
                t(self.net_ifs.iter().find(|i| i.0 == "eth0").map(|i| i.1.clone()).unwrap_or_else(|| self.net.clone()))
            }
            "SYSTem:COMMunicate:LAN:MAC" => if q { t(self.net_value(Item::NetMac)) } else { Err(undefined(key)) },
            "SYSTem:COMMunicate:LAN:STATus" => if q { t(self.net_value(Item::NetEth)) } else { Err(undefined(key)) },
            "SYSTem:COMMunicate:LAN:RETRy10" => return Some(self.sel(Item::NetRetry10, q, arg)),
            "SYSTem:WLAN:STATus" => if q { t(self.net_value(Item::NetWlan)) } else { Err(undefined(key)) },
            "SYSTem:WLAN:SCAN" => { self.wlan_scan(); action(q) }
            "SYSTem:WLAN:NETWorks" => if q { t(self.wlan_net_options().join("\n")) } else { Err(undefined(key)) },
            "SYSTem:WLAN:SAVed" => if q { t(self.wlan.link.saved.iter().map(|n| oos_hal::wlan::name(&n.ssid)).collect::<Vec<_>>().join("\n")) } else { Err(undefined(key)) },
            "SYSTem:WLAN:FORGet" => {
                if q { return Some(Err(undefined(key))); }
                let Some(n) = arg.trim().parse::<usize>().ok().filter(|n| *n >= 1 && *n <= self.wlan.link.saved.len()) else { return Some(Err(bad(arg))) };
                if let Some(m) = self.wlan_blocked() { return Some(Err(conflict(m))); }
                self.wlan_forget_pick(n - 1);
                DONE
            }
            // ---- remote services; switching the SCPI service off needs CONFIRM (the command that does it ends this session)
            "SYSTem:REMote:SERVice:SCPI" | "SYSTem:REMote:SERVice:VXI11" | "SYSTem:REMote:SERVice:TELNet" | "SYSTem:REMote:SERVice:WEB" | "SYSTem:REMote:SERVice:MDNS" => {
                let it = match key.rsplit(':').next().unwrap_or("") { "SCPI" => Item::RemScpi, "VXI11" => Item::RemVxi, "TELNet" => Item::RemTelnet, "WEB" => Item::RemWeb, _ => Item::RemMdns };
                let (a, yes) = confirm(arg);
                if !q && it == Item::RemScpi && parse_bool(&a) == Some(false) && !yes { return Some(Err(needs_confirm("Switching SCPI off"))); }
                return Some(self.sel(it, q, &a));
            }
            "SYSTem:NAME" => {
                if q { return Some(t(self.net_name())); }
                if self.remote_osk_done(&text_arg(arg)) { DONE } else { Err(bad(arg)) }
            }
            // ---- display
            "DISPlay:BACKlight" => {
                if q { return Some(t(self.sys.backlight.to_string())); }
                let Some(v) = num(arg).filter(|v| *v >= backlight::MIN as f32 && *v <= 100.0) else { return Some(Err(bad(arg))) };
                self.backlight_set(v.round() as u32);
                DONE
            }
            "DISPlay:HDMI" => return Some(self.sel(Item::Hdmi, q, arg)),
            "DISPlay:TEXTsize" => return Some(self.sel(Item::TextSize, q, arg)),
            "DISPlay:LAYout" => return Some(self.sel(Item::Layout, q, arg)),
            "SYSTem:LOCK" => boolf(q, arg, &mut self.m.touch_lock),
            "SESSion" => {
                if q { return Some(t(if self.session_is_sim { "SIM" } else if self.offline { "REF" } else { "LIVE" })); }
                if kw(arg.trim(), "LIVE") { self.return_to_live(); DONE } else { Err(bad(arg)) }
            }
            "SYSTem:INFO" => if q { t(self.sysinfo_text()) } else { Err(undefined(key)) },
            // ---- the ones that need CONFIRM
            "SYSTem:POWer:OFF" | "SYSTem:POWer:RESTart" => {
                if q { return Some(Err(undefined(key))); }
                let (_, yes) = confirm(arg);
                if !yes { return Some(Err(needs_confirm(if key.ends_with("OFF") { "Power off" } else { "Restart" }))); }
                let ok = self.power_request(if key.ends_with("OFF") { PowerAction::Off } else { PowerAction::Restart });
                if ok { DONE } else { Err(conflict("an update job is running")) }
            }
            "SYSTem:UPDate:STATus" => if q { t(self.update_lines().join("\n")) } else { Err(undefined(key)) },
            "SYSTem:UPDate:ROLLback" => {
                if q { return Some(Err(undefined(key))); }
                if !confirm(arg).1 { return Some(Err(needs_confirm("Roll back"))); }
                self.update_rollback_confirmed();
                DONE
            }
            "SYSTem:UPDate:INSTall" => {
                if q { return Some(Err(undefined(key))); }
                let (rest, yes) = confirm(arg);
                if !yes { return Some(Err(needs_confirm("Update"))); }
                let path = text_arg(&rest);
                let body = match std::fs::read(&path) { Ok(b) => b, Err(e) => return Some(Err(bad(&format!("{}: {}", path, e)))) };
                match self.update_upload(&body) { Ok(_) => DONE, Err(e) => Err(conflict(&e)) }
            }
            "SYSTem:BOOT" => {
                if q { return Some(t(if self.boot_mode() == "Persist" { "PERS" } else { "ONES" })); }
                let (a, yes) = confirm(arg);
                let persist = if kw(a.trim(), "PERSist") { true } else if kw(a.trim(), "ONEShot") { false } else { return Some(Err(bad(arg))) };
                if persist == (self.boot_mode() == "Persist") { return Some(DONE); }
                if !yes { return Some(Err(needs_confirm("Changing the boot mode"))); }
                self.boot_mode_toggle();
                DONE
            }
            "SYSTem:OVERclock" => {
                if q { return Some(t(en(self.clock_next))); }
                let (a, yes) = confirm(arg);
                let Some(hz) = a.trim().parse::<f64>().ok().filter(|h| h.is_finite()) else { return Some(Err(bad(arg))) };
                let steps = board().clock_steps();
                let to = steps.iter().position(|&s| (s - hz).abs() <= s * 1e-3).ok_or_else(|| bad(arg));
                let to = match to { Ok(i) => i, Err(e) => return Some(Err(e)) };
                let from = steps.iter().position(|&s| s == self.clock_next).unwrap_or(0);
                if to == from { return Some(DONE); }
                if !yes { return Some(Err(needs_confirm("Overclock"))); }
                self.overclock_step(to as i32 - from as i32);
                DONE
            }
            "SYSTem:STOCk" => {
                if q { return Some(Err(undefined(key))); }
                if !confirm(arg).1 { return Some(Err(needs_confirm("Back to stock"))); }
                self.stock_tap();
                DONE
            }
            _ => return self.ext_c2(key, _idx, q, arg),
        })
    }
}

impl Ui {
    /// Decoder parameters, the plot bus, logic analyser details, zones, generator file, small rows.
    fn ext_c2(&mut self, key: &str, idx: Option<usize>, q: bool, arg: &str) -> Option<Reply> {
        let action = |q: bool| if q { Err(undefined(key)) } else { DONE };
        Some(match key {
            // ---- the parameters of decoder n: the items of the Decode menu, the decoder made the selected one for the moment
            "BUS<n>:SPI:MODE" | "BUS<n>:CAN:BAUD" | "BUS<n>:CAN:SIGNal" | "BUS<n>:CAN:FD:BAUD" | "BUS<n>:CAN:FD:ABAUd" | "BUS<n>:SENT:TICK" | "BUS<n>:SENT:NIBBles"
            | "BUS<n>:USB:SPEed" | "BUS<n>:OPTion1" | "BUS<n>:OPTion2" | "BUS<n>:OPTion3" | "BUS<n>:OPTion4" => {
                let k = idx.and_then(|n| n.checked_sub(1)).filter(|&k| k < DECODERS)?;
                let it = match key {
                    "BUS<n>:SPI:MODE" => Item::DecMode, "BUS<n>:CAN:BAUD" => Item::DecCanRate, "BUS<n>:CAN:SIGNal" => Item::DecCanH,
                    "BUS<n>:CAN:FD:BAUD" => Item::DecOpt(1), "BUS<n>:CAN:FD:ABAUd" => Item::DecOpt(0), "BUS<n>:SENT:TICK" => Item::DecOpt(0),
                    "BUS<n>:SENT:NIBBles" => Item::DecOpt(1), "BUS<n>:USB:SPEed" => Item::DecUsbSpeed,
                    "BUS<n>:OPTion1" => Item::DecOpt(0), "BUS<n>:OPTion2" => Item::DecOpt(1), "BUS<n>:OPTion3" => Item::DecOpt(2), _ => Item::DecOpt(3),
                };
                let prev = self.m.dec_sel;
                self.m.dec_select(k);
                let r = self.sel(it, q, arg);
                self.m.dec_select(prev);
                if !q && r.is_ok() { self.mark_changed(); }
                r
            }
            // ---- the plot bus (the I2C lane)
            "BUS:LANE:STATe" => return Some(self.sel(Item::BusOn, q, arg)),
            "BUS:LANE:RW" => return Some(self.sel(Item::BusRw, q, arg)),
            "BUS:LANE:CLOCk:SOURce" => return Some(self.sel(Item::BusClk, q, arg)),
            "BUS:LANE:DATA:SOURce" => return Some(self.sel(Item::BusData, q, arg)),
            "BUS:LANE:LABel" => {
                if q { return Some(t(self.m.bus.label.clone())); }
                self.m.bus.label = text_arg(arg).chars().take(12).collect();
                DONE
            }
            "BUS:LANE:POSition" => numf(q, arg, &mut self.m.bus.pos, BUS_POS_MIN, BUS_POS_MAX),
            "BUS:LANE:CLOCk:THReshold" => numf(q, arg, &mut self.m.bus.clk_thr, -1e3, 1e3),
            "BUS:LANE:DATA:THReshold" => numf(q, arg, &mut self.m.bus.data_thr, -1e3, 1e3),
            "DECode:SAVE" => { self.activate(Item::DecExport); action(q) }
            // ---- logic analyser
            "LA:DIGital<n>:PATTern" => {
                let n = idx.filter(|&n| n < 16)?;
                return Some(self.sel(Item::LaPat(n), q, arg));
            }
            "LA:POD<n>:PRESet" => {
                let p = idx.filter(|n| (1..=2).contains(n))? - 1;
                return Some(self.sel(Item::LaThr(p), q, arg));
            }
            "LA:SIZE" => return Some(self.sel(Item::LaHeight, q, arg)),
            // ---- zones
            "ZONE<n>:STATe" | "ZONE<n>:SOURce" | "ZONE<n>:CONDition" | "ZONE<n>:BOX" | "ZONE<n>:POINts" | "ZONE<n>:CLEar" => {
                let k = idx.and_then(|n| n.checked_sub(1)).filter(|&k| k < self.m.zones.len())?;
                let r = self.zone_cmd(k, key, q, arg);
                if !q && matches!(r, Ok(None)) { self.mark_changed(); }
                r
            }
            // ---- generator: arbitrary wave from a file
            "SOURce:ARB:LOAD" | "SOURce<n>:ARB:LOAD" => {
                if q { return Some(Err(undefined(key))); }
                let p = if arg.trim().is_empty() { self.store_dir.join("arb.csv") } else { std::path::PathBuf::from(text_arg(arg)) };
                let v = std::fs::read_to_string(&p).map(|t| board().arb_parse(&t)).unwrap_or_default();
                if self.afg_arb_load(&v) { DONE } else { Err(conflict(&format!("no wave in {}", p.display()))) }
            }
            // ---- measurement catalogue: add a row by id and sources, read it, the sources C and D of the Add page
            "MEASure:CATalog:SOURceC" => return Some(self.sel(Item::CatSourceC, q, arg)),
            "MEASure:CATalog:SOURceD" => return Some(self.sel(Item::CatSourceD, q, arg)),
            "MEASure:CATalog:ITEM" => {
                let parts: Vec<&str> = arg.split(',').map(str::trim).collect();
                let Some(id) = parts.first().and_then(|s| s.parse::<u16>().ok()) else { return Some(Err(bad(arg))) };
                let Some(cat) = super::meascatalog::cat_from(id) else { return Some(Err(bad(arg))) };
                let src = |i: usize, d: WaveId| match parts.get(i) { Some(s) => crate::scpi::parse_wave(s).ok_or_else(|| bad(arg)), None => Ok(d) };
                let (a, b, c, d) = match (src(1, self.meas_src), src(2, self.meas_src_b), src(3, self.cat_c), src(4, self.cat_d)) { (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d), _ => return Some(Err(bad(arg))) };
                if q { return Some(t(en(self.cat_row(id, a, b, c, d).map_or(f64::NAN, |r| r.v)))); }
                let keep = (self.meas_src, self.meas_src_b, self.cat_c, self.cat_d);
                (self.meas_src, self.meas_src_b, self.cat_c, self.cat_d) = (a, b, c, d);
                if !self.cat_added(cat) { self.catalog_add(cat); }
                (self.meas_src, self.meas_src_b, self.cat_c, self.cat_d) = keep;
                DONE
            }
            // ---- small rows
            "CHANnel:ACTive" => {
                let mut v = self.m.active as u32 + 1;
                let r = intf(q, arg, &mut v, 1, n_ch() as u32);
                if !q && r.is_ok() { self.m.active = v as usize - 1; }
                r
            }
            "SYSTem:QUICk" => return Some(self.sel(Item::QuickAct, q, arg)),
            "DISPlay:LOWer" => return Some(self.sel(Item::PaneSel, q, arg)),
            "DISPlay:TRENd1" => return Some(self.sel(Item::TrendSel(0), q, arg)),
            "DISPlay:TRENd2" => return Some(self.sel(Item::TrendSel(1), q, arg)),
            _ => return None,
        })
    }

    fn zone_cmd(&mut self, k: usize, key: &str, q: bool, arg: &str) -> Reply {
        let z = &mut self.m.zones[k];
        let nums = |arg: &str| arg.split(',').map(|s| s.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect::<Option<Vec<f64>>>();
        match key {
            "ZONE<n>:STATe" => boolf(q, arg, &mut z.on),
            "ZONE<n>:SOURce" => {
                if q { return t(format!("CHAN{}", z.src + 1)); }
                match crate::scpi::parse_ch(arg).filter(|&c| c < n_ch()) { Some(c) => { z.src = c; DONE } None => Err(bad(arg)) }
            }
            "ZONE<n>:CONDition" => pick(q, arg, &mut z.inside, &[(true, "INTersect"), (false, "NOTintersect")]),
            "ZONE<n>:BOX" => {
                if q {
                    return if z.pts.len() == 2 { t(format!("{},{},{},{}", en(z.pts[0].0), en(z.pts[0].1), en(z.pts[1].0), en(z.pts[1].1))) } else { Err(conflict("the zone is not a box")) };
                }
                match nums(arg) { Some(v) if v.len() == 4 => { z.pts = vec![(v[0], v[1]), (v[2], v[3])]; DONE } _ => Err(bad(arg)) }
            }
            "ZONE<n>:POINts" => {
                if q { return t(z.pts.iter().map(|p| format!("{},{}", en(p.0), en(p.1))).collect::<Vec<_>>().join(",")); }
                match nums(arg) { Some(v) if v.len() >= 4 && v.len() % 2 == 0 => { z.pts = v.chunks(2).map(|c| (c[0], c[1])).collect(); DONE } _ => Err(bad(arg)) }
            }
            _ => {
                if q { return Err(undefined(key)); }
                *z = crate::zone::Zone { src: z.src, ..Default::default() };
                DONE
            }
        }
    }
}
