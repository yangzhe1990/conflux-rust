use network;
use rlp::DecoderError;

error_chain! {
    links {
        Network(network::Error, network::ErrorKind);
    }

    foreign_links {
        Decoder(DecoderError);
    }

    errors {
        Invalid {
            description("Invalid block"),
            display("Invalid block"),
        }

        Useless {
            description("Useless block"),
            display("Useless block"),
        }

        Unknown {
            description("Unknown error"),
            display("Unknown error"),
        }
    }
}
