use io::IoError;
use std::{fmt, io, net};

error_chain! {
    foreign_links {
        SocketIo(IoError);
    }

    errors {
        UnknownProtocol {
            description("Unknown protocol"),
            display("Unknown protocl"),
        }

        Io(err: io::Error) {
            description("IO Error"),
            display("Unexpected IO error: {}", err),
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self { Error::from_kind(ErrorKind::Io(err)) }
}
