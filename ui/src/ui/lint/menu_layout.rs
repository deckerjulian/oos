// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The layout check of every menu: the menus with their pickers and the extreme values of their rows, at every
//! text size and on every screen (`every_menu_fits`). A new menu or row is checked once `menus_fit_at` opens it.

use super::*;
use std::collections::BTreeSet;

fn check(ui: &Ui, ctx: &str, out: &mut BTreeSet<String>) {
    let Some(d) = ui.dlg_layout() else { return };
    let f = ui.fonts();
    let mut bad = |what: String| {
        out.insert(format!("{}: {}", ctx, what));
    };
    if d.r.x < 0 || d.r.y < 0 || d.r.right() > ui.scr.w || d.r.bottom() > ui.scr.h {
        bad(format!("dialog off the screen {:?}", d.r));
    }
    for (i, row) in d.rows.iter().enumerate() {
        let c = row.ctrl;
        let name = format!("{:?}", row.it);
        if c.y + c.h > d.r.y + d.r.h || c.x + c.w > d.r.x + d.r.w {
            bad(format!("{} outside the dialog", name));
        }
        if d.rows[i + 1..].iter().any(|o| overlaps(o.ctrl, c)) {
            bad(format!("{} overlaps another row", name));
        }
        let kind = Ui::kind(row.it);
        if row.label.w > 0 && kind != Kind::Action {
            let t = ui.item_label(row.it);
            if t.is_empty() {
                bad(format!("{} has no label", name));
            }
            if f.s.width(t) > row.label.w {
                bad(format!("label '{}' {} > {}", t, f.s.width(t), row.label.w));
            }
        }
        match &row.ctl {
            Ctl::Seg(opts, _) => {
                let n = opts.len() as i32;
                let sw = (c.w - (n - 1) * 4) / n;
                for o in opts {
                    if f.mb.width(o) + 8 > sw {
                        bad(format!("{} option '{}' {} > {}", name, o, f.mb.width(o) + 8, sw));
                    }
                }
            }
            Ctl::Value | Ctl::Pick => {
                let (_, mid, _) = Ui::value_parts(c);
                let v = ui.value(row.it);
                let room = if matches!(row.ctl, Ctl::Pick) { mid.w - 44 } else { mid.w - 8 };
                if f.mb.width(&v) > room {
                    bad(format!("{} value '{}' {} > {}", name, v, f.mb.width(&v), room));
                }
            }
            Ctl::Text => {
                let v = ui.value(row.it);
                if ui.text_row_font(c.w, &v).width(&v) > c.w {
                    bad(format!("{} text too long", name));
                }
            }
            Ctl::Button => {
                let t = match (row.it, ui.pick) {
                    (Item::PickOpt(k), Some(p)) => ui.options(p.it).get(k).cloned().unwrap_or_default(),
                    _ if kind == Kind::Action => ui.item_label(row.it).to_string(),
                    _ => ui.value(row.it),
                };
                if t.is_empty() {
                    bad(format!("{} has no text", name));
                }
                let (font, r, fits) = ui.btn_text(c, matches!(row.it, Item::Open(_)), &t);
                if !fits {
                    bad(format!("{} text '{}' {} > {}", name, t, font.width(&t), r.w - 8));
                }
            }
        }
    }
}

/// The menu as it is, its pickers, and the rows with their extreme values.
fn walk(make: &dyn Fn() -> Ui, ctx: &str, out: &mut BTreeSet<String>) {
    let ui = make();
    check(&ui, ctx, out);
    let items: Vec<Item> = ui.dlg_layout().map_or(Vec::new(), |d| d.rows.iter().map(|r| r.it).collect());
    for it in items {
        if Ui::kind(it) == Kind::Action {
            continue;
        }
        let mut u = make();
        if !u.options(it).is_empty() {
            u.open_picker(it);
            check(&u, &format!("{} > picker {:?}", ctx, it), out);
        }
        for d in [-100_000, 100_000] {
            let mut u = make();
            u.adjust(it, d);
            check(&u, &format!("{} > {:?} {}", ctx, it, if d < 0 { "min" } else { "max" }), out);
        }
    }
}

/// Every menu at every text size, on every screen.
#[test]
fn every_menu_fits() {
    let mut out = BTreeSet::new();
    for scr in SCREENS {
        for ts in TEXT_SIZES {
            menus_fit_at(scr, ts, &mut out);
        }
    }
    let list: Vec<&String> = out.iter().collect();
    for l in &list {
        println!("{}", l);
    }
    assert!(list.is_empty(), "{} layout problems", list.len());
}

fn menus_fit_at(scr: Screen, ts: TextSize, all: &mut BTreeSet<String>) {
    let mut out = BTreeSet::new();
    let open = |m: MenuId, set: &dyn Fn(&mut Ui)| {
        let mut u = demo_on(scr);
        u.m.text_size = ts;
        set(&mut u);
        u.relayout();
        u.open_menu(m);
        u
    };
    use MenuId::*;
    // the pages not in `PAGES` yet, and channel 4 (`PAGES` opens a channel page with channel 1)
    for m in [Main, Channel(3), Measure, MeasAdd, MeasHold, Afg, La, LaLines, LaPat] {
        walk(&|| open(m, &|_| {}), &format!("{:?}", m), &mut out);
        walk(&|| open(m, &|u| u.m.la.on = true), &format!("{:?} LA", m), &mut out);
    }
    // every page of `PAGES` (ADR 0060), and the states its module names, with the pickers that open from actions
    for p in &pages::PAGES {
        walk(&|| open(p.id, &|_| {}), &format!("{:?}", p.id), &mut out);
        walk(&|| open(p.id, &|u| u.m.la.on = true), &format!("{:?} LA", p.id), &mut out);
    }
    for &(m, name, set, pickers) in pages::LAYOUTS {
        walk(&|| open(m, &set), &format!("{:?} {}", m, name), &mut out);
        for &it in pickers {
            let mut u = open(m, &set);
            u.open_picker(it);
            check(&u, &format!("{:?} {} > picker {:?}", m, name, it), &mut out);
        }
    }
    walk(&|| open(Inverter, &|u| u.m.inv_page = InvPage::Mech), "Inverter mechanical", &mut out);
    walk(
        &|| {
            open(Inverter, &|u| {
                u.m.inv_page = InvPage::Mech;
                u.m.inv_ang = InvAngle::Analog;
            })
        },
        "Inverter analog",
        &mut out,
    );
    walk(
        &|| {
            open(Inverter, &|u| {
                u.m.inv_page = InvPage::Mech;
                u.m.inv_ang = InvAngle::Quad;
            })
        },
        "Inverter quadrature",
        &mut out,
    );
    walk(
        &|| {
            open(Inverter, &|u| {
                u.m.inv_page = InvPage::Mech;
                u.m.inv_ang = InvAngle::Hall;
            })
        },
        "Inverter hall",
        &mut out,
    );
    walk(&|| open(Inverter, &|u| u.m.inv_ang = InvAngle::Analog), "Inverter DQ0 analog", &mut out);
    walk(&|| open(Dpt, &|u| u.m.dpt_4th = DptFourth::Gate), "Double pulse gate", &mut out);
    walk(
        &|| {
            open(Magnetic, &|u| {
                u.m.mag_ae = 1e-4;
                u.m.mag_le = 0.05;
                u.m.mag_plot = MagPlot::BH;
            })
        },
        "Magnetic B-H",
        &mut out,
    );
    let trend = |u: &mut Ui| {
        u.m.pane = tables::PANE_TREND;
        u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(3))];
        u.m.meas2 = vec![(0, WaveId::Ch(0), WaveId::Ch(1))];
    };
    walk(&|| open(Display, &trend), "Display Trend", &mut out);
    walk(&|| open(Windows, &|u| u.select_option(Item::WinLayout, "Split")), "Windows split", &mut out);
    walk(&|| open(Windows, &|u| u.select_option(Item::WinLayout, "Four")), "Windows four", &mut out);
    walk(&|| open(Bode, &|u| u.m.bode_gen = bode::FREE), "Bode free sweep", &mut out);
    walk(
        &|| {
            open(Bode, &|u| {
                u.m.bode_gen = bode::FREE;
                u.m.bode_sw_in = 0
            })
        },
        "Bode free sweep no input",
        &mut out,
    );
    walk(&|| open(Bode, &|u| u.m.bode_gen = 5), "Bode UNI-T LAN", &mut out);
    walk(
        &|| {
            open(Bode, &|u| {
                u.m.bode_gen = 5;
                u.m.bode_usb = true
            })
        },
        "Bode UNI-T USB",
        &mut out,
    );
    walk(
        &|| {
            open(Spectrum, &|u| {
                u.m.pane = tables::PANE_SPECTRUM;
                u.azr.spec_stop = 1.5e8;
            })
        },
        "Spectrum pane",
        &mut out,
    );
    for t in TRIG_TYPES {
        walk(&|| open(Trigger, &|u| u.set_trig_type(t)), &format!("Trigger {}", t.name()), &mut out);
        walk(
            &|| {
                open(Trigger, &|u| {
                    u.m.la.on = true;
                    u.set_trig_type(t)
                })
            },
            &format!("Trigger {} LA", t.name()),
            &mut out,
        );
    }
    for p in PROTOS.iter().copied() {
        walk(&|| open(Decode, &|u| u.set_dec_proto(p)), &format!("Decode {}", p.name()), &mut out);
        walk(
            &|| {
                open(Decode, &|u| {
                    u.m.la.on = true;
                    u.m.dec_la_in = true;
                    u.set_dec_proto(p)
                })
            },
            &format!("Decode {} LA", p.name()),
            &mut out,
        );
    }
    for t in TRIG_TYPES {
        for p in [Proto::UART, Proto::CAN, Proto::I2C] {
            walk(
                &|| {
                    open(Trigger, &|u| {
                        u.set_dec_proto(p);
                        u.set_trig_type(t)
                    })
                },
                &format!("Trigger {} {}", t.name(), p.name()),
                &mut out,
            );
        }
    }
    all.extend(out.into_iter().map(|l| format!("{}x{} {}: {}", scr.w, scr.h, ts.name(), l)));
}
