use ethereum_types::{H256, U256};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Message;
use MsgId;

#[derive(Debug, PartialEq)]
pub struct TerminalBlockHashes {
    pub hashes: Vec<H256>,
}

impl Message for TerminalBlockHashes {
    fn msg_id(&self) -> MsgId { MsgId::TERMINAL_BLOCK_HASHES }
}

impl Encodable for TerminalBlockHashes {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.hashes);
    }
}

impl Decodable for TerminalBlockHashes {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(TerminalBlockHashes {
            hashes: rlp.as_list()?,
        })
    }
}
