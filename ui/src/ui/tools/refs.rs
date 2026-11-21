// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The reference slots as the menus and SCPI see them: their files (export, import, load), and per slot whether
//! it is shown, its source, scale, offset, colour and label. A new setting of a reference slot goes here.

use super::*;

/// The Reference page as the screen keeps it (ADR 0062).
#[derive(Default)]
pub(in crate::ui) struct RefPage {
    /// the channel Save copies into the slot while the slot is empty (Item::RefSrc, `:REFerence:SOURce`)
    pub src: usize,
    /// the files the Import slot picker is showing
    pub files: Vec<std::path::PathBuf>,
}

impl Ui {
    /// File of a reference slot, beside the setups.
    fn ref_path(&self) -> std::path::PathBuf {
        self.setup_path().with_file_name(format!("ref{}.csv", self.ref_sel + 1))
    }

    /// Waveforms Import slot can open: the slot's own files, then Storage. Same extensions as `load`.
    fn ref_wave_candidates(&self) -> Vec<std::path::PathBuf> {
        let slot = self.ref_sel + 1;
        let mut v = Vec::new();
        let mut add = |p: std::path::PathBuf| {
            if p.is_file() && !v.iter().any(|q| q == &p) {
                v.push(p);
            }
        };
        if let Some(dir) = self.setup_path().parent() {
            for ext in crate::store::WAVE_EXTS {
                add(dir.join(format!("ref{}.{ext}", slot)));
            }
        }
        if let Ok(rd) = std::fs::read_dir(self.save_dir()) {
            let mut files: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            files.sort();
            for p in files {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                if crate::store::WAVE_EXTS.iter().any(|e| *e == ext) {
                    add(p);
                }
            }
        }
        v
    }

    /// The reference of the Slot row as CSV: Time (s),Volts.
    pub(in crate::ui) fn ref_export(&mut self) {
        let Some(r) = &self.refs[self.ref_sel] else { return self.toast("That slot is empty") };
        let mut t = String::from("Time (s),Volts\n");
        for (k, v) in r.v.iter().enumerate() {
            t.push_str(&format!("{:.12e},{}\n", r.t0 + k as f64 * r.dt, v));
        }
        let p = self.ref_path();
        let res = crate::store::write(&p, t.as_bytes());
        self.toast_saved(&p, res);
    }

    /// Opens a captured waveform into the selected R slot.
    pub(in crate::ui) fn ref_import(&mut self) {
        let slot = self.ref_sel + 1;
        if let Some(dir) = self.setup_path().parent() {
            for ext in crate::store::WAVE_EXTS {
                let p = dir.join(format!("ref{}.{ext}", slot));
                if p.is_file() {
                    return self.ref_load_path(&p);
                }
            }
        }
        self.ref_page.files = self.ref_wave_candidates();
        match self.ref_page.files.len() {
            0 => self.toast("No CSV, BIN, WFM, TRC, ISF, TR0 or WFMX in storage"),
            1 => {
                let p = self.ref_page.files[0].clone();
                self.ref_load_path(&p);
            }
            _ => self.open_picker(Item::RefImport),
        }
    }

    /// Reference > Load file: a waveform file from storage into the Slot row's reference, as Import slot does.
    pub(in crate::ui) fn ref_load(&mut self) {
        self.ref_import();
    }

    pub(in crate::ui) fn ref_load_path(&mut self, path: &std::path::Path) {
        match self.ref_load_file(path) {
            Ok(()) => self.toast(&format!("R{} loaded from {}", self.ref_sel + 1, crate::store::file_name(path))),
            Err(error) => self.toast(&error),
        }
    }

    /// The Slot row: R1 is 0.
    pub(crate) fn ref_slot(&self) -> usize {
        self.ref_sel
    }

    pub(crate) fn ref_slot_set(&mut self, slot: usize) -> bool {
        if slot >= NREF {
            return false;
        }
        self.ref_sel = slot;
        true
    }

    /// Whether the selected slot is drawn. None when it is empty.
    #[cfg(test)]
    pub(crate) fn ref_shown(&self) -> Option<bool> {
        self.refs.get(self.ref_sel).and_then(|r| r.as_ref()).map(|r| r.on)
    }

    pub(crate) fn ref_show_set(&mut self, on: bool) -> bool {
        let Some(r) = self.refs.get_mut(self.ref_sel).and_then(|r| r.as_mut()) else { return false };
        r.on = on;
        true
    }

    /// Whether slot `slot` is drawn. None when it is empty or out of range.
    pub(crate) fn ref_shown_slot(&self, slot: usize) -> Option<bool> {
        self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.on)
    }

    pub(crate) fn ref_show_slot(&mut self, slot: usize, on: bool) -> bool {
        if self.refs.get(slot).is_none_or(Option::is_none) {
            return false;
        }
        self.ref_sel = slot;
        self.ref_show_set(on)
    }

    /// Channel a `:REFerence:SAVE` of this slot copies. A saved slot remembers its own source.
    pub(crate) fn ref_source_of(&self, slot: usize) -> usize {
        self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.src).unwrap_or(if slot == self.ref_sel {
            self.ref_page.src
        } else {
            0
        })
    }

    pub(crate) fn ref_source_set(&mut self, slot: usize, ch: usize) -> bool {
        if slot >= NREF || ch >= n_ch() {
            return false;
        }
        self.ref_sel = slot;
        self.ref_page.src = ch;
        if let Some(r) = self.refs.get_mut(slot).and_then(|r| r.as_mut()) {
            r.src = ch;
        }
        true
    }

    pub(crate) fn ref_vscale(&self, slot: usize) -> Option<f32> {
        self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.vpd)
    }

    pub(crate) fn ref_vscale_set(&mut self, slot: usize, vpd: f32) -> bool {
        self.ref_edit(slot, vpd.is_finite() && vpd > 0.0, |r| r.vpd = vpd)
    }

    pub(crate) fn ref_voffset(&self, slot: usize) -> Option<f32> {
        self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.offset)
    }

    pub(crate) fn ref_voffset_set(&mut self, slot: usize, offset: f32) -> bool {
        self.ref_edit(slot, offset.is_finite(), |r| r.offset = offset)
    }

    pub(crate) fn ref_color(&self, slot: usize) -> Option<usize> {
        self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.color)
    }

    pub(crate) fn ref_color_set(&mut self, slot: usize, color: usize) -> bool {
        self.ref_edit(slot, color < REF_COLORS.len(), |r| r.color = color)
    }

    /// `:REFerence:RESet`: offset back to zero, scale back to the source channel.
    pub(crate) fn ref_reset_scale(&mut self, slot: usize) -> bool {
        if slot >= NREF {
            return false;
        }
        let Some(src) = self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.src) else { return false };
        let vpd = self.m.ch[src.min(n_ch() - 1)].volts_per_div();
        let Some(r) = self.refs.get_mut(slot).and_then(|r| r.as_mut()) else { return false };
        r.offset = 0.0;
        r.vpd = vpd.max(1e-9);
        true
    }

    pub(crate) fn ref_label(&self, slot: usize) -> Option<String> {
        self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.label.clone())
    }

    pub(crate) fn ref_label_set(&mut self, slot: usize, label: String) -> bool {
        self.ref_edit(slot, true, |r| r.label = label)
    }

    /// With a `valid` value and a reference in `slot`: selects the slot, changes the reference with `set`, true.
    fn ref_edit(&mut self, slot: usize, valid: bool, set: impl FnOnce(&mut RefWave)) -> bool {
        if slot >= NREF || !valid {
            return false;
        }
        let Some(r) = self.refs.get_mut(slot).and_then(|r| r.as_mut()) else { return false };
        self.ref_sel = slot;
        set(r);
        true
    }

    /// Empties every reference slot, the same as Clear all.
    pub(crate) fn ref_clear_all(&mut self) {
        self.refs = std::array::from_fn(|_| None);
        // readouts of the cleared slots go with them: an import adds Vpp, Freq, RMS and Rise of its first slot, and those
        // rows stayed as "----" after the clear (unit 2026-11-16)
        let is_ref = |w: &WaveId| matches!(w, WaveId::Ref(_));
        self.m.meas.retain(|(_, w)| !is_ref(w));
        self.m.meas2.retain(|(_, a, b)| !is_ref(a) && !is_ref(b));
        if is_ref(&self.sources.a) {
            self.sources.a = WaveId::Ch(0);
        }
        if is_ref(&self.sources.b) {
            self.sources.b = WaveId::Ch(0);
        }
        self.ensure_meas_own();
        self.measure_refs();
        self.analyzer_update();
    }

    /// Writes the Source channel into the selected slot.
    pub(crate) fn ref_save_slot(&mut self) -> Result<(), String> {
        let c = self.ref_page.src;
        let rec = self.last[0].clone();
        let Some(r) = rec.as_ref().filter(|r| c < r.ch.len() && !r.ch[c].is_empty()) else {
            return Err("source channel has no data".into());
        };
        let (v, dt) = self.meas_wave(r, c);
        if v.len() < 2 || !(dt > 0.0) {
            return Err("source channel has no data".into());
        }
        let w = self.ref_of_channel(r, c, v, dt);
        self.ref_put(self.ref_sel, w);
        Ok(())
    }

    /// The reference of channel `c` of record `r`: its samples `v` at interval `dt` (`meas_wave`), starting where the
    /// record or, for a raw read longer than the screen's, the raw record starts; the channel's scale and offset.
    pub(in crate::ui) fn ref_of_channel(&self, r: &Record, c: usize, v: Vec<f32>, dt: f64) -> RefWave {
        let t0 = if v.len() > r.ch[c].len() { r.raw.as_ref().map(|x| x.t0).unwrap_or(r.t0) } else { r.t0 };
        RefWave {
            src: c,
            color: 0,
            label: String::new(),
            t0,
            dt,
            v: v.into(),
            vpd: self.m.ch[c].volts_per_div(),
            offset: self.m.ch[c].offset,
            on: true,
        }
    }

    /// `w` into reference slot `slot`, with the deskew of its channel; the measurements and analysers follow.
    pub(in crate::ui) fn ref_put(&mut self, slot: usize, w: RefWave) {
        self.m.deskew_ref[slot] = self.m.deskew_ch[w.src];
        self.refs[slot] = Some(w);
        self.measure_refs();
        self.analyzer_update();
    }

    /// `refN` beside the setups, in `WAVE_EXTS` order. The first file that is there is the one loaded.
    pub(crate) fn ref_import_slot(&mut self) -> Result<(), String> {
        let slot = self.ref_sel + 1;
        let Some(dir) = self.setup_path().parent().map(|d| d.to_path_buf()) else {
            return Err(format!("missing ref{slot}"));
        };
        for ext in crate::store::WAVE_EXTS {
            let p = dir.join(format!("ref{slot}.{ext}"));
            if p.is_file() {
                return self.ref_load_file(&p);
            }
        }
        Err(format!("missing ref{slot} waveform"))
    }

    /// Loads a waveform into the selected slot.
    /// A relative path is beside the setups. A missing file starts with `missing `.
    pub(crate) fn ref_load_named(&mut self, path: &str) -> Result<(), String> {
        let raw = path.trim().trim_matches('"');
        if raw.is_empty() {
            return Err("no file".into());
        }
        let p = std::path::Path::new(raw);
        let p = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.setup_path().parent().map(|d| d.join(p)).unwrap_or_else(|| p.to_path_buf())
        };
        self.ref_load_file(&p)
    }

    fn ref_load_file(&mut self, p: &std::path::Path) -> Result<(), String> {
        let bytes = crate::store::read_waveform_file(p)?;
        self.load_waveform_bytes(&bytes, &crate::store::file_name(p))
    }
}
