pub mod sca {
    use fvm_ipld_encoding::tuple::{Deserialize_tuple, Serialize_tuple};
    use fvm_shared::bigint::bigint_ser;
    use fvm_shared::clock::ChainEpoch;
    use fvm_shared::econ::TokenAmount;

    pub const MIN_CHECK_PERIOD: ChainEpoch = 10;
    pub const MIN_STAKE: u64 = 10_u64.pow(18);
    pub const SCA_ACTOR_ADDR: u64 = 64;
    pub enum Methods {
        Register = 2,
        AddStake = 3,
        ReleaseStake = 4,
        Kill = 5,
        CommitChildCheckpoint = 6,
        Fund = 7,
        Release = 8,
        SendCross = 9,
        ApplyMessage = 10,
    }

    #[derive(Serialize_tuple, Deserialize_tuple, Clone)]
    pub struct FundParams {
        #[serde(with = "bigint_ser")]
        pub value: TokenAmount,
    }
}
