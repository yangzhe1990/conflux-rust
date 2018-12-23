use crate::evm::{Factory as EvmFactory, VMType};
use crate::vm::{ActionParams, Exec, Spec};

/// Virtual machine factory
#[derive(Default, Clone)]
pub struct VmFactory {
    evm: EvmFactory,
}

impl VmFactory {
    pub fn create(
        &self, params: ActionParams, spec: &Spec, depth: usize,
    ) -> Box<dyn Exec> {
        self.evm.create(params, spec, depth)
    }

    pub fn new(cache_size: usize) -> Self {
        VmFactory {
            evm: EvmFactory::new(VMType::Interpreter, cache_size),
        }
    }
}

impl From<EvmFactory> for VmFactory {
    fn from(evm: EvmFactory) -> Self { VmFactory { evm } }
}
