use io::IoError;
use rlp;
use std::{fmt, io, net};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DisconnectReason {
    DisconnectRequested,
    Unknown,
}

impl DisconnectReason {
    pub fn from_u8(n: u8) -> DisconnectReason {
        match n {
            0 => DisconnectReason::DisconnectRequested,
            _ => DisconnectReason::Unknown,
        }
    }
}

impl fmt::Display for DisconnectReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg = match *self {
            DisconnectRequested => "disconnect requested",
            Unknown => "unknown",
        };

        f.write_str(msg)
    }
}

error_chain! {
    foreign_links {
        SocketIo(IoError);
    }

    errors {
        BadProtocol {
            description("Bad protocol"),
            display("Bad protocol"),
        }

        Decoder {
            description("Decoder error"),
            display("Decoder error"),
        }

        Expired {
            description("Expired message"),
            display("Expired message"),
        }

        Disconnect(reason: DisconnectReason) {
            description("Peer disconnected"),
            display("Peer disconnected: {}", reason),
        }

        OversizedPacket {
            description("Packet is too large"),
            display("Packet is too large"),
        }

        Io(err: io::Error) {
            description("IO Error"),
            display("Unexpected IO error: {}", err),
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self { Error::from_kind(ErrorKind::Io(err)) }
}

impl From<rlp::DecoderError> for Error {
    fn from(_err: rlp::DecoderError) -> Self { ErrorKind::Decoder.into() }
}
