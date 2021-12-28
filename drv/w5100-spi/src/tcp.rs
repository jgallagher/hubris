use crate::{
    w5100::{Socket, SocketCommand, SocketIndex, SocketMode, SocketStatus},
    W5100Error, W5100,
};
use core::marker::PhantomData;
use ringbuf::{ringbuf, ringbuf_entry};
use userlib::*;

#[derive(Copy, Clone, PartialEq)]
enum Trace {
    Opened(u8),
    Listening(u8),
    Accepted(u8),
    Disconnect(u8),
    PeerClosed(u8),
    StartRead(u8, usize),
    Read(u8, usize),
    StartWrite(u8, usize),
    Write(u8, usize),
    Close(u8),
    Error(u8),
    None,
}

ringbuf!(Trace, 64, Trace::None);

pub(crate) struct TcpSocket<'a, State> {
    socket: Socket<'a>,
    marker: PhantomData<State>,
}

impl<'a> TcpSocket<'a, Init> {
    pub(super) fn open(
        device: &'a W5100,
        socket_index: SocketIndex,
        source_port: u16,
    ) -> Result<Self, W5100Error> {
        let socket = Socket::new(device, socket_index);

        socket.set_mode(SocketMode::PROTO_TCP)?;
        socket.set_source_port(source_port)?;
        socket.send_command(SocketCommand::Open)?;

        match socket.status()? {
            SocketStatus::Init => {
                ringbuf_entry!(Trace::Opened(socket.index()));
                Ok(Self {
                    socket,
                    marker: PhantomData,
                })
            }
            _ => Err(W5100Error::OpenFailed),
        }
    }
}

impl<'a> TcpSocket<'a, Init> {
    pub(crate) fn listen(self) -> Result<TcpSocket<'a, Listening>, W5100Error> {
        self.socket.send_command(SocketCommand::Listen)?;
        match self.socket.status()? {
            SocketStatus::Listen => {
                ringbuf_entry!(Trace::Listening(self.socket.index()));
                Ok(self.new_state())
            }
            _ => {
                ringbuf_entry!(Trace::Error(self.socket.index()));
                self.fail(W5100Error::ListenFailed)
            }
        }
    }
}

impl<'a> TcpSocket<'a, Listening> {
    pub(crate) fn accept(
        self,
    ) -> Result<TcpSocket<'a, Established>, W5100Error> {
        // TODO interrupts; for now busy wait
        loop {
            match self.socket.status()? {
                SocketStatus::Listen => {
                    // still listening; sleep
                    hl::sleep_for(100);
                }
                SocketStatus::Established => {
                    ringbuf_entry!(Trace::Accepted(self.socket.index()));
                    return Ok(self.new_state());
                }
                other => {
                    // This shouldn't be possible (although not specified in datasheet) (?)
                    ringbuf_entry!(Trace::Error(self.socket.index()));
                    return self.fail(W5100Error::BadSocketState(other as u8));
                }
            }
        }
    }
}

impl<'a> TcpSocket<'a, Established> {
    pub(crate) fn close(self) -> Result<(), W5100Error> {
        ringbuf_entry!(Trace::Disconnect(self.socket.index()));
        self.socket.send_command(SocketCommand::Disconnect)?;

        // TODO should we not close the socket now? Writing `Disconnect`
        // immediately puts us into the `FinWait` state, and we would stay there
        // until our peer closes their side. Probably safest to go ahead and
        // close.
        self.close_raw()
    }

    // Returns number of bytes written.
    pub(crate) fn write(
        &self,
        buf: &idol_runtime::Leased<idol_runtime::R, [u8]>,
    ) -> Result<usize, idol_runtime::RequestError<W5100Error>> {
        // Make sure we're still connected
        match self.socket.status()? {
            SocketStatus::Established => (), // what we expect to be
            SocketStatus::CloseWait => {
                // peer requested close
                ringbuf_entry!(Trace::PeerClosed(self.socket.index()));
                return self.fail(W5100Error::PeerClosed).map_err(Into::into);
            }
            other => {
                return self
                    .fail(W5100Error::BadSocketState(other as u8))
                    .map_err(Into::into);
            }
        }

        ringbuf_entry!(Trace::StartWrite(self.socket.index(), buf.len()));

        match self.socket.send(buf) {
            Ok(n) => {
                ringbuf_entry!(Trace::Write(self.socket.index(), n));
                Ok(n)
            }
            Err(err @ idol_runtime::RequestError::Fail(_)) => {
                // client task failed; server impl will handle cleanup
                return Err(err);
            }
            Err(idol_runtime::RequestError::Runtime(err)) => {
                return self.fail(err).map_err(Into::into);
            }
        }
    }

    // Returns number of bytes read; 0 if peer has closed the connection.
    pub(crate) fn read(
        &self,
        out: &idol_runtime::Leased<idol_runtime::W, [u8]>,
    ) -> Result<usize, idol_runtime::RequestError<W5100Error>> {
        ringbuf_entry!(Trace::StartRead(self.socket.index(), out.len()));
        loop {
            // Make sure we're still connected
            match self.socket.status()? {
                SocketStatus::Established => (), // what we expect to be
                SocketStatus::CloseWait => {
                    // peer requested close
                    ringbuf_entry!(Trace::PeerClosed(self.socket.index()));
                    return Ok(0);
                }
                other => {
                    return self
                        .fail(W5100Error::BadSocketState(other as u8))
                        .map_err(Into::into);
                }
            }

            match self.socket.recv(out) {
                Ok(0) => {
                    hl::sleep_for(10); // TODO interrupts
                    continue;
                }
                Ok(n) => {
                    ringbuf_entry!(Trace::Read(self.socket.index(), n));
                    return Ok(n);
                }
                Err(err @ idol_runtime::RequestError::Fail(_)) => {
                    // client task failed; server impl will handle cleanup
                    return Err(err);
                }
                Err(idol_runtime::RequestError::Runtime(err)) => {
                    return self.fail(err).map_err(Into::into);
                }
            }
        }
    }
}

impl<'a, T> TcpSocket<'a, T> {
    fn fail<U>(&self, error: W5100Error) -> Result<U, W5100Error> {
        ringbuf_entry!(Trace::Error(self.socket.index()));
        self.close_raw()?;
        Err(error)
    }

    pub(crate) fn close_raw(&self) -> Result<(), W5100Error> {
        ringbuf_entry!(Trace::Close(self.socket.index()));
        self.socket.send_command(SocketCommand::Close)
    }

    fn new_state<U>(self) -> TcpSocket<'a, U> {
        TcpSocket {
            socket: self.socket,
            marker: PhantomData,
        }
    }
}

// Phantom types for current socket state
pub(crate) enum Init {}
pub(crate) enum Listening {}
pub(crate) enum Established {}
