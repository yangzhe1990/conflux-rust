use io::*;
use network::host::Host;
use network::{Error, ErrorKind, NetworkConfiguration, NetworkIoMessage};
use parking_lot::RwLock;
use std::net::SocketAddr;
use std::sync::Arc;

pub struct NetworkService {
    io_service: IoService<NetworkIoMessage>,
    host: RwLock<Option<Arc<Host>>>,
    config: NetworkConfiguration,
}

impl NetworkService {
    pub fn new(config: NetworkConfiguration) -> Result<NetworkService, Error> {
        let io_service = IoService::<NetworkIoMessage>::start()?;

        Ok(NetworkService {
            io_service: io_service,
            host: RwLock::new(None),
            config: config,
        })
    }

    pub fn io(&self) -> &IoService<NetworkIoMessage> {
        &self.io_service
    }

    pub fn start(&self) -> Result<(), Error> {
        let mut host = self.host.write();
        if host.is_none() {
            let h = Arc::new(Host::new(&self.config)?);
            self.io_service.register_handler(h.clone())?;
            *host = Some(h);
        }

        Ok(())
    }

    pub fn stop(&self) {
        let mut host = self.host.write();
        // if let Some(ref host) = *host {
        //     let io = IoContext::new(self.io_service.channel(), 0);
        //     host.stop(&io);
        // }
        *host = None;
    }
}
