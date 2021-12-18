// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Client API for the STM32F3/4 RCC server.

#![no_std]

use core::cell::Cell;

use byteorder::LittleEndian;
use zerocopy::{AsBytes, U32};

use userlib::*;

enum Op {
    EnableClock = 1,
    DisableClock = 2,
    EnterReset = 3,
    LeaveReset = 4,
}

#[derive(Clone, Debug)]
pub struct Rcc(Cell<TaskId>);

impl From<TaskId> for Rcc {
    fn from(t: TaskId) -> Self {
        Self(Cell::new(t))
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(u32)]
pub enum RccError {
    BadArg = 2,
}

impl From<u32> for RccError {
    fn from(x: u32) -> Self {
        match x {
            2 => RccError::BadArg,
            // Panicking here might be rude. TODO.
            _ => panic!(),
        }
    }
}

impl Rcc {
    /// Requests that the clock to a peripheral be turned on.
    ///
    /// This operation is idempotent and will be retried automatically should
    /// the RCC server crash while processing it.
    ///
    /// # Panics
    ///
    /// If the RCC server has died.
    pub fn enable_clock(&self, peripheral: Peripheral) {
        // We are unwrapping here because the RCC server should not return
        // BadArg for a valid member of the Peripheral enum.
        self.enable_clock_raw(peripheral as u32).unwrap()
    }

    pub fn enable_clock_raw(&self, index: u32) -> Result<(), RccError> {
        #[derive(AsBytes)]
        #[repr(C)]
        struct Request(U32<LittleEndian>);

        impl hl::Call for Request {
            const OP: u16 = Op::EnableClock as u16;
            type Response = ();
            type Err = RccError;
        }

        hl::send_with_retry(&self.0, &Request(U32::new(index)))
    }

    /// Requests that the clock to a peripheral be turned off.
    ///
    /// This operation is idempotent and will be retried automatically should
    /// the RCC server crash while processing it.
    ///
    /// # Panics
    ///
    /// If the RCC server has died.
    pub fn disable_clock(&self, peripheral: Peripheral) {
        // We are unwrapping here because the RCC server should not return
        // BadArg for a valid member of the Peripheral enum.
        self.disable_clock_raw(peripheral as u32).unwrap()
    }

    pub fn disable_clock_raw(&self, index: u32) -> Result<(), RccError> {
        #[derive(AsBytes)]
        #[repr(C)]
        struct Request(U32<LittleEndian>);

        impl hl::Call for Request {
            const OP: u16 = Op::DisableClock as u16;
            type Response = ();
            type Err = RccError;
        }

        hl::send_with_retry(&self.0, &Request(U32::new(index)))
    }

    /// Requests that the reset line to a peripheral be asserted.
    ///
    /// This operation is idempotent and will be retried automatically should
    /// the RCC server crash while processing it.
    ///
    /// # Panics
    ///
    /// If the RCC server has died.
    pub fn enter_reset(&self, peripheral: Peripheral) {
        // We are unwrapping here because the RCC server should not return
        // BadArg for a valid member of the Peripheral enum.
        self.enter_reset_raw(peripheral as u32).unwrap()
    }

    pub fn enter_reset_raw(&self, index: u32) -> Result<(), RccError> {
        #[derive(AsBytes)]
        #[repr(C)]
        struct Request(U32<LittleEndian>);

        impl hl::Call for Request {
            const OP: u16 = Op::EnterReset as u16;
            type Response = ();
            type Err = RccError;
        }

        hl::send_with_retry(&self.0, &Request(U32::new(index)))
    }

    /// Requests that the reset line to a peripheral be deasserted.
    ///
    /// This operation is idempotent and will be retried automatically should
    /// the RCC server crash while processing it.
    ///
    /// # Panics
    ///
    /// If the RCC server has died.
    pub fn leave_reset(&self, peripheral: Peripheral) {
        // We are unwrapping here because the RCC server should not return
        // BadArg for a valid member of the Peripheral enum.
        self.leave_reset_raw(peripheral as u32).unwrap()
    }

    pub fn leave_reset_raw(&self, index: u32) -> Result<(), RccError> {
        #[derive(AsBytes)]
        #[repr(C)]
        struct Request(U32<LittleEndian>);

        impl hl::Call for Request {
            const OP: u16 = Op::LeaveReset as u16;
            type Response = ();
            type Err = RccError;
        }

        hl::send_with_retry(&self.0, &Request(U32::new(index)))
    }
}

//
// A few macros for purposes of defining the Peripheral enum in terms that our
// driver is expecting:
//
// - AHB1ENR[31:0] are indices 31-0.
// - AHB2ENR[31:0] are indices 63-32.
// - AHB3ENR[31:0] are indices 95-64.
// - AHB4ENR[31:0] are indices 127-96.
// - APB1LENR[31:0] are indices 159-128.
// - APB1HENR[31:0] are indices 191-160.
// - APB2ENR[31:0] are indices 223-192.
// - APB3ENR[31:0] are indices 255-224.
// - APB4ENR[31:0] are indices 287-256.
//
macro_rules! ahb1 {
    ($bit:literal) => {
        (0 * 32) + $bit
    };
}

macro_rules! ahb2 {
    ($bit:literal) => {
        (1 * 32) + $bit
    };
}

/*
// TODO - No AHB3 with STM32F411?
macro_rules! ahb3 {
    ($bit:literal) => {
        (2 * 32) + $bit
    };
}
*/

macro_rules! apb1 {
    ($bit:literal) => {
        (3 * 32) + $bit
    };
}

macro_rules! apb2 {
    ($bit:literal) => {
        (4 * 32) + $bit
    };
}

/// Peripheral numbering.
///
/// Peripheral bit numbers per the STM32F3/4 documentation.
///
/// TODO THIS IS ONLY STM32F411!
///
/// STM32F4 PART    SECTION
/// 11              6.3.9
#[derive(Copy, Clone, Eq, PartialEq, Debug, FromPrimitive, AsBytes)]
#[repr(u32)]
pub enum Peripheral {
    Dma2 = ahb1!(22),
    Dma1 = ahb1!(21),
    Crc = ahb1!(12),
    GpioH = ahb1!(7),
    GpioE = ahb1!(4),
    GpioD = ahb1!(3),
    GpioC = ahb1!(2),
    GpioB = ahb1!(1),
    GpioA = ahb1!(0),

    OtgFs = ahb2!(7),

    Pwr = apb1!(28),
    I2c3 = apb1!(23),
    I2c2 = apb1!(22),
    I2c1 = apb1!(21),
    Usart2 = apb1!(17),
    Spi3 = apb1!(15),
    Spi2 = apb1!(14),
    Wwdg = apb1!(11),
    Tim5 = apb1!(3),
    Tim4 = apb1!(2),
    Tim3 = apb1!(1),
    Tim2 = apb1!(0),

    Spi5 = apb2!(20),
    Tim11 = apb2!(18),
    Tim10 = apb2!(17),
    Tim9 = apb2!(16),
    SysCfg = apb2!(14),
    Spi4 = apb2!(13),
    Spi1 = apb2!(12),
    Sdio = apb2!(11),
    Adc1 = apb2!(8),
    Usart6 = apb2!(5),
    Usart1 = apb2!(4),
    Tim1 = apb2!(0),
}
