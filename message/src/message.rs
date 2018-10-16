//use std::convert::Into;
use rlp::Encodable;
use std::fmt;

pub type MsgIdInner = u8;
#[derive(Debug, PartialEq, Eq)]
pub struct MsgId(MsgIdInner);

impl MsgId {
    pub const BLOCKS: MsgId = MsgId(0x0d);
    pub const BLOCK_BODIES: MsgId = MsgId(0x08);
    pub const BLOCK_HASHES: MsgId = MsgId(0x04);
    pub const BLOCK_HEADERS: MsgId = MsgId(0x06);
    pub const GET_BLOCKS: MsgId = MsgId(0x0c);
    pub const GET_BLOCK_BODIES: MsgId = MsgId(0x07);
    pub const GET_BLOCK_HASHES: MsgId = MsgId(0x03);
    pub const GET_BLOCK_HEADERS: MsgId = MsgId(0x05);
    pub const GET_TERMINAL_BLOCK_HASHES: MsgId = MsgId(0x0b);
    pub const NEW_BLOCK: MsgId = MsgId(0x09);
    pub const NEW_BLOCK_HASHES: MsgId = MsgId(0x01);
    pub const STATUS: MsgId = MsgId(0x00);
    pub const TERMINAL_BLOCK_HASHES: MsgId = MsgId(0x0a);
    pub const TRANSACTIONS: MsgId = MsgId(0x02);
}

impl From<u8> for MsgId {
    fn from(inner: u8) -> Self { MsgId(inner) }
}

impl Into<u8> for MsgId {
    fn into(self) -> u8 { self.0 }
}

impl fmt::Display for MsgId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub trait Message: Send + Encodable + 'static {
    fn msg_id(&self) -> MsgId;
}
