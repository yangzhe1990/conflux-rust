use io::*;
use mio::deprecated::EventLoop;
use mio::tcp::*;
use mio::*;
use parking_lot::{Mutex, RwLock};
use session::Session;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use {
    Error, ErrorKind, MsgId, NetworkConfiguration,
    NetworkContext as NetworkContextTrait, NetworkIoMessage,
    NetworkProtocolHandler, PeerId, ProtocolId,
};

type Slab<T> = ::slab::Slab<T, usize>;

const MAX_SESSIONS: usize = 1024 + MAX_HANDSHAKES;
const MAX_HANDSHAKES: usize = 1024;

const DEFAULT_PORT: u16 = 32323;

const TCP_ACCEPT: StreamToken = SYS_TIMER + 1;
const FIRST_SESSION: StreamToken = 0;
const LAST_SESSION: StreamToken = FIRST_SESSION + MAX_SESSIONS - 1;
const SYS_TIMER: TimerToken = LAST_SESSION + 1;

pub struct NetworkService {
    io_service: IoService<NetworkIoMessage>,
    inner: RwLock<Option<Arc<NetworkServiceInner>>>,
    config: NetworkConfiguration,
}

impl NetworkService {
    pub fn new(config: &NetworkConfiguration) -> Result<NetworkService, Error> {
        let io_service = IoService::<NetworkIoMessage>::start()?;

        Ok(NetworkService {
            io_service: io_service,
            inner: RwLock::new(None),
            config: config.clone(),
        })
    }

    pub fn start(&self) -> Result<(), Error> {
        let mut w = self.inner.write();
        if w.is_none() {
            let inner = Arc::new(NetworkServiceInner::new(&self.config)?);
            self.io_service.register_handler(inner.clone())?;
            *w = Some(inner);
        }

        Ok(())
    }
}

type SharedSession = Arc<Mutex<Session>>;

struct NetworkServiceInner {
    tcp_listener: Mutex<TcpListener>,
    sessions: Arc<RwLock<Slab<SharedSession>>>,
    handlers: RwLock<HashMap<ProtocolId, Arc<NetworkProtocolHandler + Sync>>>,
}

impl NetworkServiceInner {
    pub fn new(
        config: &NetworkConfiguration,
    ) -> Result<NetworkServiceInner, Error> {
        let listen_address = match config.listen_address {
            None => SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(0, 0, 0, 0),
                DEFAULT_PORT,
            )),
            Some(addr) => addr,
        };
        let tcp_listener = TcpListener::bind(&listen_address)?;

        Ok(NetworkServiceInner {
            tcp_listener: Mutex::new(tcp_listener),
            sessions: Arc::new(RwLock::new(Slab::new_starting_at(
                FIRST_SESSION,
                LAST_SESSION,
            ))),
            handlers: RwLock::new(HashMap::new()),
        })
    }

    fn start(&self, io: &IoContext<NetworkIoMessage>) -> Result<(), Error> {
        io.register_stream(TCP_ACCEPT)?;
        Ok(())
    }

    fn create_connection(
        &self, socket: TcpStream, io: &IoContext<NetworkIoMessage>,
    ) -> Result<(), Error> {
        let mut sessions = self.sessions.write();

        let token = sessions.insert_with_opt(|token| {
            trace!(target: "network", "{}: Initiating session", token);
            match Session::new(io, socket, token) {
                Ok(sess) => Some(Arc::new(Mutex::new(sess))),
                Err(e) => {
                    debug!(target: "network", "Error creating session: {:?}", e);
                    None
                }
            }
        });

        match token {
            Some(token) => {
                io.register_stream(token).map(|_| ()).map_err(Into::into)
            }
            None => {
                debug!(target: "network", "Max sessions reached");
                Ok(())
            }
        }
    }

    fn connection_closed(
        &self, stream: StreamToken, io: &IoContext<NetworkIoMessage>,
    ) {
    }

    fn session_readable(
        &self, stream: StreamToken, io: &IoContext<NetworkIoMessage>,
    ) {
    }

    fn session_writable(
        &self, stream: StreamToken, io: &IoContext<NetworkIoMessage>,
    ) {
        let session = self.sessions.read().get(stream).cloned();

        if let Some(session) = session {
            let mut sess = session.lock();
            if let Err(e) = sess.writable(io) {
                trace!(target: "network", "{}: Session write error: {:?}", stream, e);
            }
            if sess.done() {
                io.deregister_stream(stream).unwrap_or_else(|e| {
                    debug!("Error deregistering stream: {:?}", e)
                });
            }
        }
    }

    fn accept(&self, io: &IoContext<NetworkIoMessage>) {
        trace!(target: "network", "Accepting incoming connection");
        loop {
            let socket = match self.tcp_listener.lock().accept() {
                Ok((sock, _addr)) => sock,
                Err(e) => {
                    if e.kind() != io::ErrorKind::WouldBlock {
                        debug!(target: "network", "Error accepting connection: {:?}", e);
                    }
                    break;
                }
            };
        }
    }
}

impl IoHandler<NetworkIoMessage> for NetworkServiceInner {
    fn initialize(&self, io: &IoContext<NetworkIoMessage>) {
        io.message(NetworkIoMessage::Start).unwrap_or_else(|e| {
            warn!("Error sending IO notification: {:?}", e)
        });
    }

    fn stream_hup(
        &self, io: &IoContext<NetworkIoMessage>, stream: StreamToken,
    ) {
        trace!(target: "network", "Hup: {}", stream);
        match stream {
            FIRST_SESSION...LAST_SESSION => self.connection_closed(stream, io),
            _ => warn!(target: "network", "Unexpected hup"),
        }
    }

    fn stream_readable(
        &self, io: &IoContext<NetworkIoMessage>, stream: StreamToken,
    ) {
        match stream {
            FIRST_SESSION...LAST_SESSION => self.session_readable(stream, io),
            TCP_ACCEPT => self.accept(io),
            _ => panic!("Received unknown readable token"),
        }
    }

    fn stream_writable(
        &self, io: &IoContext<NetworkIoMessage>, stream: StreamToken,
    ) {
        match stream {
            FIRST_SESSION...LAST_SESSION => self.session_writable(stream, io),
            _ => panic!("Received unknown writable token"),
        }
    }

    fn timeout(&self, io: &IoContext<NetworkIoMessage>, token: TimerToken) {}

    fn message(
        &self, io: &IoContext<NetworkIoMessage>, message: &NetworkIoMessage,
    ) {
        match *message {
            NetworkIoMessage::Start => self.start(io).unwrap_or_else(|e| {
                warn!("Error starting network service: {:?}", e)
            }),
        }
    }

    fn register_stream(
        &self, stream: StreamToken, reg: Token,
        event_loop: &mut EventLoop<IoManager<NetworkIoMessage>>,
    )
    {
        match stream {
            FIRST_SESSION...LAST_SESSION => {}
            TCP_ACCEPT => {
                event_loop
                    .register(
                        &*self.tcp_listener.lock(),
                        Token(TCP_ACCEPT),
                        Ready::all(),
                        PollOpt::edge(),
                    )
                    .expect("Error registering stream");
            }
            _ => warn!("Unexpected stream registeration"),
        }
    }

    fn deregister_stream(
        &self, stream: StreamToken,
        event_loop: &mut EventLoop<IoManager<NetworkIoMessage>>,
    )
    {
        match stream {
            FIRST_SESSION...LAST_SESSION => {}
            _ => warn!("Unexpected stream deregistration"),
        }
    }

    fn update_stream(
        &self, stream: StreamToken, reg: Token,
        event_loop: &mut EventLoop<IoManager<NetworkIoMessage>>,
    )
    {
        match stream {
            FIRST_SESSION...LAST_SESSION => {}
            TCP_ACCEPT => event_loop
                .reregister(
                    &*self.tcp_listener.lock(),
                    Token(TCP_ACCEPT),
                    Ready::all(),
                    PollOpt::edge(),
                )
                .expect("Error reregistering stream"),
            _ => warn!("Unexpected stream update"),
        }
    }
}

pub struct NetworkContext<'a> {
    io: &'a IoContext<NetworkIoMessage>,
    protocol: ProtocolId,
}

impl<'a> NetworkContextTrait for NetworkContext<'a> {
    fn send(
        &self, peer: PeerId, msg_id: MsgId, data: Vec<u8>,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn disconnect_peer(&self, peer: PeerId) {}
}
