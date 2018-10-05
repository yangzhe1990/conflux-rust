extern crate core;
extern crate ethcore_bytes as bytes;
extern crate ethereum_types;
extern crate primitives;
extern crate rlp;

pub type MsgIdInner = u8;
#[derive(Debug, PartialEq, Eq)]
pub struct MsgId(MsgIdInner);

impl MsgId {
    pub const STATUS: MsgId = MsgId(0x00);
    pub const NEW_BLOCK_HASHES: MsgId = MsgId(0x01);
    pub const TRANSACTIONS: MsgId = MsgId(0x02);
    pub const GET_BLOCK_HASHES: MsgId = MsgId(0x03);
    pub const BLOCK_HASHES: MsgId = MsgId(0x04);
    pub const GET_BLOCK_HEADERS: MsgId = MsgId(0x05);
    pub const BLOCK_HEADERS: MsgId = MsgId(0x06);
    pub const GET_BLOCK_BODIES: MsgId = MsgId(0x07);
    pub const BLOCK_BODIES: MsgId = MsgId(0x08);
    pub const NEW_BLOCK: MsgId = MsgId(0x09);
}

impl From<u8> for MsgId {
    fn from(inner: u8) -> Self {
        MsgId(inner)
    }
}

impl std::convert::Into<u8> for MsgId {
    fn into(self) -> u8 {
        self.0
    }
}

impl std::fmt::Display for MsgId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub use blockbodies::{BlockBodies, BlockBody};
pub use blockheaders::BlockHeaders;
pub use getblockbodies::GetBlockBodies;
pub use getblockhashes::GetBlockHashes;
pub use getblockheaders::{BlockId, GetBlockHeaders};
pub use message::Message;
pub use newblock::NewBlock;
pub use newblockhashes::NewBlockHashes;
pub use status::Status;
pub use transactions::Transactions;

mod blockbodies;
mod blockheaders;
mod getblockbodies;
mod getblockhashes;
mod getblockheaders;
mod message;
mod newblock;
mod newblockhashes;
mod status;
mod transactions;
