use io::*;
use mio::deprecated::EventLoop;
use mio::tcp::*;
use mio::*;
use network::node_table::*;
use network::{Error, ErrorKind, NetworkConfiguration, NetworkIoMessage};
use parking_lot::{Mutex, RwLock};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;

type Slab<T> = ::slab::Slab<T, usize>;

const MAX_SESSIONS: usize = 1024 + MAX_HANDSHAKES;
const MAX_HANDSHAKES: usize = 1024;

const DEFAULT_PORT: u16 = 32323;

const TCP_ACCEPT: StreamToken = SYS_TIMER + 1;
const FIRST_SESSION: StreamToken = 0;
const LAST_SESSION: StreamToken = FIRST_SESSION + MAX_SESSIONS - 1;
const SYS_TIMER: TimerToken = LAST_SESSION + 1;

type Session = i32;

pub struct Host {
    tcp_listener: Mutex<TcpListener>,
    sessions: Arc<RwLock<Slab<Session>>>,
}

impl Host {
    pub fn new(config: &NetworkConfiguration) -> Result<Host, Error> {
        let mut listen_address = match config.listen_address {
            None => SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), DEFAULT_PORT)),
            Some(addr) => addr,
        };
        let tcp_listener = TcpListener::bind(&listen_address)?;
        listen_address = SocketAddr::new(listen_address.ip(), tcp_listener.local_addr()?.port());

        let host = Host {
            tcp_listener: Mutex::new(tcp_listener),
            sessions: Arc::new(RwLock::new(Slab::new_starting_at(
                FIRST_SESSION,
                MAX_SESSIONS,
            ))),
        };
        Ok(host)
    }

    fn init_public_interface(&self, io: &IoContext<NetworkIoMessage>) -> Result<(), Error> {
        io.register_stream(TCP_ACCEPT)?;
        Ok(())
    }

    fn accept(&self, io: &IoContext<NetworkIoMessage>) {
        loop {
            let socket = match self.tcp_listener.lock().accept() {
                Ok((sock, _addr)) => {
                    println!("Connection accepted");
                    sock
                }
                Err(e) => {
                    break;
                }
            };
            if let Err(e) = self.create_connection(socket, None, io) {
                println!("Can't accept connection: {:?}", e);
            }
        }
    }

    fn create_connection(
        &self, socket: TcpStream, id: Option<&NodeId>, io: &IoContext<NetworkIoMessage>,
    ) -> Result<(), Error> {
        let mut sessions = self.sessions.write();

        let token = sessions.insert_with_opt(|token| {
            println!("{}: initiating session {:?}", token, id);
            Some(123)
        });

        match token {
            Some(token) => io.register_stream(token).map(|_| ()).map_err(Into::into),
            None => {
                println!("Max session reached");
                Ok(())
            }
        }
    }
}

impl IoHandler<NetworkIoMessage> for Host {
    fn initialize(&self, io: &IoContext<NetworkIoMessage>) {
        io.message(NetworkIoMessage::InitPublicInterface).unwrap();
    }

    fn stream_hup(&self, io: &IoContext<NetworkIoMessage>, stream: StreamToken) {}

    fn stream_readable(&self, io: &IoContext<NetworkIoMessage>, stream: StreamToken) {
        match stream {
            TCP_ACCEPT => self.accept(io),
            _ => panic!("Received unknown readable token"),
        }
    }

    fn stream_writable(&self, io: &IoContext<NetworkIoMessage>, stream: StreamToken) {}

    fn timeout(&self, io: &IoContext<NetworkIoMessage>, token: TimerToken) {}

    fn message(&self, io: &IoContext<NetworkIoMessage>, message: &NetworkIoMessage) {
        match *message {
            NetworkIoMessage::InitPublicInterface => self.init_public_interface(io).unwrap(),
            _ => {}
        }
    }

    fn register_stream(
        &self, stream: StreamToken, reg: Token,
        event_loop: &mut EventLoop<IoManager<NetworkIoMessage>>,
    ) {
        match stream {
            FIRST_SESSION...LAST_SESSION => {
                println!("Registering socket {:?}", reg);
            }
            TCP_ACCEPT => event_loop
                .register(
                    &*self.tcp_listener.lock(),
                    Token(TCP_ACCEPT),
                    Ready::all(),
                    PollOpt::edge(),
                )
                .expect("Error registering stream"),
            _ => println!("Unexpected stream registration: token = {}", stream),
        }
    }

    fn deregister_stream(
        &self, stream: StreamToken, event_loop: &mut EventLoop<IoManager<NetworkIoMessage>>,
    ) {
    }

    fn update_stream(
        &self, stream: StreamToken, reg: Token,
        event_loop: &mut EventLoop<IoManager<NetworkIoMessage>>,
    ) {
    }
}
