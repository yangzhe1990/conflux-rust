use bytes::{Bytes, IntoBuf};
use io::{IoContext, StreamToken};
use mio::deprecated::*;
use mio::tcp::*;
use mio::*;
use session::SessionMsgHandler;
use std::collections::VecDeque;
use std::io::{self, Cursor, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use {Error, ErrorKind};

#[derive(PartialEq, Eq)]
pub enum WriteStatus {
    Ongoing,
    Complete,
}

pub trait GenericSocket: Read + Write {}

impl GenericSocket for TcpStream {}

pub trait GenericMsgHandler {
    fn on_msg(self: &mut Self, buf: Cursor<&Bytes>) -> usize;
}

pub struct GenericConnection<Socket: GenericSocket, Handler: GenericMsgHandler>
{
    token: StreamToken,
    socket: Socket,
    handler: Handler,
    recv_buf: Bytes,
    send_queue: VecDeque<(Vec<u8>, u64)>,
    interest: Ready,
    registered: AtomicBool,
}

impl<Socket: GenericSocket, Handler: GenericMsgHandler>
    GenericConnection<Socket, Handler>
{
    pub fn readable(&mut self) -> io::Result<usize> {
        let mut buf: [u8; 1024] = [0; 1024];
        loop {
            match self.socket.read(&mut buf) {
                Ok(size) => {
                    trace!(target: "network", "{}: Read {} bytes", self.token, size);
                    if (size == 0) {
                        break;
                    }
                    self.recv_buf.extend_from_slice(&buf[0..size]);
                }
                Err(e) => {
                    debug!(target: "network", "{}: Error reading: {:?}", self.token, e);
                    return Err(e);
                }
            }
        }

        let mut size = 0;
        loop {
            let consumed = self.handler.on_msg((&self.recv_buf).into_buf());
            if consumed == 0 {
                break;
            }
            self.recv_buf.advance(consumed);
            size += consumed;
        }
        Ok(size)
    }

    pub fn writable<Message: Sync + Send + Clone + 'static>(
        &mut self, io: &IoContext<Message>,
    ) -> Result<WriteStatus, Error> {
        {
            let buf = match self.send_queue.front_mut() {
                Some(buf) => buf,
                None => return Ok(WriteStatus::Complete),
            };
            let len = buf.0.len();
            let pos = buf.1 as usize;
            if pos >= len {
                warn!(target: "network", "Unexpected connection data");
                return Ok(WriteStatus::Complete);
            }
            match self.socket.write(&buf.0[pos..]) {
                Ok(size) => {
                    if (pos + size < len) {
                        buf.1 += size as u64;
                        Ok(WriteStatus::Ongoing)
                    } else {
                        trace!(target: "network", "Wrote {} bytes token={:?}", len, self.token);
                        Ok(WriteStatus::Complete)
                    }
                }
                Err(e) => Err(e)?,
            }
        }.and_then(|status| {
            if status == WriteStatus::Complete {
                self.send_queue.pop_front();
            }
            if self.send_queue.is_empty() {
                self.interest.remove(Ready::writable());
            }
            io.update_registration(self.token)?;
            Ok(status)
        })
    }

    pub fn send<Message: Sync + Send + Clone + 'static>(
        &mut self, io: &IoContext<Message>, data: Vec<u8>,
    ) {
        if !data.is_empty() {
            trace!(target: "network", "Sending {} bytes token={:?}", data.len(), self.token);
            self.send_queue.push_back((data, 0));
            if !self.interest.is_writable() {
                self.interest.insert(Ready::writable());
            }
            io.update_registration(self.token).ok();
        }
    }
}

pub type Connection = GenericConnection<TcpStream, SessionMsgHandler>;

impl Connection {
    pub fn new(
        token: StreamToken, socket: TcpStream, handler: SessionMsgHandler,
    ) -> Self {
        Connection {
            token: token,
            socket: socket,
            handler: handler,
            recv_buf: Bytes::new(),
            send_queue: VecDeque::new(),
            interest: Ready::hup() | Ready::readable(),
            registered: AtomicBool::new(false),
        }
    }

    pub fn register_socket<H: Handler>(
        &self, reg: Token, event_loop: &mut EventLoop<H>,
    ) -> io::Result<()> {
        if self.registered.load(AtomicOrdering::SeqCst) {
            return Ok(());
        }
        trace!(target: "network", "Connection register; token={:?} reg={:?}", self.token, reg);
        if let Err(e) = event_loop.register(
            &self.socket,
            reg,
            self.interest,
            PollOpt::edge(),
        ) {
            trace!(target: "network", "Error registering; token={:?} reg={:?}: {:?}", self.token, reg, e);
        }
        self.registered.store(true, AtomicOrdering::SeqCst);
        Ok(())
    }

    pub fn update_socket<H: Handler>(
        &self, reg: Token, event_loop: &mut EventLoop<H>,
    ) -> io::Result<()> {
        trace!(target: "network", "Connection reregister; token={:?} reg={:?}", self.token, reg);
        if !self.registered.load(AtomicOrdering::SeqCst) {
            self.register_socket(reg, event_loop)
        } else {
            event_loop
                .reregister(&self.socket, reg, self.interest, PollOpt::edge())
                .unwrap_or_else(|e| {
                    trace!(target: "network", "Error reregistering; token={:?} reg={:?}: {:?}", self.token, reg, e);
                });
            Ok(())
        }
    }

    pub fn deregister_socket<H: Handler>(
        &self, event_loop: &mut EventLoop<H>,
    ) -> io::Result<()> {
        trace!(target: "network", "Connection deregister; token={:?}", self.token);
        event_loop.deregister(&self.socket).ok();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cmp;
    use std::collections::VecDeque;
    use std::io::{Cursor, Error, ErrorKind, Read, Result, Write};

    use super::*;
    use bytes::{Buf, Bytes};
    use io::*;
    use mio::Ready;

    struct TestSocket {
        read_buf: Vec<u8>,
        write_buf: Vec<u8>,
        cursor: usize,
        buf_size: usize,
    }

    impl TestSocket {
        fn new() -> Self {
            TestSocket {
                read_buf: vec![],
                write_buf: vec![],
                cursor: 0,
                buf_size: 0,
            }
        }

        fn with_buf(buf_size: usize) -> Self {
            TestSocket {
                read_buf: vec![],
                write_buf: vec![],
                cursor: 0,
                buf_size: buf_size,
            }
        }
    }

    impl Read for TestSocket {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            let end = cmp::min(self.read_buf.len(), self.cursor + buf.len());
            if self.cursor > end {
                return Ok(0);
            }
            let len = end - self.cursor;
            if len == 0 {
                Ok(0)
            } else {
                for i in self.cursor..end {
                    buf[i - self.cursor] = self.read_buf[i];
                }
                self.cursor = end;
                Ok(len)
            }
        }
    }

    impl Write for TestSocket {
        fn write(&mut self, buf: &[u8]) -> Result<usize> {
            if self.buf_size == 0 || buf.len() < self.buf_size {
                self.write_buf.extend(buf.iter().cloned());
                Ok(buf.len())
            } else {
                self.write_buf
                    .extend(buf.iter().take(self.buf_size).cloned());
                Ok(self.buf_size)
            }
        }

        fn flush(&mut self) -> Result<()> {
            unimplemented!();
        }
    }

    impl GenericSocket for TestSocket {}

    struct TestMsgHandler;

    impl GenericMsgHandler for TestMsgHandler {
        fn on_msg(&mut self, buf: Cursor<&Bytes>) -> usize {
            if buf.remaining() >= 2 {
                let bytes = (&buf as &Buf).bytes();
                let size = (bytes[0] as usize) | ((bytes[1] as usize) << 8);
                if buf.remaining() >= size {
                    size
                } else {
                    0
                }
            } else {
                0
            }
        }
    }

    type TestConnection = GenericConnection<TestSocket, TestMsgHandler>;

    impl TestConnection {
        fn new() -> Self {
            TestConnection {
                token: 1234567890usize,
                socket: TestSocket::new(),
                handler: TestMsgHandler,
                send_queue: VecDeque::new(),
                recv_buf: Bytes::new(),
                interest: Ready::hup() | Ready::readable(),
                registered: AtomicBool::new(false),
            }
        }
    }

    fn test_io() -> IoContext<i32> {
        IoContext::new(IoChannel::disconnected(), 0)
    }

    #[test]
    fn connection_write_empty() {
        let mut connection = TestConnection::new();
        let status = connection.writable(&test_io());
        assert!(status.is_ok());
        assert!(WriteStatus::Complete == status.unwrap());
    }

    #[test]
    fn connection_write_is_buffered() {
        let mut connection = TestConnection::new();
        connection.socket = TestSocket::with_buf(1024);
        let data = (vec![0; 10240], 0);
        connection.send_queue.push_back(data);

        let status = connection.writable(&test_io());

        assert!(status.is_ok());
        assert_eq!(1, connection.send_queue.len());
    }

    #[test]
    fn connection_read_nothing() {
        let mut connection = TestConnection::new();
        connection.socket.read_buf = vec![3, 0];

        let status = connection.readable();

        assert!(status.is_ok());
        assert_eq!(status.unwrap(), 0);
    }

    #[test]
    fn connection_read_full() {
        let mut connection = TestConnection::new();
        connection.socket.read_buf = vec![3, 0, 0];

        let status = connection.readable();

        assert!(status.is_ok());
        assert_eq!(status.unwrap(), 3);
    }
}
