use ethereum_types::H256;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Message;
use MsgId;

#[derive(Debug, PartialEq)]
pub struct GetTerminalBlockHashes;

impl Message for GetTerminalBlockHashes {
    fn msg_id(&self) -> MsgId { MsgId::GET_TERMINAL_BLOCK_HASHES }
}

impl Encodable for GetTerminalBlockHashes {
    fn rlp_append(&self, stream: &mut RlpStream) {}
}

impl Decodable for GetTerminalBlockHashes {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetTerminalBlockHashes)
    }
}
