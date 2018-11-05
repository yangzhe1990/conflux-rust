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

        UnknownPeer {
            description("Unknown peer"),
            display("Unknown peer"),
        }

        UnexpectedResponse {
            description("Unexpected response"),
            display("Unexpected response"),
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
