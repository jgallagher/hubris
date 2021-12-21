// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Client API for the Piezo Element driver.

#![no_std]

use userlib::*;

#[derive(Copy, Clone, Debug)]
pub enum PiezoError {
    FrequencyTooLow = 1,
}

impl From<u32> for PiezoError {
    fn from(x: u32) -> Self {
        match x {
            1 => PiezoError::FrequencyTooLow,
            _ => panic!(),
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/client_stub.rs"));
