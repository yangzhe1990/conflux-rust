use bytes::{Buf, Bytes};
use connection::{Connection, GenericMsgHandler};
use io::*;
use mio::deprecated::*;
use mio::tcp::*;
use mio::*;
use std::io::Cursor;
use {Error, ErrorKind};

pub struct Session {
    connection: Connection,
}

impl Session {
    pub fn new<Message: Send + Sync + Clone + 'static>(
        io: &IoContext<Message>, socket: TcpStream, token: StreamToken,
    ) -> Result<Session, Error> {
        Ok(Session {
            connection: Connection::new(token, socket, SessionMsgHandler),
        })
    }

    pub fn done(&self) -> bool { false }

    fn connection(&self) -> &Connection { &self.connection }

    pub fn register_socket<H: Handler>(
        &self, reg: Token, event_loop: &mut EventLoop<H>,
    ) -> Result<(), Error> {
        self.connection.register_socket(reg, event_loop)?;
        Ok(())
    }

    pub fn update_socket<H: Handler>(
        &self, reg: Token, event_loop: &mut EventLoop<H>,
    ) -> Result<(), Error> {
        self.connection.update_socket(reg, event_loop)?;
        Ok(())
    }

    pub fn readable<Message: Send + Sync + Clone>(
        &mut self, io: &IoContext<Message>,
    ) -> Result<(), Error> {
        Ok(())
    }

    pub fn writable<Message: Send + Sync + Clone>(
        &mut self, io: &IoContext<Message>,
    ) -> Result<(), Error> {
        Ok(())
    }
}

pub struct SessionMsgHandler;

impl GenericMsgHandler for SessionMsgHandler {
    fn on_msg(&mut self, buf: Cursor<&Bytes>) -> usize { buf.remaining() }
}
