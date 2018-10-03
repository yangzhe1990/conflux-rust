use rlp::{Decodable, Encodable};

pub trait Payload: Send + Encodable + Decodable + 'static {
    fn command() -> u8;
}
