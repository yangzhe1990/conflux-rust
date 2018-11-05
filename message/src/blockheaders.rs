use primitives::BlockHeader;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Message;
use MsgId;

#[derive(Debug, PartialEq, Default)]
pub struct GetBlockHeadersResponse {
    pub reqid: u16,
    pub headers: Vec<BlockHeader>,
}

impl Message for GetBlockHeadersResponse {
    fn msg_id(&self) -> MsgId {
        MsgId::GET_BLOCK_HEADERS_RESPONSE
    }
}

impl Encodable for GetBlockHeadersResponse {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(2)
            .append(&self.reqid)
            .append_list(&self.headers);
    }
}

impl Decodable for GetBlockHeadersResponse {
    fn decode(rlp: &Rlp) -> Result<GetBlockHeadersResponse, DecoderError> {
        Ok(GetBlockHeadersResponse {
            reqid: rlp.val_at(0)?,
            headers: rlp.list_at(1)?,
        })
    }
}
