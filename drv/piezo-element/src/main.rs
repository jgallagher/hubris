// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A driver for a GPIO-controlled piezo element. Assumes a connection to GPIO
//! pin PC6, and we use general purpose timer Tim3 in PWM mode to control the
//! frequency.
//!
//! # IPC protocol
//!
//! ## `piezo_on` (1)
//!
//! Turn the piezo on at a given frequency.
//!
//! Request message format: single `u16` giving frequency in Hz. Lowest support
//! frequency is 16Hz. Any lower value will give a `BadArg` response.
//!
//! ## `piezo_off` (2)
//!
//! Turn the piezo off.
//!
//! Request message format: takes no arguments.

#![no_std]
#![no_main]

use core::mem;
use idol_runtime::RequestError;
use static_assertions::const_assert_eq;
use userlib::*;
use zerocopy::AsBytes;

task_slot!(GPIO, gpio_driver);
task_slot!(RCC, rcc_driver);

#[derive(Copy, Clone, Debug, FromPrimitive)]
#[repr(u32)]
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

impl From<PiezoError> for u16 {
    fn from(x: PiezoError) -> Self {
        x as u16
    }
}

#[export_name = "main"]
fn main() -> ! {
    enable_output_pin();
    let timer = Timer::setup();

    // Field messages.
    // Ensure our buffer is aligned properly for a u16 by declaring it as one.
    let mut buffer = 0u16;
    const_assert_eq!(mem::size_of::<u16>(), idl::INCOMING_SIZE);
    let mut server = ServerImpl { timer };
    loop {
        idol_runtime::dispatch(buffer.as_bytes_mut(), &mut server);
    }
}

struct ServerImpl {
    timer: Timer,
}

impl idl::InOrderPiezoImpl for ServerImpl {
    fn piezo_on(
        &mut self,
        _: &RecvMessage,
        freq_hz: u16,
    ) -> Result<(), RequestError<PiezoError>> {
        self.timer.set_frequency(freq_hz)?;
        Ok(())
    }

    fn piezo_off(
        &mut self,
        _: &RecvMessage,
    ) -> Result<(), RequestError<PiezoError>> {
        self.timer.disable();
        Ok(())
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Timer(&'static stm32f4::stm32f411::tim3::RegisterBlock);

impl Timer {
    fn setup() -> Self {
        // TODO should we have a timer driver? for now poke it directly

        // turn on tim3
        use drv_stm32fx_rcc_api::{Peripheral, Rcc};
        let rcc_driver = Rcc::from(RCC.get_task_id());
        rcc_driver.enable_clock(Peripheral::Tim3);
        rcc_driver.leave_reset(Peripheral::Tim3); // TODO what does this do? Do we need it?

        let tim3 = unsafe { &*stm32f4::stm32f411::TIM3::ptr() };

        tim3.ccmr1_output().write(|w| {
            w.oc1m().bits(0b111) // PWM mode 2 (TODO what's the difference between 1 and 2?)
                                 //.oc1pe().set_bit() // TODO do we want to turn on preload?
        });
        tim3.ccer.write(|w| w.cc1p().set_bit().cc1e().set_bit()); // activate timer channel 1

        // Default clock rate is 16MHz, and we don't currently bump that up on
        // boot.  Set our prescaler to divide by 16 (down to 1MHz) so we can
        // accept frequencies down to ceil(1MHz / u16::MAX) = 16Hz. The
        // prescaler has an implicit +1.
        tim3.psc.write(|w| w.psc().bits(15));

        // Start out disabled.
        let this = Self(tim3);
        this.disable();

        // Setup complete - enable tim3
        this.0.cr1.write(|w| w.cen().set_bit());

        this
    }

    fn disable(self) {
        // Set duty cycle to 0%.
        self.0.ccr1.write(|w| w.ccr().bits(0));
        self.0.arr.write(|w| w.arr().bits(1));
    }

    fn set_frequency(self, freq: u16) -> Result<(), PiezoError> {
        // We set our timer to 1MHz, so need to set arr to (1MHz / freq - 1). If
        // freq is < 16, this division won't fit in a u16; we could change the
        // prescaler to handle low frequencies, but for now we'll just punt and
        // return an error.
        const TIMER_FREQ: u32 = 1_000_000;

        if freq < 16 {
            return Err(PiezoError::FrequencyTooLow);
        }

        let arr = ((TIMER_FREQ / u32::from(freq)) - 1) as u16;
        self.0.arr.write(|w| w.arr().bits(arr));
        self.0.ccr1.write(|w| w.ccr().bits(arr / 2 - 1)); // 50% duty cycle; TODO is the -1 correct? probably can't tell...

        Ok(())
    }
}

fn enable_output_pin() {
    use drv_stm32fx_gpio_api::*;

    const OUTPUT_PIN: PinSet = Port::C.pin(6);

    let gpio_driver = GPIO.get_task_id();
    let gpio_driver = Gpio::from(gpio_driver);
    gpio_driver.set_to(OUTPUT_PIN, false).unwrap();
    gpio_driver
        .configure_alternate(
            OUTPUT_PIN,
            OutputType::PushPull,
            Speed::High,
            Pull::None,
            Alternate::AF2, // Tim3 channel 1
        )
        .unwrap();
}

#[allow(dead_code)] // TODO we only use `INCOMING_SIZE` in a const assert, which the compiler thinks is dead?
mod idl {
    use super::PiezoError;

    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}
