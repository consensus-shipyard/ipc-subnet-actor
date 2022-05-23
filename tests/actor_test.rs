mod harness;
use std::str::FromStr;

use fil_hierarchical_subnet_actor::types::{ConsensusType, ConstructParams, Status};
use fvm_shared::address::{Address, SubnetID};
use fvm_shared::econ::TokenAmount;

use crate::harness::Harness;

#[test]
fn test_constructor() {
    let mut h = Harness::new();
    h.constructor(std_params());
}

#[test]
fn test_join() {
    let mut h = Harness::new();
    h.constructor(std_params());

    // join without enough to be activated
    let sender = Address::new_id(h.senders[0].0);
    let value = TokenAmount::from(5_u64.pow(18));
    h.join(sender, value.clone());
    let st = h.get_state();
    assert_eq!(st.validator_set.len(), 0);
    assert_eq!(st.status, Status::Instantiated);
    assert_eq!(st.total_stake, value);
    h.verify_stake(&st, sender, value);

    // new miner joins and activates it
    let sender = Address::new_id(h.senders[1].0);
    let value = TokenAmount::from(10_u64.pow(18));
    h.join(sender, value.clone());
    let st = h.get_state();
    assert_eq!(st.validator_set.len(), 1);
    assert_eq!(st.status, Status::Active);
    assert_eq!(st.total_stake, 10_u64.pow(18)+&value);
    h.verify_stake(&st, sender, value);

    // TODO: Expect send!
}

fn std_params() -> ConstructParams {
    ConstructParams {
        parent: SubnetID::from_str("/root").unwrap(),
        name: String::from("test"),
        consensus: ConsensusType::Delegated,
        min_validator_stake: TokenAmount::from(10_u64.pow(18)),
        check_period: 10,
        genesis: Vec::new(),
    }
}
