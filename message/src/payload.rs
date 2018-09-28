use rlp::{Encodable, Decodable};

pub trait Payload: Send + Encodable + Decodable + 'static {
    fn command() -> u8;
}
