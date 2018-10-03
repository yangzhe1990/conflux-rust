use rlp::RlpStream;

use bytes::{Bytes, TaggedBytes};
use Payload;

pub struct Message<T> {
    bytes: TaggedBytes<T>,
}

impl<T> Message<T>
where T: Payload
{
    pub fn new(payload: &T) -> Self {
        let mut rlp = RlpStream::new_list(2);
        rlp.append(&(T::command()));
        payload.rlp_append(&mut rlp);

        Message {
            bytes: TaggedBytes::new(rlp.out()),
        }
    }

    pub fn len(&self) -> usize { self.bytes.len() }
}
