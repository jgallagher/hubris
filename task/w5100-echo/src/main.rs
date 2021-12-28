// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use drv_w5100_spi_api::{W5100Driver, W5100Error};
use userlib::*;

task_slot!(W5100, w5100_driver);

#[export_name = "main"]
fn main() -> ! {
    const SOURCE_PORT: u16 = 8080;

    let w5100 = W5100.get_task_id();
    let w5100 = W5100Driver::from(w5100);

    loop {
        let socket = w5100.tcp_open(SOURCE_PORT).unwrap();
        sys_log!("waiting for incoming connection...");
        w5100.tcp_accept(socket).unwrap();
        sys_log!("connection established! starting to echo");

        match run_echo(&w5100, socket) {
            Ok(n) => {
                sys_log!("connection completed; echoed {} bytes", n);
            }
            Err(err) => {
                sys_log!("connection failed: {:?}", err);
            }
        }
        w5100.tcp_close(socket).unwrap();
    }
}

fn run_echo(w5100: &W5100Driver, socket: u8) -> Result<usize, W5100Error> {
    // TODO increase buf size? for now keep small so we can see how it behaves
    // if incoming data is > buf.len()
    let mut buf = [0; 32];
    let mut echoed = 0;

    loop {
        let n = w5100.tcp_read(socket, &mut buf)?;
        if n == 0 {
            return Ok(echoed);
        }

        w5100.tcp_write(socket, &buf[..n])?;
        echoed = echoed.saturating_add(n);
    }
}
