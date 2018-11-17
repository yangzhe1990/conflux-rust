use super::{
    Error, SynchronizationProtocolHandler, SYNCHRONIZATION_PROTOCOL_VERSION,
};
use consensus::SharedConsensusGraph;
use ethereum_types::H256;
use network::{
    node_table::{NodeEntry, NodeId},
    Error as NetworkError, NetworkConfiguration, NetworkService, PeerInfo,
    ProtocolId,
};
use primitives::Block;
use std::sync::Arc;

pub struct SynchronizationConfiguration {
    pub network: NetworkConfiguration,
    pub consensus: SharedConsensusGraph,
}

pub struct SynchronizationService {
    network: NetworkService,
    protocol_handler: Arc<SynchronizationProtocolHandler>,
    protocol: ProtocolId,
}

impl SynchronizationService {
    pub fn new(config: SynchronizationConfiguration) -> Self {
        SynchronizationService {
            network: NetworkService::new(config.network),
            protocol_handler: Arc::new(SynchronizationProtocolHandler::new(
                config.consensus,
            )),
            protocol: *b"cfx",
        }
    }

    pub fn start(&mut self) -> Result<(), Error> {
        self.network.start()?;
        self.network.register_protocol(
            self.protocol_handler.clone(),
            self.protocol,
            &[SYNCHRONIZATION_PROTOCOL_VERSION],
        )?;
        Ok(())
    }

    pub fn announce_new_blocks(&self, hashes: &[H256]) {
        self.network.with_context(self.protocol, |io| {
            self.protocol_handler.announce_new_blocks(io, hashes);
        });
    }

    pub fn on_mined_block(&self, block: Block) {
        let hash = block.hash();
        self.protocol_handler.on_mined_block(block);
        self.announce_new_blocks(&[hash]);
    }

    pub fn add_peer(&self, node: NodeEntry) -> Result<(), NetworkError> {
        self.network.add_peer(node)
    }

    pub fn drop_peer(&self, node: NodeEntry) -> Result<(), NetworkError> {
        self.network.drop_peer(node)
    }

    pub fn get_peer_info(&self) -> Vec<PeerInfo> {
        self.network.get_peer_info().unwrap()
    }

    pub fn sign_challenge(
        &self, challenge: Vec<u8>,
    ) -> Result<Vec<u8>, NetworkError> {
        self.network.sign_challenge(challenge)
    }

    pub fn add_latency(
        &self, id: NodeId, latency_ms: f64,
    ) -> Result<(), NetworkError> {
        self.network.add_latency(id, latency_ms)
    }

    pub fn block_by_hash(&self, hash: &H256) -> Option<Block> {
        self.protocol_handler.block_by_hash(hash)
    }
}

pub type SharedSynchronizationService = Arc<SynchronizationService>;
