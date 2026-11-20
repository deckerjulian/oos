// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Display settings: histogram, the QUICK key, the external monitor, text size, cursors, waveform windows, density,
//! grid, the measurement strip and persistence. A new display setting goes here.

/// Waveform histogram: off, vertical (voltage distribution of the whole screen), horizontal (time distribution in a
/// band of +-0.5 div around the trigger level).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hist {
    Off,
    Vertical,
    Horizontal,
}

/// What the QUICK key (and the Quick button of the top bar) does; set in Utility > Quick key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quick {
    /// PPM of the screen in /tmp (the behaviour before the setting existed)
    Screenshot,
    /// PNG in the storage directory (Storage > Save screenshot)
    SavePng,
    SaveSetup,
    SaveCsv,
    Autoset,
    Single,
    RunStop,
    Clear,
    /// the next of Display > Views (ui/views.rs)
    NextView,
}

pub const QUICKS: [Quick; 9] = [
    Quick::Screenshot,
    Quick::SavePng,
    Quick::SaveSetup,
    Quick::SaveCsv,
    Quick::Autoset,
    Quick::Single,
    Quick::RunStop,
    Quick::Clear,
    Quick::NextView,
];

impl Quick {
    /// Name in the Utility menu and its picker.
    pub fn name(self) -> &'static str {
        match self {
            Quick::Screenshot => "Screenshot",
            Quick::SavePng => "Save PNG",
            Quick::SaveSetup => "Save setup",
            Quick::SaveCsv => "Save CSV",
            Quick::Autoset => "Autoset",
            Quick::Single => "Single",
            Quick::RunStop => "Run/Stop",
            Quick::Clear => "Clear",
            Quick::NextView => "Next view",
        }
    }

    /// Label of the Quick button in the top bar.
    pub fn short(self) -> &'static str {
        match self {
            Quick::Screenshot => "Snap",
            Quick::SavePng => "PNG",
            Quick::SaveSetup => "Setup",
            Quick::SaveCsv => "CSV",
            Quick::Autoset => "Auto",
            Quick::Single => "Single",
            Quick::RunStop => "Run/Stop",
            Quick::Clear => "Clear",
            Quick::NextView => "View",
        }
    }
}

/// What an HDMI monitor shows. Mirror: the panel picture, scaled. Waveform: a settings line and the waveform area,
/// scaled. Layout: the whole screen laid out again at the monitor's size, sharp, with the traces stretched; it costs a
/// second render per monitor frame (about 5 to 10 ms on the unit at 1920 x 1080), so the panel may get fewer frames.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HdmiMode {
    Mirror,
    #[default]
    Waveform,
    Layout,
}

pub const HDMI_MODES: [HdmiMode; 3] = [HdmiMode::Mirror, HdmiMode::Waveform, HdmiMode::Layout];

/// Size of the text on the screen (Display > Text size): Normal is the original look, Large and Larger use fonts 2 and
/// 4 px bigger with the rows, cards and dialogs grown to match. A preference: Default keeps it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TextSize {
    #[default]
    Normal,
    Large,
    Larger,
}

pub const TEXT_SIZES: [TextSize; 3] = [TextSize::Normal, TextSize::Large, TextSize::Larger];

impl TextSize {
    pub fn name(self) -> &'static str {
        match self {
            TextSize::Normal => "Normal",
            TextSize::Large => "Large",
            TextSize::Larger => "Larger",
        }
    }

    /// 0, 1, 2: the font set and the layout metrics.
    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorMode {
    Off,
    Time,
    Volt,
    /// time cursors that read the active channel where they stand
    Track,
    /// time and voltage cursors at once
    Xy,
}

/// How the plot is split into waveform windows; every channel and math channel stands in one of them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WinLayout {
    #[default]
    One,
    /// two side by side
    Cols,
    /// two one above the other
    Rows,
    /// two by two
    Quad,
}

impl WinLayout {
    pub const ALL: [WinLayout; 4] = [WinLayout::One, WinLayout::Cols, WinLayout::Rows, WinLayout::Quad];

    pub fn count(self) -> usize {
        match self {
            WinLayout::One => 1,
            WinLayout::Cols | WinLayout::Rows => 2,
            WinLayout::Quad => 4,
        }
    }
}

/// How much of the 1024×600 picture the plot takes. The same four apply to the panel and to a scaled window of it,
/// and to the waveform on the external display.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Density {
    /// top bar, plot, measurement strip, cards
    #[default]
    Scope,
    /// plot and measurement strip
    Full,
    /// plot, and under it a pane of numbers or two stacked traces
    Split,
    /// four equal panes
    Four,
}

impl Density {
    pub const ALL: [Density; 4] = [Density::Scope, Density::Full, Density::Split, Density::Four];

    pub fn label(self) -> &'static str {
        match self {
            Density::Scope => "Scope",
            Density::Full => "Full",
            Density::Split => "Split",
            Density::Four => "Four",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grid {
    Full,
    Light,
    None,
}

/// Where the measurement strip is drawn. Under is below the graticule.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StripDock {
    Under,
    Left,
    Right,
    Above,
}

impl StripDock {
    pub const ALL: [StripDock; 4] = [StripDock::Under, StripDock::Left, StripDock::Right, StripDock::Above];

    pub fn label(self) -> &'static str {
        match self {
            StripDock::Under => "Under",
            StripDock::Left => "Left",
            StripDock::Right => "Right",
            StripDock::Above => "Above",
        }
    }
}

/// Persistence: name, seconds (0 = minimum, <0 = infinite).
pub const PERSIST: [(&str, f32); 7] =
    [("Min", 0.0), ("100 ms", 0.1), ("500 ms", 0.5), ("1 s", 1.0), ("5 s", 5.0), ("10 s", 10.0), ("Infinite", -1.0)];
