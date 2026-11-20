// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the plot area shows (the scope or an analyser page) and the choices of the analyser pages: eye placement,
//! power, magnetic, inverter and double-pulse modes. The setting of a new analyser page goes here.

/// What the plot area shows: the scope traces, or one of the analysers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AView {
    Scope,
    Spectrum,
    Eye,
    Power,
    Bode,
    /// bandwidth test: step response of the front end (ui/bwtest.rs)
    Step,
    Magnetic,
    Inverter,
    Dpt,
}

impl AView {
    /// Every view (the tests check each analyser page against it).
    #[cfg(test)]
    pub const ALL: [AView; 9] = [
        AView::Scope,
        AView::Spectrum,
        AView::Eye,
        AView::Power,
        AView::Bode,
        AView::Step,
        AView::Magnetic,
        AView::Inverter,
        AView::Dpt,
    ];

    /// The views a setup keeps (`view = <word>`): every one but the bandwidth test, a procedure and not a page to come
    /// back to. A new analyser page goes here too (ui/analysers.rs checks it).
    pub const STORED: [AView; 8] = [
        AView::Scope,
        AView::Spectrum,
        AView::Eye,
        AView::Power,
        AView::Bode,
        AView::Magnetic,
        AView::Inverter,
        AView::Dpt,
    ];
}

/// Where the eye is drawn. The full page is the default. Side and Under are extra panes and leave that page available.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EyePlace {
    /// The eye fills the plot: the figures, the jitter column and the charts.
    Full,
    /// A pane to the right of the trace.
    Side,
    /// A pane under the trace.
    Under,
}

/// Power analysis page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerMode {
    Quality,
    Harmonics,
    Switching,
    Ripple,
    /// C = q / v over the turn-on record
    Capacitance,
    /// rail overshoot, undershoot, DC and turn-on maximum
    Rail,
}

/// Magnetic plot: current against flux linkage, or B against H when the core is set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MagPlot {
    IPsi,
    BH,
}

/// Inverter page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InvPage {
    Dq0,
    Mech,
}

/// Where the inverter takes its angle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InvAngle {
    /// θ from the phase trio (DQ0 only)
    Off,
    Phase,
    Analog,
    Quad,
    Hall,
}

/// The optional fourth probe of a double pulse.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DptFourth {
    Off,
    Gate,
    DiodeV,
    DiodeI,
}

/// Which edge the double-pulse plot is zoomed to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DptZoom {
    Both,
    TurnOn,
    TurnOff,
}
