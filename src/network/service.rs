use network::{Error, ErrorKind, NetworkConfiguration};

pub struct NetworkService;

impl NetworkService {
    pub fn new(config: NetworkConfiguration) -> Result<NetworkService, Error> {
        Err(ErrorKind::ConfluxError.into())
    }
}
