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
use self::w5100::{NetworkConfig, SocketConfig, SocketIndex, W5100};

task_slot!(SPI_DRIVER, spi2_driver);

#[derive(Debug)]
enum Error {
    ResetFailed,
    SpiError(SpiError),
    OpenFailed,
    ListenFailed,
    PeerClosed,
    BadSocketState(u8),
    UnknownSocketStatus(u8),
}

impl From<SpiError> for Error {
    fn from(err: SpiError) -> Self {
        Self::SpiError(err)
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
    const SOCKET_CONFIG: SocketConfig = SocketConfig::OneSocket8Kib;
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
}
