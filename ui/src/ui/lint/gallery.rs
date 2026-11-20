// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Every screen as a picture, for looking at (`gallery`, ignored by default). A new screen gets its picture in
//! `gallery_at`.

use super::*;

/// Every screen as a picture, for looking at: `OOS_GALLERY=dir cargo test gallery -- --ignored`, one directory per
/// text size (`OOS_TEXT_SIZE=Large` for one of them; `OOS_SIZE=1920x1080` for another screen than the panel).
#[test]
#[ignore]
fn gallery() {
    let Ok(base) = std::env::var("OOS_GALLERY") else { return };
    let only = std::env::var("OOS_TEXT_SIZE").ok();
    for ts in TEXT_SIZES.into_iter().filter(|t| only.as_deref().is_none_or(|o| o == t.name())) {
        gallery_at(&std::path::PathBuf::from(&base).join(ts.name().to_lowercase()), ts);
    }
}

fn gallery_at(dir: &std::path::Path, ts: TextSize) {
    use MenuId as M;
    let _ = std::fs::create_dir_all(dir);
    let scr = std::env::var("OOS_SIZE").ok().and_then(|s| Screen::parse(&s)).unwrap_or(Screen::PANEL);
    let new = || {
        let mut u = demo_on(scr);
        u.m.text_size = ts;
        u.relayout();
        u
    };
    let k = |u: &mut Ui, s: &str| {
        crate::cmd::run(u, &format!("key {}", s));
    };
    type S = (&'static str, fn(&mut Ui));
    let menus: Vec<(&str, M)> = vec![
        ("menu-main", M::Main),
        ("menu-ch", M::Channel(0)),
        ("menu-hor", M::Horizontal),
        ("menu-trig", M::Trigger),
        ("menu-acq", M::Acquire),
        ("menu-measure", M::Measure),
        ("menu-measadd", M::MeasAdd),
        ("menu-cursor", M::Cursor),
        ("menu-display", M::Display),
        ("menu-system", M::Utility),
        ("menu-math", M::Math),
        ("menu-ref", M::Ref),
        ("menu-decode", M::Decode),
        ("menu-storage", M::Storage),
        ("menu-mask", M::Mask),
        ("menu-zone", M::Zone),
        ("menu-spectrum", M::Spectrum),
        ("menu-eye", M::Eye),
        ("menu-power", M::Power),
        ("menu-identify", M::Identify),
        ("menu-selfcal", M::SelfCal),
        ("menu-bode", M::Bode),
        ("menu-afg", M::Afg),
        ("menu-bw", M::BwTest),
        ("menu-la", M::La),
        ("menu-lalines", M::LaLines),
        ("menu-lapat", M::LaPat),
        ("menu-network", M::Network),
        ("menu-remote", M::Remote),
        ("menu-time", M::Time),
    ];
    let shot = |u: &mut Ui, n: &str| {
        for _ in 0..24 {
            u.step(0.04);
        }
        let mut cv = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut cv);
        let _ = crate::store::write(&dir.join(format!("{}.png", n)), &crate::store::png_of(&cv));
    };
    for (n, m) in menus {
        let mut u = new();
        u.open_menu(m);
        shot(&mut u, n);
    }
    let scenes: Vec<S> = vec![
        ("s-default", |_| {}),
        ("s-all-ch", |u| {
            for c in 0..4 {
                u.m.ch[c].on = true;
            }
        }),
        ("s-meas6", |u| {
            u.m.meas.clear();
            for i in 0..6 {
                u.m.meas.push((ALL_MEAS[i], WaveId::Ch(0)));
            }
        }),
        ("s-cursor", |u| u.m.cursor = CursorMode::Time),
        ("s-more", |u| {
            u.m.ch[0].unit = AmpUnit::A;
            u.open_menu(MenuId::ChMore(0));
        }),
        ("s-logger", |u| u.open_menu(MenuId::Logger)),
        ("s-trend", |u| {
            u.m.pane = 4;
            for _ in 0..60 {
                u.step(0.04);
            }
        }),
        ("s-formula", |u| {
            u.m.math = MathOp::Expr;
            u.m.math_expr[0] = "CH1*sin(2*pi*5000*t)".into();
            u.m.math_scale = 6;
            for _ in 0..4 {
                u.step(0.04);
            }
            u.open_menu(MenuId::Math);
        }),
        ("s-limit", |u| {
            u.m.view = AView::Spectrum;
            for _ in 0..12 {
                u.step(0.04);
            }
            u.activate(Item::SpecLimitSet);
        }),
        ("s-cursor-xy", |u| {
            u.m.cursor = CursorMode::Xy;
            u.open_menu(MenuId::Cursor);
        }),
        ("s-fine", |u| {
            u.m.fine_v = true;
            u.adjust(Item::ChScale(0), 7);
            u.open_menu(MenuId::Channel(0));
        }),
        ("s-ref", |u| {
            for _ in 0..24 {
                u.step(0.04);
            }
            u.activate(Item::RefSave);
            u.adjust(Item::RefOffset, 8);
            u.open_menu(MenuId::Ref);
        }),
        ("s-zoom", |u| u.set_zoom(true)),
        ("s-xy", |u| {
            u.m.ch[1].on = true;
            u.xy = true;
        }),
        ("s-stop", |u| u.m.run = RunState::Stop),
        ("s-spectrum", |u| u.m.view = AView::Spectrum),
        ("s-eye", |u| u.m.view = AView::Eye),
        ("s-power", |u| u.m.view = AView::Power),
        ("s-bode", |u| u.m.view = AView::Bode),
        ("s-step", |u| u.m.view = AView::Step),
        ("s-sysinfo", |u| {
            u.sysinfo_open();
            u.dialog = Some(vec![]);
        }),
        ("s-power-dlg", |u| u.power.open = true),
        ("s-touchlock", |u| u.m.touch_lock = true),
        ("s-la", |u| u.m.la.on = true),
        ("s-cards-1", |u| {
            u.m.la.on = true;
            u.m.afg_on = true;
        }),
        ("s-cards-4", |u| {
            u.m.la.on = true;
            u.m.afg_on = true;
            for c in 0..4 {
                u.m.ch[c].on = true;
            }
        }),
        ("s-pane-spec", |u| {
            u.m.ch[1].on = true;
            u.m.pane = 1;
        }),
        ("s-pane-res", |u| {
            u.m.ch[1].on = true;
            u.m.pane = 2;
            u.m.meas2 = vec![(0, WaveId::Ch(0), WaveId::Ch(1)), (4, WaveId::Ch(0), WaveId::Ch(1))];
            u.m.dvm_on = true;
            u.m.cnt_on = true;
        }),
        ("s-decoders", |u| {
            u.m.ch[2].on = true;
            u.m.dec_on = true;
            u.m.dec_src = 2;
            u.m.dec_select(1);
            u.m.dec_on = true;
            u.m.dec_src = 2;
            u.set_dec_proto(Proto::SPI);
            u.m.dec_select(0);
            u.table.kind = Some(tables::TableKind::Decode);
        }),
        ("s-track", |u| {
            u.m.cursor = CursorMode::Track;
        }),
        ("s-network", |u| {
            (u.net.addr, u.net.iface) = ("192.168.177.64".into(), "wlan0".into());
            u.net.ifs = vec![("eth0".into(), "192.0.2.51".into()), ("wlan0".into(), "192.168.177.64".into())];
            u.wlan.link = wlan::tests_link("Labor", "192.168.177.64");
            u.open_menu(MenuId::Network);
        }),
        ("s-decode", |u| {
            u.m.ch[2].on = true;
            u.m.dec_on = true;
            u.m.dec_src = 2;
        }),
        ("s-4ch-meas", |u| {
            for c in 0..4 {
                u.m.ch[c].on = true;
            }
            u.m.meas.extend([
                (Meas::Vpp, WaveId::Ch(1)),
                (Meas::Freq, WaveId::Ch(1)),
                (Meas::Vrms, WaveId::Ch(1)),
                (Meas::Vpp, WaveId::Ch(2)),
                (Meas::Period, WaveId::Ch(2)),
                (Meas::Vmax, WaveId::Ch(3)),
                (Meas::Vmin, WaveId::Ch(3)),
            ]);
            u.m.cursor = CursorMode::Time;
        }),
        ("s-events", |u| {
            u.m.ch[2].on = true;
            u.m.dec_on = true;
            u.m.dec_src = 2;
            u.m.pane = tables::PANE_EVENTS;
        }),
        ("s-stats", |u| {
            u.m.ch[1].on = true;
            u.m.meas.push((Meas::Freq, WaveId::Ch(1)));
            u.stats_on = true;
        }),
        ("s-osk", |u| u.activate(Item::ChLabel(0))),
        ("s-startup", |u| {
            use oos_hal::bootstatus::line;
            u.start_up_begin(None);
            let t = line(3.9, "ok", "Linux", "6.12.115")
                + &line(4.0, "ok", "FPGA image", "")
                + &line(14.3, "warn", "Calibration data", "missing: cal_afg.hex")
                + &line(14.4, "run", "Sample clock (PLL lock)", "waiting for the lock");
            u.start_up_update(&t, 16.0);
        }),
    ];
    for (n, f) in scenes {
        let mut u = new();
        f(&mut u);
        shot(&mut u, n);
    }
    let mut u = new();
    k(&mut u, "measure");
    shot(&mut u, "k-measure");
    let mut u = new();
    u.activate(Item::About);
    shot(&mut u, "s-about");
    let mut u = new();
    u.activate(Item::Update);
    shot(&mut u, "s-update");
}
