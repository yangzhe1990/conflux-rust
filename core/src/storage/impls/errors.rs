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
    }
}
