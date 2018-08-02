use bytes::BufMut;
use io::*;
use mio::deprecated::EventLoop;
use mio::tcp::*;
use mio::*;
use parking_lot::{Mutex, RwLock};
use session::Session;
use session::SessionData;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use {
    Capability, DisconnectReason, Error, ErrorKind, NetworkConfiguration,
    NetworkContext as NetworkContextTrait, NetworkIoMessage,
    NetworkProtocolHandler, PeerId, ProtocolId,
};

type Slab<T> = ::slab::Slab<T, usize>;

const MAX_SESSIONS: usize = 2048;

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
    pub fn new(config: NetworkConfiguration) -> Result<NetworkService, Error> {
        let io_service = IoService::<NetworkIoMessage>::start()?;

        Ok(NetworkService {
            io_service: io_service,
            inner: RwLock::new(None),
            config: config,
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

    pub fn local_address(&self) -> Option<SocketAddr> {
        let inner = self.inner.read();
        inner.as_ref().map(|inner_ref| inner_ref.local_address())
    }

    pub fn register_protocol(
        &self, handler: Arc<NetworkProtocolHandler + Sync>,
        protocol: ProtocolId, versions: &[u8],
    ) -> Result<(), Error>
    {
        self.io_service.send_message(NetworkIoMessage::AddHandler {
            handler,
            protocol,
            versions: versions.to_vec(),
        });
        Ok(())
    }
}

type SharedSession = Arc<Mutex<Session>>;

pub struct HostMetadata {
    config: NetworkConfiguration,
    pub capabilities: Vec<Capability>,
    pub local_address: SocketAddr,
}

struct NetworkServiceInner {
    metadata: RwLock<HostMetadata>,
    tcp_listener: Mutex<TcpListener>,
    sessions: Arc<RwLock<Slab<SharedSession>>>,
    handlers: RwLock<HashMap<ProtocolId, Arc<NetworkProtocolHandler + Sync>>>,
}

impl NetworkServiceInner {
    pub fn new(
        config: &NetworkConfiguration,
    ) -> Result<NetworkServiceInner, Error> {
        let mut listen_address = match config.listen_address {
            None => SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(0, 0, 0, 0),
                DEFAULT_PORT,
            )),
            Some(addr) => addr,
        };
        let tcp_listener = TcpListener::bind(&listen_address)?;
        listen_address = SocketAddr::new(
            listen_address.ip(),
            tcp_listener.local_addr()?.port(),
        );
        debug!(target: "network", "Listening at {:?}", listen_address);

        Ok(NetworkServiceInner {
            metadata: RwLock::new(HostMetadata {
                config: config.clone(),
                capabilities: Vec::new(),
                local_address: listen_address,
            }),
            tcp_listener: Mutex::new(tcp_listener),
            sessions: Arc::new(RwLock::new(Slab::new_starting_at(
                FIRST_SESSION,
                LAST_SESSION,
            ))),
            handlers: RwLock::new(HashMap::new()),
        })
    }

    pub fn local_address(&self) -> SocketAddr {
        self.metadata.read().local_address
    }

    pub fn connected_peers(&self) -> Vec<PeerId> {
        let sessions = self.sessions.read();
        let sessions = &*sessions;

        let mut peers = Vec::with_capacity(sessions.count());
        for i in (0..MAX_SESSIONS).map(|x| x + FIRST_SESSION) {
            if sessions.get(i).is_some() {
                peers.push(i);
            }
        }
        peers
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
        let mut ready_protocols: Vec<ProtocolId> = Vec::new();
        let mut messages: Vec<(ProtocolId, Vec<u8>)> = Vec::new();
        let session = self.sessions.read().get(stream).cloned();

        // if let Some(session) = session.clone()
        if let Some(session) = session {
            loop {
                let data = session.lock().readable(io, &self.metadata.read());
                match data {
                    Ok(SessionData::Ready) => {
                        let mut sess = session.lock();
                        for (protocol, _) in self.handlers.read().iter() {
                            if sess.have_capability(*protocol) {
                                ready_protocols.push(*protocol);
                            }
                        }
                    }
                    Ok(SessionData::Message { data, protocol }) => {
                        match self.handlers.read().get(&protocol) {
                            None => {
                                warn!(target: "network", "No handler found for protocol: {:?}", protocol)
                            }
                            Some(_) => messages.push((protocol, data)),
                        }
                    }
                    Ok(SessionData::Continue) => (),
                    Ok(SessionData::None) => break,
                    Err(e) => {}
                }
            }
        }

        let handlers = self.handlers.read();
        if !ready_protocols.is_empty() {
            for protocol in ready_protocols {
                if let Some(handler) = handlers.get(&protocol).clone() {
                    handler.on_peer_connected(
                        &NetworkContext::new(
                            io,
                            protocol,
                            self.sessions.clone(),
                        ),
                        stream,
                    );
                }
            }
        }
        for (protocol, data) in messages {
            if let Some(handler) = handlers.get(&protocol).clone() {
                handler.on_message(
                    &NetworkContext::new(io, protocol, self.sessions.clone()),
                    stream,
                    &data,
                );
            }
        }
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
            NetworkIoMessage::AddHandler {
                ref handler,
                ref protocol,
                ref versions,
            } => {
                let h = handler.clone();
                h.initialize(&NetworkContext::new(
                    io,
                    *protocol,
                    self.sessions.clone(),
                ));
                self.handlers.write().insert(*protocol, h);
                let mut metadata = self.metadata.write();
                for &version in versions {
                    metadata.capabilities.push(Capability {
                        protocol: *protocol,
                        version,
                    });
                }
            }
            NetworkIoMessage::Disconnect(ref peer) => {
                let session = self.sessions.read().get(*peer).cloned();
                if let Some(session) = session {
                    session
                        .lock()
                        .disconnect(io, DisconnectReason::DisconnectRequested);
                }
                trace!(target: "network", "Disconnect requested {}", peer);
                //self.kill_connection(*peer, io, false);
            }
            _ => {}
        }
    }

    fn register_stream(
        &self, stream: StreamToken, reg: Token,
        event_loop: &mut EventLoop<IoManager<NetworkIoMessage>>,
    )
    {
        match stream {
            FIRST_SESSION...LAST_SESSION => {
                let session = self.sessions.read().get(stream).cloned();
                if let Some(session) = session {
                    session
                        .lock()
                        .register_socket(reg, event_loop)
                        .expect("Error registering socket");
                }
            }
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
            FIRST_SESSION...LAST_SESSION => {
                let mut sessions = self.sessions.write();
                if let Some(session) = sessions.get(stream).cloned() {
                    let sess = session.lock();
                    if sess.expired() {
                        sess.deregister_socket(event_loop)
                            .expect("Error deregistering socket");
                        sessions.remove(stream);
                    }
                }
            }
            _ => warn!("Unexpected stream deregistration"),
        }
    }

    fn update_stream(
        &self, stream: StreamToken, reg: Token,
        event_loop: &mut EventLoop<IoManager<NetworkIoMessage>>,
    )
    {
        match stream {
            FIRST_SESSION...LAST_SESSION => {
                let session = self.sessions.read().get(stream).cloned();
                if let Some(session) = session {
                    session
                        .lock()
                        .update_socket(reg, event_loop)
                        .expect("Error updating socket");
                }
            }
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
    sessions: Arc<RwLock<Slab<SharedSession>>>,
}

impl<'a> NetworkContext<'a> {
    fn new(
        io: &'a IoContext<NetworkIoMessage>, protocol: ProtocolId,
        sessions: Arc<RwLock<Slab<SharedSession>>>,
    ) -> NetworkContext<'a>
    {
        NetworkContext {
            io,
            protocol,
            sessions,
        }
    }
}

impl<'a> NetworkContextTrait for NetworkContext<'a> {
    fn send(&self, peer: PeerId, msg: Vec<u8>) -> Result<(), Error> { Ok(()) }

    fn disconnect_peer(&self, peer: PeerId) {}
}
