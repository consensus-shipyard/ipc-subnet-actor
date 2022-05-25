mod harness;
use std::str::FromStr;

use fvm_ipld_encoding::RawBytes;
use fvm_shared::address::{Address, SubnetID};
use fvm_shared::econ::TokenAmount;

use crate::harness::Harness;
use fil_hierarchical_subnet_actor::ext;
use fil_hierarchical_subnet_actor::types::{ConsensusType, ConstructParams, Status};

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
    let sender = h.senders.get_sender_by_index(0).unwrap();
    let value = TokenAmount::from(5_u64.pow(18));
    h.join(sender, value.clone());
    let st = h.get_state();
    assert_eq!(st.validator_set.len(), 0);
    assert_eq!(st.status, Status::Instantiated);
    assert_eq!(st.total_stake, value);
    h.verify_stake(&st, sender, value);

    // miner adds stake and activates it
    let sender = h.senders.get_sender_by_index(0).unwrap();
    let value = TokenAmount::from(ext::sca::MIN_STAKE - 5_u64.pow(18));
    h.join(sender, value.clone());
    let st = h.get_state();
    assert_eq!(st.validator_set.len(), 1);
    assert_eq!(st.status, Status::Active);
    assert_eq!(st.total_stake, TokenAmount::from(ext::sca::MIN_STAKE));
    h.verify_stake(&st, sender, TokenAmount::from(ext::sca::MIN_STAKE));
    h.expect_send(
        &st,
        &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
        ext::sca::Methods::Register as u64,
        RawBytes::default(),
        TokenAmount::from(ext::sca::MIN_STAKE),
    );

    // new miner joins
    let sender = h.senders.get_sender_by_index(1).unwrap();
    let value = TokenAmount::from(ext::sca::MIN_STAKE);
    h.join(sender, value.clone());
    let st = h.get_state();
    assert_eq!(st.validator_set.len(), 2);
    assert_eq!(st.status, Status::Active);
    assert_eq!(st.total_stake, TokenAmount::from(2 * ext::sca::MIN_STAKE));
    h.verify_stake(&st, sender, value.clone());
    h.expect_send(
        &st,
        &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
        ext::sca::Methods::AddStake as u64,
        RawBytes::default(),
        TokenAmount::from(ext::sca::MIN_STAKE),
    );
}

fn std_params() -> ConstructParams {
    ConstructParams {
        parent: SubnetID::from_str("/root").unwrap(),
        name: String::from("test"),
        consensus: ConsensusType::PoW,
        min_validator_stake: TokenAmount::from(10_u64.pow(18)),
        check_period: 10,
        genesis: Vec::new(),
    }
}
