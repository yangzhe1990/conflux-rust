mod evm;
#[macro_use]
pub mod factory;
mod instructions;
mod interpreter;
mod vmtype;

#[cfg(test)]
mod tests;

pub use self::{
    evm::{CostType, FinalizationResult, Finalize},
    factory::Factory,
    vmtype::VMType,
};
pub use vm::{
    ActionParams, CallType, CleanDustMode, Context, ContractCreateResult,
    CreateContractAddress, EnvInfo, GasLeft, MessageCallResult, ReturnData,
    Spec,
};
