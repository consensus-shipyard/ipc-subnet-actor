use anyhow::anyhow;
use cid::Cid;
use fvm_ipld_blockstore::Blockstore;
use fvm_ipld_encoding::Cbor;
use fvm_ipld_hamt::BytesKey;
use fvm_shared::address::Address;
use fvm_shared::bigint::Zero;
use fvm_shared::clock::ChainEpoch;
use fvm_shared::econ::TokenAmount;
use ipc_gateway::{Checkpoint, SubnetID, DEFAULT_CHECKPOINT_PERIOD, MIN_COLLATERAL_AMOUNT};
use primitives::{TCid, THamt};
use serde::{Deserialize, Serialize};

use crate::types::*;

// lazy_static! {
//     static ref VOTING_THRESHOLD: Ratio<TokenAmount> =
//         Ratio::new(TokenAmount::from_atto(2), TokenAmount::from_atto(3));
// }

/// The state object.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct State {
    pub name: String,
    pub parent_id: SubnetID,
    pub ipc_gateway_addr: Address,
    pub consensus: ConsensusType,
    pub min_validator_stake: TokenAmount,
    pub total_stake: TokenAmount,
    pub stake: TCid<THamt<Cid, TokenAmount>>,
    pub status: Status,
    pub genesis: Vec<u8>,
    pub finality_threshold: ChainEpoch,
    pub check_period: ChainEpoch,
    pub checkpoints: TCid<THamt<Cid, Checkpoint>>,
    pub window_checks: TCid<THamt<Cid, Votes>>,
    pub validator_set: Vec<Validator>,
    pub min_validators: u64,
}

impl Cbor for State {}

/// We should probably have a derive macro to mark an object as a state object,
/// and have load and save methods automatically generated for them as part of a
/// StateObject trait (i.e. impl StateObject for State).
impl State {
    pub fn new<BS: Blockstore>(store: &BS, params: ConstructParams) -> anyhow::Result<State> {
        let min_stake = TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT);

        let state = State {
            name: params.name,
            parent_id: params.parent,
            ipc_gateway_addr: Address::new_id(params.ipc_gateway_addr),
            consensus: params.consensus,
            total_stake: TokenAmount::zero(),
            min_validator_stake: if params.min_validator_stake < min_stake {
                min_stake
            } else {
                params.min_validator_stake
            },
            min_validators: params.min_validators,
            finality_threshold: params.finality_threshold,
            check_period: if params.check_period < DEFAULT_CHECKPOINT_PERIOD {
                DEFAULT_CHECKPOINT_PERIOD
            } else {
                params.check_period
            },
            genesis: params.genesis,
            status: Status::Instantiated,
            checkpoints: TCid::new_hamt(store)?,
            stake: TCid::new_hamt(store)?,
            window_checks: TCid::new_hamt(store)?,
            validator_set: Vec::new(),
        };

        Ok(state)
    }

    /// Get the stake of an address.
    pub fn get_stake<BS: Blockstore>(
        &self,
        store: &BS,
        addr: &Address,
    ) -> anyhow::Result<Option<TokenAmount>> {
        let hamt = self.stake.load(store)?;
        let amount = hamt.get(&BytesKey::from(addr.to_bytes()))?;
        Ok(amount.cloned())
    }

    /// Adds stake from a validator
    pub(crate) fn add_stake<BS: Blockstore>(
        &mut self,
        store: &BS,
        addr: &Address,
        net_addr: &str,
        amount: &TokenAmount,
    ) -> anyhow::Result<()> {
        // update miner stake
        self.stake.modify(store, |hamt| {
            // Note that when trying to get stake, if it is not found in the
            // hamt, that means it's the first time adding stake and we just
            // give default stake amount 0.
            let key = BytesKey::from(addr.to_bytes());
            let stake = hamt.get(&key)?.unwrap_or(&TokenAmount::zero()).clone();
            let updated_stake = stake + amount;

            hamt.set(key, updated_stake.clone())?;

            // update total collateral
            self.total_stake += amount;

            // check if the miner has collateral to become a validator
            if updated_stake >= self.min_validator_stake
                && (self.consensus != ConsensusType::Delegated || self.validator_set.is_empty())
            {
                self.validator_set.push(Validator {
                    addr: *addr,
                    net_addr: String::from(net_addr),
                });
            }

            Ok(true)
        })?;

        Ok(())
    }

    pub fn rm_stake<BS: Blockstore>(
        &mut self,
        store: &BS,
        addr: &Address,
        amount: &TokenAmount,
    ) -> anyhow::Result<()> {
        // update miner stake
        self.stake.modify(store, |hamt| {
            // Note that when trying to get stake, if it is not found in the
            // hamt, that means it's the first time adding stake and we just
            // give default stake amount 0.
            let key = BytesKey::from(addr.to_bytes());
            let mut stake = hamt.get(&key)?.unwrap_or(&TokenAmount::zero()).clone();
            stake = stake.div_floor(LEAVING_COEFF);

            if stake.lt(amount) {
                return Err(anyhow!(format!(
                    "address not enough stake to withdraw: {:?}",
                    addr
                )));
            }

            hamt.set(key, stake - amount)?;

            // update total collateral
            self.total_stake -= amount;

            // remove miner from list of validators
            // NOTE: We currently only support full recovery of collateral.
            // And additional check will be needed here if we consider part-recoveries.
            self.validator_set.retain(|x| x.addr != *addr);

            Ok(true)
        })?;

        Ok(())
    }

    // /// Send new message from actor. It includes some custom code that is run
    // /// for test cases.
    // pub(crate) fn send(
    //     &mut self,
    //     to: &Address,
    //     method: MethodNum,
    //     params: RawBytes,
    //     value: TokenAmount,
    // ) -> anyhow::Result<RawBytes> {
    //     self.expected_msg.push(ExpectedSend {
    //         to: to.clone(),
    //         method,
    //         params,
    //         value,
    //     });
    //
    //     // Returning default RawBytes, we'll have to send expected instead to
    //     // make it work in tests.
    //     Ok(RawBytes::default())
    // }
    //
    // pub(crate) fn has_majority_vote(&self, votes: &Votes) -> anyhow::Result<bool> {
    //     let bt = make_map_with_root::<_, BigIntDe>(&self.stake, &Blockstore)?;
    //     let mut sum = TokenAmount::from(0);
    //     for v in &votes.validators {
    //         let stake = get_stake(&bt, v)
    //             .map_err(|e| anyhow!(format!("error getting stake from Hamt: {:?}", e)))?;
    //         sum += stake;
    //     }
    //     let ftotal = Ratio::from_integer(self.total_stake.clone());
    //     Ok(Ratio::from_integer(sum) / ftotal >= *VOTING_THRESHOLD)
    // }
    //
    pub fn mutate_state(&mut self) {
        match self.status {
            Status::Instantiated => {
                if self.total_stake >= TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT) {
                    self.status = Status::Active
                }
            }
            Status::Active => {
                if self.total_stake < TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT) {
                    self.status = Status::Inactive
                }
            }
            Status::Inactive => {
                if self.total_stake >= TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT) {
                    self.status = Status::Active
                }
            }
            // if no total_stake and current_balance left (except if we are testing where the funds
            // are never leaving the actor)
            Status::Terminating => {
                if self.total_stake == TokenAmount::zero() {
                    self.status = Status::Killed
                }
            }
            _ => {}
        }
    }

    // pub(crate) fn verify_checkpoint(&mut self, ch: &Checkpoint) -> anyhow::Result<()> {
    //     // check that subnet is active
    //     if self.status != Status::Active {
    //         return Err(anyhow!(
    //             "submitting checkpoints is not allowed while subnet is not active"
    //         ));
    //     }
    //
    //     // check that a checkpoint for the epoch doesn't exist already.
    //     let checkpoints = make_map_with_root::<_, Checkpoint>(&self.checkpoints, &Blockstore)
    //         .map_err(|e| anyhow!("failed to load checkpoints: {}", e))?;
    //     match get_checkpoint(&checkpoints, &ch.epoch())? {
    //         Some(_) => return Err(anyhow!("cannot submit checkpoint for epoch")),
    //         None => {}
    //     };
    //
    //     // check that the epoch is correct
    //     if ch.epoch() % self.check_period != 0 {
    //         return Err(anyhow!(
    //             "epoch in checkpoint doesn't correspond with a signing window"
    //         ));
    //     }
    //     // check the source is correct
    //     // FIXME: Using to_string is a workaraound because builtin_actors and this package
    //     // use different versions of fvm_shared
    //     if ch.source().to_string()
    //         != SubnetID::new(&self.parent_id, Address::new_id(sdk::message::receiver())).to_string()
    //     {
    //         return Err(anyhow!("submitting checkpoint with the wrong source"));
    //     }
    //     // check previous checkpoint
    //     if self.prev_checkpoint_cid(&checkpoints, &ch.epoch())? != ch.prev_check() {
    //         return Err(anyhow!(
    //             "previous checkpoint not consistent with previously committed"
    //         ));
    //     }
    //
    //     // check signature
    //     // FIXME: we should probably make this its own trait or
    //     // function, as every implementation of the subnet actor should
    //     // be entitled to perform its own signature verification.
    //     // In this case we are verifying a signature of the validator over
    //     // the cid of the checkpoint.
    //     let caller = Address::new_id(sdk::message::caller());
    //     // FIXME: We are skipping the signature verification for
    //     // testing. This is not good.
    //     let pkey = self.resolve_secp_bls(&caller)?;
    //     if !sdk::crypto::verify_signature(
    //         &RawBytes::deserialize(&ch.signature().clone().into())?,
    //         &pkey,
    //         &ch.cid().to_bytes(),
    //     )? {
    //         return Err(anyhow!("signature verification failed"));
    //     }
    //
    //     // verify that signer is a validator
    //     if !self.validator_set.iter().any(|x| x.addr == caller) {
    //         return Err(anyhow!("checkpoint not signed by a validator"));
    //     }
    //
    //     Ok(())
    // }
    //
    // // we need mutable reference to self due to the expected message for testing.
    // fn resolve_secp_bls(&mut self, addr: &Address) -> anyhow::Result<Address> {
    //     let resolved = match sdk::actor::resolve_address(addr) {
    //         Some(id) => Address::new_id(id),
    //         None => return Err(anyhow!("couldn't resolve actor address")),
    //     };
    //     let ret = self.send(
    //         &resolved,
    //         ext::account::PUBKEY_ADDRESS_METHOD,
    //         RawBytes::default(),
    //         TokenAmount::zero(),
    //     )?;
    //     // if testing return a testing address without
    //     // processing the response.
    //     // FIXME: We should include some "expectedReturn" thing
    //     // to dynamically select this.
    //     let pub_key: Address = deserialize(&ret, "address response")?;
    //     Ok(pub_key)
    // }
    //
    // fn prev_checkpoint_cid<BS: fvm_ipld_blockstore::Blockstore>(
    //     &self,
    //     checkpoints: &Map<BS, Checkpoint>,
    //     epoch: &ChainEpoch,
    // ) -> anyhow::Result<Cid> {
    //     let mut epoch = epoch - self.check_period;
    //     while epoch >= 0 {
    //         match get_checkpoint(checkpoints, &epoch)? {
    //             Some(ch) => return Ok(ch.cid()),
    //             None => {
    //                 epoch -= self.check_period;
    //             }
    //         }
    //     }
    //     Ok(Cid::default())
    // }
    //
    // pub fn load() -> Self {
    //     // First, load the current state root.
    //     let root = match sdk::sself::root() {
    //         Ok(root) => root,
    //         Err(err) => abort!(USR_ILLEGAL_STATE, "failed to get root: {:?}", err),
    //     };
    //
    //     // Load the actor state from the state tree.
    //     match Blockstore.get_cbor::<Self>(&root) {
    //         Ok(Some(state)) => state,
    //         Ok(None) => abort!(USR_ILLEGAL_STATE, "state does not exist"),
    //         Err(err) => abort!(USR_ILLEGAL_STATE, "failed to get state: {}", err),
    //     }
    // }
    //
    // pub(crate) fn flush_checkpoint<BS: fvm_ipld_blockstore::Blockstore>(
    //     &mut self,
    //     ch: &Checkpoint,
    // ) -> anyhow::Result<()> {
    //     let mut checkpoints = make_map_with_root::<_, Checkpoint>(&self.checkpoints, &Blockstore)
    //         .map_err(|e| anyhow!("error loading checkpoints: {}", e))?;
    //     set_checkpoint(&mut checkpoints, ch.clone())?;
    //     self.checkpoints = checkpoints
    //         .flush()
    //         .map_err(|e| anyhow!("error flushing checkpoints: {}", e))?;
    //     Ok(())
    // }
    //
    // pub fn save(&self) -> Cid {
    //     let serialized = match to_vec(self) {
    //         Ok(s) => s,
    //         Err(err) => abort!(USR_SERIALIZATION, "failed to serialize state: {:?}", err),
    //     };
    //     let cid = match sdk::ipld::put(Code::Blake2b256.into(), 32, DAG_CBOR, serialized.as_slice())
    //     {
    //         Ok(cid) => cid,
    //         Err(err) => abort!(USR_SERIALIZATION, "failed to store initial state: {:}", err),
    //     };
    //     if let Err(err) = sdk::sself::set_root(&cid) {
    //         abort!(USR_ILLEGAL_STATE, "failed to set root ciid: {:}", err);
    //     }
    //     cid
    // }
}

impl Default for State {
    fn default() -> Self {
        Self {
            name: String::new(),
            parent_id: SubnetID::default(),
            ipc_gateway_addr: Address::new_id(0),
            consensus: ConsensusType::Delegated,
            min_validator_stake: TokenAmount::from_atto(MIN_COLLATERAL_AMOUNT),
            total_stake: TokenAmount::zero(),
            finality_threshold: 5,
            check_period: 10,
            genesis: Vec::new(),
            status: Status::Instantiated,
            checkpoints: TCid::default(),
            stake: TCid::default(),
            window_checks: TCid::default(),
            validator_set: Vec::new(),
            min_validators: 0,
        }
    }
}

// pub fn set_stake<BS: fvm_ipld_blockstore::Blockstore>(
//     stakes: &mut Hamt<BS, BigIntDe>,
//     addr: &Address,
//     amount: TokenAmount,
// ) -> anyhow::Result<()> {
//     stakes
//         .set(addr.to_bytes().into(), BigIntDe(amount))
//         .map_err(|e| anyhow!(format!("failed to set stake for addr {}: {:?}", addr, e)))?;
//     Ok(())
// }

// /// Gets token amount for given address in balance table
// pub fn get_stake<'m, BS: fvm_ipld_blockstore::Blockstore>(
//     stakes: &'m Hamt<BS, BigIntDe>,
//     key: &Address,
// ) -> Result<TokenAmount, HamtError> {
//     if let Some(v) = stakes.get(&key.to_bytes())? {
//         Ok(v.0.clone())
//     } else {
//         Ok(0.into())
//     }
// }

// pub fn get_checkpoint<'m, BS: fvm_ipld_blockstore::Blockstore>(
//     checkpoints: &'m Map<BS, Checkpoint>,
//     epoch: &ChainEpoch,
// ) -> anyhow::Result<Option<&'m Checkpoint>> {
//     checkpoints
//         .get(&BytesKey::from(epoch.to_ne_bytes().to_vec()))
//         .map_err(|e| anyhow!("failed to get checkpoint for id {}: {}", epoch, e))
// }

// pub fn set_checkpoint<BS: fvm_ipld_blockstore::Blockstore>(
//     checkpoints: &mut Map<BS, Checkpoint>,
//     ch: Checkpoint,
// ) -> anyhow::Result<()> {
//     let epoch = ch.epoch();
//     checkpoints
//         .set(BytesKey::from(epoch.to_ne_bytes().to_vec()), ch)
//         .map_err(|e| anyhow!("failed to set checkpoint: {}", e))?;
//     Ok(())
// }
//
// pub fn get_votes<'m, BS: fvm_ipld_blockstore::Blockstore>(
//     check_votes: &'m Map<BS, Votes>,
//     cid: &Cid,
// ) -> anyhow::Result<Option<&'m Votes>> {
//     check_votes
//         .get(&cid.to_bytes())
//         .map_err(|e| anyhow!("failed to get checkpoint votes: {}", e))
// }
//
// pub fn set_votes<BS: fvm_ipld_blockstore::Blockstore>(
//     check_votes: &mut Map<BS, Votes>,
//     cid: &Cid,
//     votes: Votes,
// ) -> anyhow::Result<()> {
//     check_votes
//         .set(cid.to_bytes().into(), votes)
//         .map_err(|e| anyhow!("failed to set checkpoint votes: {}", e))?;
//     Ok(())
// }
