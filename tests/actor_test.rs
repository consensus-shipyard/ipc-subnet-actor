mod harness;
use std::str::FromStr;

use fil_actor_hierarchical_sca::{FundParams, Method, MIN_COLLATERAL_AMOUNT};
use fvm_ipld_encoding::RawBytes;
use fvm_shared::address::{Address, SubnetID};
use fvm_shared::econ::TokenAmount;
use fvm_shared::error::ExitCode;
use fvm_shared::METHOD_SEND;

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
    let value = TokenAmount::from(MIN_COLLATERAL_AMOUNT - 5_u64.pow(18));
    h.join(sender, value.clone());
    let st = h.get_state();
    assert_eq!(st.validator_set.len(), 1);
    assert_eq!(st.status, Status::Active);
    assert_eq!(st.total_stake, TokenAmount::from(MIN_COLLATERAL_AMOUNT));
    h.verify_stake(&st, sender, TokenAmount::from(MIN_COLLATERAL_AMOUNT));
    h.expect_send(
        &st,
        &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
        Method::Register as u64,
        RawBytes::default(),
        TokenAmount::from(MIN_COLLATERAL_AMOUNT),
    );

    // new miner joins
    let sender = h.senders.get_sender_by_index(1).unwrap();
    let value = TokenAmount::from(MIN_COLLATERAL_AMOUNT);
    h.join(sender, value.clone());
    let st = h.get_state();
    assert_eq!(st.validator_set.len(), 2);
    assert_eq!(st.status, Status::Active);
    assert_eq!(st.total_stake, TokenAmount::from(2 * MIN_COLLATERAL_AMOUNT));
    h.verify_stake(&st, sender, value.clone());
    h.expect_send(
        &st,
        &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
        Method::AddStake as u64,
        RawBytes::default(),
        TokenAmount::from(MIN_COLLATERAL_AMOUNT),
    );
}

#[test]
fn test_leave_and_kill() {
    let mut h = Harness::new();
    h.constructor(std_params());

    // first miner joins the subnet
    let sender = h.senders.get_sender_by_index(0).unwrap();
    let value = TokenAmount::from(10_u64.pow(18));
    let mut total_stake = value.clone();
    h.join(sender, value.clone());
    let st = h.get_state();
    assert_eq!(st.validator_set.len(), 1);
    assert_eq!(st.status, Status::Active);
    assert_eq!(st.total_stake, total_stake);
    h.verify_stake(&st, sender, value.clone());
    h.expect_send(
        &st,
        &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
        Method::Register as u64,
        RawBytes::default(),
        value.clone(),
    );

    // second miner joins the subnet
    let sender = h.senders.get_sender_by_index(1).unwrap();
    let value = TokenAmount::from(10_u64.pow(18));
    total_stake = total_stake + &value;
    h.join(sender, value.clone());
    let st = h.get_state();
    assert_eq!(st.validator_set.len(), 2);
    assert_eq!(st.status, Status::Active);
    assert_eq!(st.total_stake, total_stake);
    h.verify_stake(&st, sender, value.clone());
    h.expect_send(
        &st,
        &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
        Method::AddStake as u64,
        RawBytes::default(),
        value,
    );

    // non-miner joins
    let sender = h.senders.get_sender_by_index(2).unwrap();
    let value = TokenAmount::from(5u64.pow(18));
    total_stake = total_stake + &value;
    h.join(sender, value.clone());
    let st = h.get_state();
    assert_eq!(st.validator_set.len(), 2);
    assert_eq!(st.status, Status::Active);
    assert_eq!(st.total_stake, total_stake);
    h.verify_stake(&st, sender, value.clone());
    h.expect_send(
        &st,
        &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
        Method::AddStake as u64,
        RawBytes::default(),
        value,
    );

    // one miner leaves the subnet
    let sender = h.senders.get_sender_by_index(0).unwrap();
    let value = TokenAmount::from(MIN_COLLATERAL_AMOUNT);
    total_stake = total_stake - &value;
    h.leave(sender, value.clone());
    let st = h.get_state();
    assert_eq!(st.validator_set.len(), 1);
    assert_eq!(st.status, Status::Active);
    assert_eq!(st.total_stake, total_stake);
    h.verify_stake(&st, sender, 0.into());
    h.expect_send(
        &st,
        &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
        Method::ReleaseStake as u64,
        RawBytes::serialize(FundParams {
            value: value.clone(),
        })
        .unwrap(),
        0.into(),
    );
    h.expect_send(
        &st,
        &sender,
        METHOD_SEND,
        RawBytes::default(),
        value.clone(),
    );

    // subnet can't be killed if there are still miners
    h.kill(sender, ExitCode::USR_ILLEGAL_STATE);

    // next miner inactivates the subnet
    let sender = h.senders.get_sender_by_index(1).unwrap();
    let value = TokenAmount::from(MIN_COLLATERAL_AMOUNT);
    total_stake = total_stake - &value;
    h.leave(sender, value.clone());
    let st = h.get_state();
    assert_eq!(st.validator_set.len(), 0);
    assert_eq!(st.status, Status::Inactive);
    assert_eq!(st.total_stake, total_stake);
    h.verify_stake(&st, sender, 0.into());
    h.expect_send(
        &st,
        &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
        Method::ReleaseStake as u64,
        RawBytes::serialize(FundParams {
            value: value.clone(),
        })
        .unwrap(),
        0.into(),
    );
    h.expect_send(
        &st,
        &sender,
        METHOD_SEND,
        RawBytes::default(),
        value.clone(),
    );

    // last joiner gets the stake and kills the subnet
    let sender = h.senders.get_sender_by_index(2).unwrap();
    let value = TokenAmount::from(5u64.pow(18));
    total_stake = total_stake - &value;
    h.leave(sender, value.clone());
    let st = h.get_state();
    assert_eq!(st.validator_set.len(), 0);
    assert_eq!(st.status, Status::Inactive);
    assert_eq!(st.total_stake, total_stake);
    h.verify_stake(&st, sender, 0.into());
    h.expect_send(
        &st,
        &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
        Method::ReleaseStake as u64,
        RawBytes::serialize(FundParams {
            value: value.clone(),
        })
        .unwrap(),
        0.into(),
    );
    h.expect_send(
        &st,
        &sender,
        METHOD_SEND,
        RawBytes::default(),
        value.clone(),
    );
    h.kill(sender, ExitCode::OK);
    let st = h.get_state();
    assert_eq!(st.total_stake, 0.into());
    assert_eq!(st.status, Status::Killed);
    h.expect_send(
        &st,
        &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
        Method::Kill as u64,
        RawBytes::default(),
        0.into(),
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
