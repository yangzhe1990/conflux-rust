use bytes::BufMut;
use io::*;
use mio::deprecated::EventLoop;
use mio::tcp::*;
use mio::*;
use parking_lot::{Mutex, RwLock};
use session::Session;
use session::SessionData;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;
use {
    Capability, DisconnectReason, Error, ErrorKind, NetworkConfiguration,
    NetworkContext as NetworkContextTrait, NetworkIoMessage,
    NetworkProtocolHandler, NodeId, PeerId, ProtocolId,
};

type Slab<T> = ::slab::Slab<T, usize>;

const MAX_SESSIONS: usize = 2048;

const DEFAULT_PORT: u16 = 32323;

const FIRST_SESSION: StreamToken = 0;
const LAST_SESSION: StreamToken = FIRST_SESSION + MAX_SESSIONS - 1;
const SYS_TIMER: TimerToken = LAST_SESSION + 1;
const TCP_ACCEPT: StreamToken = SYS_TIMER + 1;
const HOUSEKEEPING: TimerToken = SYS_TIMER + 2;

const HOUSEKEEPING_TIMEOUT: Duration = Duration::from_secs(1);

pub struct NetworkService {
    io_service: Option<IoService<NetworkIoMessage>>,
    inner: Option<Arc<NetworkServiceInner>>,
    config: NetworkConfiguration,
}

impl NetworkService {
    pub fn new(config: NetworkConfiguration) -> NetworkService {
        NetworkService {
            io_service: None,
            inner: None,
            config: config,
        }
    }

    pub fn start(&mut self) -> Result<(), Error> {
        let raw_io_service = IoService::<NetworkIoMessage>::start()?;
        self.io_service = Some(raw_io_service);

        if self.inner.is_none() {
            let inner = Arc::new(NetworkServiceInner::new(&self.config)?);
            self.io_service
                .as_ref()
                .unwrap()
                .register_handler(inner.clone())?;
            self.inner = Some(inner);
        }

        Ok(())
    }

    pub fn add_peer(&self, peer: SocketAddr) -> Result<NodeId, Error> {
        if let Some(ref x) = self.inner {
            x.add_node(peer)
        } else {
            Err("Network service not started yet!".into())
        } 
    }

    pub fn drop_peer(&self, peer: SocketAddr) -> Result<(), Error> {
        if let Some(ref x) = self.inner {
            x.drop_node(peer)
        } else {
            Err("Network service not started yet!".into())
        }
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.inner.as_ref().map(|inner_ref| inner_ref.local_addr())
    }

    pub fn register_protocol(
        &self, handler: Arc<NetworkProtocolHandler + Sync>,
        protocol: ProtocolId, versions: &[u8],
    ) -> Result<(), Error>
    {
        self.io_service.as_ref().unwrap().send_message(
            NetworkIoMessage::AddHandler {
                handler,
                protocol,
                versions: versions.to_vec(),
            },
        )?;
        Ok(())
    }

    /// Executes action in the network context
    pub fn with_context<F>(&self, protocol: ProtocolId, action: F)
    where F: FnOnce(&NetworkContext) {
        let io = IoContext::new(self.io_service.as_ref().unwrap().channel(), 0);
        if let Some(ref inner) = self.inner {
            inner.with_context(protocol, &io, action);
        };
    }
}

type SharedSession = Arc<Mutex<Session>>;

pub struct HostMetadata {
    config: NetworkConfiguration,
    pub capabilities: Vec<Capability>,
    pub local_address: SocketAddr,
}

#[derive(Debug)]
struct NodeEntry {
    id: NodeId,
    local_address: SocketAddr,
    stream_token: Option<StreamToken>,
}

struct NetworkServiceInner {
    metadata: RwLock<HostMetadata>,
    tcp_listener: Mutex<TcpListener>,
    sessions: Arc<RwLock<Slab<SharedSession>>>,
    handlers: RwLock<HashMap<ProtocolId, Arc<NetworkProtocolHandler + Sync>>>,
    nodes: RwLock<HashMap<NodeId, NodeEntry>>,
    dropped_nodes: RwLock<HashSet<StreamToken>>,
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

        let inner = NetworkServiceInner {
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
            nodes: RwLock::new(HashMap::new()),
            dropped_nodes: RwLock::new(HashSet::new()),
        };

        for n in &config.boot_nodes {
            inner.add_node(n.parse()?)?;
        }

        Ok(inner)
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.metadata.read().local_address
    }

    fn add_node(&self, local_address: SocketAddr) -> Result<NodeId, Error> {
        let id = local_address;
        let node = NodeEntry {
            id: id,
            local_address: local_address,
            stream_token: None,
        };
        self.nodes.write().insert(node.id, node);
        Ok(id)
    }

    fn drop_node(&self, local_id: NodeId) -> Result<(), Error> { 
        let mut wn = self.nodes.write();
        if wn.contains_key(&local_id) {
            let entry = wn.get(&local_id).unwrap();
            if let Some(stream_token) = entry.stream_token {
                let mut wd = self.dropped_nodes.write();
                wd.insert(stream_token);
            }
        }
        wn.remove(&local_id);
        Ok(())
    }

    fn on_housekeeping(&self, io: &IoContext<NetworkIoMessage>) {
        self.connect_peers(io);
        self.drop_peers(io);
    }

    fn connect_peers(&self, io: &IoContext<NetworkIoMessage>) {
        let nodes: Vec<NodeId> =
            { self.nodes.read().keys().map(|key| key.clone()).collect() };

        for id in &nodes {
            self.connect_peer(&id, io);
        }
    }

    fn drop_peers(&self, io: &IoContext<NetworkIoMessage>) {
        {
            if self.dropped_nodes.read().len() == 0 {
                return;
            }
        }
        let mut w = self.dropped_nodes.write();
        for token in w.iter() {
            self.kill_connection(*token, io, true);
        }
        w.clear();
    }

    fn have_session(&self, id: &NodeId) -> bool {
        self.sessions
            .read()
            .iter()
            .any(|sess| sess.lock().metadata.id == Some(*id))
    }

    fn connect_peer(&self, id: &NodeId, io: &IoContext<NetworkIoMessage>) {
        if self.have_session(id) {
            trace!(target: "network", "Abort connect. Node already connected");
            return;
        }

        let socket = {
            let address = {
                let mut nodes = self.nodes.write();
                if let Some(node) = nodes.get_mut(id) {
                    node.local_address
                } else {
                    debug!(target: "network", "Abort connect. Node expired");
                    return;
                }
            };
            match TcpStream::connect(&address) {
                Ok(socket) => {
                    trace!(target: "network", "{}: connecting to {:?}", id, address);
                    socket
                }
                Err(e) => {
                    debug!(target: "network", "{}: can't connect o address {:?} {:?}", id, address, e);
                    return;
                }
            }
        };

        if let Err(e) = self.create_connection(socket, Some(id), io) {
            debug!(target: "network", "Can't create connection: {:?}", e);
        }
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
        &self, socket: TcpStream, id: Option<&NodeId>,
        io: &IoContext<NetworkIoMessage>,
    ) -> Result<(), Error>
    {
        let mut sessions = self.sessions.write();

        let token = sessions.insert_with_opt(|token| {
            trace!(target: "network", "{}: Initiating session", token);
            match Session::new(io, socket, id, token, &self.metadata.read()) {
                Ok(sess) => Some(Arc::new(Mutex::new(sess))),
                Err(e) => {
                    debug!(target: "network", "Error creating session: {:?}", e);
                    None
                }
            }
        });

        match token {
            Some(token) => {
                if let Some(id) = id {
                    let mut w = self.nodes.write();
                    let mut entry = w.get_mut(id);
                    if let Some(entry) = entry {
                        entry.stream_token = Some(token);
                    }
                }
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
        trace!(target: "network", "Connection closed: {}", stream);
        self.kill_connection(stream, io, true);
    }

    fn session_readable(
        &self, stream: StreamToken, io: &IoContext<NetworkIoMessage>,
    ) {
        // We check dropped_nodes first to make sure we stop processing communications from any
        // dropped peers
        let to_drop = { self.dropped_nodes.read().contains(&stream) };
        self.drop_peers(io);
        if to_drop {
            return;
        }

        let mut ready_protocols: Vec<ProtocolId> = Vec::new();
        let mut messages: Vec<(ProtocolId, Vec<u8>)> = Vec::new();
        let mut kill = false;
        let session = self.sessions.read().get(stream).cloned();

        // if let Some(session) = session.clone()
        if let Some(session) = session {
            loop {
                let mut sess = session.lock();
                let data = sess.readable(io, &self.metadata.read());
                match data {
                    Ok(SessionData::Ready) => {
                        //let mut sess = session.lock();
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
                    Err(_) => {
                        //let sess = session.lock();
                        kill = true;
                        break;
                    }
                }
            }
        }

        if kill {
            self.kill_connection(stream, io, true);
        }

        let handlers = self.handlers.read();
        if !ready_protocols.is_empty() {
            for protocol in ready_protocols {
                if let Some(handler) = handlers.get(&protocol).clone() {
                    debug!(
                        "Network Service: {}: peer {} connected",
                        self.local_addr(),
                        stream
                    );
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
        // We check dropped_nodes first to make sure we stop processing communications from any
        // dropped peers
        let to_drop = { self.dropped_nodes.read().contains(&stream) };
        self.drop_peers(io);
        if to_drop {
            return;
        }

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
            if let Err(e) = self.create_connection(socket, None, io) {
                debug!(target: "netweork", "Can't accept connection: {:?}", e);
            }
        }
    }

    fn kill_connection(
        &self, token: StreamToken, io: &IoContext<NetworkIoMessage>,
        remote: bool,
    )
    {
        let mut to_disconnect: Vec<ProtocolId> = Vec::new();
        let mut expired_session = None;
        let mut deregister = false;

        if let FIRST_SESSION...LAST_SESSION = token {
            let sessions = self.sessions.read();
            if let Some(session) = sessions.get(token).cloned() {
                expired_session = Some(session.clone());
                let mut sess = session.lock();
                if !sess.expired() {
                    if sess.is_ready() {
                        for (p, _) in self.handlers.read().iter() {
                            if sess.have_capability(*p) {
                                to_disconnect.push(*p);
                            }
                        }
                    }
                    sess.set_expired();
                }
                deregister = remote || sess.done();
            }
        }
        for p in to_disconnect {
            if let Some(h) = self.handlers.read().get(&p).clone() {
                println!("{}: peer {} disconnected", self.local_addr(), token);
                h.on_peer_disconnected(
                    &NetworkContext::new(io, p, self.sessions.clone()),
                    token,
                );
            }
        }
        if deregister {
            io.deregister_stream(token).unwrap_or_else(|e| {
                debug!("Error deregistering stream {:?}", e);
            })
        }
    }

    pub fn with_context<F>(
        &self, protocol: ProtocolId, io: &IoContext<NetworkIoMessage>,
        action: F,
    ) where
        F: FnOnce(&NetworkContext),
    {
        let context = NetworkContext::new(io, protocol, self.sessions.clone());
        action(&context);
    }
}

impl IoHandler<NetworkIoMessage> for NetworkServiceInner {
    fn initialize(&self, io: &IoContext<NetworkIoMessage>) {
        io.register_timer(HOUSEKEEPING, HOUSEKEEPING_TIMEOUT)
            .expect("Error registering housekeeping timer");
        io.message(NetworkIoMessage::Start).unwrap_or_else(|e| {
            warn!("Error sending IO notification: {:?}", e)
        });
        self.on_housekeeping(io);
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

    fn timeout(&self, io: &IoContext<NetworkIoMessage>, token: TimerToken) {
        match token {
            HOUSEKEEPING => self.on_housekeeping(io),
            _ => {}
        }
    }

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
                    ).expect("Error registering stream");
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
                ).expect("Error reregistering stream"),
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
