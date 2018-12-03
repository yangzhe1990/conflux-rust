use ethereum_types::Address;
use rlp::DecoderError;
use storage::Error as StorageError;

error_chain! {
    links {
    }

    foreign_links {
        Storage(StorageError);
        Decoder(DecoderError);
    }

    errors {
        IncompleteDatabase(address: Address) {
            description("incomplete database")
            display("incomplete database: address={}", address)
        }
    }
}
