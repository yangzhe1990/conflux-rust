use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Message;
use MsgId;

#[derive(Debug, PartialEq)]
pub struct GetTerminalBlockHashes {
    pub reqid: u16,
}

impl Message for GetTerminalBlockHashes {
    fn msg_id(&self) -> MsgId {
        MsgId::GET_TERMINAL_BLOCK_HASHES
    }
    fn set_request_id(&mut self, reqid: u16) {
        self.reqid = reqid
    }
}

impl Encodable for GetTerminalBlockHashes {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.begin_list(1).append(&self.reqid);
    }
}

impl Decodable for GetTerminalBlockHashes {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetTerminalBlockHashes {
            reqid: rlp.val_at(0)?,
        })
    }
}
