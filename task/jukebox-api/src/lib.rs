// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Client API for the SPI server

#![no_std]

use userlib::*;

#[derive(Copy, Clone, Debug, FromPrimitive, PartialEq)]
#[repr(u32)]
pub enum JukeboxError {
    /// Cannot start playing while another song plays; try again later
    BusyPlaying = 1,

    /// Song index out of bounds
    BadSongIndex = 2,
}

impl From<JukeboxError> for u16 {
    fn from(err: JukeboxError) -> Self {
        err as u16
    }
}

impl core::convert::TryFrom<u32> for JukeboxError {
    type Error = ();
    fn try_from(x: u32) -> Result<Self, Self::Error> {
        Self::from_u32(x).ok_or(())
    }
}

include!(concat!(env!("OUT_DIR"), "/client_stub.rs"));
