use std::{io, num};

error_chain! {
    links {
    }

    foreign_links {
        Io(io::Error);
        ParseIntError(num::ParseIntError);
        RlpDecodeError(::rlp::DecoderError);
    }

    errors {
        OutOfCapacity {
            description("Out of capacity"),
            display("Out of capacity"),
        }

        OutOfMem {
            description("Out of memory."),
            display("Out of memory."),
        }

        SlabKeyError {
            description("Slab: invalid position accessed"),
            display("Slab: invalid position accessed")
        }

        // TODO(yz): encode key into error message.
        MPTKeyNotFound {
            description("Key not found."),
            display("Key not found.")
        }

        // TODO(yz): encode value into error message.
        MPTInvalidKey {
            description("Invalid key."),
            display("Invalid key.")
        }

        // TODO(yz): encode value into error message.
        MPTInvalidValue {
            description("Invalid value."),
            display("Invalid value.")
        }

        MPTTooManyNodes {
            description("Too many nodes."),
            display("Too many nodes.")
        }
    }
}
