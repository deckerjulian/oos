// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The native Open dialog. Canceling never changes the session.

use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use windows_sys::Win32::UI::Controls::Dialogs::*;

pub fn waveform(owner: *mut std::ffi::c_void) -> Result<Option<PathBuf>, String> {
    let filter: Vec<u16> = "Waveforms (*.csv;*.bin;*.wfm;*.trc;*.isf;*.tr0;*.wfmx)\0*.csv;*.bin;*.wfm;*.trc;*.isf;*.tr0;*.wfmx\0All files (*.*)\0*.*\0\0".encode_utf16().collect();
    let title: Vec<u16> = "Load waveform into the selected reference slot\0".encode_utf16().collect();
    let mut name = vec![0u16; 32_768];
    let mut options = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: filter.as_ptr(),
        nFilterIndex: 1,
        lpstrFile: name.as_mut_ptr(),
        nMaxFile: name.len() as u32,
        lpstrTitle: title.as_ptr(),
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if unsafe { GetOpenFileNameW(&mut options) } == 0 {
        let error = unsafe { CommDlgExtendedError() };
        return if error == 0 { Ok(None) } else { Err(format!("The file dialog could not open (error {error:#x}).")) };
    }
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Ok(Some(std::ffi::OsString::from_wide(&name[..end]).into()))
}
