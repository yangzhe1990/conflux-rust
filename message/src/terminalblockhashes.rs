use ethereum_types::{H256};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Message;
use MsgId;

#[derive(Debug, PartialEq)]
pub struct GetTerminalBlockHashesResponse {
    pub reqid: u16,
    pub hashes: Vec<H256>,
}

impl Message for GetTerminalBlockHashesResponse {
    fn msg_id(&self) -> MsgId {
        MsgId::GET_TERMINAL_BLOCK_HASHES_RESPONSE
    }
}

impl Encodable for GetTerminalBlockHashesResponse {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.reqid)
            .append_list(&self.hashes);
    }
}

impl Decodable for GetTerminalBlockHashesResponse {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetTerminalBlockHashesResponse {
            reqid: rlp.val_at(0)?,
            hashes: rlp.list_at(1)?,
        })
    }
}
