// SPDX-License-Identifier: GPL-2.0-or-later

//! Batch 5: overlapping and clipped text in the analysis views. OOS_SHOTS=<dir> writes a picture of each scene.
use super::*;

/// PR #8's layouts: Default is the card bar here, Minimal the compact bar (Display > Bottom bar); Studio is not taken.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Layout {
    Default,
    Minimal,
}

fn fresh(l: Layout) -> Ui {
    let mut u = crate::ui::testkit::demo_ui();
    u.m.bar_compact = l == Layout::Minimal;
    u.m.ch[1].on = true;
    u.relayout();
    for _ in 0..10 {
        u.step(0.05);
    }
    u
}

fn put_ref(u: &mut Ui, slot: usize, color: usize, v: Vec<f32>, dt: f64) {
    u.refs[slot] = Some(RefWave { color, vpd: 0.5, ..RefWave::test(v, dt, 0.0) });
}

fn snap(u: &mut Ui) -> Canvas {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    for _ in 0..8 {
        u.step(0.05);
    }
    u.render(&mut cv);
    cv
}

fn shot(u: &mut Ui, name: &str) {
    shotc(u, name, &[]);
}

/// A picture and 3x crops (x, y, w, h) of it.
fn shotc(u: &mut Ui, name: &str, crops: &[(i32, i32, i32, i32)]) {
    let cv = snap(u);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), crate::store::png_of(&cv)).unwrap();
        for (k, &(x, y, w, h)) in crops.iter().enumerate() {
            let mut big = Canvas::new(w * 3, h * 3);
            for j in 0..h * 3 {
                for i in 0..w * 3 {
                    big.px[(j * w * 3 + i) as usize] = cv.px[((y + j / 3) * W + x + i / 3) as usize];
                }
            }
            std::fs::write(std::path::Path::new(&dir).join(format!("{name}-c{k}.png")), crate::store::png_of(&big))
                .unwrap();
        }
    }
}

fn sim_eye(l: Layout, place: EyePlace) -> Ui {
    let mut u = fresh(l);
    u.files.dir = std::env::temp_dir().join("oos-batch5-eye");
    assert!(u.sim_preset_set("PCIe 3.0"));
    u.sim_activate(Item::SimPlay);
    u.wait_sim_generation();
    for _ in 0..10 {
        u.step(0.05);
        u.wait_sim_generation();
    }
    u.key(key::RUNSTOP);
    u.m.view = AView::Eye;
    u.eye_set_place(place);
    for _ in 0..300 {
        u.step(0.05);
        u.wait_sim_generation();
        std::thread::sleep(std::time::Duration::from_millis(20));
        if u.azr.eye.is_some() {
            break;
        }
    }
    for _ in 0..10 {
        u.step(0.05);
    }
    u
}

#[test]
fn shots_eye() {
    if std::env::var_os("OOS_SHOTS").is_none() {
        return;
    }
    for (l, n) in [(Layout::Default, "d"), (Layout::Minimal, "m")] {
        for p in [EyePlace::Full, EyePlace::Side, EyePlace::Under] {
            let mut u = sim_eye(l, p);
            shotc(&mut u, &format!("{n}-simeye-{p:?}"), &[(560, 40, 464, 280), (0, 330, 560, 200)]);
        }
    }
}

#[test]
fn shots() {
    if std::env::var_os("OOS_SHOTS").is_none() {
        return;
    }
    let names = [(Layout::Default, "d"), (Layout::Minimal, "m")];
    for (l, n) in names {
        for (vn, v) in [
            ("spec", AView::Spectrum),
            ("eye", AView::Eye),
            ("power", AView::Power),
            ("bode", AView::Bode),
            ("step", AView::Step),
        ] {
            let mut u = fresh(l);
            u.m.view = v;
            shot(&mut u, &format!("{n}-{vn}"));
        }
        for p in [EyePlace::Side, EyePlace::Under] {
            let mut u = fresh(l);
            u.m.view = AView::Eye;
            u.eye_set_place(p);
            shot(&mut u, &format!("{n}-eye-{p:?}"));
        }
        for h in [Hist::Vertical, Hist::Horizontal] {
            let mut u = fresh(l);
            u.m.hist = h;
            shot(&mut u, &format!("{n}-hist-{h:?}"));
        }
        let sq: Vec<f32> = (0..2000).map(|i| if (i / 250) % 2 == 0 { 1.5 } else { -1.5 }).collect();
        let tri: Vec<f32> = (0..2000).map(|i| ((i % 500) as f32 / 250.0 - 1.0).abs() * 3.0 - 1.5).collect();
        let sin: Vec<f32> = (0..2000).map(|i| 2.0 * (i as f32 * 0.0314).sin()).collect();
        let mut u = fresh(l);
        put_ref(&mut u, 0, 0, sin.clone(), 1e-6);
        put_ref(&mut u, 1, 1, sq.clone(), 1e-6);
        put_ref(&mut u, 2, 2, tri.clone(), 1e-6);
        shotc(&mut u, &format!("{n}-refs"), &[(500, 80, 200, 120), (500, 410, 200, 60)]);
        u.m.math = MathOp::Add;
        u.m.math_select(0);
        shot(&mut u, &format!("{n}-refs-math"));
        u.toast("Loaded wave1.wfm into R1");
        shot(&mut u, &format!("{n}-toast"));
        let mut u = fresh(l);
        u.set_zoom(true);
        shot(&mut u, &format!("{n}-zoom"));
        for (mn, m) in [
            ("spectrum", MenuId::Spectrum),
            ("identify", MenuId::Identify),
            ("eyemenu", MenuId::Eye),
            ("bodemenu", MenuId::Bode),
        ] {
            let mut u = fresh(l);
            u.open_menu(m);
            shot(&mut u, &format!("{n}-menu-{mn}"));
        }
    }
}

#[test]
fn shots_more() {
    if std::env::var_os("OOS_SHOTS").is_none() {
        return;
    }
    for (l, n) in [(Layout::Default, "d"), (Layout::Minimal, "m")] {
        let mut u = fresh(l);
        let _ = u.identify_text();
        u.open_menu(MenuId::Identify);
        shot(&mut u, &format!("{n}-identify-found"));
        let sq: Vec<f32> = (0..2000).map(|i| if (i / 250) % 2 == 0 { 1.5 } else { -1.5 }).collect();
        let mut u = fresh(l);
        put_ref(&mut u, 1, 1, sq, 1e-6);
        shotc(&mut u, &format!("{n}-refgrid"), &[(500, 60, 200, 120)]);
        let mut u = fresh(l);
        u.set_zoom(true);
        let sq: Vec<f32> = (0..2000).map(|i| if (i / 250) % 2 == 0 { 1.5 } else { -1.5 }).collect();
        put_ref(&mut u, 1, 1, sq.clone(), 1e-6);
        put_ref(&mut u, 0, 0, sq, 1e-6);
        shot(&mut u, &format!("{n}-zoom-refs"));
        let mut u = fresh(l);
        u.m.zoom = true;
        u.m.zscale = 0;
        shot(&mut u, &format!("{n}-zoom-deep"));
    }
}

#[test]
fn shots_stacked() {
    if std::env::var_os("OOS_SHOTS").is_none() {
        return;
    }
    for (l, n) in [(Layout::Default, "d"), (Layout::Minimal, "m")] {
        let mut u = fresh(l);
        u.m.ch[2].on = true;
        u.workspace.stacked = true;
        u.relayout();
        shot(&mut u, &format!("{n}-stacked"));
        u.m.math = MathOp::Add;
        u.m.math_select(0);
        shot(&mut u, &format!("{n}-stacked-math"));
    }
}

#[test]
fn shots_pan() {
    if std::env::var_os("OOS_SHOTS").is_none() {
        return;
    }
    for (l, n) in [(Layout::Default, "d"), (Layout::Minimal, "m")] {
        for (k, zs, zd) in [(0, 3usize, -0.3041e-3f32), (1, 6, 0.1237e-3), (2, 9, 2.0e-6)] {
            let mut u = fresh(l);
            u.set_zoom(true);
            u.m.zscale = zs;
            u.m.zdelay = zd;
            shotc(&mut u, &format!("{n}-pan{k}"), &[(0, 480, 1024, 55)]);
        }
    }
}
