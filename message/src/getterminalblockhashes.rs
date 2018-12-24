use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use std::ops::{Deref, DerefMut};
use crate::Message;
use crate::MsgId;
use crate::RequestId;

#[derive(Debug, PartialEq)]
pub struct GetTerminalBlockHashes {
    pub request_id: RequestId,
}

impl Message for GetTerminalBlockHashes {
    fn msg_id(&self) -> MsgId { MsgId::GET_TERMINAL_BLOCK_HASHES }
}

impl Deref for GetTerminalBlockHashes {
    type Target = RequestId;

    fn deref(&self) -> &Self::Target { &self.request_id }
}

impl DerefMut for GetTerminalBlockHashes {
    fn deref_mut(&mut self) -> &mut RequestId { &mut self.request_id }
}

impl Encodable for GetTerminalBlockHashes {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.begin_list(1).append(&self.request_id);
    }
}

impl Decodable for GetTerminalBlockHashes {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(GetTerminalBlockHashes {
            request_id: rlp.val_at(0)?,
        })
    }
}
