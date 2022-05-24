use std::env;

use cid::Cid;
use fvm::executor::ApplyKind;
use fvm::executor::Executor;
use fvm::machine::Machine;
use fvm::state_tree::StateTree;
use fvm_integration_tests::tester::{Account, Tester};
use fvm_ipld_blockstore::{Blockstore, MemoryBlockstore};
use fvm_ipld_encoding::RawBytes;
use fvm_shared::address::Address;
use fvm_shared::bigint::bigint_ser::BigIntDe;
use fvm_shared::bigint::BigInt;
use fvm_shared::econ::TokenAmount;
use fvm_shared::error::ExitCode;
use fvm_shared::message::Message;
use fvm_shared::state::StateTreeVersion;
use fvm_shared::version::NetworkVersion;
use fvm_shared::ActorID;
use num_traits::Zero;
use fil_actor_hierarchical_sca::State as SCAState;

use fil_hierarchical_subnet_actor::blockstore::make_map_with_root;
use fil_hierarchical_subnet_actor::state::{get_stake, State};
use fil_hierarchical_subnet_actor::types::{ConstructParams, Status};

pub const TEST_INIT_ACTOR_ADDR: ActorID = 339;
pub const TEST_ACTOR_ADDR: ActorID = 9999;
pub const NUM_ACC: usize = 3;

pub struct Harness {
    pub tester: Tester<MemoryBlockstore>,
    pub actor_address: Address,
    pub senders: [Account; NUM_ACC],
}

const WASM_COMPILED_PATH: &str =
    "target/debug/wbuild/fil_hierarchical_subnet_actor/fil_hierarchical_subnet_actor.compact.wasm";

// FIXME: This is not being updated with the SCA. We should come up with a way
// to dynamically compile and fetch an up to date WASM for SCA.
const SCA_COMPILED_PATH: &str =
    "tests/harness/fil_actor_hierarchical_sca.wasm";

impl Harness {
    pub fn new() -> Self {
        // Instantiate tester
        // let mut tester = Tester::new(
        let mut tester = Tester::new(
            NetworkVersion::V15,
            StateTreeVersion::V4,
            MemoryBlockstore::default(),
        )
        .unwrap();

        // Get wasm bin
        let wasm_path = env::current_dir()
            .unwrap()
            .join(WASM_COMPILED_PATH)
            .canonicalize()
            .unwrap();
        let wasm_bin = std::fs::read(wasm_path).expect("Unable to read file");
        let state_cid = tester.set_state(&State::default()).unwrap();

        // initialize a list of senders
        let senders: [Account; NUM_ACC] = tester.create_accounts().unwrap();

        // Set actor
        let actor_address = Address::new_id(TEST_ACTOR_ADDR);
        // Initialize test init address in list of accounts.
        tester
            .make_id_account(TEST_INIT_ACTOR_ADDR, TokenAmount::from(10_u64.pow(18)))
            .unwrap();

        // Instantiate actor from bin
        tester
            .set_actor_from_bin(&wasm_bin, state_cid.clone(), actor_address, BigInt::zero())
            .unwrap();

        // Instantiate machine
        tester.instantiate_machine().unwrap();
        Self {
            tester,
            actor_address,
            senders,
        }
    }

    pub fn state_tree(&self) -> &StateTree<impl Blockstore> {
        let exec = self.tester.executor.as_ref().unwrap();
        exec.state_tree()
    }

    pub fn get_state(&self) -> State {
        let state_tree = self.state_tree();
        let store = state_tree.store();
        let st_cid = state_tree
            .get_actor(&self.actor_address)
            .unwrap()
            .unwrap()
            .state;
        let st = store.get(&st_cid).unwrap().unwrap();
        RawBytes::deserialize(&RawBytes::from(st)).unwrap()
    }

    pub fn store(&self) -> &impl Blockstore {
        self.state_tree().store()
    }

    pub fn constructor(&mut self, params: ConstructParams) {
        let message = Message {
            from: Address::new_id(TEST_INIT_ACTOR_ADDR), // INIT_ACTOR_ADDR
            to: self.actor_address,
            gas_limit: 1000000000,
            method_num: 1,
            params: RawBytes::serialize(params.clone()).unwrap(),
            ..Message::default()
        };

        let res = self
            .tester
            .executor
            .as_mut()
            .unwrap()
            .execute_message(message, ApplyKind::Explicit, 100)
            .unwrap();

        assert_eq!(
            ExitCode::from(res.msg_receipt.exit_code.value()),
            ExitCode::OK
        );

        // check init state
        let sst = self.get_state();
        let store = self.store();
        assert_eq!(sst.name, params.name);
        assert_eq!(sst.parent_id, params.parent);
        assert_eq!(sst.consensus, params.consensus);
        assert_eq!(sst.check_period, params.check_period);
        assert_eq!(sst.min_validator_stake, params.min_validator_stake);
        assert_eq!(sst.genesis, params.genesis);
        assert_eq!(sst.status, Status::Instantiated);
        assert_eq!(sst.total_stake, TokenAmount::zero());
        verify_empty_map(store, sst.stake);
        verify_empty_map(store, sst.checkpoints);
        verify_empty_map(store, sst.window_checks);
    }

    pub fn join(&mut self, sender: Address, value: TokenAmount) {
        let message = Message {
            from: sender,
            to: self.actor_address,
            gas_limit: 1000000000,
            method_num: 2,
            params: RawBytes::default(),
            value: value,
            ..Message::default()
        };

        let res = self
            .tester
            .executor
            .as_mut()
            .unwrap()
            .execute_message(message, ApplyKind::Explicit, 100)
            .unwrap();

        match res.failure_info {
            Some(err) => println!(">>>>: {}", err),
            None => {},
        };

        assert_eq!(
            ExitCode::from(res.msg_receipt.exit_code.value()),
            ExitCode::OK
        );
    }

    pub fn verify_stake(&self, st: &State, addr: Address, expect: TokenAmount){
        let store = self.store();
        let bt = make_map_with_root::<_, BigIntDe>(&st.stake, store).unwrap();
        let stake = get_stake(&bt, &addr).unwrap();
        assert_eq!(stake, expect);
            
    }
}

pub fn verify_empty_map<BS: Blockstore>(store: &BS, key: Cid) {
    let map = make_map_with_root::<_, BigIntDe>(&key, store).unwrap();
    map.for_each(|_key, _val| panic!("expected no keys"))
        .unwrap();
}
