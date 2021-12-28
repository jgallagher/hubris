// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use drv_spi_api::{CsState, Spi, SpiError};
use userlib::*;

mod tcp;
mod w5100;

use self::tcp::TcpSocket;
use self::w5100::{
    NetworkConfig, SocketConfig, SocketIndex, SocketStatus, W5100,
};

task_slot!(SPI_DRIVER, spi2_driver);

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

impl From<SpiError> for W5100Error {
    fn from(err: SpiError) -> Self {
        Self::SpiError(err)
    }
}

impl From<W5100Error> for u16 {
    fn from(err: W5100Error) -> Self {
        match err {
            W5100Error::ResetFailed => 0x0001,
            W5100Error::OpenFailed => 0x0002,
            W5100Error::ListenFailed => 0x0003,
            W5100Error::PeerClosed => 0x0004,
            W5100Error::BadSocketNumber => 0x0005,
            W5100Error::SpiError(inner) => 0x0100 | u16::from(inner),
            W5100Error::BadSocketState(inner) => 0x0200 | u16::from(inner),
            W5100Error::UnknownSocketStatus(inner) => 0x0300 | u16::from(inner),
        }
    }
}

const NETWORK_CONFIG: NetworkConfig = NetworkConfig::Static {
    ip: [192, 168, 1, 239],
    subnet: [255, 255, 255, 0],
    gateway: [192, 168, 1, 1],
};

const MAC_ADDR: [u8; 6] = [0x90, 0xa2, 0xda, 0x00, 0xdc, 0x5f];

#[export_name = "main"]
pub fn main() -> ! {
    const DEVICE_INDEX: u8 = 0;

    // TODO should we expose this to clients? For now, we're just an `InOrder`
    // server and only support a single open socket.
    const SOCKET_CONFIG: SocketConfig = SocketConfig::OneSocket8Kib;

    let spi = SPI_DRIVER.get_task_id();
    let spi = Spi::from(spi);

    // Per the Arduino implementation for the ethernet shield, we may need to
    // wait up to 560ms and then toggle CS for the w5100 chip to wake up
    // properly.
    hl::sleep_for(560);
    spi.lock(DEVICE_INDEX, CsState::Asserted).unwrap();
    spi.release().unwrap();

    let device = spi.device(DEVICE_INDEX);
    let w5100 = W5100::reset(device, SOCKET_CONFIG, MAC_ADDR).unwrap();
    w5100.set_network_config(&NETWORK_CONFIG).unwrap();

    let mut buffer = [0; idl::INCOMING_SIZE];
    let mut server = ServerImpl {
        device: &w5100,
        socket: None,
    };
    loop {
        idol_runtime::dispatch(&mut buffer, &mut server);
    }
    /*
    const SOURCE_PORT: u16 = 8080;

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
        let w5100 = W5100::reset(device, SOCKET_CONFIG, MAC_ADDR).unwrap();
        w5100.set_network_config(&NETWORK_CONFIG).unwrap();

        sys_log!("waiting for connection on port {}", SOURCE_PORT);
        let mut tcp = TcpSocket::open(&w5100, SocketIndex::Zero, SOURCE_PORT)
            .unwrap()
            .listen()
            .unwrap()
            .accept()
            .unwrap();

        sys_log!("accepted new connection");
        let mut buf = [0; 32];
        loop {
            let data_read = match tcp.read(&mut buf) {
                Ok(0) => {
                    sys_log!("read returned 0; closing");
                    break;
                }
                Ok(n) => {
                    let buf = &buf[..n];
                    match core::str::from_utf8(buf) {
                        Ok(s) => sys_log!("read {} bytes: {:?}", n, s),
                        Err(_) => sys_log!("read {} bytes: {:?}", n, buf),
                    }
                    buf
                }
                Err(err) => {
                    sys_log!("read failed: {:?}", err);
                    break;
                }
            };
            match tcp.write(data_read) {
                Ok(n) => {
                    sys_log!("wrote {} bytes back", n);
                }
                Err(err) => {
                    sys_log!("write failed: {:?}", err);
                    break;
                }
            }
        }

        sys_log!("closing connection");
        tcp.close().unwrap();
    }
    */
}

struct ServerImpl<'a> {
    device: &'a W5100,
    socket: Option<(TaskId, SocketState<'a>)>,
}

enum SocketState<'a> {
    Open(TcpSocket<'a, tcp::Init>),
    Established(TcpSocket<'a, tcp::Established>),
}

impl SocketState<'_> {
    fn close(self) -> Result<(), W5100Error> {
        match self {
            SocketState::Open(socket) => socket.close_raw(),
            SocketState::Established(socket) => socket.close(),
        }
    }
}

impl ServerImpl<'_> {
    fn close(&mut self) -> Result<(), W5100Error> {
        if let Some((_, socket)) = self.socket.take() {
            socket.close()
        } else {
            Ok(())
        }
    }
}

impl idl::InOrderW5100DriverImpl for ServerImpl<'_> {
    fn recv_source(&self) -> Option<userlib::TaskId> {
        self.socket.as_ref().map(|&(task_id, _)| task_id)
    }

    fn closed_recv_fail(&mut self) {
        // closing shouldn't fail unless something has gone seriously wrong,
        // in which case we probably need to restart and reset the device
        self.close().unwrap();
    }

    fn tcp_open(
        &mut self,
        msg: &userlib::RecvMessage,
        source_port: u16,
    ) -> Result<u8, idol_runtime::RequestError<W5100Error>> {
        // `open` is only legal if the socket is currently closed
        match self.socket.as_ref() {
            None => (),
            Some((_, SocketState::Open(_))) => {
                return Err(W5100Error::BadSocketState(
                    SocketStatus::Init as u8,
                )
                .into())
            }
            Some((_, SocketState::Established(_))) => {
                return Err(W5100Error::BadSocketState(
                    SocketStatus::Established as u8,
                )
                .into())
            }
        }

        // TODO: support sockets 1-3; do we need to be Pipelined and/or support
        // interrupts first?
        let socket_index = SocketIndex::Zero;
        let socket = TcpSocket::open(self.device, socket_index, source_port)?;

        self.socket = Some((msg.sender, SocketState::Open(socket)));

        Ok(socket_index.into())
    }

    fn tcp_accept(
        &mut self,
        _msg: &userlib::RecvMessage,
        socket: u8,
    ) -> Result<(), idol_runtime::RequestError<W5100Error>> {
        // sanity check socket number; TODO support sockets 1-3
        if socket != 0 {
            return Err(W5100Error::BadSocketNumber.into());
        }

        // accept is only legal if the socket is open but not connected
        let (client, socket) = match self.socket.take() {
            Some((client, SocketState::Open(socket))) => (client, socket),
            None => {
                return Err(W5100Error::BadSocketState(
                    SocketStatus::Closed as u8,
                )
                .into())
            }
            original @ Some((_, SocketState::Established(_))) => {
                // put socket back in its current state
                self.socket = original;
                return Err(W5100Error::BadSocketState(
                    SocketStatus::Established as u8,
                )
                .into());
            }
        };

        let socket = socket.listen()?;
        let socket = socket.accept()?;

        self.socket = Some((client, SocketState::Established(socket)));

        Ok(())
    }

    fn tcp_read(
        &mut self,
        _msg: &userlib::RecvMessage,
        socket: u8,
        buf: idol_runtime::LenLimit<
            idol_runtime::Leased<idol_runtime::W, [u8]>,
            8192,
        >,
    ) -> Result<usize, idol_runtime::RequestError<W5100Error>> {
        // sanity check socket number; TODO support sockets 1-3
        if socket != 0 {
            return Err(W5100Error::BadSocketNumber.into());
        }

        // read is only legal if the socket is established
        let socket = match self.socket.as_ref() {
            Some((_, SocketState::Established(socket))) => socket,
            None => {
                return Err(W5100Error::BadSocketState(
                    SocketStatus::Closed as u8,
                )
                .into())
            }
            Some((_, SocketState::Open(_))) => {
                return Err(W5100Error::BadSocketState(
                    SocketStatus::Init as u8,
                )
                .into())
            }
        };

        socket.read(&buf)
    }

    fn tcp_write(
        &mut self,
        _msg: &userlib::RecvMessage,
        socket: u8,
        buf: idol_runtime::LenLimit<
            idol_runtime::Leased<idol_runtime::R, [u8]>,
            8192,
        >,
    ) -> Result<usize, idol_runtime::RequestError<W5100Error>> {
        // sanity check socket number; TODO support sockets 1-3
        if socket != 0 {
            return Err(W5100Error::BadSocketNumber.into());
        }

        // write is only legal if the socket is established
        let socket = match self.socket.as_ref() {
            Some((_, SocketState::Established(socket))) => socket,
            None => {
                return Err(W5100Error::BadSocketState(
                    SocketStatus::Closed as u8,
                )
                .into())
            }
            Some((_, SocketState::Open(_))) => {
                return Err(W5100Error::BadSocketState(
                    SocketStatus::Init as u8,
                )
                .into())
            }
        };

        socket.write(&buf)
    }

    fn tcp_close(
        &mut self,
        _msg: &userlib::RecvMessage,
        socket: u8,
    ) -> Result<(), idol_runtime::RequestError<W5100Error>> {
        // sanity check socket number; TODO support sockets 1-3
        if socket != 0 {
            return Err(W5100Error::BadSocketNumber.into());
        }

        // TODO Could check socket status and return an error if it wasn't open;
        // for now just close (closing a closed socket on W5100 is fine)
        self.close()?;

        Ok(())
    }
}

mod idl {
    use super::W5100Error;
    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}
