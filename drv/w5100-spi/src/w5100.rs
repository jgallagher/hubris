// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::W5100Error;
use bitflags::bitflags;
use drv_spi_api::{CsState, SpiDevice};
use userlib::*;

mod socket;
mod spi_stream;

use self::spi_stream::SpiStream;

pub(crate) use self::socket::Command as SocketCommand;
pub(crate) use self::socket::Mode as SocketMode;
pub(crate) use self::socket::Status as SocketStatus;
pub(crate) use self::socket::{Socket, SocketIndex};

// There is a total of 8KiB tx and 8KiB rx buffer space. We support:
//  1 socket with 8KiB tx/rx
//  2 sockets with 4KiB tx/rx each
//  4 sockets with 2KiB tx/rx each
// Asymmetric tx/rx and giving more space to socket 0 are supported by the
// hardware, but not us (for now?).
#[derive(Copy, Clone)]
#[repr(u8)]
#[allow(dead_code)] // We don't expose this and hard-code a single one to use
pub(crate) enum SocketConfig {
    OneSocket8Kib,
    TwoSockets4KiB,
    FourSockets2KiB,
}

// TODO: DHCP client for network configuration
pub(crate) enum NetworkConfig {
    Static {
        ip: [u8; 4],
        subnet: [u8; 4],
        gateway: [u8; 4],
    },
}

pub(crate) struct W5100 {
    device: SpiDevice,
    socket_config: SocketConfig,
}

impl W5100 {
    pub(crate) fn reset(
        device: SpiDevice,
        socket_config: SocketConfig,
        mac: [u8; 6],
    ) -> Result<Self, W5100Error> {
        let this = Self {
            device,
            socket_config,
        };
        this.reset_raw()?;
        this.configure_socket_buffers()?;
        this.configure_mac_address(mac)?;
        Ok(this)
    }

    pub(crate) fn socket_config(&self) -> SocketConfig {
        self.socket_config
    }

    pub(crate) fn set_network_config(
        &self,
        network_config: &NetworkConfig,
    ) -> Result<(), W5100Error> {
        match network_config {
            NetworkConfig::Static {
                ip,
                subnet,
                gateway,
            } => {
                self.write_reg(WriteableRegister::Sipr(*ip))?;
                self.write_reg(WriteableRegister::Subr(*subnet))?;
                self.write_reg(WriteableRegister::Gar(*gateway))?;
            }
        }
        Ok(())
    }

    fn reset_raw(&self) -> Result<(), W5100Error> {
        self.write_reg(WriteableRegister::Mr(Mode::RESET))?;

        // Wait until Mr register is reset
        for _ in 0..20 {
            if self.read_reg_u8(ReadableRegisterU8::Mr)? == Mode::empty().bits {
                return self.confirm_reset();
            }
            hl::sleep_for(1);
        }

        Err(W5100Error::ResetFailed)
    }

    fn confirm_reset(&self) -> Result<(), W5100Error> {
        // attempt to read/write MR; this serves as a sanity check on our SPI
        // setup and presence of the device.
        for mode in [
            Mode::PING_BLOCK,
            Mode::PING_BLOCK | Mode::ADDRESS_AUTO_INCREMENT,
            Mode::empty(),
        ] {
            self.write_reg(WriteableRegister::Mr(mode))?;
            if self.read_reg_u8(ReadableRegisterU8::Mr)? != mode.bits() {
                return Err(W5100Error::ResetFailed);
            }
        }
        Ok(())
    }

    fn configure_socket_buffers(&self) -> Result<(), W5100Error> {
        // See RMSR docs in W5100 datasheet
        let mask = match self.socket_config {
            SocketConfig::OneSocket8Kib => 0x03,
            SocketConfig::TwoSockets4KiB => 0x0a,
            SocketConfig::FourSockets2KiB => 0x55,
        };
        self.write_reg(WriteableRegister::Rmsr(mask))?;
        self.write_reg(WriteableRegister::Tmsr(mask))?;
        Ok(())
    }

    fn configure_mac_address(&self, mac: [u8; 6]) -> Result<(), W5100Error> {
        self.write_reg(WriteableRegister::Shar(mac))
    }

    fn write_reg(&self, reg: WriteableRegister) -> Result<(), W5100Error> {
        match reg {
            WriteableRegister::Mr(mode) => {
                self.write_raw(0x0000, &[mode.bits()])
            }
            WriteableRegister::Gar(val) => self.write_raw(0x0001, &val),
            WriteableRegister::Subr(val) => self.write_raw(0x0005, &val),
            WriteableRegister::Shar(val) => self.write_raw(0x0009, &val),
            WriteableRegister::Sipr(val) => self.write_raw(0x000f, &val),
            WriteableRegister::Rmsr(val) => self.write_raw(0x001a, &[val]),
            WriteableRegister::Tmsr(val) => self.write_raw(0x001b, &[val]),
        }
    }

    fn read_reg_u8(&self, reg: ReadableRegisterU8) -> Result<u8, W5100Error> {
        let addr = match reg {
            ReadableRegisterU8::Mr => 0x0000,
        };
        self.read_u8(addr)
    }

    fn write_raw(&self, addr: u16, data: &[u8]) -> Result<(), W5100Error> {
        // We never try to write 0 bytes.
        assert!(!data.is_empty());

        let mut cmd = SpiStream::write(addr, data[0]);
        self.exchange_raw(&cmd)?;

        for &b in &data[1..] {
            cmd.set_data(b);
            cmd.increment_addr();
            self.exchange_raw(&cmd)?;
        }

        Ok(())
    }

    fn write_raw_lease(
        &self,
        addr: u16,
        len: u16,
        buf: &idol_runtime::Leased<idol_runtime::R, [u8]>,
        pos: usize,
    ) -> Result<(), idol_runtime::RequestError<W5100Error>> {
        // We never try to write 0 bytes, and `len` should fit in `buf` starting at `pos`.
        assert!(len > 0);
        assert!(usize::from(len) <= buf.len() - pos);

        let mut cmd = SpiStream::write(addr, 0);
        for i in 0..usize::from(len) {
            cmd.set_data(buf.read_at(pos + i).ok_or(
                idol_runtime::RequestError::Fail(
                    idol_runtime::ClientError::WentAway,
                ),
            )?);
            self.exchange_raw(&cmd)?;
            cmd.increment_addr();
        }

        Ok(())
    }

    fn read_u8(&self, addr: u16) -> Result<u8, W5100Error> {
        let mut out = [0];
        self.read_raw(addr, &mut out)?;
        Ok(out[0])
    }

    fn read_u16(&self, addr: u16) -> Result<u16, W5100Error> {
        let mut out = [0; 2];
        self.read_raw(addr, &mut out)?;
        Ok(u16::from_be_bytes(out))
    }

    fn read_raw(&self, addr: u16, out: &mut [u8]) -> Result<(), W5100Error> {
        // We never try to read 0 bytes.
        assert!(!out.is_empty());

        let mut cmd = SpiStream::read(addr);
        out[0] = self.exchange_raw(&cmd)?;

        for b in &mut out[1..] {
            cmd.increment_addr();
            *b = self.exchange_raw(&cmd)?;
        }

        Ok(())
    }

    fn read_raw_lease(
        &self,
        addr: u16,
        len: u16,
        lease: &idol_runtime::Leased<idol_runtime::W, [u8]>,
        pos: usize,
    ) -> Result<(), idol_runtime::RequestError<W5100Error>> {
        // We never try to read 0 bytes, and `len` should fit in `lease` starting at `pos`.
        assert!(len > 0);
        assert!(usize::from(len) <= lease.len() - pos);

        let mut cmd = SpiStream::read(addr);
        for i in 0..usize::from(len) {
            let value = self.exchange_raw(&cmd)?;
            lease.write_at(pos + i, value).map_err(|()| {
                idol_runtime::RequestError::Fail(
                    idol_runtime::ClientError::WentAway,
                )
            })?;
            cmd.increment_addr(); // increments addr on last iteration; should we guard this with an `if` or break the loop up like we do in `read_raw()`?
        }

        Ok(())
    }

    fn exchange_raw(&self, cmd: &SpiStream) -> Result<u8, W5100Error> {
        let mut out = [0; 4];

        // W5100 requires us to assert/deassert CS around each 32-bit stream
        let _lock = self.device.lock_auto(CsState::Asserted)?;
        self.device.exchange(cmd.as_bytes(), &mut out)?;
        Ok(out[3])
    }
}

bitflags! {
    struct Mode: u8 {
        const RESET = 0x80;
        const PING_BLOCK = 0x10;
        //const PPPOE = 0x40; // not supported
        const ADDRESS_AUTO_INCREMENT = 0x02; // not really supported, but we use it in `confirm_reset()`
        //const INDIRECT_BUS = 0x01; // not supported
    }
}

enum WriteableRegister {
    Mr(Mode),      // Mode
    Gar([u8; 4]),  // Gateway IP address
    Subr([u8; 4]), // Subnet mask
    Shar([u8; 6]), // MAC ("source hardware address")
    Sipr([u8; 4]), // IP address ("source IP address")
    Rmsr(u8),      // RX memory size mask
    Tmsr(u8),      // TX memory size mask
}

enum ReadableRegisterU8 {
    Mr,
}
