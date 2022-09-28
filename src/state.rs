use anyhow::anyhow;
use cid::multihash::Code;
use cid::Cid;
use fvm_ipld_encoding::tuple::{Deserialize_tuple, Serialize_tuple};
use fvm_ipld_encoding::{serde_bytes, to_vec, CborStore, RawBytes, DAG_CBOR};
use fvm_ipld_hamt::{BytesKey, Error as HamtError, Hamt};
use fvm_sdk as sdk;
use fvm_shared::address::{Address, SubnetID};
use fvm_shared::bigint::bigint_ser;
use fvm_shared::bigint::bigint_ser::BigIntDe;
use fvm_shared::bigint::Zero;
use fvm_shared::clock::ChainEpoch;
use fvm_shared::econ::TokenAmount;
use fvm_shared::MethodNum;
use lazy_static::lazy_static;
use num::rational::Ratio;

use fil_actor_hierarchical_sca::{Checkpoint, DEFAULT_CHECKPOINT_PERIOD, MIN_COLLATERAL_AMOUNT};

use crate::blockstore::*;
use crate::types::*;
use crate::utils::{deserialize, ExpectedSend};
use crate::{abort, ext};

lazy_static! {
    static ref VOTING_THRESHOLD: Ratio<TokenAmount> =
        Ratio::new(TokenAmount::from(2), TokenAmount::from(3));
}

/// The state object.
#[derive(Serialize_tuple, Deserialize_tuple, Clone, Debug)]
pub struct State {
    pub name: String,
    pub parent_id: SubnetID,
    pub consensus: ConsensusType,
    #[serde(with = "bigint_ser")]
    pub min_validator_stake: TokenAmount,
    #[serde(with = "bigint_ser")]
    pub total_stake: TokenAmount,
    pub stake: Cid, // BalanceTable of stake (HAMT[address]TokenAmount)
    pub status: Status,
    #[serde(with = "serde_bytes")]
    pub genesis: Vec<u8>,
    pub finality_threshold: ChainEpoch,
    pub check_period: ChainEpoch,
    pub checkpoints: Cid,   // HAMT[cid]Checkpoint
    pub window_checks: Cid, // HAMT[cid]Votes
    pub validator_set: Vec<Validator>,
    pub min_validators: u64,
    // testing flag notifying that we are testing
    pub testing: bool,
    pub expected_msg: Vec<ExpectedSend>,
}

/// We should probably have a derive macro to mark an object as a state object,
/// and have load and save methods automatically generated for them as part of a
/// StateObject trait (i.e. impl StateObject for State).
impl State {
    pub fn new(params: ConstructParams, is_test: bool) -> Self {
        let empty_checkpoint_map = match make_empty_map::<_, ()>(&Blockstore).flush() {
            Ok(c) => c,
            Err(e) => abort!(USR_ILLEGAL_STATE, "failed to create empty map: {:?}", e),
        };
        let empty_votes_map = match make_empty_map::<_, ()>(&Blockstore).flush() {
            Ok(c) => c,
            Err(e) => abort!(USR_ILLEGAL_STATE, "failed to create empty map: {:?}", e),
        };
        let empty_stake_map = match make_empty_map::<_, ()>(&Blockstore).flush() {
            Ok(c) => c,
            Err(e) => abort!(USR_ILLEGAL_STATE, "failed to create empty map: {:?}", e),
        };

        let min_stake = TokenAmount::from(MIN_COLLATERAL_AMOUNT);

        State {
            name: params.name,
            parent_id: params.parent,
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
            checkpoints: empty_checkpoint_map,
            stake: empty_stake_map,
            window_checks: empty_votes_map,
            validator_set: Vec::new(),
            testing: is_test,
            expected_msg: Vec::new(),
        }
    }

    /// Adds stake from a validator
    pub(crate) fn add_stake(&mut self, addr: &Address, amount: &TokenAmount) -> anyhow::Result<()> {
        // update miner stake
        let mut bt = make_map_with_root::<_, BigIntDe>(&self.stake, &Blockstore)?;
        let mut stake = get_stake(&bt, addr)
            .map_err(|e| anyhow!(format!("error getting stake from Hamt: {:?}", e)))?;
        stake += amount;
        set_stake(&mut bt, addr, stake.clone())?;
        self.stake = bt.flush()?;

        // update total collateral
        self.total_stake += amount;

        // check if the miner has coollateral to become a validator
        if stake >= self.min_validator_stake
            && (self.consensus != ConsensusType::Delegated || self.validator_set.len() < 1)
        {
            self.validator_set.push(Validator {
                subnet: SubnetID::new(&self.parent_id, Address::new_id(sdk::message::receiver())),
                addr: addr.clone(),
                // FIXME: Receive address in params
                net_addr: String::new(),
            });
        }

        Ok(())
    }

    pub(crate) fn rm_stake(&mut self, addr: &Address, amount: &TokenAmount) -> anyhow::Result<()> {
        // update miner stake
        let mut bt = make_map_with_root::<_, BigIntDe>(&self.stake, &Blockstore)?;
        let stake = get_stake(&bt, addr)
            .map_err(|e| anyhow!(format!("error getting stake from Hamt: {:?}", e)))?;
        // funds being returned
        let mut stake = stake / LEAVING_COEFF;
        stake -= amount;
        set_stake(&mut bt, addr, stake.clone())?;
        self.stake = bt.flush()?;

        // update total collateral
        self.total_stake -= amount;

        // remove miner from list of validators
        // NOTE: We currently only support full recovery of collateral.
        // And additional check will be needed here if we consider part-recoveries.
        self.validator_set.retain(|x| x.addr != *addr);

        Ok(())
    }

    /// Send new message from actor. It includes some custom code that is run
    /// for test cases.
    pub(crate) fn send(
        &mut self,
        to: &Address,
        method: MethodNum,
        params: RawBytes,
        value: TokenAmount,
    ) -> anyhow::Result<RawBytes> {
        if !self.testing {
            return Ok(sdk::send::send(to, method, params, value)?.return_data);
        } else {
            self.expected_msg.push(ExpectedSend {
                to: to.clone(),
                method,
                params,
                value,
            });
        }

        // Returning default RawBytes, we'll have to send expected instead to
        // make it work in tests.
        Ok(RawBytes::default())
    }

    pub(crate) fn has_majority_vote(&self, votes: &Votes) -> anyhow::Result<bool> {
        let bt = make_map_with_root::<_, BigIntDe>(&self.stake, &Blockstore)?;
        let mut sum = TokenAmount::from(0);
        for v in &votes.validators {
            let stake = get_stake(&bt, v)
                .map_err(|e| anyhow!(format!("error getting stake from Hamt: {:?}", e)))?;
            sum += stake;
        }
        let ftotal = Ratio::from_integer(self.total_stake.clone());
        Ok(Ratio::from_integer(sum) / ftotal >= *VOTING_THRESHOLD)
    }

    pub(crate) fn mutate_state(&mut self) {
        match self.status {
            Status::Instantiated => {
                if self.total_stake >= TokenAmount::from(MIN_COLLATERAL_AMOUNT) {
                    self.status = Status::Active
                }
            }
            Status::Active => {
                if self.total_stake < TokenAmount::from(MIN_COLLATERAL_AMOUNT) {
                    self.status = Status::Inactive
                }
            }
            Status::Inactive => {
                if self.total_stake >= TokenAmount::from(MIN_COLLATERAL_AMOUNT) {
                    self.status = Status::Active
                }
            }
            // if no total_stake and current_balance left (except if we are testing where the funds
            // are never leaving the actor)
            Status::Terminating => {
                if self.total_stake == TokenAmount::zero()
                    && (sdk::sself::current_balance() == TokenAmount::zero() || self.testing)
                {
                    self.status = Status::Killed
                }
            }
            _ => {}
        }
    }

    pub(crate) fn verify_checkpoint(&mut self, ch: &Checkpoint) -> anyhow::Result<()> {
        // check that subnet is active
        if self.status != Status::Active {
            return Err(anyhow!(
                "submitting checkpoints is not allowed while subnet is not active"
            ));
        }

        // check that a checkpoint for the epoch doesn't exist already.
        let checkpoints = make_map_with_root::<_, Checkpoint>(&self.checkpoints, &Blockstore)
            .map_err(|e| anyhow!("failed to load checkpoints: {}", e))?;
        match get_checkpoint(&checkpoints, &ch.epoch())? {
            Some(_) => return Err(anyhow!("cannot submit checkpoint for epoch")),
            None => {}
        };

        // check that the epoch is correct
        if ch.epoch() % self.check_period != 0 {
            return Err(anyhow!(
                "epoch in checkpoint doesn't correspond with a signing window"
            ));
        }
        // check the source is correct
        // FIXME: Using to_string is a workaraound because builtin_actors and this package
        // use different versions of fvm_shared
        if ch.source().to_string()
            != SubnetID::new(&self.parent_id, Address::new_id(sdk::message::receiver())).to_string()
        {
            return Err(anyhow!("submitting checkpoint with the wrong source"));
        }
        // check previous checkpoint
        if self.prev_checkpoint_cid(&checkpoints, &ch.epoch())? != ch.prev_check() {
            return Err(anyhow!(
                "previous checkpoint not consistent with previously committed"
            ));
        }

        // check signature
        // FIXME: we should probably make this its own trait or
        // function, as every implementation of the subnet actor should
        // be entitled to perform its own signature verification.
        // In this case we are verifying a signature of the validator over
        // the cid of the checkpoint.
        let caller = Address::new_id(sdk::message::caller());
        // FIXME: We are skipping the signature verification for
        // testing. This is not good.
        let pkey = self.resolve_secp_bls(&caller)?;
        if !self.testing
            && !sdk::crypto::verify_signature(
                &RawBytes::deserialize(&ch.signature().clone().into())?,
                &pkey,
                &ch.cid().to_bytes(),
            )?
        {
            return Err(anyhow!("signature verification failed"));
        }

        // verify that signer is a validator
        if !self.validator_set.iter().any(|x| x.addr == caller) {
            return Err(anyhow!("checkpoint not signed by a validator"));
        }

        Ok(())
    }

    // we need mutable reference to self due to the expected message for testing.
    fn resolve_secp_bls(&mut self, addr: &Address) -> anyhow::Result<Address> {
        let resolved = match sdk::actor::resolve_address(addr) {
            Some(id) => Address::new_id(id),
            None => return Err(anyhow!("couldn't resolve actor address")),
        };
        let ret = self.send(
            &resolved,
            ext::account::PUBKEY_ADDRESS_METHOD,
            RawBytes::default(),
            TokenAmount::zero(),
        )?;
        // if testing return a testing address without
        // processing the response.
        // FIXME: We should include some "expectedReturn" thing
        // to dynamically select this.
        if self.testing {
            return Ok(Address::new_id(TESTING_ID));
        }
        let pub_key: Address = deserialize(&ret, "address response")?;
        Ok(pub_key)
    }

    fn prev_checkpoint_cid<BS: fvm_ipld_blockstore::Blockstore>(
        &self,
        checkpoints: &Map<BS, Checkpoint>,
        epoch: &ChainEpoch,
    ) -> anyhow::Result<Cid> {
        let mut epoch = epoch - self.check_period;
        while epoch >= 0 {
            match get_checkpoint(checkpoints, &epoch)? {
                Some(ch) => return Ok(ch.cid()),
                None => {
                    epoch -= self.check_period;
                }
            }
        }
        Ok(Cid::default())
    }

    pub fn load() -> Self {
        // First, load the current state root.
        let root = match sdk::sself::root() {
            Ok(root) => root,
            Err(err) => abort!(USR_ILLEGAL_STATE, "failed to get root: {:?}", err),
        };

        // Load the actor state from the state tree.
        match Blockstore.get_cbor::<Self>(&root) {
            Ok(Some(state)) => state,
            Ok(None) => abort!(USR_ILLEGAL_STATE, "state does not exist"),
            Err(err) => abort!(USR_ILLEGAL_STATE, "failed to get state: {}", err),
        }
    }

    // check if initial test is for testing
    pub fn is_test() -> bool {
        // first check if there is state already initialized
        let root = match sdk::sself::root() {
            Ok(root) => root,
            // if err it may be because there's nothing so no state has
            // been set
            Err(_) => return false,
        };

        // Load the actor state from the state tree.
        match Blockstore.get_cbor::<Self>(&root) {
            // if we have state check if we are testing or not
            Ok(Some(state)) => return state.testing,
            // if not found we are definitely not testing
            _ => return false,
        }
    }

    pub(crate) fn flush_checkpoint<BS: fvm_ipld_blockstore::Blockstore>(
        &mut self,
        ch: &Checkpoint,
    ) -> anyhow::Result<()> {
        let mut checkpoints = make_map_with_root::<_, Checkpoint>(&self.checkpoints, &Blockstore)
            .map_err(|e| anyhow!("error loading checkpoints: {}", e))?;
        set_checkpoint(&mut checkpoints, ch.clone())?;
        self.checkpoints = checkpoints
            .flush()
            .map_err(|e| anyhow!("error flushing checkpoints: {}", e))?;
        Ok(())
    }

    pub fn save(&self) -> Cid {
        let serialized = match to_vec(self) {
            Ok(s) => s,
            Err(err) => abort!(USR_SERIALIZATION, "failed to serialize state: {:?}", err),
        };
        let cid = match sdk::ipld::put(Code::Blake2b256.into(), 32, DAG_CBOR, serialized.as_slice())
        {
            Ok(cid) => cid,
            Err(err) => abort!(USR_SERIALIZATION, "failed to store initial state: {:}", err),
        };
        if let Err(err) = sdk::sself::set_root(&cid) {
            abort!(USR_ILLEGAL_STATE, "failed to set root ciid: {:}", err);
        }
        cid
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            name: String::new(),
            parent_id: SubnetID::default(),
            consensus: ConsensusType::Delegated,
            min_validator_stake: TokenAmount::from(MIN_COLLATERAL_AMOUNT),
            total_stake: TokenAmount::zero(),
            finality_threshold: 5,
            check_period: 10,
            genesis: Vec::new(),
            status: Status::Instantiated,
            checkpoints: Cid::default(),
            stake: Cid::default(),
            window_checks: Cid::default(),
            validator_set: Vec::new(),
            min_validators: 0,
            testing: true,
            expected_msg: Vec::new(),
        }
    }
}

pub fn set_stake<BS: fvm_ipld_blockstore::Blockstore>(
    stakes: &mut Hamt<BS, BigIntDe>,
    addr: &Address,
    amount: TokenAmount,
) -> anyhow::Result<()> {
    stakes
        .set(addr.to_bytes().into(), BigIntDe(amount))
        .map_err(|e| anyhow!(format!("failed to set stake for addr {}: {:?}", addr, e)))?;
    Ok(())
}

/// Gets token amount for given address in balance table
pub fn get_stake<'m, BS: fvm_ipld_blockstore::Blockstore>(
    stakes: &'m Hamt<BS, BigIntDe>,
    key: &Address,
) -> Result<TokenAmount, HamtError> {
    if let Some(v) = stakes.get(&key.to_bytes())? {
        Ok(v.0.clone())
    } else {
        Ok(0.into())
    }
}

pub fn get_checkpoint<'m, BS: fvm_ipld_blockstore::Blockstore>(
    checkpoints: &'m Map<BS, Checkpoint>,
    epoch: &ChainEpoch,
) -> anyhow::Result<Option<&'m Checkpoint>> {
    checkpoints
        .get(&BytesKey::from(epoch.to_ne_bytes().to_vec()))
        .map_err(|e| anyhow!("failed to get checkpoint for id {}: {}", epoch, e))
}

pub fn set_checkpoint<BS: fvm_ipld_blockstore::Blockstore>(
    checkpoints: &mut Map<BS, Checkpoint>,
    ch: Checkpoint,
) -> anyhow::Result<()> {
    let epoch = ch.epoch();
    checkpoints
        .set(BytesKey::from(epoch.to_ne_bytes().to_vec()), ch)
        .map_err(|e| anyhow!("failed to set checkpoint: {}", e))?;
    Ok(())
}

pub fn get_votes<'m, BS: fvm_ipld_blockstore::Blockstore>(
    check_votes: &'m Map<BS, Votes>,
    cid: &Cid,
) -> anyhow::Result<Option<&'m Votes>> {
    check_votes
        .get(&cid.to_bytes())
        .map_err(|e| anyhow!("failed to get checkpoint votes: {}", e))
}

pub fn set_votes<BS: fvm_ipld_blockstore::Blockstore>(
    check_votes: &mut Map<BS, Votes>,
    cid: &Cid,
    votes: Votes,
) -> anyhow::Result<()> {
    check_votes
        .set(cid.to_bytes().into(), votes)
        .map_err(|e| anyhow!("failed to set checkpoint votes: {}", e))?;
    Ok(())
}
