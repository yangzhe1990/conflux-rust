use bytes::{Buf, BufMut, Bytes, BytesMut, IntoBuf};
use connection::{
    Connection as TcpConnection, PacketSizer as PacketSizerTrait,
    MAX_PAYLOAD_SIZE,
};
use io::*;
use mio::deprecated::*;
use mio::tcp::*;
use mio::*;
use rlp::{DecoderError, Rlp, RlpStream, EMPTY_LIST_RLP};
use service::HostMetadata;
use std::{io, str};
use {
    Capability, DisconnectReason, Error, ErrorKind, NodeId, ProtocolId,
    SessionMetadata,
};

struct PacketSizer;

impl PacketSizerTrait for PacketSizer {
    fn packet_size(raw_packet: &Bytes) -> usize {
        let buf = &mut raw_packet.into_buf() as &mut Buf;
        if buf.remaining() >= ::std::mem::size_of::<u16>() {
            let size = buf.get_u16_le() as usize;
            if buf.remaining() >= size {
                size + ::std::mem::size_of::<u16>()
            } else {
                0
            }
        } else {
            0
        }
    }
}

type Connection = TcpConnection<PacketSizer>;

pub struct Session {
    pub metadata: SessionMetadata,
    connection: Connection,
    sent_hello: bool,
    had_hello: bool,
    expired: bool,
}

pub enum SessionData {
    None,
    Ready,
    Message { data: Vec<u8>, protocol: ProtocolId },
    Continue,
}

const PACKET_HELLO: u8 = 0x80;
const PACKET_DISCONNECT: u8 = 0x01;
const PACKET_PING: u8 = 0x02;
const PACKET_PONG: u8 = 0x03;
pub const PACKET_USER: u8 = 0x10;

impl Session {
    pub fn new<Message: Send + Sync + Clone + 'static>(
        io: &IoContext<Message>, socket: TcpStream, id: Option<&NodeId>,
        token: StreamToken, host: &HostMetadata,
    ) -> Result<Session, Error>
    {
        let originated = id.is_some();
        let mut session = Session {
            metadata: SessionMetadata {
                id: id.cloned(),
                capabilities: Vec::new(),
                peer_capabilities: Vec::new(),
                originated,
            },
            connection: Connection::new(token, socket),
            sent_hello: false,
            had_hello: false,
            expired: false,
        };
        if originated {
            session.write_hello(io, host)?;
            session.sent_hello = true;
        }
        Ok(session)
    }

    pub fn have_capability(&self, protocol: ProtocolId) -> bool {
        self.metadata
            .capabilities
            .iter()
            .any(|c| c.protocol == protocol)
    }

    pub fn is_ready(&self) -> bool { self.had_hello }

    pub fn expired(&self) -> bool { self.expired }

    pub fn set_expired(&mut self) { self.expired = true; }

    pub fn done(&self) -> bool {
        self.expired() && !self.connection().is_sending()
    }

    fn connection(&self) -> &Connection { &self.connection }

    pub fn token(&self) -> StreamToken { self.connection().token() }

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

    pub fn deregister_socket<H: Handler>(
        &self, event_loop: &mut EventLoop<H>,
    ) -> Result<(), Error> {
        self.connection().deregister_socket(event_loop)?;
        Ok(())
    }

    pub fn readable<Message: Send + Sync + Clone>(
        &mut self, io: &IoContext<Message>, host: &HostMetadata,
    ) -> Result<SessionData, Error> {
        if self.expired() {
            return Ok(SessionData::None);
        }
        if !self.sent_hello {
            debug!(target: "network", "{} Write Hello, sent_hello {}", self.token(), self.sent_hello);
            self.write_hello(io, host)?;
            self.sent_hello = true;
        }
        match self.connection.readable()? {
            Some(data) => Ok(self.read_packet(io, &data, host)?),
            None => Ok(SessionData::None),
        }
    }

    fn read_packet<Message: Send + Sync + Clone>(
        &mut self, io: &IoContext<Message>, data: &Bytes, host: &HostMetadata,
    ) -> Result<SessionData, Error> {
        let packet_id = data[2];
        if packet_id != PACKET_HELLO
            && packet_id != PACKET_DISCONNECT
            && !self.had_hello
        {
            return Err(ErrorKind::BadProtocol.into());
        }
        let data = &data[3..];
        match packet_id {
            PACKET_HELLO => {
                debug!(target: "network", "{}: Hello", self.token());
                let rlp = Rlp::new(&data);
                self.read_hello(io, &rlp, host)?;
                Ok(SessionData::Ready)
            }
            PACKET_DISCONNECT => {
                let rlp = Rlp::new(&data);
                let reason: u8 = rlp.val_at(0)?;
                if self.had_hello {
                    debug!(target: "network", "{}: Disconnected: {:?}", self.token(), DisconnectReason::from_u8(reason));
                }
                Err(ErrorKind::Disconnect(DisconnectReason::from_u8(reason))
                    .into())
            }
            PACKET_PING => {
                self.send_pong(io)?;
                Ok(SessionData::Continue)
            }
            PACKET_PONG => Ok(SessionData::Continue),
            PACKET_USER => {
                if data.len() < 3 {
                    Err(ErrorKind::Decoder.into())
                } else {
                    let mut protocol: ProtocolId = [0u8; 3];
                    protocol.clone_from_slice(&data[..3]);
                    Ok(SessionData::Message {
                        data: (&data[3..]).to_vec(),
                        protocol,
                    })
                }
            }
            _ => {
                debug!(target: "network", "Unknown packet: {:?}", packet_id);
                Ok(SessionData::Continue)
            }
        }
    }

    fn read_hello<Message: Send + Sync + Clone>(
        &mut self, io: &IoContext<Message>, rlp: &Rlp, host: &HostMetadata,
    ) -> Result<(), Error> {
        let mut peer_caps: Vec<Capability> = rlp.list_at(0)?;

        let mut caps: Vec<Capability> = Vec::new();
        for hc in &host.capabilities {
            if peer_caps
                .iter()
                .any(|c| c.protocol == hc.protocol && c.version == hc.version)
            {
                caps.push(hc.clone());
            }
        }

        caps.retain(|c| {
            host.capabilities
                .iter()
                .any(|hc| hc.protocol == c.protocol && hc.version == c.version)
        });
        let mut i = 0;
        while i < caps.len() {
            if caps.iter().any(|c| {
                c.protocol == caps[i].protocol && c.version > caps[i].version
            }) {
                caps.remove(i);
            } else {
                i += 1;
            }
        }
        caps.sort();

        self.metadata.capabilities = caps;
        self.metadata.peer_capabilities = peer_caps;
        if self.metadata.capabilities.is_empty() {
            trace!(target: "network", "No common capabilities with peer.");
            return Err(self.disconnect(io, DisconnectReason::UselessPeer));
        }
        self.send_ping(io)?;
        self.had_hello = true;
        Ok(())
    }

    pub fn send_packet<Message>(
        &mut self, io: &IoContext<Message>, protocol: Option<ProtocolId>,
        packet_id: u8, data: &[u8],
    ) -> Result<(), Error>
    where
        Message: Send + Sync + Clone,
    {
        if protocol.is_some()
            && (self.metadata.capabilities.is_empty() || !self.had_hello)
        {
            debug!(target: "network", "Sending to unconfirmed session {}, protocol: {:?}, packet: {}", self.token(), protocol.as_ref().map(|p| str::from_utf8(&p[..]).unwrap_or("???")), packet_id);
            bail!(ErrorKind::BadProtocol);
        }
        if self.expired() {
            return Err(ErrorKind::Expired.into());
        }
        let packet_size =
            1 + protocol.map(|p| p.len()).unwrap_or(0) + data.len();
        if packet_size > MAX_PAYLOAD_SIZE {
            bail!(ErrorKind::OversizedPacket);
        }
        let mut packet = BytesMut::with_capacity(2 + packet_size);
        packet.put_u16_le(packet_size as u16);
        packet.put_u8(packet_id);
        if let Some(protocol) = protocol {
            packet.put_slice(&protocol);
        }
        packet.put_slice(&data);
        self.connection.send(io, &packet[..]);
        Ok(())
    }

    pub fn disconnect<Message: Send + Sync + Clone>(
        &mut self, io: &IoContext<Message>, reason: DisconnectReason,
    ) -> Error {
        let mut rlp = RlpStream::new();
        rlp.begin_list(1).append(&(reason as u32));
        self.send_packet(io, None, PACKET_DISCONNECT, &rlp.drain())
            .ok();
        ErrorKind::Disconnect(reason).into()
    }

    fn send_ping<Message: Send + Sync + Clone>(
        &mut self, io: &IoContext<Message>,
    ) -> Result<(), Error> {
        self.send_packet(io, None, PACKET_PING, &[])
    }

    fn send_pong<Message: Send + Sync + Clone>(
        &mut self, io: &IoContext<Message>,
    ) -> Result<(), Error> {
        self.send_packet(io, None, PACKET_PONG, &[])
    }

    fn write_hello<Message: Send + Sync + Clone>(
        &mut self, io: &IoContext<Message>, host: &HostMetadata,
    ) -> Result<(), Error> {
        debug!(target: "network", "{} Sending Hello", self.token());
        let mut rlp = RlpStream::new();
        rlp.begin_list(2)
            .append_list(&host.capabilities)
            .append(&host.local_address.port());
        self.send_packet(io, None, PACKET_HELLO, &rlp.drain())
    }

    pub fn writable<Message: Send + Sync + Clone>(
        &mut self, io: &IoContext<Message>,
    ) -> Result<(), Error> {
        self.connection.writable(io)?;
        Ok(())
    }
}
