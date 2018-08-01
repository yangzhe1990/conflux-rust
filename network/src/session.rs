use bytes::{Buf, BufMut, Bytes, BytesMut, IntoBuf};
use connection::{
    Connection as TcpConnection, PacketSizer as PacketSizerTrait,
    MAX_PAYLOAD_SIZE,
};
use io::*;
use mio::deprecated::*;
use mio::tcp::*;
use mio::*;
use service::HostMetadata;
use std::{io, str};
use {
    Capability, DisconnectReason, Error, ErrorKind, ProtocolId, SessionMetadata,
};

struct PacketSizer;

impl PacketSizerTrait for PacketSizer {
    fn packet_size(raw_packet: &Bytes) -> usize {
        let buf = &mut raw_packet.into_buf() as &mut Buf;
        if buf.remaining() >= ::std::mem::size_of::<u16>() {
            let size = buf.get_u16_le() as usize;
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
const PACKET_USER: u8 = 0x10;

impl Session {
    pub fn new<Message: Send + Sync + Clone + 'static>(
        io: &IoContext<Message>, socket: TcpStream, token: StreamToken,
    ) -> Result<Session, Error> {
        Ok(Session {
            metadata: SessionMetadata {
                capabilities: Vec::new(),
                peer_capabilities: Vec::new(),
            },
            connection: Connection::new(token, socket),
            sent_hello: false,
            had_hello: false,
            expired: false,
        })
    }

    pub fn disconnect<Message: Send + Sync + Clone>(
        &mut self, io: &IoContext<Message>, reason: DisconnectReason,
    ) -> Error {
        let mut buf = BytesMut::new();
        buf.put_u8(reason as u8);
        self.send_packet(io, None, PACKET_DISCONNECT, &buf[..]).ok();
        ErrorKind::Disconnect(reason).into()
    }

    pub fn have_capability(&self, protocol: ProtocolId) -> bool {
        self.metadata
            .capabilities
            .iter()
            .any(|c| c.protocol == protocol)
    }

    pub fn expired(&self) -> bool { self.expired }

    pub fn done(&self) -> bool {
        self.expired() && !self.connection().is_sending()
    }

    fn connection(&self) -> &Connection { &self.connection }

    fn token(&self) -> StreamToken { self.connection().token() }

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
        let packet_id = data[0];
        if packet_id != PACKET_HELLO
            && packet_id != PACKET_DISCONNECT
            && !self.had_hello
        {
            return Err(ErrorKind::BadProtocol.into());
        }
        let mut buf = data.into_buf();
        buf.advance(1);
        match packet_id {
            PACKET_HELLO => {
                self.read_hello(io, &mut buf, host)?;
                Ok(SessionData::Ready)
            }
            PACKET_DISCONNECT => {
                let reason: u8 = buf.get_u8();
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
                let protocol: ProtocolId =
                    [buf.get_u8(), buf.get_u8(), buf.get_u8()];
                Ok(SessionData::Message {
                    data: buf.collect(),
                    protocol,
                })
            }
            _ => {
                debug!(target: "network", "Unknown packet: {:?}", packet_id);
                Ok(SessionData::Continue)
            }
        }
    }

    fn read_hello<Message: Send + Sync + Clone, B: Buf>(
        &mut self, io: &IoContext<Message>, buf: &mut B, host: &HostMetadata,
    ) -> Result<(), Error> {
        let mut peer_caps: Vec<Capability> = Vec::new();
        let len = buf.get_u16_le();
        for i in 0..len {
            peer_caps.push(Capability::decode(buf));
        }

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
        let mut size = 1usize;
        if let Some(protocol) = protocol {
            size += protocol.len();
        }
        size += data.len();
        if size > MAX_PAYLOAD_SIZE {
            bail!(ErrorKind::OversizedPacket);
        }
        let mut buf = BytesMut::with_capacity(size + 2);
        buf.put_u16_le(size as u16);
        buf.put_u8(packet_id);
        if let Some(protocol) = protocol {
            buf.put_slice(&protocol[..]);
        }
        buf.put_slice(data);
        self.connection.send(io, buf.to_vec());
        Ok(())
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
        let mut buf = BytesMut::new();
        buf.put_u16_le(host.capabilities.len() as u16);
        for i in 0..host.capabilities.len() {
            host.capabilities[i].encode(&mut buf);
        }
        self.send_packet(io, None, PACKET_HELLO, &buf[..])
    }

    pub fn writable<Message: Send + Sync + Clone>(
        &mut self, io: &IoContext<Message>,
    ) -> Result<(), Error> {
        self.connection.writable(io)?;
        Ok(())
    }
}
