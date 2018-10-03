use ethereum_types::{H256, U256};
use primitives::TransactionWithSignature;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use Payload;

#[derive(Debug, PartialEq)]
pub struct BlockBody {
    pub transactions: Vec<TransactionWithSignature>,
}

impl Encodable for BlockBody {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.transactions);
    }
}

impl Decodable for BlockBody {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(BlockBody {
            transactions: rlp.as_list()?,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct BlockBodies {
    bodies: Vec<BlockBody>,
}

impl Payload for BlockBodies {
    fn command() -> u8 { 0x07 }
}

impl Encodable for BlockBodies {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_list(&self.bodies);
    }
}

impl Decodable for BlockBodies {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(BlockBodies {
            bodies: rlp.as_list()?,
        })
    }
}
