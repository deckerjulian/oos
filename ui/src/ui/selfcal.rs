// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Self-calibration page (System > Self-cal): channels, AFE zero on/off, start (pressed twice: disconnect all inputs
//! first), cancel, progress and result. The work runs in the acquisition thread (`hw.rs`, `oos_hal::dho::selfcal`); the
//! results go to OOS's own files in /oos/cal, never to /rigol/data.

use super::*;

/// Minutes the scope should run before a calibration.
const WARM_UP_MIN: f64 = 30.0;

/// UI state of the page.
#[derive(Default)]
pub struct State {
    /// 0 = all channels, 1..4 = CHn
    pub ch: usize,
    pub zero: bool,
    /// ADC core gain/offset calibration first (all inputs of the set disconnected)
    pub adc: bool,
    /// Start was pressed once at this UI time (the second press within 10 s starts)
    pub armed: Option<f64>,
    pub last: Option<String>,
}

impl Ui {
    fn cal_shared(&self) -> Option<crate::hw::HwShared> {
        match &self.src {
            Source::Hw { shared, .. } => Some(shared.clone()),
            _ => None,
        }
    }

    pub(super) fn cal_items(&self) -> Vec<Item> {
        // Start while nothing runs, Cancel only while it does
        let run = self.cal_running();
        vec![Item::CalCh, Item::CalAdc, Item::CalZero, if run { Item::CalCancel } else { Item::CalStart }, Item::CalStatus]
    }

    /// Text about the warm-up for the dialog before the start.
    fn cal_warmup(minutes: f64) -> String {
        if minutes < WARM_UP_MIN {
            format!("NOT WARM YET: on for {:.0} of {:.0} minutes. The result will drift.", minutes, WARM_UP_MIN)
        } else {
            format!("Warmed up: on for {:.0} minutes.", minutes)
        }
    }

    pub(super) fn cal_kind(it: Item) -> Option<Kind> {
        Some(match it {
            // the status is plain text (`Ctl::Text`)
            Item::CalCh | Item::CalZero | Item::CalAdc | Item::CalStatus => Kind::Choice,
            Item::CalStart | Item::CalCancel => Kind::Action,
            _ => return None,
        })
    }

    pub(super) fn cal_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::SelfCal) => "Self-cal",
            Item::CalCh => "Channels",
            Item::CalZero => "AFE zero",
            Item::CalAdc => "ADC cores",
            Item::CalStart => "Start",
            Item::CalCancel => "Cancel",
            Item::CalStatus => "Status",
            _ => return None,
        })
    }

    pub(super) fn cal_options(&self, it: Item) -> Option<Vec<String>> {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
        Some(match it {
            Item::CalCh => v(&["All", "CH1", "CH2", "CH3", "CH4"]),
            Item::CalZero | Item::CalAdc => v(&["Off", "On"]),
            _ => return None,
        })
    }

    pub(super) fn cal_value(&self, it: Item) -> Option<String> {
        Some(match it {
            Item::CalCh => if self.cal.ch == 0 { "All".into() } else { format!("CH{}", self.cal.ch) },
            Item::CalZero => if self.cal.zero { "On".into() } else { "Off".into() },
            Item::CalAdc => if self.cal.adc { "On".into() } else { "Off".into() },
            Item::CalStart | Item::CalCancel => String::new(),
            Item::CalStatus => self.cal_status(),
            _ => return None,
        })
    }

    /// A self-calibration is requested or running.
    pub(super) fn cal_running(&self) -> bool {
        self.cal_shared().is_some_and(|sh| {
            let g = sh.lock().unwrap();
            g.cal_progress.is_some() || g.cal_req.is_some()
        })
    }

    /// Progress or the last result.
    pub(super) fn cal_status(&self) -> String {
        let Some(sh) = self.cal_shared() else { return "needs the hardware source".into() };
        let g = sh.lock().unwrap();
        if let Some((d, t, s)) = &g.cal_progress {
            return format!("{:.0} % {}", 100.0 * *d as f64 / (*t).max(1) as f64, s);
        }
        if g.cal_req.is_some() {
            return "starting ...".into();
        }
        self.cal.last.clone().unwrap_or_else(|| "idle".into())
    }

    pub(super) fn cal_adjust(&mut self, it: Item, d: i32) -> bool {
        match it {
            Item::CalCh => self.cal.ch = (self.cal.ch as i32 + d).rem_euclid(5) as usize,
            Item::CalZero => self.cal.zero = !self.cal.zero,
            Item::CalAdc => self.cal.adc = !self.cal.adc,
            Item::CalStatus => {}
            _ => return false,
        }
        true
    }

    pub(super) fn cal_activate(&mut self, it: Item) -> bool {
        match it {
            Item::CalStart => {
                let Some(sh) = self.cal_shared() else {
                    self.toast("Self-cal needs the hardware source");
                    return true;
                };
                if self.cal.armed.is_some_and(|t| self.now - t < 10.0) {
                    self.cal.armed = None;
                    let chans: Vec<usize> = if self.cal.ch == 0 { (0..NCH).collect() } else { vec![self.cal.ch - 1] };
                    let mut g = sh.lock().unwrap();
                    (g.cal_req, g.cal_result, g.cal_cancel) = (Some((chans, self.cal.zero, self.cal.adc)), None, false);
                    drop(g);
                    self.toast("Self-cal started");
                } else {
                    self.cal.armed = Some(self.now);
                    self.dialog = Some(vec![
                        "Self-calibration".into(),
                        "Disconnect ALL inputs (no probes, no cables).".into(),
                        Self::cal_warmup(std::fs::read_to_string("/proc/uptime").ok().and_then(|t| t.split(' ').next()?.parse::<f64>().ok()).unwrap_or(self.now) / 60.0),
                        "Takes about 5 minutes per channel and pass (4 passes).".into(),
                        "The result goes to /oos/cal; /rigol/data stays as it is.".into(),
                        "Press Start again within 10 s to begin.".into(),
                    ]);
                }
            }
            Item::CalCancel => {
                if let Some(sh) = self.cal_shared() {
                    sh.lock().unwrap().cal_cancel = true;
                    self.toast("Cancelling self-cal ...");
                }
            }
            _ => return false,
        }
        true
    }

    /// SCPI: the status line of the self-calibration.
    pub fn cal_status_text(&self) -> String {
        self.cal_status()
    }

    /// Remote test hook (`/tmp/oos-ui.ctl`: `selfcal 0..4 [zero]`, `selfcal cancel`): starts without the dialog.
    pub fn selfcal_remote(&mut self, arg: &str, zero: bool) {
        self.selfcal_remote_full(arg, zero, false)
    }

    /// `selfcal_remote` with the ADC core calibration.
    pub fn selfcal_remote_full(&mut self, arg: &str, zero: bool, adc: bool) {
        self.cal.adc = adc;
        if arg == "cancel" {
            self.cal_activate(Item::CalCancel);
            return;
        }
        self.cal.ch = arg.parse::<usize>().unwrap_or(0).min(4);
        self.cal.zero = zero;
        self.cal.armed = Some(self.now);
        self.cal_activate(Item::CalStart);
    }

    /// Per frame: pick up a finished run.
    pub(super) fn cal_poll(&mut self) {
        let Some(sh) = self.cal_shared() else { return };
        let r = sh.lock().unwrap().cal_result.take();
        if let Some(r) = r {
            let s = match r {
                Ok(s) => format!("done: {}", s),
                Err(e) => format!("failed: {}", e),
            };
            self.toast(&format!("Self-cal {}", s.split(':').next().unwrap_or("")));
            self.cal.last = Some(s);
        }
    }

    /// Progress banner over the plot while a run is active.
    pub(super) fn draw_cal_banner(&self, cv: &mut Canvas) {
        let Some(sh) = self.cal_shared() else { return };
        let p = sh.lock().unwrap().cal_progress.clone();
        let Some((d, t, s)) = p else { return };
        let a = self.area;
        let f = &self.fonts;
        let txt = format!("Self-cal {:.0} %  {}  (inputs disconnected; Cancel in System > Self-cal)", 100.0 * d as f64 / t.max(1) as f64, s);
        let r = Rect::new(a.x + 20, a.y + a.h / 2 - 20, a.w - 40, 40);
        cv.round(r, 6, rgb(0x30, 0x22, 0x08), 240);
        let bar = Rect::new(r.x, r.y + r.h - 4, (r.w as f64 * d as f64 / t.max(1) as f64) as i32, 4);
        cv.fill(bar, ACCENT);
        cv.text_in(&f.sb, r, 1, &txt, TEXT);
    }
}
