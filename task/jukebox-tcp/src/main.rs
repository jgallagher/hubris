// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use core::ops::Range;

use task_jukebox_api::{Jukebox, JukeboxError};
use drv_w5100_spi_api::{W5100Driver, W5100Error};
use userlib::*;

task_slot!(W5100, w5100_driver);

task_slot!(JUKEBOX, jukebox);

#[derive(Debug)]
enum Error {
    NetworkError(W5100Error),
    BadInput,
}

impl From<W5100Error> for Error {
    fn from(err: W5100Error) -> Self {
        Self::NetworkError(err)
    }
}

#[export_name = "main"]
fn main() -> ! {
    const SOURCE_PORT: u16 = 8080;

    let w5100 = W5100.get_task_id();
    let w5100 = W5100Driver::from(w5100);

    loop {
        let socket = w5100.tcp_open(SOURCE_PORT).unwrap();
        sys_log!("waiting for incoming connection...");
        w5100.tcp_accept(socket).unwrap();
        sys_log!("connection established! starting to read lines...");

        match run_connection(&w5100, socket) {
            Ok(()) => {
                sys_log!("connection closed");
            }
            Err(Error::NetworkError(err)) => {
                sys_log!("connection failed: {:?}", err);
            }
            Err(Error::BadInput) => {
                // try to tell client we're closing our side, but ignore errors if they're gone
                let _ = write_all(&w5100, socket, "bad input; goodbye\n".as_bytes());
            }
        }
        w5100.tcp_close(socket).unwrap();
    }
}

fn run_connection(w5100: &W5100Driver, socket: u8) -> Result<(), Error> {
    let mut line_reader = LineReader::new();
    let jukebox = JUKEBOX.get_task_id();
    let jukebox = Jukebox::from(jukebox);

    loop {
        let line = match line_reader.read_line(w5100, socket)? {
            Some(s) => s,
            None => return Ok(()),
        };

        let value = line.parse::<usize>().map_err(|_| Error::BadInput)?;
        let resp_str = match jukebox.play_song(value) {
            Ok(()) => "now playing!\n",
            Err(JukeboxError::BusyPlaying) => "busy playing! please wait\n",
            Err(JukeboxError::BadSongIndex) => "not that many songs\n",
        };

        write_all(w5100, socket, resp_str.as_bytes())?;
    }
}

fn write_all(w5100: &W5100Driver, socket: u8, mut data: &[u8]) -> Result<(), Error> {
    while !data.is_empty() {
        let n = w5100.tcp_write(socket, data)?;
        data = &data[n..];
    }
    Ok(())
}

struct LineReader {
    buf: [u8; 8], // we're only parsing 1 usize, so this can be quite small
    leftover: Option<Range<usize>>,
}

impl LineReader {
    fn new() -> Self {
        Self {
            buf: [0; 8],
            leftover: None,
        }
    }

    fn read_line(
        &mut self,
        w5100: &W5100Driver,
        socket: u8,
    ) -> Result<Option<&str>, Error> {
        // shift leftover data down to front, if we have any
        let mut start = if let Some(leftover) = self.leftover.take() {
            let end = leftover.end; // Range doesn't impl Copy :(
            self.buf.copy_within(leftover, 0);
            end
        } else {
            0
        };

        loop {
            let n = w5100.tcp_read(socket, &mut self.buf[start..])?;
            if n == 0 {
                return Ok(None);
            }
            let end = start + n;

            // see if we have a newline
            if let Some(pos) =
                self.buf.iter().take(end).position(|&b| b == b'\n')
            {
                // validating utf8 just to parse for an ascii int seems
                // unnecessary; might be able to parse as &[u8]?
                match core::str::from_utf8(&self.buf[..pos]) {
                    Ok(s) => {
                        if end > pos + 1 {
                            self.leftover = Some(pos + 1..end)
                        }
                        return Ok(Some(s));
                    }
                    Err(_) => return Err(Error::BadInput),
                }
            }

            // no newline yet; increment start and give up if we're out of space
            start = end;
            if start >= self.buf.len() {
                return Err(Error::BadInput);
            }
        }
    }
}
