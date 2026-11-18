// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The settings of `:SYSTem` (scpi/system.rs): the draw path of the Acquire menu (ui/setting_items.rs). A change over
//! SCPI does not mark the model changed, as before the table. The slice is the place of `store::setup_text` that
//! writes it, so the keys of a setup file keep their order.
//!
//! Still written out by hand: `aux_out` (`:SYSTem:AOUTput`; the menu and the command offer pass/fail only where the
//! board has the mask or the zone unit, and a refused value is queued without setting the status bit), and the
//! graphics choice (`:SYSTem:GRAPhics`, a setting of the scope in `/oos/system.conf`, not of a setup).

use super::{CustomIo, Scpi, Setting, Ty, Val};

/// The slice, for the docs table and the SCPI handler.
pub const SLICES: [&[Setting]; 1] = [&DRAW_PATH];

/// Written after `bar`.
pub const DRAW_PATH: [Setting; 1] = [Setting {
    key: "draw_path",
    scpi: Scpi::Cmd("PIPeline"),
    ty: Ty::Custom(&DRAW_PATH_IO),
    label: "Draw path",
    #[cfg(test)]
    doc: "The draw path of the Acquire menu: Batch, the default \
          ([ADR 0029](adr/code/0029-batch-is-the-default-draw-path.md)), or Frames, the standard path; `STANdard` is \
          an alias of `FRAMes`. Which path runs is the `pipeline` part of `:SYSTem:STATe?`.",
    accept: None,
    get: |m, _| Val::Bool(m.draw_batch),
    put: |m, _, v| m.draw_batch = v.index() != 0,
}];

/// `batch` or `standard` in a setup; over SCPI any word that starts with `BATC`, `FRAM` or `STAN`, in any case; the
/// query answers `BATC` or `FRAM`.
const DRAW_PATH_IO: CustomIo = CustomIo {
    store: |v| if v.index() != 0 { "batch" } else { "standard" }.to_string(),
    load: |t| match t.trim() {
        "batch" => Ok(Val::Bool(true)),
        "standard" => Ok(Val::Bool(false)),
        _ => Err(format!("{:?}: standard or batch", t)),
    },
    query: |v| if v.index() != 0 { "BATC" } else { "FRAM" }.to_string(),
    take: |a| {
        let a = a.trim().to_ascii_uppercase();
        if a.starts_with("BATC") {
            Some(Val::Bool(true))
        } else if a.starts_with("FRAM") || a.starts_with("STAN") {
            Some(Val::Bool(false))
        } else {
            None
        }
    },
    labels: &["Standard", "Batch"],
    #[cfg(test)]
    values: "`BATCh`, `FRAMes` or `STANdard`, any word that starts with `BATC`, `FRAM` or `STAN`, in any case; the \
             query answers `BATC` or `FRAM`",
};
