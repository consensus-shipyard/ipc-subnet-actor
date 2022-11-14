#[cfg(test)]
mod test {
    use cid::Cid;
    use fil_actors_runtime::runtime::Runtime;
    use fil_actors_runtime::test_utils::{expect_abort, MockRuntime};
    use fil_actors_runtime::{cbor, INIT_ACTOR_ADDR};
    use fvm_ipld_encoding::RawBytes;
    use fvm_shared::address::Address;
    use fvm_shared::econ::TokenAmount;
    use fvm_shared::error::ExitCode;
    use ipc_gateway::{FundParams, MIN_COLLATERAL_AMOUNT};
    use ipc_subnet_actor::{
        Actor, ConsensusType, ConstructParams, JoinParams, Method, State, Status,
    };
    use num_traits::Zero;

    // just a test address
    const IPC_GATEWAY_ADDR: u64 = 1024;
    const NETWORK_NAME: &'static str = "test";

    fn std_construct_param() -> ConstructParams {
        ConstructParams {
            parent: Default::default(),
            name: NETWORK_NAME.to_string(),
            ipc_gateway_addr: IPC_GATEWAY_ADDR,
            consensus: ConsensusType::Dummy,
            min_validator_stake: Default::default(),
            min_validators: 0,
            finality_threshold: 0,
            check_period: 0,
            genesis: vec![],
        }
    }

    fn construct_runtime() -> MockRuntime {
        let caller = *INIT_ACTOR_ADDR;
        let receiver = Address::new_id(1);
        let mut runtime = MockRuntime::new(caller, receiver);

        let params = std_construct_param();

        runtime.expect_validate_caller_addr(vec![caller]);

        runtime
            .call::<Actor>(
                Method::Constructor as u64,
                &cbor::serialize(&params, "test").unwrap(),
            )
            .unwrap();

        runtime
    }

    #[test]
    fn test_constructor() {
        let runtime = construct_runtime();
        assert_eq!(runtime.state.is_some(), true);

        let state: State = runtime.get_state();
        assert_eq!(state.name, NETWORK_NAME);
        assert_eq!(state.ipc_gateway_addr, Address::new_id(IPC_GATEWAY_ADDR));
        assert_eq!(state.total_stake, TokenAmount::zero());
        assert_eq!(state.validator_set.is_empty(), true);
    }

    #[test]
    fn test_join_fail_no_min_collateral() {
        let mut runtime = construct_runtime();

        let validator = Address::new_id(100);
        let params = JoinParams {
            validator_net_addr: validator.to_string(),
        };

        expect_abort(
            ExitCode::USR_ILLEGAL_ARGUMENT,
            runtime.call::<Actor>(
                Method::Join as u64,
                &cbor::serialize(&params, "test").unwrap(),
            ),
        );
    }

    #[test]
    fn test_join_works() {
        let mut runtime = construct_runtime();

        let caller = Address::new_id(10);
        let validator = Address::new_id(100);
        let start_token_value = 5_u64.pow(18);
        let params = JoinParams {
            validator_net_addr: validator.to_string(),
        };

        // Part 1. join without enough to be activated

        // execution
        let value = TokenAmount::from_atto(start_token_value);
        runtime.set_value(value.clone());
        runtime.set_caller(Cid::default(), caller.clone());
        runtime
            .call::<Actor>(
                Method::Join as u64,
                &cbor::serialize(&params, "test").unwrap(),
            )
            .unwrap();

        // verify state.
        // as the value is less than min collateral, state is initiated
        let st: State = runtime.get_state();
        assert_eq!(st.validator_set.len(), 0);
        assert_eq!(st.status, Status::Instantiated);
        assert_eq!(st.total_stake, value);
        let stake = st.get_stake(runtime.store(), &caller).unwrap();
        assert_eq!(stake.unwrap(), value);

        // Part 2. miner adds stake and activates it
        let value = TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT - start_token_value);
        runtime.set_value(value.clone());
        runtime.set_caller(Cid::default(), caller.clone());
        runtime.set_balance(TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT));
        runtime.expect_send(
            Address::new_id(IPC_GATEWAY_ADDR),
            ipc_gateway::Method::Register as u64,
            RawBytes::default(),
            TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT),
            RawBytes::default(),
            ExitCode::new(0),
        );
        runtime
            .call::<Actor>(
                Method::Join as u64,
                &cbor::serialize(&params, "test").unwrap(),
            )
            .unwrap();

        // verify state.
        // as the value is less than min collateral, state is active
        let st: State = runtime.get_state();
        assert_eq!(st.validator_set.len(), 1);
        assert_eq!(st.status, Status::Active);
        assert_eq!(
            st.total_stake,
            TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT)
        );
        let stake = st.get_stake(runtime.store(), &caller).unwrap();
        assert_eq!(
            stake.unwrap(),
            TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT)
        );
        runtime.verify();

        // Part 3. miner joins already active subnet
        let caller = Address::new_id(11);
        let value = TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT);
        runtime.set_value(value.clone());
        runtime.set_caller(Cid::default(), caller.clone());
        runtime.set_balance(TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT));
        runtime.expect_send(
            Address::new_id(IPC_GATEWAY_ADDR),
            ipc_gateway::Method::AddStake as u64,
            RawBytes::default(),
            TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT),
            RawBytes::default(),
            ExitCode::new(0),
        );
        runtime
            .call::<Actor>(
                Method::Join as u64,
                &cbor::serialize(&params, "test").unwrap(),
            )
            .unwrap();

        // verify state.
        // as the value is less than min collateral, state is active
        let st: State = runtime.get_state();
        assert_eq!(st.validator_set.len(), 2);
        assert_eq!(st.status, Status::Active);
        assert_eq!(
            st.total_stake,
            TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT * 2)
        );
        let stake = st.get_stake(runtime.store(), &caller).unwrap();
        assert_eq!(
            stake.unwrap(),
            TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT)
        );
        runtime.verify();
    }

    #[test]
    fn test_leave_and_kill() {
        let mut runtime = construct_runtime();

        let caller = Address::new_id(10);
        let validator = Address::new_id(100);
        let params = JoinParams {
            validator_net_addr: validator.to_string(),
        };

        // first miner joins the subnet
        let value = TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT);
        let mut total_stake = value.clone();

        runtime.set_value(value.clone());
        runtime.set_balance(TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT));
        runtime.set_caller(Cid::default(), caller.clone());
        runtime.expect_send(
            Address::new_id(IPC_GATEWAY_ADDR),
            ipc_gateway::Method::Register as u64,
            RawBytes::default(),
            TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT),
            RawBytes::default(),
            ExitCode::new(0),
        );
        runtime
            .call::<Actor>(
                Method::Join as u64,
                &cbor::serialize(&params, "test").unwrap(),
            )
            .unwrap();

        // Just some santity check here as it should have been tested by previous methods
        let st: State = runtime.get_state();
        assert_eq!(st.status, Status::Active);
        assert_eq!(
            st.get_stake(runtime.store(), &caller).unwrap().unwrap(),
            TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT)
        );

        // second miner joins the subnet
        let caller = Address::new_id(20);
        let value = TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT);
        let params = JoinParams {
            validator_net_addr: caller.clone().to_string(),
        };
        total_stake = total_stake + &value;
        runtime.set_value(value.clone());
        runtime.set_balance(TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT));
        runtime.set_caller(Cid::default(), caller.clone());
        runtime.expect_send(
            Address::new_id(IPC_GATEWAY_ADDR),
            ipc_gateway::Method::AddStake as u64,
            RawBytes::default(),
            TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT),
            RawBytes::default(),
            ExitCode::new(0),
        );
        runtime
            .call::<Actor>(
                Method::Join as u64,
                &cbor::serialize(&params, "test").unwrap(),
            )
            .unwrap();

        let st: State = runtime.get_state();
        assert_eq!(st.total_stake, total_stake);
        assert_eq!(st.validator_set.len(), 2);
        assert_eq!(
            st.get_stake(runtime.store(), &caller).unwrap().unwrap(),
            TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT)
        );

        // non-miner joins
        let caller = Address::new_id(30);
        let params = JoinParams {
            validator_net_addr: caller.clone().to_string(),
        };
        let value = TokenAmount::from_atto(5u64.pow(18));
        total_stake = total_stake + &value;

        runtime.set_value(value.clone());
        runtime.set_balance(TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT));
        runtime.set_caller(Cid::default(), caller.clone());
        runtime.expect_send(
            Address::new_id(IPC_GATEWAY_ADDR),
            ipc_gateway::Method::AddStake as u64,
            RawBytes::default(),
            value.clone(),
            RawBytes::default(),
            ExitCode::new(0),
        );
        runtime
            .call::<Actor>(
                Method::Join as u64,
                &cbor::serialize(&params, "test").unwrap(),
            )
            .unwrap();
        let st: State = runtime.get_state();
        assert_eq!(st.total_stake, total_stake);
        assert_eq!(st.validator_set.len(), 2);
        assert_eq!(
            st.get_stake(runtime.store(), &caller).unwrap().unwrap(),
            value
        );

        // one miner leaves the subnet
        let caller = Address::new_id(10);
        let value = TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT);
        total_stake = total_stake - &value;
        runtime.set_value(value.clone());
        runtime.set_balance(TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT));
        runtime.set_caller(Cid::default(), caller.clone());
        runtime.expect_send(
            Address::new_id(IPC_GATEWAY_ADDR),
            ipc_gateway::Method::ReleaseStake as u64,
            RawBytes::serialize(FundParams {
                value: value.clone(),
            })
            .unwrap(),
            value.clone(),
            RawBytes::default(),
            ExitCode::new(0),
        );
        runtime
            .call::<Actor>(Method::Leave as u64, &RawBytes::default())
            .unwrap();

        let st: State = runtime.get_state();
        assert_eq!(st.validator_set.len(), 1);
        assert_eq!(st.status, Status::Active);
        assert_eq!(st.total_stake, total_stake);
        assert_eq!(
            st.get_stake(runtime.store(), &caller).unwrap().unwrap(),
            TokenAmount::zero()
        );

        // subnet can't be killed if there are still miners

        expect_abort(
            ExitCode::USR_ILLEGAL_STATE,
            runtime.call::<Actor>(Method::Kill as u64, &RawBytes::default()),
        );

        // // next miner inactivates the subnet
        let caller = Address::new_id(20);
        let value = TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT);
        total_stake = total_stake - &value;
        runtime.set_value(value.clone());
        runtime.set_balance(TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT));
        runtime.set_caller(Cid::default(), caller.clone());
        runtime.expect_send(
            Address::new_id(IPC_GATEWAY_ADDR),
            ipc_gateway::Method::ReleaseStake as u64,
            RawBytes::serialize(FundParams {
                value: value.clone(),
            })
            .unwrap(),
            value.clone(),
            RawBytes::default(),
            ExitCode::new(0),
        );
        runtime
            .call::<Actor>(Method::Leave as u64, &RawBytes::default())
            .unwrap();

        let st: State = runtime.get_state();
        assert_eq!(st.validator_set.len(), 0);
        assert_eq!(st.status, Status::Inactive);
        assert_eq!(st.total_stake, total_stake);
        assert_eq!(
            st.get_stake(runtime.store(), &caller).unwrap().unwrap(),
            TokenAmount::zero()
        );

        // last joiner gets the stake and kills the subnet
        let caller = Address::new_id(30);
        let value = TokenAmount::from_atto(5u64.pow(18));
        total_stake = total_stake - &value;
        runtime.set_value(value.clone());
        runtime.set_balance(TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT));
        runtime.set_caller(Cid::default(), caller.clone());
        runtime.expect_send(
            Address::new_id(IPC_GATEWAY_ADDR),
            ipc_gateway::Method::ReleaseStake as u64,
            RawBytes::serialize(FundParams {
                value: value.clone(),
            })
            .unwrap(),
            value.clone(),
            RawBytes::default(),
            ExitCode::new(0),
        );
        runtime
            .call::<Actor>(Method::Leave as u64, &RawBytes::default())
            .unwrap();
        let st: State = runtime.get_state();
        assert_eq!(st.validator_set.len(), 0);
        assert_eq!(st.status, Status::Inactive);
        assert_eq!(st.total_stake, total_stake);
        assert_eq!(
            st.get_stake(runtime.store(), &caller).unwrap().unwrap(),
            TokenAmount::zero()
        );

        // to kill the subnet
        runtime.set_value(value.clone());
        runtime.set_balance(TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT));
        runtime.set_caller(Cid::default(), caller.clone());
        runtime.expect_send(
            Address::new_id(IPC_GATEWAY_ADDR),
            ipc_gateway::Method::Kill as u64,
            RawBytes::default(),
            TokenAmount::zero(),
            RawBytes::default(),
            ExitCode::new(0),
        );
        runtime
            .call::<Actor>(Method::Kill as u64, &RawBytes::default())
            .unwrap();
        let st: State = runtime.get_state();
        assert_eq!(st.total_stake, TokenAmount::zero());
        assert_eq!(st.status, Status::Killed);
    }
}

// use fil_actors_runtime::test_utils::MockRuntime;
// use fil_actors_runtime::{actor_error, cbor, INIT_ACTOR_ADDR};
// use fvm_shared::address::Address;
// use fvm_shared::econ::TokenAmount;
// use ipc_gateway::MIN_COLLATERAL_AMOUNT;
// use ipc_subnet_actor::state::State;
// use ipc_subnet_actor::types::{ConsensusType, ConstructParams, JoinParams, Status};
// use ipc_subnet_actor::{Actor, Method};
//
// // use std::str::FromStr;
// //
// // use cid::Cid;
// // use fvm_ipld_encoding::RawBytes;
// // use fvm_shared::address::{Address, SubnetID};
// // use fvm_shared::econ::TokenAmount;
// // use fvm_shared::error::ExitCode;
// // use fvm_shared::METHOD_SEND;
// //
// // use crate::harness::Harness;
// // use fil_actor_hierarchical_sca::{FundParams, Method, MIN_COLLATERAL_AMOUNT};
// // use fil_hierarchical_subnet_actor::ext;
// // use fil_hierarchical_subnet_actor::types::{ConsensusType, ConstructParams, JoinParams, Status};
// //
// // mod harness;
// //
//
// //
// // #[test]
// // fn test_leave_and_kill() {
// //     let mut h = Harness::new();
// //     h.constructor(std_params());
// //
// //     // first miner joins the subnet
// //     let sender = h.senders.get_sender_by_index(0).unwrap();
// //     let value = TokenAmount::from(10_u64.pow(18));
// //     let params = std_join_params();
// //     let mut total_stake = value.clone();
// //     h.join(sender, value.clone(), params.clone());
// //     let st = h.get_state();
// //     assert_eq!(st.validator_set.len(), 1);
// //     assert_eq!(st.status, Status::Active);
// //     assert_eq!(st.total_stake, total_stake);
// //     h.verify_stake(&st, sender, value.clone());
// //     h.expect_send(
// //         &st,
// //         &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
// //         Method::Register as u64,
// //         RawBytes::default(),
// //         value.clone(),
// //     );
// //
// //     // second miner joins the subnet
// //     let sender = h.senders.get_sender_by_index(1).unwrap();
// //     let value = TokenAmount::from(10_u64.pow(18));
// //     total_stake = total_stake + &value;
// //     h.join(sender, value.clone(), params.clone());
// //     let st = h.get_state();
// //     assert_eq!(st.validator_set.len(), 2);
// //     assert_eq!(st.status, Status::Active);
// //     assert_eq!(st.total_stake, total_stake);
// //     h.verify_stake(&st, sender, value.clone());
// //     h.expect_send(
// //         &st,
// //         &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
// //         Method::AddStake as u64,
// //         RawBytes::default(),
// //         value,
// //     );
// //
// //     // non-miner joins
// //     let sender = h.senders.get_sender_by_index(2).unwrap();
// //     let value = TokenAmount::from(5u64.pow(18));
// //     total_stake = total_stake + &value;
// //     h.join(sender, value.clone(), params.clone());
// //     let st = h.get_state();
// //     assert_eq!(st.validator_set.len(), 2);
// //     assert_eq!(st.status, Status::Active);
// //     assert_eq!(st.total_stake, total_stake);
// //     h.verify_stake(&st, sender, value.clone());
// //     h.expect_send(
// //         &st,
// //         &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
// //         Method::AddStake as u64,
// //         RawBytes::default(),
// //         value,
// //     );
// //
// //     // one miner leaves the subnet
// //     let sender = h.senders.get_sender_by_index(0).unwrap();
// //     let value = TokenAmount::from(MIN_COLLATERAL_AMOUNT);
// //     total_stake = total_stake - &value;
// //     h.leave(sender, value.clone());
// //     let st = h.get_state();
// //     assert_eq!(st.validator_set.len(), 1);
// //     assert_eq!(st.status, Status::Active);
// //     assert_eq!(st.total_stake, total_stake);
// //     h.verify_stake(&st, sender, 0.into());
// //     h.expect_send(
// //         &st,
// //         &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
// //         Method::ReleaseStake as u64,
// //         RawBytes::serialize(FundParams {
// //             value: value.clone(),
// //         })
// //         .unwrap(),
// //         0.into(),
// //     );
// //     h.expect_send(
// //         &st,
// //         &sender,
// //         METHOD_SEND,
// //         RawBytes::default(),
// //         value.clone(),
// //     );
// //
// //     // subnet can't be killed if there are still miners
// //     h.kill(sender, ExitCode::USR_ILLEGAL_STATE);
// //
// //     // next miner inactivates the subnet
// //     let sender = h.senders.get_sender_by_index(1).unwrap();
// //     let value = TokenAmount::from(MIN_COLLATERAL_AMOUNT);
// //     total_stake = total_stake - &value;
// //     h.leave(sender, value.clone());
// //     let st = h.get_state();
// //     assert_eq!(st.validator_set.len(), 0);
// //     assert_eq!(st.status, Status::Inactive);
// //     assert_eq!(st.total_stake, total_stake);
// //     h.verify_stake(&st, sender, 0.into());
// //     h.expect_send(
// //         &st,
// //         &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
// //         Method::ReleaseStake as u64,
// //         RawBytes::serialize(FundParams {
// //             value: value.clone(),
// //         })
// //         .unwrap(),
// //         0.into(),
// //     );
// //     h.expect_send(
// //         &st,
// //         &sender,
// //         METHOD_SEND,
// //         RawBytes::default(),
// //         value.clone(),
// //     );
// //
// //     // last joiner gets the stake and kills the subnet
// //     let sender = h.senders.get_sender_by_index(2).unwrap();
// //     let value = TokenAmount::from(5u64.pow(18));
// //     total_stake = total_stake - &value;
// //     h.leave(sender, value.clone());
// //     let st = h.get_state();
// //     assert_eq!(st.validator_set.len(), 0);
// //     assert_eq!(st.status, Status::Inactive);
// //     assert_eq!(st.total_stake, total_stake);
// //     h.verify_stake(&st, sender, 0.into());
// //     h.expect_send(
// //         &st,
// //         &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
// //         Method::ReleaseStake as u64,
// //         RawBytes::serialize(FundParams {
// //             value: value.clone(),
// //         })
// //         .unwrap(),
// //         0.into(),
// //     );
// //     h.expect_send(
// //         &st,
// //         &sender,
// //         METHOD_SEND,
// //         RawBytes::default(),
// //         value.clone(),
// //     );
// //     h.kill(sender, ExitCode::OK);
// //     let st = h.get_state();
// //     assert_eq!(st.total_stake, 0.into());
// //     assert_eq!(st.status, Status::Killed);
// //     h.expect_send(
// //         &st,
// //         &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
// //         Method::Kill as u64,
// //         RawBytes::default(),
// //         0.into(),
// //     );
// // }
// //
// // #[test]
// // fn test_submit_checkpoint() {
// //     let mut h = Harness::new();
// //     h.constructor(std_params());
// //
// //     let mut i = 0;
// //     // add three validators
// //     let senders: Vec<Address> = h.senders.m.keys().cloned().collect();
// //     for addr in senders {
// //         let value = TokenAmount::from(MIN_COLLATERAL_AMOUNT);
// //         let params = std_join_params();
// //         h.join(addr, value.clone(), params.clone());
// //         let st = h.get_state();
// //         let mut method = Method::AddStake as u64;
// //         if i == 0 {
// //             method = Method::Register as u64;
// //         }
// //         h.expect_send(
// //             &st,
// //             &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
// //             method,
// //             RawBytes::default(),
// //             TokenAmount::from(MIN_COLLATERAL_AMOUNT),
// //         );
// //         i += 1;
// //         if i == 3 {
// //             break;
// //         }
// //     }
// //     // verify that we have an active subnet with 3 validators.
// //     let st = h.get_state();
// //     assert_eq!(st.validator_set.len(), 3);
// //     assert_eq!(st.status, Status::Active);
// //
// //     // Send first checkpoint
// //     let epoch = 10;
// //     let sender = h.senders.get_sender_by_index(0).unwrap();
// //     let ch = h.submit_checkpoint(sender, epoch, &Cid::default(), ExitCode::OK);
// //     let st = h.get_state();
// //     h.verify_check_votes(&st, &ch.cid(), 1);
// //     h.expect_send(
// //         &st,
// //         &sender,
// //         ext::account::PUBKEY_ADDRESS_METHOD,
// //         RawBytes::default(),
// //         0.into(),
// //     );
// //     // no checkpoint committed yet.
// //     h.verify_checkpoint(&st, &epoch, None);
// //     // same miner shouldn't be allowed to submit checkpoint again
// //     h.submit_checkpoint(sender, epoch, &Cid::default(), ExitCode::USR_ILLEGAL_STATE);
// //
// //     let sender = h.senders.get_sender_by_index(1).unwrap();
// //     let ch = h.submit_checkpoint(sender, epoch, &Cid::default(), ExitCode::OK);
// //     let st = h.get_state();
// //     h.expect_send(
// //         &st,
// //         &sender,
// //         ext::account::PUBKEY_ADDRESS_METHOD,
// //         RawBytes::default(),
// //         0.into(),
// //     );
// //     h.expect_send(
// //         &st,
// //         &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
// //         Method::CommitChildCheckpoint as u64,
// //         RawBytes::serialize(ch.clone()).unwrap(),
// //         0.into(),
// //     );
// //     // 2/3 votes. Checkpoint committed
// //     h.verify_checkpoint(&st, &epoch, Some(&ch));
// //     // votes should have been cleaned
// //     h.verify_check_votes(&st, &ch.cid(), 0);
// //
// //     // Trying to submit an already committed checkpoint should fail
// //     let sender = h.senders.get_sender_by_index(2).unwrap();
// //     h.submit_checkpoint(sender, epoch, &Cid::default(), ExitCode::USR_ILLEGAL_STATE);
// //
// //     // If the epoch is wrong in the next checkpoint, it should be rejected.
// //     let prev_cid = ch.cid();
// //     let sender = h.senders.get_sender_by_index(0).unwrap();
// //     h.submit_checkpoint(sender, 11, &prev_cid, ExitCode::USR_ILLEGAL_STATE);
// //
// //     // Only validators should be entitled to submit checkpoints.
// //     let epoch = 20;
// //     let sender = h.senders.get_sender_by_index(3).unwrap();
// //     h.submit_checkpoint(sender, epoch, &prev_cid, ExitCode::USR_ILLEGAL_STATE);
// //
// //     let sender = h.senders.get_sender_by_index(0).unwrap();
// //     // Using wrong prev_cid should fail
// //     h.submit_checkpoint(sender, epoch, &Cid::default(), ExitCode::USR_ILLEGAL_STATE);
// //
// //     // Submit checkpoint for subsequent epoch
// //     let ch = h.submit_checkpoint(sender, epoch, &prev_cid, ExitCode::OK);
// //     let st = h.get_state();
// //     h.verify_check_votes(&st, &ch.cid(), 1);
// //     h.expect_send(
// //         &st,
// //         &sender,
// //         ext::account::PUBKEY_ADDRESS_METHOD,
// //         RawBytes::default(),
// //         0.into(),
// //     );
// //     // no checkpoint committed yet.
// //     h.verify_checkpoint(&st, &epoch, None);
// //
// //     let sender = h.senders.get_sender_by_index(1).unwrap();
// //     let ch = h.submit_checkpoint(sender, epoch, &prev_cid, ExitCode::OK);
// //     let st = h.get_state();
// //     h.expect_send(
// //         &st,
// //         &sender,
// //         ext::account::PUBKEY_ADDRESS_METHOD,
// //         RawBytes::default(),
// //         0.into(),
// //     );
// //     h.expect_send(
// //         &st,
// //         &Address::new_id(ext::sca::SCA_ACTOR_ADDR),
// //         Method::CommitChildCheckpoint as u64,
// //         RawBytes::serialize(ch.clone()).unwrap(),
// //         0.into(),
// //     );
// //     // 2/3 votes. Checkpoint committed
// //     h.verify_checkpoint(&st, &epoch, Some(&ch));
// //     // votes should have been cleaned
// //     h.verify_check_votes(&st, &ch.cid(), 0);
// // }
