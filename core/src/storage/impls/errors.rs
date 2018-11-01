error_chain! {
    links {
    }

    foreign_links {
    }

    errors {
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
        MPTInvalidValue {
            description("Invalid value."),
            display("Invalid value.")
        }
    }
}
