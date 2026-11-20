// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Saving and recalling: the band the HDMI capture takes, the toast after a save, the USB stick (where files go, the
//! flush before it is pulled, noticing it come and go) and the set-up file with the looks of the references, and
//! recalling it. A new kind of file to save goes next to `Ui::save_dir`.

use super::*;

impl Ui {
    /// The band the external display scales when it shows the waveform: between the top bar and the cards.
    /// HDMI workspace: the waveform area as it is now (one canvas: the band between the bars; with layers: the layer
    /// over the traces, the GPU puts it together itself).
    pub(super) fn capture_hdmi(&mut self, l: &Layers) {
        match l.over.as_deref() {
            None => {
                let cv = &*l.under;
                let body = self.frame().body;
                self.hdmi.snap.clear();
                self.hdmi.snap.extend_from_slice(&cv.px[(body.y * cv.w) as usize..(body.bottom() * cv.w) as usize]);
            }
            Some(o) => {
                let s = self.hdmi.over.get_or_insert_with(|| Canvas::new_layer(o.w, o.h));
                if (s.w, s.h) != (o.w, o.h) {
                    *s = Canvas::new_layer(o.w, o.h);
                }
                s.copy_from(o);
            }
        }
    }

    pub(super) fn toast_saved(&mut self, p: &std::path::Path, r: std::io::Result<()>) {
        match r {
            Ok(()) => {
                self.flush_stick();
                self.toast(&format!("Saved {}{}", crate::store::file_name(p), self.saved_where()));
            }
            Err(e) => self.toast(&format!("Save failed: {}", e)),
        }
    }

    /// A USB stick is mounted and the storage directory was not set by hand: files go to it.
    pub fn usb_used(&self) -> bool {
        self.usb && self.store_dir == std::path::Path::new(crate::store::DEFAULT_DIR)
    }

    /// Where screenshots, CSV, mask copies and fast-frame files go now.
    pub fn save_dir(&self) -> std::path::PathBuf {
        match self.store_dest {
            crate::store::StoreDest::Stick => std::path::PathBuf::from(crate::store::USB_DIR),
            crate::store::StoreDest::Local => self.store_dir.clone(),
            crate::store::StoreDest::Auto => {
                if self.usb_used() {
                    std::path::PathBuf::from(crate::store::USB_DIR)
                } else {
                    self.store_dir.clone()
                }
            }
        }
    }

    /// For the message after saving.
    pub fn saved_where(&self) -> &'static str {
        match self.store_dest {
            crate::store::StoreDest::Local if self.usb => " locally, not on the stick",
            crate::store::StoreDest::Stick => " on the USB stick",
            crate::store::StoreDest::Auto if self.usb_used() => " on the USB stick",
            _ => "",
        }
    }

    /// After a file: on the stick for real, so that it can be pulled. Not in the frame loop, a stick can take its time.
    pub fn flush_stick(&self) {
        if self.usb_used() {
            let _ = std::thread::Builder::new().name("sync".into()).spawn(crate::store::sync_disks);
        }
    }

    /// From `step`: is a stick mounted (`/bin/oos-usb`); says so when that changes.
    pub(super) fn usb_poll(&mut self) {
        if cfg!(test) || self.now < self.usb_check {
            return;
        }
        self.usb_check = self.now + 2.0;
        self.net.wlan_stick = wlan::stick_there();
        self.bt.stick = bt::stick_there();
        self.bt.inputs = bt::inputs_connected(&bt::inputs());
        let on = std::fs::read_to_string("/proc/mounts").is_ok_and(|t| crate::store::usb_mounted(&t));
        if on != self.usb {
            self.usb = on;
            if self.store_dir == std::path::Path::new(crate::store::DEFAULT_DIR) {
                self.toast(if on { "USB stick ready: files are saved to it" } else { "USB stick removed" });
            }
        }
    }

    /// File of the current setup slot: a setup is small and has to survive a power cycle, so the default storage directory
    /// in RAM is not used for it when OOS's data partition is there.
    pub(super) fn setup_path(&self) -> std::path::PathBuf {
        let keep = std::path::Path::new("/oos/setups");
        let dir = if self.store_dir == std::path::Path::new(crate::store::DEFAULT_DIR) && keep.is_dir() {
            keep
        } else {
            self.store_dir.as_path()
        };
        dir.join(format!("setup{}.txt", self.store_slot))
    }

    /// The `ref<N>.*` lines of a setup: how each loaded ref slot looks (on/off, scale, offset, colour, name).
    pub(super) fn ref_setup_text(&self) -> String {
        let looks: Vec<_> = self
            .refs
            .iter()
            .map(|r| {
                r.as_ref().map(|r| crate::store::RefLook {
                    on: Some(r.on),
                    vdiv: Some(r.vpd),
                    offset: Some(r.offset),
                    color: Some(r.color),
                    label: Some(r.label.clone()),
                })
            })
            .collect();
        crate::store::ref_setup_text(&looks)
    }

    /// Applies the saved look to the ref slots that hold a waveform now (the samples are not in a setup).
    fn apply_ref_looks(&mut self, looks: &[Option<crate::store::RefLook>]) {
        for (r, l) in self.refs.iter_mut().zip(looks) {
            let (Some(r), Some(l)) = (r.as_mut(), l) else { continue };
            if let Some(v) = l.on {
                r.on = v;
            }
            if let Some(v) = l.vdiv {
                r.vpd = v;
            }
            if let Some(v) = l.offset {
                r.offset = v;
            }
            if let Some(v) = l.color {
                r.color = v.min(REF_COLORS.len() - 1);
            }
            if let Some(v) = &l.label {
                r.label = v.clone();
            }
        }
    }

    /// Loads setup<slot>.txt of the storage directory; the acquisition state (run / stop) stays as it is.
    pub(super) fn recall_setup(&mut self) {
        let p = self.setup_path();
        // what this build cannot take (a decoder it does not have, a key of a newer build) is left out and named
        let text = std::fs::read_to_string(&p).map_err(|e| e.to_string());
        let parsed = text.as_ref().map_err(|e| e.clone()).map(|t| crate::store::parse_setup_lenient(t));
        match parsed {
            Ok((mut m, bad)) => {
                for e in &bad {
                    eprintln!("oos-ui: recall {}: left out: {}", p.display(), e);
                }
                // a set-up is a live one, as with the Default key: a reference or SIM session ends first, so a Live
                // afterwards does not bring back the set-up from before it over the recalled one
                if self.record_mode() {
                    self.leave_session();
                }
                if let Ok(t) = &text {
                    self.apply_ref_looks(&crate::store::parse_ref_looks(t, NREF));
                }
                (m.run, m.status) = (self.m.run, self.m.status);
                // the text size is a preference of this screen, not part of a set-up
                m.text_size = self.m.text_size;
                if m.mask_src != self.m.mask_src {
                    self.mask = None;
                    self.mask_ref = None;
                }
                m.keep_session(&self.m);
                self.m = m;
                // a set-up written by hand or by an older build may have the two levels the wrong way round
                self.m.order_trigger_levels();
                // what Bus switched on belongs to the set-up before: leaving Bus leaves the recalled one as it is
                self.bus_auto = None;
                self.math_state.borrow_mut().forget_fit();
                self.math_fam = if self.m.math == MathOp::Off { MathFamily::Arith } else { self.m.math.family() };
                self.sync_full();
                if self.mask.is_none() {
                    self.m.mask_on = false;
                }
                self.build_mask();
                self.clamp_zoom();
                self.relayout();
                self.dirty_map = true;
                self.clear_measurement_statistics();
                match bad.first() {
                    None => self.toast(&format!("Recalled {}", crate::store::file_name(&p))),
                    Some(e) => self.toast(&format!(
                        "Recalled {}, {} line{} left out ({})",
                        crate::store::file_name(&p),
                        bad.len(),
                        if bad.len() == 1 { "" } else { "s" },
                        e
                    )),
                }
            }
            Err(e) => self.toast(&format!("Recall failed: {}", e)),
        }
    }
}
