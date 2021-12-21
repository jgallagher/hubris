// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A driver for the STM32F3/4 (only 4 currently) SPI, in host mode.
//!
//! This is the core logic, separated from the IPC server. The peripheral also
//! supports I2S, which we haven't bothered implementing because we don't have a
//! need for it.
//!
//! # Clocking
//!
//! The SPI block has no fewer than three clock domains.
//!
//! 1. `pclk` contains most of the control logic and operates at the APB
//!    frequency.
//!
//! 2. `ker_ck` contains the clock generator and is driven as a "kernel clock"
//!    from the RCC -- there is a separate mux there to choose its source.
//!
//! 3. The "serial interface domain" (no catchy abbreviation provided) is
//!    clocked at the external SCK rate. This is derived from `ker_ck` in host
//!    role.
//!
//! In host role, the SPI needs to have at least `ker_ck` running to do useful
//! work.
//!
//! # Automagic CRC generation
//!
//! We do not currently support the hardware's automatic CRC features.

#![no_std]

#[cfg(feature = "f411")]
use stm32f4::stm32f411 as device;

pub struct Spi {
    /// Pointer to our register block.
    ///
    /// This is not a `SPIx` type from the `stm32fx` crate because then we're
    /// generic for no good reason and type parameters multiply. Ew.
    ///
    /// Pay no heed to the 1 in `spi1` -- that's what the common module is
    /// called.
    reg: &'static device::spi1::RegisterBlock,
}

impl From<&'static device::spi1::RegisterBlock> for Spi {
    fn from(reg: &'static device::spi1::RegisterBlock) -> Self {
        Self { reg }
    }
}

impl Spi {
    pub fn initialize(
        &mut self,
        br: device::spi1::cr1::BR_A,
        dff: device::spi1::cr1::DFF_A,
        bidimode: device::spi1::cr1::BIDIMODE_A,
        rxonly: device::spi1::cr1::RXONLY_A,
        lsbfirst: device::spi1::cr1::LSBFIRST_A,
        cpha: device::spi1::cr1::CPHA_A,
        cpol: device::spi1::cr1::CPOL_A,
    ) {
        // Expected preconditions:
        // - GPIOs configured to proper AF etc - we cannot do this, because we
        // cannot presume to have either direct GPIO access _or_ IPC access.
        // - Clock on, reset off - again, we can't do this directly.

        // Write CR1/CR2 to configure
        #[rustfmt::skip]
        self.reg.cr1.write(|w| {
            w
                .bidimode().variant(bidimode)
                .dff().variant(dff)
                .rxonly().variant(rxonly)
                .lsbfirst().variant(lsbfirst)
                .br().variant(br)
                .cpol().variant(cpol)
                .cpha().variant(cpha)
                // This bit determines if software manages SS (SSM = 1) or
                // hardware (SSM = 0). We currently only have a single attached
                // device in any of our `CONFIG`s below, so we'll just manage
                // this in software; we use GPIO pins as CS outputs.
                .ssm().set_bit().ssi().clear_bit()
                // This is currently a host-only driver.
                .mstr().set_bit()
                // Fields left at reset values:
                //   BIDIOE - bidirectional output enable; currently we really only support full duplex
                //   CRCEN/CRCNEXT - hardware CRC currently unsupported
                //   SPE - set in `enable()`
        });

        #[rustfmt::skip]
        self.reg.cr2.write(|w| {
            w
                .frf().variant(device::spi1::cr2::FRF_A::MOTOROLA)
                .ssoe().variant(device::spi1::cr2::SSOE_A::ENABLED)
                // Fields left at reset values:
                //   TXEIE - tx interrupts (TODO?)
                //   RXNEIE - rx interrupts (TODO?)
                //   ERRIE - error interrupts (TODO?)
                //   TXDMAEN - tx DMA (TODO?)
                //   RXDMAEN - rx DMA (TODO?)
        });

        self.reg.i2scfgr.write(|w| w.i2smod().clear_bit());
    }

    pub fn read_cr1(&self) -> u32 {
        self.reg.cr1.read().bits()
    }

    pub fn enable(&mut self) {
        self.reg.cr1.modify(|_, w| w.spe().set_bit());
    }

    pub fn disable(&mut self) {
        self.reg.cr1.modify(|_, w| w.spe().clear_bit());
    }

    pub fn can_rx(&self) -> bool {
        let sr = self.reg.sr.read();
        sr.rxne().bit()
    }

    pub fn can_tx(&self) -> bool {
        let sr = self.reg.sr.read();
        sr.txe().bit()
    }

    pub fn busy(&self) -> bool {
        let sr = self.reg.sr.read();
        sr.bsy().bit()
    }

    pub fn send16(&self, word: u16) {
        self.reg.dr.write(|w| w.dr().bits(word));
    }

    pub fn send8(&self, byte: u8) {
        self.send16(u16::from(byte))
    }

    pub fn recv16(&self) -> u16 {
        self.reg.dr.read().dr().bits()
    }

    pub fn recv8(&self) -> u8 {
        self.recv16() as u8
    }

    /*
    pub fn end(&mut self) {
        // Clear flags that tend to get set during transactions.
        self.reg.ifcr.write(|w| w.txtfc().set_bit());
        // Disable the transfer state machine.
        self.reg.cr1.modify(|_, w| w.spe().clear_bit());
        // Turn off interrupt enables.
        self.reg.ier.reset();

        // This is where we'd report errors (TODO). For now, just clear the
        // error flags, as they're sticky.
        self.reg.ifcr.write(|w| {
            w.ovrc()
                .set_bit()
                .udrc()
                .set_bit()
                .modfc()
                .set_bit()
                .tifrec()
                .set_bit()
        });
    }

    pub fn enable_transfer_interrupts(&mut self) {
        self.reg
            .ier
            .write(|w| w.txpie().set_bit().rxpie().set_bit().eotie().set_bit());
    }

    pub fn disable_can_tx_interrupt(&mut self) {
        self.reg.ier.modify(|_, w| w.txpie().clear_bit());
    }
    */
}
