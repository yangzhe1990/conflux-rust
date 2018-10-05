use rlp::{Decodable, Encodable};
use MsgId;

pub trait Message: Send + Encodable + 'static {
    fn msg_id(&self) -> MsgId;
}
