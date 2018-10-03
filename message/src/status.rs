use ethereum_types::{H256, U256};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Payload;

#[derive(Debug, PartialEq)]
pub struct Status {
    pub protocol_version: u8,
    pub network_id: u8,
    pub total_difficulty: U256,
    pub best_hash: H256,
    pub genesis_hash: H256,
}

impl Payload for Status {
    fn command() -> u8 { 0x00 }
}

impl Encodable for Status {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream
            .begin_list(5)
            .append(&self.protocol_version)
            .append(&self.network_id)
            .append(&self.total_difficulty)
            .append(&self.best_hash)
            .append(&self.genesis_hash);
    }
}

impl Decodable for Status {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(Status {
            protocol_version: rlp.val_at::<u8>(0)?,
            network_id: rlp.val_at::<u8>(1)?,
            total_difficulty: rlp.val_at::<U256>(2)?,
            best_hash: rlp.val_at::<H256>(3)?,
            genesis_hash: rlp.val_at::<H256>(4)?,
        })
    }
}
