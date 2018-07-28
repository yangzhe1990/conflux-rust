use bytes::{BufMut, BytesMut};
use io::{IoContext, StreamToken};
use mio::deprecated::*;
use mio::tcp::*;
use mio::*;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use {Error, ErrorKind};

#[derive(PartialEq, Eq)]
pub enum WriteStatus {
    Ongoing,
    Complete,
}

pub trait GenericSocket: Read + Write {}

impl GenericSocket for TcpStream {}

pub struct GenericConnection<Socket: GenericSocket> {
    token: StreamToken,
    socket: Socket,
    recv_buf: BytesMut,
    recv_size: usize,
    send_queue: VecDeque<(Vec<u8>, u64)>,
    interest: Ready,
    registered: AtomicBool,
}

impl<Socket: GenericSocket> GenericConnection<Socket> {
    pub fn expect(&mut self, size: usize) {
        trace!(target: "network", "Expect to read {} bytes", size);
        if self.recv_size > 0 && self.recv_size != self.recv_buf.len() {
            warn!(target: "network", "Unexpected conenction read start");
        }
        self.recv_size = size;
        self.recv_buf = BytesMut::with_capacity(self.recv_size);
    }

    pub fn readable(&mut self) -> io::Result<Option<Vec<u8>>> {
        match self.socket.read(unsafe { self.recv_buf.bytes_mut() }) {
            Ok(size) => {
                trace!(target: "network", "Read {} bytes token={:?}", size, self.token);
                if (size == self.recv_buf.remaining_mut()) {
                    self.recv_size = 0;
                    Ok(Some(self.recv_buf.to_vec()))
                } else {
                    unsafe { self.recv_buf.advance_mut(size) }
                    Ok(None)
                }
            }
            Err(e) => {
                debug!(target: "network", "Read error {} token={:?}", e, self.token);
                Err(e)
            }
        }
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

pub type Connection = GenericConnection<TcpStream>;

impl Connection {
    pub fn new(token: StreamToken, socket: TcpStream) -> Self {
        Connection {
            token: token,
            socket: socket,
            recv_buf: BytesMut::new(),
            recv_size: 0,
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
    use std::io::{Error, ErrorKind, Read, Result, Write};

    use super::*;
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

    type TestConnection = GenericConnection<TestSocket>;

    impl TestConnection {
        fn new() -> Self {
            TestConnection {
                token: 1234567890usize,
                socket: TestSocket::new(),
                send_queue: VecDeque::new(),
                recv_buf: BytesMut::new(),
                recv_size: 0,
                interest: Ready::hup() | Ready::readable(),
                registered: AtomicBool::new(false),
            }
        }
    }

    struct TestBrokenSocket {
        error: String,
    }

    impl Read for TestBrokenSocket {
        fn read(&mut self, _: &mut [u8]) -> Result<usize> {
            Err(Error::new(ErrorKind::Other, self.error.clone()))
        }
    }

    impl Write for TestBrokenSocket {
        fn write(&mut self, _: &[u8]) -> Result<usize> {
            Err(Error::new(ErrorKind::Other, self.error.clone()))
        }

        fn flush(&mut self) -> ::std::io::Result<()> {
            unimplemented!();
        }
    }

    impl GenericSocket for TestBrokenSocket {}

    type TestBrokenConnection = GenericConnection<TestBrokenSocket>;

    impl TestBrokenConnection {
        pub fn new() -> Self {
            TestBrokenConnection {
                token: 1234567890usize,
                socket: TestBrokenSocket {
                    error: "test broken socket".to_owned(),
                },
                send_queue: VecDeque::new(),
                recv_buf: BytesMut::new(),
                recv_size: 0,
                interest: Ready::hup() | Ready::readable(),
                registered: AtomicBool::new(false),
            }
        }
    }

    fn test_io() -> IoContext<i32> {
        IoContext::new(IoChannel::disconnected(), 0)
    }

    #[test]
    fn connection_expect() {
        let mut connection = TestConnection::new();
        connection.expect(1024);
        assert_eq!(1024, connection.recv_size);
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
    fn connection_write_to_broken() {
        let mut connection = TestBrokenConnection::new();
        let data = (vec![0; 10240], 0);
        connection.send_queue.push_back(data);

        let status = connection.writable(&test_io());

        assert!(!status.is_ok());
        assert_eq!(1, connection.send_queue.len());
    }

    #[test]
    fn connection_read() {
        let mut connection = TestConnection::new();
        connection.recv_size = 2048;
        connection.recv_buf = BytesMut::with_capacity(2048);
        connection.recv_buf.put(&[10u8; 1024][..]);
        connection.socket.read_buf = vec![99; 2048];

        let status = connection.readable();

        assert!(status.is_ok());
        assert_eq!(1024, connection.socket.cursor);
    }

    #[test]
    fn connection_read_from_broken() {
        let mut connection = TestBrokenConnection::new();
        connection.recv_size = 2048;

        let status = connection.readable();
        assert!(!status.is_ok());
        assert_eq!(0, connection.recv_buf.len());
    }

    #[test]
    fn connection_read_nothing() {
        let mut connection = TestConnection::new();
        connection.recv_size = 2048;

        let status = connection.readable();

        assert!(status.is_ok());
        assert_eq!(0, connection.recv_buf.len());
    }

    #[test]
    fn connection_read_full() {
        let mut connection = TestConnection::new();
        connection.recv_size = 1024;
        connection.recv_buf = BytesMut::from(&[76u8; 1024][..]);

        let status = connection.readable();

        assert!(status.is_ok());
        assert_eq!(0, connection.socket.cursor);
    }
}
