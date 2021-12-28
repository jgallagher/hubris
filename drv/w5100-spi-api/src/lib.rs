// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Client API for the STM32F3/4 RCC server.

#![no_std]

use core::convert::TryFrom;
use drv_spi_api::SpiError;
use userlib::*;

#[derive(Debug)]
pub enum W5100Error {
    ResetFailed,
    OpenFailed,
    ListenFailed,
    PeerClosed,
    BadSocketNumber,
    SpiError(SpiError),
    BadSocketState(u8),
    UnknownSocketStatus(u8),
}

impl From<u32> for W5100Error {
    fn from(x: u32) -> Self {
        match x {
            0x0001 => Self::ResetFailed,
            0x0002 => Self::OpenFailed,
            0x0003 => Self::ListenFailed,
            0x0004 => Self::PeerClosed,
            0x0005 => Self::BadSocketNumber,
            _ => match x >> 8 {
                0x01 => Self::SpiError(SpiError::try_from(x & 0xff).unwrap()),
                0x02 => Self::BadSocketState(x as u8),
                0x03 => Self::UnknownSocketStatus(x as u8),
                // Panicking here might be rude. TODO.
                _ => panic!(),
            },
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/client_stub.rs"));
