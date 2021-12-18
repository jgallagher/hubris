// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Client API for the Piezo Element driver.

#![no_std]

use core::cell::Cell;
use zerocopy::AsBytes;

use userlib::*;

#[derive(FromPrimitive)]
enum Op {
    On = 1,
    Off = 2,
}

#[derive(Clone, Debug)]
pub struct PiezoElement(Cell<TaskId>);

impl From<TaskId> for PiezoElement {
    fn from(t: TaskId) -> Self {
        Self(Cell::new(t))
    }
}

#[derive(Copy, Clone, Debug)]
pub enum PiezoError {
    FrequencyTooLow = 1,
    BadArg = 2,
}

impl From<u32> for PiezoError {
    fn from(x: u32) -> Self {
        match x {
            1 => PiezoError::FrequencyTooLow,
            2 => PiezoError::BadArg,
            _ => panic!(),
        }
    }
}

impl PiezoElement {
    /// Turns the piezo element on at the given frequency in Hz.
    pub fn piezo_on(&self, freq_hz: u16) -> Result<(), PiezoError> {
        #[derive(AsBytes)]
        #[repr(C)]
        struct On(u16);

        impl hl::Call for On {
            const OP: u16 = Op::On as u16;
            type Response = ();
            type Err = PiezoError;
        }

        hl::send_with_retry(&self.0, &On(freq_hz))
    }

    /// Turns the piezo element off.
    pub fn piezo_off(&self) -> Result<(), PiezoError> {
        #[derive(AsBytes)]
        #[repr(C)]
        struct Off;

        impl hl::Call for Off {
            const OP: u16 = Op::Off as u16;
            type Response = ();
            type Err = PiezoError;
        }

        hl::send_with_retry(&self.0, &Off)
    }
}
