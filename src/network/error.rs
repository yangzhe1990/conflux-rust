use io::IoError;
use std::io;

error_chain! {
    foreign_links {
        SocketIo(IoError);
    }

    errors {
        ConfluxError

        Io(err: io::Error) {
            description("IO Error"),
            display("Unexpected IO error: {}", err),
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::from_kind(ErrorKind::Io(err))
    }
}
