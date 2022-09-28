use fvm_ipld_encoding::repr::*;
use fvm_ipld_encoding::tuple::{Deserialize_tuple, Serialize_tuple};
use fvm_ipld_encoding::{serde_bytes, Cbor};
use fvm_shared::address::{Address, SubnetID};
use fvm_shared::bigint::bigint_ser;
use fvm_shared::clock::ChainEpoch;
use fvm_shared::econ::TokenAmount;

/// Optional leaving coefficient to penalize
/// validators leaving the subnet.
// It should be a float between 0-1 but
// setting it to 1_u64 for now for convenience.
// This will change once we figure out the econ model.
pub const LEAVING_COEFF: u64 = 1;
pub const TESTING_ID: u64 = 339;

#[derive(Clone, Debug, Serialize_tuple, Deserialize_tuple, PartialEq)]
pub struct Validator {
    pub subnet: SubnetID,
    pub addr: Address,
    pub net_addr: String,
}

#[derive(Clone, Debug, Serialize_tuple, Deserialize_tuple, PartialEq)]
pub struct Votes {
    pub validators: Vec<Address>,
}
impl Cbor for Votes {}

/// Consensus types supported by hierarchical consensus
#[derive(PartialEq, Eq, Clone, Copy, Debug, Deserialize_repr, Serialize_repr)]
#[repr(u64)]
pub enum ConsensusType {
    Delegated,
    PoW,
    Tendermint,
    Mir,
    FilecoinEC,
    Dummy,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, Deserialize_repr, Serialize_repr)]
#[repr(i32)]
pub enum Status {
    Instantiated,
    Active,
    Inactive,
    Terminating,
    Killed,
}

#[derive(Clone, Debug, Serialize_tuple, Deserialize_tuple, PartialEq)]
pub struct ConstructParams {
    pub parent: SubnetID,
    pub name: String,
    pub consensus: ConsensusType,
    #[serde(with = "bigint_ser")]
    pub min_validator_stake: TokenAmount,
    pub min_validators: u64,
    pub finality_threshold: ChainEpoch,
    pub check_period: ChainEpoch,
    // genesis is no longer generated by the actor
    // on-the-fly, but it is accepted as a construct
    // param
    #[serde(with = "serde_bytes")]
    pub genesis: Vec<u8>,
}
impl Cbor for ConstructParams {}
