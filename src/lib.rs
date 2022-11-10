pub mod ext;
pub mod state;
pub mod types;

use fil_actors_runtime::runtime::{ActorCode, Runtime};
use fil_actors_runtime::{actor_error, cbor, ActorDowncast, ActorError, INIT_ACTOR_ADDR};
use fvm_ipld_blockstore::Blockstore;
use fvm_ipld_encoding::RawBytes;

use fvm_shared::econ::TokenAmount;
use fvm_shared::error::ExitCode;
use fvm_shared::{MethodNum, METHOD_CONSTRUCTOR};
use ipc_gateway::MIN_COLLATERAL_AMOUNT;
use num_derive::FromPrimitive;
use num_traits::{FromPrimitive, Zero};

use crate::state::State;
use crate::types::*;

fil_actors_runtime::wasm_trampoline!(Actor);

/// Atomic execution coordinator actor methods available
#[derive(FromPrimitive)]
#[repr(u64)]
pub enum Method {
    Constructor = METHOD_CONSTRUCTOR,
    Join = 2,
    // Leave = 3,
    // Kill = 4,
    // SubmitCheckpoint = 5,
}

/// SubnetActor trait. Custom subnet actors need to implement this trait
/// in order to be used as part of hierarchical consensus.
///
/// Subnet actors are responsible for the governing policies of HC subnets.
pub trait SubnetActor {
    /// Deploys subnet actor with the corresponding parameters.
    fn constructor<BS, RT>(rt: &mut RT, params: ConstructParams) -> Result<(), ActorError>
    where
        BS: Blockstore,
        RT: Runtime<BS>;
    /// Logic for new peers to join a subnet.
    fn join<BS, RT>(rt: &mut RT, params: JoinParams) -> Result<Option<RawBytes>, ActorError>
    where
        BS: Blockstore,
        RT: Runtime<BS>;
    // /// Called by peers to leave a subnet.
    // fn leave() -> anyhow::Result<Option<RawBytes>>;
    // /// Sends a kill signal for the subnet to the SCA.
    // fn kill() -> anyhow::Result<Option<RawBytes>>;
    // /// Submits a new checkpoint for the subnet.
    // fn submit_checkpoint(ch: Checkpoint) -> anyhow::Result<Option<RawBytes>>;
}

/// SubnetActor trait. Custom subnet actors need to implement this trait
/// in order to be used as part of hierarchical consensus.
///
/// Subnet actors are responsible for the governing policies of HC subnets.
pub struct Actor;

impl SubnetActor for Actor {
    /// The constructor populates the initial state.
    ///
    /// Method num 1. This is part of the Filecoin calling convention.
    /// InitActor#Exec will call the constructor on method_num = 1.
    fn constructor<BS, RT>(rt: &mut RT, params: ConstructParams) -> Result<(), ActorError>
    where
        BS: Blockstore,
        RT: Runtime<BS>,
    {
        rt.validate_immediate_caller_is(std::iter::once(&*INIT_ACTOR_ADDR))?;

        let st = State::new(rt.store(), params).map_err(|e| {
            e.downcast_default(ExitCode::USR_ILLEGAL_STATE, "Failed to create actor state")
        })?;

        rt.create(&st)?;

        Ok(())
    }

    /// Called by peers looking to join a subnet.
    ///
    /// It implements the basic logic to onboard new peers to the subnet.
    fn join<BS, RT>(rt: &mut RT, params: JoinParams) -> Result<Option<RawBytes>, ActorError>
    where
        BS: Blockstore,
        RT: Runtime<BS>,
    {
        let caller = rt.message().caller();
        // TODO: shall we check caller interface instead here?
        // let code_cid = get_actor_code_cid(&caller).unwrap_or(Cid::default());
        // if sdk::actor::get_builtin_actor_type(&code_cid) != Some(Type::Account) {
        //     abort!(USR_FORBIDDEN, "caller not account actor type");
        // }

        let amount = rt.message().value_received();
        if amount <= TokenAmount::zero() {
            return Err(actor_error!(
                illegal_argument,
                "a minimum collateral is required to join the subnet"
            ));
        }

        let mut msg = None;
        rt.transaction(|st: &mut State, rt| {
            // increase collateral
            st.add_stake(rt.store(), &caller, &params.validator_net_addr, &amount)
                .map_err(|e| {
                    e.downcast_default(ExitCode::USR_ILLEGAL_STATE, "failed to load subnet")
                })?;

            // TODO: cur_balance should be the updated total stake?
            let cur_balance = st.total_stake.clone();

            if st.status == Status::Instantiated {
                if cur_balance >= TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT) {
                    msg = Some(CrossActorPayload::new(
                        st.ipc_gateway_addr,
                        ipc_gateway::Method::Register as u64,
                        RawBytes::default(),
                        st.total_stake.clone(),
                    ));
                }
            } else {
                msg = Some(CrossActorPayload::new(
                    st.ipc_gateway_addr,
                    ipc_gateway::Method::AddStake as u64,
                    RawBytes::default(),
                    amount,
                ));
            }

            st.mutate_state(&cur_balance);

            Ok(true)
        })?;

        if let Some(p) = msg {
            rt.send(p.to, p.method, p.params, p.value)?;
        }

        Ok(None)
    }

    // /// Called by peers looking to leave a subnet.
    // fn leave() -> anyhow::Result<Option<RawBytes>> {
    //     let mut st = State::load();
    //     let caller = Address::new_id(sdk::message::caller());
    //     // check type of caller
    //     let code_cid = get_actor_code_cid(&caller).unwrap_or(Cid::default());
    //     if sdk::actor::get_builtin_actor_type(&code_cid) != Some(Type::Account) {
    //         abort!(USR_FORBIDDEN, "caller not account actor type");
    //     }
    //
    //     // get stake to know how much to release
    //     let bt = make_map_with_root::<_, BigIntDe>(&st.stake, &Blockstore)?;
    //     let stake = get_stake(&bt, &caller.clone())?;
    //     if stake == TokenAmount::zero() {
    //         abort!(USR_ILLEGAL_STATE, "caller has no stake in subnet");
    //     }
    //
    //     // release from SCA
    //     if st.status != Status::Terminating {
    //         st.send(
    //             &Address::new_id(ext::sca::sca_actor_addr),
    //             Method::ReleaseStake as u64,
    //             RawBytes::serialize(FundParams {
    //                 value: stake.clone(),
    //             })?,
    //             TokenAmount::zero(),
    //         )?;
    //     }
    //
    //     // remove stake from balance table
    //     st.rm_stake(&caller, &stake)?;
    //
    //     // send back to owner
    //     st.send(&caller, METHOD_SEND, RawBytes::default(), stake)?;
    //
    //     st.mutate_state();
    //     st.save();
    //     Ok(None)
    // }
    //
    // fn kill() -> anyhow::Result<Option<RawBytes>> {
    //     let mut st = State::load();
    //
    //     if st.status == Status::Terminating || st.status == Status::Killed {
    //         abort!(
    //             USR_ILLEGAL_STATE,
    //             "the subnet is already in a killed or terminating state"
    //         );
    //     }
    //     if st.validator_set.len() != 0 {
    //         abort!(
    //             USR_ILLEGAL_STATE,
    //             "this subnet can only be killed when all validators have left"
    //         );
    //     }
    //
    //     // move to terminating state
    //     st.status = Status::Terminating;
    //
    //     // unregister subnet
    //     st.send(
    //         &Address::new_id(ext::sca::sca_actor_addr),
    //         Method::Kill as u64,
    //         RawBytes::default(),
    //         TokenAmount::zero(),
    //     )?;
    //
    //     st.mutate_state();
    //     st.save();
    //     Ok(None)
    // }
    //
    // /// SubmitCheckpoint accepts signed checkpoint votes for miners.
    // ///
    // /// This functions verifies that the checkpoint is valid before
    // /// propagating it for commitment to the SCA. It expects at least
    // /// votes from 2/3 of miners with collateral.
    // fn submit_checkpoint(checkpoint: Checkpoint) -> anyhow::Result<Option<RawBytes>> {
    //     let mut st = State::load();
    //     let caller = Address::new_id(sdk::message::caller());
    //     // check type of caller
    //     let code_cid = get_actor_code_cid(&caller).unwrap_or(Cid::default());
    //     if sdk::actor::get_builtin_actor_type(&code_cid) != Some(Type::Account) {
    //         abort!(USR_FORBIDDEN, "caller not account actor type");
    //     }
    //
    //     let ch_cid = checkpoint.cid();
    //     // verify checkpoint
    //     st.verify_checkpoint(&checkpoint)?;
    //
    //     // get votes for committed checkpoint
    //     let mut votes_map = make_map_with_root::<_, Votes>(&st.window_checks, &Blockstore)
    //         .map_err(|e| anyhow!("failed to load checkpoints: {}", e))?;
    //     let mut found = false;
    //     let mut votes = match get_votes(&votes_map, &ch_cid)? {
    //         Some(v) => {
    //             found = true;
    //             v.clone()
    //         }
    //         None => Votes {
    //             validators: Vec::new(),
    //         },
    //     };
    //
    //     if votes.validators.iter().any(|x| x == &caller) {
    //         return Err(anyhow!("miner has already voted the checkpoint"));
    //     }
    //
    //     // add miner vote
    //     votes.validators.push(caller);
    //
    //     // if has majority
    //     if st.has_majority_vote(&votes)? {
    //         // commit checkpoint
    //         st.flush_checkpoint::<&Blockstore>(&checkpoint)?;
    //         // propagate to sca
    //         st.send(
    //             &Address::new_id(sca_actor_addr),
    //             Method::CommitChildCheckpoint as u64,
    //             RawBytes::serialize(checkpoint)?,
    //             0.into(),
    //         )?;
    //         // remove votes used for commitment
    //         if found {
    //             votes_map.delete(&ch_cid.to_bytes())?;
    //         }
    //     } else {
    //         // if no majority store vote and return
    //         votes_map.set(ch_cid.to_bytes().into(), votes)?;
    //     }
    //
    //     // flush votes
    //     st.window_checks = votes_map.flush()?;
    //
    //     st.save();
    //     Ok(None)
    // }
}

impl ActorCode for Actor {
    fn invoke_method<BS, RT>(
        rt: &mut RT,
        method: MethodNum,
        params: &RawBytes,
    ) -> Result<RawBytes, ActorError>
    where
        BS: Blockstore,
        RT: Runtime<BS>,
    {
        match FromPrimitive::from_u64(method) {
            Some(Method::Constructor) => {
                Self::constructor(rt, cbor::deserialize_params(params)?)?;
                Ok(RawBytes::default())
            }
            Some(Method::Join) => {
                let res = Self::join(rt, cbor::deserialize_params(params)?)?;
                Ok(RawBytes::serialize(res)?)
            }
            None => Err(actor_error!(unhandled_message; "Invalid method")),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{Actor, ConsensusType, ConstructParams, JoinParams, Method, State, Status};
    use cid::Cid;
    use fil_actors_runtime::runtime::Runtime;
    use fil_actors_runtime::test_utils::MockRuntime;
    use fil_actors_runtime::{actor_error, cbor, INIT_ACTOR_ADDR};
    use fvm_ipld_encoding::RawBytes;
    use fvm_shared::address::Address;
    use fvm_shared::econ::TokenAmount;
    use fvm_shared::error::ExitCode;
    use ipc_gateway::MIN_COLLATERAL_AMOUNT;

    // just a test address
    const IPC_GATEWAY_ADDR: u64 = 1024;

    fn std_construct_param() -> ConstructParams {
        ConstructParams {
            parent: Default::default(),
            name: "".to_string(),
            ipc_gateway_addr: IPC_GATEWAY_ADDR,
            consensus: ConsensusType::Delegated,
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
        // let state: State = runtime.state().unwrap();
    }

    #[test]
    fn test_join_fail_no_min_collateral() {
        let mut runtime = construct_runtime();

        let validator = Address::new_id(100);
        let params = JoinParams {
            validator_net_addr: validator.to_string(),
        };

        let r = runtime.call::<Actor>(
            Method::Join as u64,
            &cbor::serialize(&params, "test").unwrap(),
        );

        assert_eq!(
            r,
            Err(actor_error!(
                illegal_argument,
                "a minimum collateral is required to join the subnet"
            ))
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
        assert_eq!(st.validator_set.len(), 1);
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
}
