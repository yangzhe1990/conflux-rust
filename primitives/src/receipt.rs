use ethereum_types::U256;
use heapsize::HeapSizeOf;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};

pub const TRANSACTION_OUTCOME_SUCCESS: u8 = 0;
pub const TRANSACTION_OUTCOME_EXCEPTION: u8 = 1;

/// Information describing execution of a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// The total gas used in the block following execution of the transaction.
    pub gas_used: U256,
    /// Transaction outcome.
    pub outcome_status: u8,
}

impl Receipt {
    pub fn new(outcome: u8, gas_used: U256) -> Self {
        Self {
            gas_used,
            outcome_status: outcome,
        }
    }
}

impl Encodable for Receipt {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(2);
        s.append(&self.gas_used);
        s.append(&self.outcome_status);
    }
}

impl Decodable for Receipt {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if rlp.item_count()? != 2 {
            return Err(DecoderError::RlpIncorrectListLen);
        }

        Ok(Receipt {
            gas_used: rlp.val_at(0)?,
            outcome_status: rlp.val_at(1)?,
        })
    }
}

impl HeapSizeOf for Receipt {
    fn heap_size_of_children(&self) -> usize { 0 }
}
