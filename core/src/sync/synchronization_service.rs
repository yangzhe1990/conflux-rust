use super::{
    Error, SharedSynchronizationGraph, SynchronizationProtocolHandler,
    SYNCHRONIZATION_PROTOCOL_VERSION,
};
use consensus::SharedConsensusGraph;
use ethereum_types::H256;
use network::{
    node_table::{NodeEntry, NodeId},
    Error as NetworkError, NetworkConfiguration, NetworkService, PeerInfo,
    ProtocolId,
};
use pow::ProofOfWorkConfig;
use primitives::Block;
use std::sync::Arc;
use verification::verification::VerificationConfig;

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
    pub fn new(
        config: SynchronizationConfiguration, pow_config: ProofOfWorkConfig,
        verification_config: VerificationConfig,
    ) -> Self
    {
        SynchronizationService {
            network: NetworkService::new(config.network),
            protocol_handler: Arc::new(SynchronizationProtocolHandler::new(
                config.consensus,
                pow_config,
                verification_config,
            )),
            protocol: *b"cfx",
        }
    }

    pub fn get_synchronization_graph(&self) -> SharedSynchronizationGraph {
        self.protocol_handler.get_synchronization_graph()
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

    pub fn relay_blocks(&self, need_to_relay: Vec<H256>) {
        self.network.with_context(self.protocol, |io| {
            self.protocol_handler.relay_blocks(io, need_to_relay);
        });
    }

    pub fn on_mined_block(&self, block: Block) {
        let hash = block.hash();
        let need_to_relay = self.protocol_handler.on_mined_block(block);
        self.relay_blocks(need_to_relay);
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
