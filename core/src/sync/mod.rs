mod error;
mod synchronization_graph;
mod synchronization_protocol_handler;
mod synchronization_service;
mod synchronization_state;

pub use self::{
    error::{Error, ErrorKind},
    synchronization_graph::{
        BestInformation, CacheId, SharedSynchronizationGraph,
        SynchronizationGraph, SynchronizationGraphInner,
        SynchronizationGraphNode,
    },
    synchronization_protocol_handler::{
        ProtocolConfiguration, SynchronizationProtocolHandler,
        SYNCHRONIZATION_PROTOCOL_VERSION,
    },
    synchronization_service::{
        SharedSynchronizationService, SynchronizationConfiguration,
        SynchronizationService,
    },
    synchronization_state::{
        SynchronizationPeerState, SynchronizationState,
        MAX_INFLIGHT_REQUEST_COUNT,
    },
};

pub mod random {
    use rand;
    pub fn new() -> rand::ThreadRng {
        rand::thread_rng()
    }
}
