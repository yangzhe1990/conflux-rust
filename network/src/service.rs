use discovery::Discovery;
use ethkey::{Generator, KeyPair, Random, Secret};
use io::*;
use ip_utils::{map_external_address, select_public_address};
use mio::deprecated::EventLoop;
use mio::tcp::*;
use mio::udp::*;
use mio::*;
use node_table::*;
use parity_bytes::Bytes;
use parity_path::restrict_permissions_owner;
use parking_lot::{Mutex, RwLock};
use session;
use session::Session;
use session::SessionData;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use {
    Capability, DisconnectReason, Error, IpFilter, NetworkConfiguration,
    NetworkContext as NetworkContextTrait, NetworkIoMessage,
    NetworkProtocolHandler, NodeId, PeerId, PeerInfo, ProtocolId,
};

type Slab<T> = ::slab::Slab<T, usize>;

const MAX_SESSIONS: usize = 2048;

const DEFAULT_PORT: u16 = 32323;

const FIRST_SESSION: StreamToken = 0;
const LAST_SESSION: StreamToken = FIRST_SESSION + MAX_SESSIONS - 1;
const SYS_TIMER: TimerToken = LAST_SESSION + 1;
const TCP_ACCEPT: StreamToken = SYS_TIMER + 1;
const HOUSEKEEPING: TimerToken = SYS_TIMER + 2;
const UDP_MESSAGE: StreamToken = SYS_TIMER + 3;

const HOUSEKEEPING_TIMEOUT: Duration = Duration::from_secs(1);

pub const MAX_DATAGRAM_SIZE: usize = 1280;

const UDP_PROTOCOL_DISCOVERY: u8 = 1;

pub struct Datagram {
    pub payload: Bytes,
    pub address: SocketAddr,
}

pub struct UdpChannel {
    pub send_queue: VecDeque<Datagram>,
}

impl UdpChannel {
    pub fn new() -> UdpChannel {
        UdpChannel {
            send_queue: VecDeque::new(),
        }
    }

    pub fn dequeue_send(&mut self) -> Option<Datagram> {
        self.send_queue.pop_front()
    }

    pub fn requeue_send(&mut self, datagram: Datagram) {
        self.send_queue.push_front(datagram)
    }
}

pub struct UdpIOContext<'a> {
    channel: &'a RwLock<UdpChannel>,
}

impl<'a> UdpIOContext<'a> {
    pub fn new(channel: &'a RwLock<UdpChannel>) -> UdpIOContext<'a> {
        UdpIOContext { channel }
    }
}

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

    pub fn get_peer_info(&self) -> Option<Vec<PeerInfo>> {
        self.inner.as_ref().map(|inner| inner.get_peer_info())
    }
}

type SharedSession = Arc<Mutex<Session>>;

pub struct HostMetadata {
    #[allow(unused)]
    /// Our private and public keys.
    keys: KeyPair,
    config: NetworkConfiguration,
    pub capabilities: Vec<Capability>,
    pub local_address: SocketAddr,
    /// Local address + discovery port
    pub local_endpoint: NodeEndpoint,
    /// Public address + discovery port
    pub public_endpoint: NodeEndpoint,
}

#[derive(Debug)]
struct NodeEntry {
    id: NodeId,
    address: SocketAddr,
    stream_token: Option<StreamToken>,
}

struct NetworkServiceInner {
    metadata: RwLock<HostMetadata>,
    udp_socket: Mutex<UdpSocket>,
    tcp_listener: Mutex<TcpListener>,
    sessions: Arc<RwLock<Slab<SharedSession>>>,
    udp_channel: RwLock<UdpChannel>,
    discovery: Mutex<Option<Discovery>>,
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

        let keys = if let Some(ref secret) = config.use_secret {
            KeyPair::from_secret(secret.clone())?
        } else {
            config
                .config_path
                .clone()
                .and_then(|ref p| load_key(Path::new(&p)))
                .map_or_else(
                    || {
                        let key = Random
                            .generate()
                            .expect("Error generating random key pair");
                        if let Some(path) = config.config_path.clone() {
                            save_key(Path::new(&path), key.secret());
                        }
                        key
                    },
                    |s| {
                        KeyPair::from_secret(s)
                            .expect("Error creating node secret key")
                    },
                )
        };

        let tcp_listener = TcpListener::bind(&listen_address)?;
        listen_address = SocketAddr::new(
            listen_address.ip(),
            tcp_listener.local_addr()?.port(),
        );
        debug!(target: "network", "Listening at {:?}", listen_address);
        let udp_port = config.udp_port.unwrap_or_else(|| listen_address.port());
        let local_endpoint = NodeEndpoint {
            address: listen_address,
            udp_port,
        };
        let mut udp_addr = local_endpoint.address;
        udp_addr.set_port(local_endpoint.udp_port);
        let udp_socket =
            UdpSocket::bind(&udp_addr).expect("Error binding UDP socket");

        let public_address = config.public_address;
        let public_endpoint = match public_address {
            None => {
                let public_address =
                    select_public_address(local_endpoint.address.port());
                let public_endpoint = NodeEndpoint {
                    address: public_address,
                    udp_port: local_endpoint.udp_port,
                };
                if config.nat_enabled {
                    match map_external_address(&local_endpoint) {
                        Some(endpoint) => {
                            info!(
                                "NAT mapped to external address {}",
                                endpoint.address
                            );
                            endpoint
                        }
                        None => public_endpoint,
                    }
                } else {
                    public_endpoint
                }
            }
            Some(addr) => NodeEndpoint {
                address: addr,
                udp_port: local_endpoint.udp_port,
            },
        };

        let allow_ips = config.ip_filter.clone();
        let discovery = {
            if config.discovery_enabled {
                Some(Discovery::new(&keys, public_endpoint.clone(), allow_ips))
            } else {
                None
            }
        };

        let inner = NetworkServiceInner {
            metadata: RwLock::new(HostMetadata {
                keys,
                config: config.clone(),
                capabilities: Vec::new(),
                local_address: listen_address,
                local_endpoint,
                public_endpoint,
            }),
            udp_channel: RwLock::new(UdpChannel::new()),
            discovery: Mutex::new(discovery),
            udp_socket: Mutex::new(udp_socket),
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

    fn initialize_udp_protocols(
        &self, io: &IoContext<NetworkIoMessage>,
    ) -> Result<(), Error> {
        // Initialize discovery
        if let Some(discovery) = self.discovery.lock().as_mut() {
            discovery.add_node_list(&UdpIOContext::new(&self.udp_channel));
            //io.register_timer(FAST_DISCOVERY_REFRESH, FAST_DISCOVERY_REFRESH_TIMEOUT)?;
            //io.register_timer(DISCOVERY_REFRESH, DISCOVERY_REFRESH_TIMEOUT)?;
            //io.register_timer(DISCOVERY_ROUND, DISCOVERY_ROUND_TIMEOUT)?;
        }
        //io.register_timer(NODE_TABLE, NODE_TABLE_TIMEOUT)?;

        Ok(())
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.metadata.read().local_address
    }

    fn add_node(&self, address: SocketAddr) -> Result<NodeId, Error> {
        // TODO: replace address with real node identifier
        let id = address;
        let node = NodeEntry {
            id: id,
            address: address,
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
            self.kill_connection(*token, io);
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

        let (socket, address) = {
            let address = {
                let mut nodes = self.nodes.write();
                if let Some(node) = nodes.get_mut(id) {
                    node.address
                } else {
                    debug!(target: "network", "Abort connect. Node expired");
                    return;
                }
            };
            match TcpStream::connect(&address) {
                Ok(socket) => {
                    trace!(target: "network", "{}: connecting to {:?}", id, address);
                    (socket, address)
                }
                Err(e) => {
                    debug!(target: "network", "{}: can't connect o address {:?} {:?}", id, address, e);
                    return;
                }
            }
        };

        if let Err(e) = self.create_connection(socket, address, Some(id), io) {
            debug!(target: "network", "Can't create connection: {:?}", e);
        }
    }

    pub fn get_peer_info(&self) -> Vec<PeerInfo> {
        let sessions = self.sessions.read();
        let sessions = &*sessions;

        let mut peers = Vec::with_capacity(sessions.count());
        for i in (0..MAX_SESSIONS).map(|x| x + FIRST_SESSION) {
            let session = sessions.get(i);
            if session.is_some() {
                let sess = session.unwrap().lock();
                peers.push(PeerInfo {
                    id: i,
                    addr: sess.address(),
                    caps: sess.metadata.peer_capabilities.clone(),
                })
            }
        }
        peers
    }

    #[allow(unused)]
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
        self.initialize_udp_protocols(io)?;
        io.register_stream(UDP_MESSAGE)?;
        io.register_stream(TCP_ACCEPT)?;
        Ok(())
    }

    fn create_connection(
        &self, socket: TcpStream, address: SocketAddr, id: Option<&NodeId>,
        io: &IoContext<NetworkIoMessage>,
    ) -> Result<(), Error>
    {
        let mut sessions = self.sessions.write();

        let token = sessions.insert_with_opt(|token| {
            trace!(target: "network", "{}: Initiating session", token);
            match Session::new(
                io,
                socket,
                address,
                id,
                token,
                &self.metadata.read(),
            ) {
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
        self.kill_connection(stream, io);
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
            self.kill_connection(stream, io);
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
            let (socket, address) = match self.tcp_listener.lock().accept() {
                Ok((sock, addr)) => (sock, addr),
                Err(e) => {
                    if e.kind() != io::ErrorKind::WouldBlock {
                        debug!(target: "network", "Error accepting connection: {:?}", e);
                    }
                    break;
                }
            };
            if let Err(e) = self.create_connection(socket, address, None, io) {
                debug!(target: "netweork", "Can't accept connection: {:?}", e);
            }
        }
    }

    fn kill_connection(
        &self, token: StreamToken, io: &IoContext<NetworkIoMessage>,
    ) {
        let mut to_disconnect: Vec<ProtocolId> = Vec::new();
        let mut deregister = false;

        if let FIRST_SESSION...LAST_SESSION = token {
            let sessions = self.sessions.read();
            if let Some(session) = sessions.get(token).cloned() {
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
                deregister = sess.done();
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

    fn udp_readable(&self, io: &IoContext<NetworkIoMessage>) {
        let udp_socket = self.udp_socket.lock();
        let mut buf = [0u8; MAX_DATAGRAM_SIZE];
        match udp_socket.recv_from(&mut buf) {
			Ok(Some((len, address))) => self.on_udp_packet(&buf[0..len], address).unwrap_or_else(|e| {
				debug!(target: "network", "Error processing UDP packet: {:?}", e);
			}),
			Ok(_) => {},
			Err(e) => {
				debug!(target: "network", "Error reading UPD socket: {:?}", e);
			},
		};
        io.update_registration(UDP_MESSAGE)
			.unwrap_or_else(|e| {
				debug!(target: "network" ,"Error updating UDP registration: {:?}", e)
			});
    }

    fn udp_writable(&self, io: &IoContext<NetworkIoMessage>) {
        let udp_socket = self.udp_socket.lock();
        let mut udp_channel = self.udp_channel.write();
        while let Some(data) = udp_channel.dequeue_send() {
            match udp_socket.send_to(&data.payload, &data.address) {
                Ok(Some(size)) if size == data.payload.len() => {}
                Ok(Some(_)) => {
                    warn!(target: "network", "UDP sent incomplete datagram");
                }
                Ok(None) => {
                    udp_channel.requeue_send(data);
                    return;
                }
                Err(e) => {
                    debug!(target: "network", "UDP send error: {:?}, address: {:?}", e, &data.address);
                    return;
                }
            }
        }
        io.update_registration(UDP_MESSAGE)
			.unwrap_or_else(|e| {
				debug!(target: "network", "Error updating UDP registration: {:?}", e)
			});
    }

    fn on_udp_packet(
        &self, packet: &[u8], from: SocketAddr,
    ) -> Result<(), Error> {
        let res = match packet[0] {
            UDP_PROTOCOL_DISCOVERY => {
                if let Some(discovery) = self.discovery.lock().as_mut() {
                    discovery.on_packet(&packet[1..], from)
                } else {
                    warn!(target: "network", "Discovery is not ready. Drop the message!");
                    Ok(())
                }
            },
            _ => {
                warn!(target: "network", "Unknown UDP protocol. Simply drops the message!");
                Ok(())
            }
        };
        res
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
            UDP_MESSAGE => self.udp_readable(io),
            _ => panic!("Received unknown readable token"),
        }
    }

    fn stream_writable(
        &self, io: &IoContext<NetworkIoMessage>, stream: StreamToken,
    ) {
        match stream {
            FIRST_SESSION...LAST_SESSION => self.session_writable(stream, io),
            UDP_MESSAGE => self.udp_writable(io),
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
            } //_ => {}
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
            UDP_MESSAGE => {
                event_loop
                    .register(
                        &*self.udp_socket.lock(),
                        reg,
                        Ready::all(),
                        PollOpt::edge(),
                    ).expect("Error registering UDP socket");
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
    fn send(&self, peer: PeerId, msg: Vec<u8>) -> Result<(), Error> {
        let session = { self.sessions.read().get(peer).cloned() };
        trace!(target: "network", "Sending {} bytes to {}", msg.len(), peer);
        if let Some(session) = session {
            session.lock().send_packet(
                self.io,
                Some(self.protocol),
                session::PACKET_USER,
                &msg,
            )?;
        }
        Ok(())
    }

    fn disconnect_peer(&self, peer: PeerId) {
        // FIXME: Here we cannot get the handler to call on_peer_disconnected()
        let sessions = self.sessions.read();
        if let Some(session) = sessions.get(peer).cloned() {
            let mut sess = session.lock();
            if !sess.expired() {
                sess.set_expired();
            }
            if sess.done() {
                self.io.deregister_stream(sess.token()).unwrap_or_else(|e| {
                    debug!("Error deregistering stream {:?}", e);
                })
            }
        }
    }
}

fn save_key(path: &Path, key: &Secret) {
    let mut path_buf = PathBuf::from(path);
    if let Err(e) = fs::create_dir_all(path_buf.as_path()) {
        warn!("Error creating key directory: {:?}", e);
        return;
    };
    path_buf.push("key");
    let path = path_buf.as_path();
    let mut file = match fs::File::create(&path) {
        Ok(file) => file,
        Err(e) => {
            warn!("Error creating key file: {:?}", e);
            return;
        }
    };
    if let Err(e) = restrict_permissions_owner(path, true, false) {
        warn!(target: "network", "Failed to modify permissions of the file ({})", e);
    }
    if let Err(e) = file.write(&key.hex().into_bytes()[2..]) {
        warn!("Error writing key file: {:?}", e);
    }
}

fn load_key(path: &Path) -> Option<Secret> {
    let mut path_buf = PathBuf::from(path);
    path_buf.push("key");
    let mut file = match fs::File::open(path_buf.as_path()) {
        Ok(file) => file,
        Err(e) => {
            debug!("Error opening key file: {:?}", e);
            return None;
        }
    };
    let mut buf = String::new();
    match file.read_to_string(&mut buf) {
        Ok(_) => {}
        Err(e) => {
            warn!("Error reading key file: {:?}", e);
            return None;
        }
    }
    match Secret::from_str(&buf) {
        Ok(key) => Some(key),
        Err(e) => {
            warn!("Error parsing key file: {:?}", e);
            None
        }
    }
}
