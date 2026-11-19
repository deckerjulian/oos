// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What a view of Display > Views (ui/views.rs) keeps of a setup: its keys, and the check of a view as a setup
//! keeps it (`view1` ...).

/// Keys of a setup that belong to a view, besides `xy` and `full` (fields of the Ui).
pub const KEYS: [&str; 21] = [
    "windows",
    "ch_window",
    "math_window",
    "la_window",
    "meas",
    "meas2",
    "dvm",
    "counter",
    "cursor",
    "cur_a",
    "cur_b",
    "cur_c",
    "cur_d",
    "pane",
    "meas_labels",
    "trend_sel",
    "zoom",
    "zoom_tdiv",
    "zdelay",
    "hist",
    "view",
];

/// Whether `k` is a key of a view's model part (not `xy` and `full`).
pub fn is_view_key(k: &str) -> bool {
    KEYS.contains(&k)
}

fn known(k: &str) -> bool {
    KEYS.contains(&k) || k == "xy" || k == "full"
}

/// A view as kept in a setup: `key=value; key=value ...` of its keys only.
pub fn slot_checked(v: &str) -> Result<String, String> {
    for part in v.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let k = part.split_once('=').map(|(k, _)| k.trim()).ok_or_else(|| format!("not key=value: {}", part))?;
        if !known(k) {
            return Err(format!("not part of a view: {}", k));
        }
    }
    Ok(v.trim().to_string())
}
