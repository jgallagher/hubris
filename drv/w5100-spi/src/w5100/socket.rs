use super::{SocketConfig, W5100Error, W5100};
use bitflags::bitflags;
use userlib::FromPrimitive;

#[derive(Debug, FromPrimitive, PartialEq)]
#[repr(u8)]
pub(crate) enum Status {
    // Main status codes
    Closed = 0x00,
    Init = 0x13,
    Listen = 0x14,
    Established = 0x17,
    CloseWait = 0x1c,
    Udp = 0x22,
    IpRaw = 0x32,
    MacRaw = 0x42,
    Pppoe = 0x5f,

    // Ephemeral status codes
    SynSent = 0x15,
    SynRecv = 0x16,
    FinWait = 0x18,
    Closing = 0x1a,
    TimeWait = 0x1b,
    LastAck = 0x1d,
    ArpTcp = 0x11, // TODO datasheet doesn't clarify which of these 3 ARP values correspond to the three protocols; making a guess here.
    ArpUdp = 0x21,
    ArpIcmp = 0x31,
}

bitflags! {
    pub(crate) struct Mode: u8 {
        // UDP only
        const MULTICAST = 0x80;
        // RESERVED: 0x40
        const NO_DELAYED_ACK = 0x20; // TCP; see datasheet
        const IGMP_VERSION = 0x20; // If UDP and MULTICAST, 0 = IGMP v2, 1 = IGMP v1
        // RESERVED: 0x10
        const PROTO_CLOSED = 0x00;
        const PROTO_TCP = 0x01;
        const PROTO_UDP = 0x02;
        const PROTO_RAW = 0x03;
    }
}

#[repr(u8)]
#[allow(dead_code)] // TODO remove once we use all commands?
pub(crate) enum Command {
    Open = 0x01,
    Listen = 0x02,
    Connect = 0x04,
    Disconnect = 0x08,
    Close = 0x10,
    Send = 0x20,
    SendMac = 0x21,
    SendKeep = 0x22,
    Recv = 0x40,
}

#[derive(Clone, Copy, FromPrimitive)]
#[repr(u8)]
pub(crate) enum SocketIndex {
    Zero = 0,
    One = 1,
    Two = 2,
    Three = 3,
}

impl From<SocketIndex> for u8 {
    fn from(idx: SocketIndex) -> Self {
        idx as u8
    }
}

pub(crate) struct Socket<'a> {
    device: &'a W5100,
    register_offset: u16,
    tx_addr: u16,
    tx_size: u16,
    rx_addr: u16,
    rx_size: u16,
}

impl<'a> Socket<'a> {
    pub(crate) fn new(device: &'a W5100, index: SocketIndex) -> Self {
        let index = index as u16;

        let register_offset = index << 8;
        let buf_size = match device.socket_config() {
            SocketConfig::OneSocket8Kib => {
                assert!(index < 1);
                8192
            }
            SocketConfig::TwoSockets4KiB => {
                assert!(index < 2);
                4096
            }
            SocketConfig::FourSockets2KiB => {
                assert!(index < 4);
                2048
            }
        };

        Self {
            device,
            register_offset,
            tx_addr: index * buf_size + 0x4000,
            tx_size: buf_size,
            rx_addr: index * buf_size + 0x6000,
            rx_size: buf_size,
        }
    }

    pub(crate) fn index(&self) -> u8 {
        (self.register_offset >> 8) as u8
    }

    pub(crate) fn status(&self) -> Result<Status, W5100Error> {
        let status_raw = self.read_reg_u8(ReadableSocketRegisterU8::Sr)?;
        Status::from_u8(status_raw)
            .ok_or(W5100Error::UnknownSocketStatus(status_raw))
    }

    pub(crate) fn set_mode(&self, mode: Mode) -> Result<(), W5100Error> {
        self.write_reg(WriteableSocketRegister::Mr(mode))
    }

    pub(crate) fn send_command(
        &self,
        command: Command,
    ) -> Result<(), W5100Error> {
        self.write_reg(WriteableSocketRegister::Cr(command))
    }

    pub(crate) fn set_source_port(&self, port: u16) -> Result<(), W5100Error> {
        self.write_reg(WriteableSocketRegister::Port(port))
    }

    // Read up to `out.len()` bytes from this socket's RX buffer and informs the
    // device we've received it. Returns `Ok(0)` if there is no data in the
    // buffer; this does not mean there may not be data in the future.
    pub(crate) fn recv(
        &self,
        out: &idol_runtime::Leased<idol_runtime::W, [u8]>,
    ) -> Result<usize, idol_runtime::RequestError<W5100Error>> {
        let nready = self.read_reg_u16(ReadableSocketRegisterU16::RxRsr)?;
        let nready = usize::min(out.len(), usize::from(nready)) as u16;
        if nready == 0 {
            return Ok(0);
        }

        let rd_pointer = self.read_reg_u16(ReadableSocketRegisterU16::RxRd)?;

        let offset = rd_pointer & (self.rx_size - 1);
        if offset + nready > self.rx_size {
            // available data wraps around; read to the end first, then from the beginning
            let to_end = self.rx_size - offset;
            self.device.read_raw_lease(
                self.rx_addr + offset,
                to_end,
                out,
                0,
            )?;
            self.device.read_raw_lease(
                self.rx_addr,
                nready - to_end,
                out,
                usize::from(to_end),
            )?;
        } else {
            self.device.read_raw_lease(
                self.rx_addr + offset,
                nready,
                out,
                0,
            )?;
        }

        // update read pointer and inform device we've consumed data
        self.write_reg(WriteableSocketRegister::RxRd(
            rd_pointer.wrapping_add(nready),
        ))?;
        self.send_command(Command::Recv)?;
        Ok(usize::from(nready))
    }

    // Write up to `buf.len()` bytes into this socket's TX buffer and instructs
    // the device to send it. Returns the number of bytes written on success.
    pub(crate) fn send(
        &self,
        buf: &idol_runtime::Leased<idol_runtime::R, [u8]>,
    ) -> Result<usize, idol_runtime::RequestError<W5100Error>> {
        let tx_free = self.read_reg_u16(ReadableSocketRegisterU16::TxFsr)?;
        let tx_free = usize::min(buf.len(), usize::from(tx_free)) as u16;
        if tx_free == 0 {
            return Ok(0);
        }

        let tx_pointer = self.read_reg_u16(ReadableSocketRegisterU16::TxWr)?;
        let offset = tx_pointer & (self.tx_size - 1);

        if offset + tx_free > self.tx_size {
            // available space wraps around; write to end first, then to beginning
            let to_end = self.tx_size - offset;

            self.device.write_raw_lease(
                self.tx_addr + offset,
                to_end,
                buf,
                0,
            )?;
            self.device.write_raw_lease(
                self.tx_addr,
                tx_free - to_end,
                buf,
                usize::from(to_end),
            )?;
        } else {
            self.device.write_raw_lease(
                self.tx_addr + offset,
                tx_free,
                buf,
                0,
            )?;
        }

        // update write pointer and inform device we've inserted data
        self.write_reg(WriteableSocketRegister::TxWr(
            tx_pointer.wrapping_add(tx_free),
        ))?;
        self.send_command(Command::Send)?;
        Ok(usize::from(tx_free))
    }

    fn write_reg(
        &self,
        reg: WriteableSocketRegister,
    ) -> Result<(), W5100Error> {
        match reg {
            WriteableSocketRegister::Mr(mode) => self
                .device
                .write_raw(0x0400 + self.register_offset, &[mode.bits()]),
            WriteableSocketRegister::Cr(cmd) => self
                .device
                .write_raw(0x0401 + self.register_offset, &[cmd as u8]),
            WriteableSocketRegister::Port(port) => self
                .device
                .write_raw(0x0404 + self.register_offset, &port.to_be_bytes()),
            WriteableSocketRegister::TxWr(val) => self
                .device
                .write_raw(0x0424 + self.register_offset, &val.to_be_bytes()),
            WriteableSocketRegister::RxRd(val) => self
                .device
                .write_raw(0x0428 + self.register_offset, &val.to_be_bytes()),
        }
    }

    fn read_reg_u8(
        &self,
        reg: ReadableSocketRegisterU8,
    ) -> Result<u8, W5100Error> {
        let mut addr = match reg {
            ReadableSocketRegisterU8::Sr => 0x0403,
        };
        addr += self.register_offset;
        self.device.read_u8(addr)
    }

    fn read_reg_u16(
        &self,
        reg: ReadableSocketRegisterU16,
    ) -> Result<u16, W5100Error> {
        let mut addr = match reg {
            ReadableSocketRegisterU16::TxFsr => 0x0420,
            ReadableSocketRegisterU16::TxWr => 0x0424,
            ReadableSocketRegisterU16::RxRsr => 0x0426,
            ReadableSocketRegisterU16::RxRd => 0x0428,
        };
        addr += self.register_offset;
        self.device.read_u16(addr)
    }
}

enum WriteableSocketRegister {
    Mr(Mode),    // socket N mode
    Cr(Command), // socket N command
    Port(u16),   // socket N source port
    TxWr(u16),   // socket N transmit pointer
    RxRd(u16),   // socket N read pointer
}

enum ReadableSocketRegisterU8 {
    Sr, // socket N status
}

enum ReadableSocketRegisterU16 {
    TxFsr, // socket N transmit free size
    TxWr,  // socket N transmit pointer
    RxRsr, // socket N received size
    RxRd,  // socket N read pointer
}
