// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use drv_spi_api::{CsState, Spi, SpiDevice, SpiError};
use userlib::*;

task_slot!(SPI_DRIVER, spi2_driver);

#[derive(Debug)]
enum W5100Error {
    ResetFailed,
    SpiError(SpiError),
}

impl From<SpiError> for W5100Error {
    fn from(err: SpiError) -> Self {
        Self::SpiError(err)
    }
}

const NETWORK_CONFIG: NetworkConfig = NetworkConfig {
    ip: [10, 68, 46, 2],
    subnet_mask: [255, 255, 255, 0],
    gateway: [10, 68, 46, 1],
    mac: [0x90, 0xa2, 0xda, 0x00, 0xdc, 0x5f],
};

#[export_name = "main"]
pub fn main() -> ! {
    const DEVICE_INDEX: u8 = 0;

    let spi = SPI_DRIVER.get_task_id();
    let spi = Spi::from(spi);

    // Per the Arduino implementation for the ethernet shield, we may need to
    // wait up to 560ms and then toggle CS for the w5100 chip to wake up
    // properly.
    hl::sleep_for(560);
    spi.lock(DEVICE_INDEX, CsState::Asserted).unwrap();
    spi.release().unwrap();

    loop {
        hl::sleep_for(1000);

        let device = spi.device(DEVICE_INDEX);
        let _w5100 =
            W5100::new(device, SocketConfig::OneSocket8Kib, &NETWORK_CONFIG)
                .unwrap();

        hl::sleep_for(5_000);
    }
}

struct W5100 {
    device: SpiDevice,
    socket_config: SocketConfig,
    // W5100 in SPI mode operates on 32-bit streams; we must always use
    // `exchange()` from the SPI driver. We pass our 32-bit buf for each
    // exchange operation. Writes do not care about the contents of `buf`; reads
    // get their data byte from `self.buf[3]` after an exchange.
    buf: [u8; 4],
}

impl W5100 {
    fn new(
        device: SpiDevice,
        socket_config: SocketConfig,
        network_config: &NetworkConfig,
    ) -> Result<Self, W5100Error> {
        let mut this = Self {
            device,
            socket_config,
            buf: [0; 4],
        };
        this.reset()?;
        this.set_socket_config()?;
        this.set_network_config(network_config)?;
        Ok(this)
    }

    fn reset(&mut self) -> Result<(), W5100Error> {
        self.write(Register::Mr.base_addr(), &[0x80])?;

        let mut mr_value = [0; 1];
        for attempt in 0..20 {
            self.read(Register::Mr.base_addr(), &mut mr_value)?;
            sys_log!("attempt {}: mr = {:#x}", attempt, mr_value[0]);
            if mr_value[0] == 0 {
                return self.sanity_check_mr_access();
            }
            hl::sleep_for(1); // TODO necessary?
        }

        Err(W5100Error::ResetFailed)
    }

    // confirm we can read/write MR; this serves as a check for our SPI setup
    // and that the device is present.
    fn sanity_check_mr_access(&mut self) -> Result<(), W5100Error> {
        for val in [0x10, 0x12, 0x00] {
            self.write(Register::Mr.base_addr(), &[val])?;
            let mut new_val = [0; 1];
            self.read(Register::Mr.base_addr(), &mut new_val)?;
            sys_log!("after writing {:#x}, mr = {:#x}", val, new_val[0]);
            if new_val[0] != val {
                return Err(W5100Error::ResetFailed);
            }
        }
        Ok(())
    }

    fn set_socket_config(&mut self) -> Result<(), W5100Error> {
        let mut before = [0; 2];
        self.read(Register::Rmsr.base_addr(), &mut before[0..1])?;
        self.read(Register::Tmsr.base_addr(), &mut before[1..2])?;

        // See RMSR docs in W5100 datasheet
        let mask = match self.socket_config {
            SocketConfig::OneSocket8Kib => 0x03,
            SocketConfig::TwoSockets4KiB => 0x0a,
            SocketConfig::FourSockets2KiB => 0x55,
        };

        self.write(Register::Rmsr.base_addr(), &[mask])?;
        self.write(Register::Tmsr.base_addr(), &[mask])?;

        let mut after = [0; 2];
        self.read(Register::Rmsr.base_addr(), &mut after[0..1])?;
        self.read(Register::Tmsr.base_addr(), &mut after[1..2])?;
        sys_log!(
            "socket config: {:#x} rx, {:#x} tx -> {:#x} rx, {:#x} tx",
            before[0],
            before[1],
            after[0],
            after[1]
        );

        Ok(())
    }

    fn set_network_config(
        &mut self,
        network_config: &NetworkConfig,
    ) -> Result<(), W5100Error> {
        self.write(Register::Gar.base_addr(), &network_config.gateway)?;
        self.write(Register::Subr.base_addr(), &network_config.subnet_mask)?;
        self.write(Register::Shar.base_addr(), &network_config.mac)?;
        self.write(Register::Sipr.base_addr(), &network_config.ip)?;

        let mut buf = [0; 6];
        self.read(Register::Gar.base_addr(), &mut buf[..4])?;
        sys_log!("gateway = [{}, {}, {}, {}]", buf[0], buf[1], buf[2], buf[3]);
        self.read(Register::Subr.base_addr(), &mut buf[..4])?;
        sys_log!("subnet = [{}, {}, {}, {}]", buf[0], buf[1], buf[2], buf[3]);
        self.read(Register::Shar.base_addr(), &mut buf[..6])?;
        sys_log!(
            "mac = [{:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}]",
            buf[0],
            buf[1],
            buf[2],
            buf[3],
            buf[4],
            buf[5]
        );
        self.read(Register::Sipr.base_addr(), &mut buf[..4])?;
        sys_log!("ip = [{}, {}, {}, {}]", buf[0], buf[1], buf[2], buf[3]);
        Ok(())
    }

    fn write(&mut self, addr: u16, data: &[u8]) -> Result<(), W5100Error> {
        // We never try to write 0 bytes.
        assert!(!data.is_empty());

        let mut cmd = SpiStream::write(addr, data[0]);
        self.exchange(&cmd)?;

        for &b in &data[1..] {
            // increment addr and set our outgoing data byte
            cmd.increment_addr();
            *cmd.data() = b;
            self.exchange(&cmd)?;
        }

        Ok(())
    }

    fn read(&mut self, addr: u16, out: &mut [u8]) -> Result<(), W5100Error> {
        // We never try to read 0 bytes.
        assert!(!out.is_empty());

        let mut cmd = SpiStream::read(addr);
        self.exchange(&cmd)?;
        out[0] = self.buf[3];

        for b in &mut out[1..] {
            cmd.increment_addr();
            self.exchange(&cmd)?;
            *b = self.buf[3];
        }

        Ok(())
    }

    fn exchange(&mut self, cmd: &SpiStream) -> Result<(), W5100Error> {
        // W5100 requires us to assert/deassert CS around each 32-bit stream
        let _lock = self.device.lock_auto(CsState::Asserted)?;

        // We should be able to do this:
        //
        // ```
        // self.device.exchange(cmd.as_bytes(), &mut self.buf)?;
        // ```
        //
        // but we sporadically get stuck in SPI rx if we do - maybe something
        // flaky with w5100 if our tx gets too far ahead of rx? Arduino ethernet
        // library has this same workaround for this chip: only tx/rx a single
        // byte at a time.
        //
        // Might be able to revist this once we add interrupt support to our SPI
        // driver?
        for (cmd, buf) in cmd.as_bytes().chunks(1).zip(self.buf.chunks_mut(1)) {
            self.device.exchange(cmd, buf)?;
        }
        Ok(())
    }
}

// See W5100 datasheet section 6.3.2; each read/write is a 32-bit stream
// containing a 1-byte opcode, 2-byte address, and 1-byte data.
#[repr(transparent)]
struct SpiStream([u8; 4]);

impl SpiStream {
    const OP_WRITE: u8 = 0xf0;
    const OP_READ: u8 = 0x0f;

    fn write(addr: u16, data: u8) -> Self {
        Self([Self::OP_WRITE, (addr >> 8) as u8, addr as u8, data])
    }

    fn read(addr: u16) -> Self {
        Self([Self::OP_READ, (addr >> 8) as u8, addr as u8, 0])
    }

    fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    fn data(&mut self) -> &mut u8 {
        &mut self.0[3]
    }

    // panics if our address is currently 0xffff
    fn increment_addr(&mut self) {
        self.0[2] = self.0[2].wrapping_add(1);
        if self.0[2] == 0 {
            self.0[1] += 1; // TODO confirm this panics on overflow?
        }
    }
}

// There is a total of 8KiB tx and 8KiB rx buffer space. We support:
//  1 socket with 8KiB tx/rx
//  2 sockets with 4KiB tx/rx each
//  4 sockets with 2KiB tx/rx each
// Asymmetric tx/rx are presumably supported by the hardware, but not us.
enum SocketConfig {
    OneSocket8Kib,
    TwoSockets4KiB,
    FourSockets2KiB,
}

struct NetworkConfig {
    // TODO IpAddr type?
    ip: [u8; 4],
    subnet_mask: [u8; 4],
    gateway: [u8; 4],
    mac: [u8; 6],
}

#[derive(Copy, Clone)]
#[repr(u16)]
enum Register {
    Mr = 0x0000,   // Mode
    Gar = 0x0001,  // Gateway address (4 bytes)
    Subr = 0x0005, // Subnet mask (4 bytes)
    Shar = 0x0009, // Source hardware address (MAC) (6 bytes)
    Sipr = 0x000f, // Source IP address (4 bytes)
    Rmsr = 0x001a, // RX memory size
    Tmsr = 0x001b, // TX memory size
}

impl Register {
    fn base_addr(self) -> u16 {
        self as u16
    }
}
