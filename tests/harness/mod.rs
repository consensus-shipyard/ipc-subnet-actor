// use std::collections::HashMap;
// use std::env;
// use std::str::FromStr;
//
// use cid::Cid;
// use fil_actor_hierarchical_sca::Checkpoint;
// use fil_actor_hierarchical_sca::FundParams;
// use fil_hierarchical_subnet_actor::state::get_checkpoint;
// use fil_hierarchical_subnet_actor::state::get_votes;
// use fil_hierarchical_subnet_actor::types::Votes;
// use fvm::executor::ApplyKind;
// use fvm::executor::Executor;
// use fvm::machine::Machine;
// use fvm::state_tree::StateTree;
// use fvm_integration_tests::tester::{Account, Tester};
// use fvm_ipld_blockstore::{Blockstore, MemoryBlockstore};
// use fvm_ipld_encoding::RawBytes;
// use fvm_shared::address::Address;
// use fvm_shared::bigint::bigint_ser::BigIntDe;
// use fvm_shared::bigint::BigInt;
// use fvm_shared::clock::ChainEpoch;
// use fvm_shared::crypto::signature::Signature;
// use fvm_shared::crypto::signature::SECP_SIG_LEN;
// use fvm_shared::econ::TokenAmount;
// use fvm_shared::error::ExitCode;
// use fvm_shared::message::Message;
// use fvm_shared::state::StateTreeVersion;
// use fvm_shared::version::NetworkVersion;
// use fvm_shared::ActorID;
// use fvm_shared::MethodNum;
// use libsecp256k1::{sign, Message as SecpMsg};
// use num_traits::Zero;
//
// use fil_hierarchical_subnet_actor::blockstore::make_map_with_root;
// use fil_hierarchical_subnet_actor::state::{get_stake, State};
// use fil_hierarchical_subnet_actor::types::{ConstructParams, JoinParams, Status};
//
// pub const TEST_INIT_ACTOR_ADDR: ActorID = 339;
// pub const TEST_ACTOR_ADDR: ActorID = 9999;
// pub const NUM_ACC: usize = 5;
//
// pub struct Harness {
//     pub tester: Tester<MemoryBlockstore>,
//     pub actor_address: Address,
//     pub senders: SendersMap,
//     exp_msg_index: usize,
// }
//
// pub struct Sender {
//     pub acc: Account,
//     pub seq: u64,
// }
// impl Sender {
//     pub fn sign_checkpoint(&self, ch: &mut Checkpoint) {
//         let hash: [u8; 32] = blake2b_simd::Params::new()
//             .hash_length(32)
//             .to_state()
//             .update(&ch.cid().to_bytes())
//             .finalize()
//             .as_bytes()
//             .try_into()
//             .expect("fixed array size");
//
//         // Generate signature
//         let priv_key = self.acc.2;
//         let (sig, recovery_id) = sign(&SecpMsg::parse(&hash), &priv_key);
//         let mut signature = [0; SECP_SIG_LEN];
//         signature[..64].copy_from_slice(&sig.serialize());
//         signature[64] = recovery_id.serialize();
//
//         ch.set_signature(
//             RawBytes::serialize::<Signature>(Signature::new_secp256k1(signature.to_vec()).into())
//                 .unwrap()
//                 .to_vec(),
//         );
//     }
// }
//
// pub struct SendersMap {
//     pub m: HashMap<Address, Sender>,
// }
//
// impl SendersMap {
//     pub fn new_from_accs(senders_vec: Vec<Account>) -> Self {
//         let mut out = SendersMap { m: HashMap::new() };
//         for s in &senders_vec {
//             out.m.insert(
//                 Address::new_id(s.0),
//                 Sender {
//                     acc: s.clone(),
//                     seq: 0,
//                 },
//             );
//         }
//         out
//     }
//
//     pub fn add_sequence(&mut self, addr: &Address) {
//         if let Some(k) = self.m.get_mut(addr) {
//             k.seq += 1;
//         }
//     }
//
//     pub fn get_sequence(&self, addr: &Address) -> u64 {
//         self.m.get(addr).unwrap().seq
//     }
//
//     pub fn get_sender_by_index(&self, index: usize) -> Option<Address> {
//         if index >= self.m.len() {
//             return None;
//         }
//         let mut i = 0;
//         for (k, _) in self.m.iter() {
//             if index == i {
//                 return Some(*k);
//             }
//             i += 1;
//         }
//         None
//     }
//
//     pub fn get_sender_by_addr(&self, addr: &Address) -> &Sender {
//         self.m.get(&addr).unwrap()
//     }
// }
//
// const WASM_COMPILED_PATH: &str =
//     "target/debug/wbuild/fil_hierarchical_subnet_actor/fil_hierarchical_subnet_actor.compact.wasm";
//
// impl Harness {
//     pub fn new() -> Self {
//         // Instantiate tester
//         // let mut tester = Tester::new(
//         let mut tester = Tester::new(
//             NetworkVersion::V15,
//             StateTreeVersion::V4,
//             MemoryBlockstore::default(),
//         )
//         .unwrap();
//
//         // Get wasm bin
//         let wasm_path = env::current_dir()
//             .unwrap()
//             .join(WASM_COMPILED_PATH)
//             .canonicalize()
//             .unwrap();
//         let wasm_bin = std::fs::read(wasm_path).expect("Unable to read file");
//         let state_cid = tester.set_state(&State::default()).unwrap();
//
//         // initialize a list of senders
//         let senders_vec: [Account; NUM_ACC] = tester.create_accounts().unwrap();
//         let senders = SendersMap::new_from_accs(senders_vec.into());
//
//         // Set actor
//         let actor_address = Address::new_id(TEST_ACTOR_ADDR);
//         // Initialize test init address in list of accounts.
//         tester
//             .make_id_account(TEST_INIT_ACTOR_ADDR, TokenAmount::from(10_u64.pow(18)))
//             .unwrap();
//
//         // Instantiate actor from bin
//         tester
//             .set_actor_from_bin(&wasm_bin, state_cid.clone(), actor_address, BigInt::zero())
//             .unwrap();
//
//         // Instantiate machine
//         tester.instantiate_machine().unwrap();
//         Self {
//             tester,
//             actor_address,
//             senders,
//             exp_msg_index: 0,
//         }
//     }
//
//     pub fn state_tree(&self) -> &StateTree<impl Blockstore> {
//         let exec = self.tester.executor.as_ref().unwrap();
//         exec.state_tree()
//     }
//
//     pub fn get_state(&self) -> State {
//         let state_tree = self.state_tree();
//         let store = state_tree.store();
//         let st_cid = state_tree
//             .get_actor(&self.actor_address)
//             .unwrap()
//             .unwrap()
//             .state;
//         let st = store.get(&st_cid).unwrap().unwrap();
//         RawBytes::deserialize(&RawBytes::from(st)).unwrap()
//     }
//
//     pub fn store(&self) -> &impl Blockstore {
//         self.state_tree().store()
//     }
//
//     pub fn constructor(&mut self, params: ConstructParams) {
//         let message = Message {
//             from: Address::new_id(TEST_INIT_ACTOR_ADDR), // INIT_ACTOR_ADDR
//             to: self.actor_address,
//             gas_limit: 1000000000,
//             method_num: 1,
//             params: RawBytes::serialize(params.clone()).unwrap(),
//             ..Message::default()
//         };
//
//         let res = self
//             .tester
//             .executor
//             .as_mut()
//             .unwrap()
//             .execute_message(message, ApplyKind::Explicit, 100)
//             .unwrap();
//
//         assert_eq!(
//             ExitCode::from(res.msg_receipt.exit_code.value()),
//             ExitCode::OK
//         );
//
//         // check init state
//         let sst = self.get_state();
//         let store = self.store();
//         assert_eq!(sst.name, params.name);
//         assert_eq!(sst.parent_id, params.parent);
//         assert_eq!(sst.consensus, params.consensus);
//         assert_eq!(sst.finality_threshold, params.finality_threshold);
//         assert_eq!(sst.check_period, params.check_period);
//         assert_eq!(sst.min_validator_stake, params.min_validator_stake);
//         assert_eq!(sst.min_validators, params.min_validators);
//         assert_eq!(sst.genesis, params.genesis);
//         assert_eq!(sst.status, Status::Instantiated);
//         assert_eq!(sst.total_stake, TokenAmount::zero());
//         assert_eq!(sst.testing, true);
//         verify_empty_map(store, sst.stake);
//         verify_empty_map(store, sst.checkpoints);
//         verify_empty_map(store, sst.window_checks);
//     }
//
//     pub fn join(&mut self, sender: Address, value: TokenAmount, params: JoinParams) {
//         let message = Message {
//             from: sender,
//             to: self.actor_address,
//             gas_limit: 1000000000,
//             method_num: 2,
//             params: RawBytes::serialize(params.clone()).unwrap(),
//             value,
//             sequence: self.senders.get_sequence(&sender),
//             ..Message::default()
//         };
//         self.senders.add_sequence(&sender);
//
//         let res = self
//             .tester
//             .executor
//             .as_mut()
//             .unwrap()
//             .execute_message(message, ApplyKind::Explicit, 100)
//             .unwrap();
//
//         match res.failure_info {
//             Some(err) => println!("Failure traces: {}", err),
//             None => {}
//         };
//
//         assert_eq!(
//             ExitCode::from(res.msg_receipt.exit_code.value()),
//             ExitCode::OK
//         );
//     }
//
//     pub fn leave(&mut self, sender: Address, value: TokenAmount) {
//         let message = Message {
//             from: sender,
//             to: self.actor_address,
//             gas_limit: 1000000000,
//             method_num: 3,
//             params: RawBytes::serialize(FundParams { value }).unwrap(),
//             value: TokenAmount::zero(),
//             sequence: self.senders.get_sequence(&sender),
//             ..Message::default()
//         };
//         self.senders.add_sequence(&sender);
//
//         let res = self
//             .tester
//             .executor
//             .as_mut()
//             .unwrap()
//             .execute_message(message, ApplyKind::Explicit, 100)
//             .unwrap();
//
//         match res.failure_info {
//             Some(err) => println!("Failure traces: {}", err),
//             None => {}
//         };
//
//         assert_eq!(
//             ExitCode::from(res.msg_receipt.exit_code.value()),
//             ExitCode::OK
//         );
//     }
//
//     pub fn kill(&mut self, sender: Address, code: ExitCode) {
//         let message = Message {
//             from: sender,
//             to: self.actor_address,
//             gas_limit: 1000000000,
//             method_num: 4,
//             params: RawBytes::default(),
//             value: TokenAmount::zero(),
//             sequence: self.senders.get_sequence(&sender),
//             ..Message::default()
//         };
//         self.senders.add_sequence(&sender);
//
//         let res = self
//             .tester
//             .executor
//             .as_mut()
//             .unwrap()
//             .execute_message(message, ApplyKind::Explicit, 100)
//             .unwrap();
//
//         match res.failure_info {
//             Some(err) => println!("Failure traces: {}", err),
//             None => {}
//         };
//
//         assert_eq!(ExitCode::from(res.msg_receipt.exit_code.value()), code);
//     }
//
//     pub fn submit_checkpoint<'m>(
//         &mut self,
//         sender: Address,
//         epoch: ChainEpoch,
//         prev_cid: &Cid,
//         code: ExitCode,
//     ) -> Checkpoint {
//         let sub_id = fvm_shared_builtin::address::SubnetID::new(
//             &fvm_shared_builtin::address::SubnetID::from_str("/root").unwrap(),
//             fvm_shared_builtin::address::Address::new_id(TEST_ACTOR_ADDR),
//         );
//         let mut ch = Checkpoint::new(sub_id, epoch);
//         ch.data.prev_check = prev_cid.clone();
//
//         // sender signs checkpoint
//         self.senders
//             .get_sender_by_addr(&sender)
//             .sign_checkpoint(&mut ch);
//
//         let message = Message {
//             from: sender,
//             to: self.actor_address,
//             gas_limit: 1000000000,
//             method_num: 5,
//             params: RawBytes::serialize(ch.clone()).unwrap(),
//             value: TokenAmount::zero(),
//             sequence: self.senders.get_sequence(&sender),
//             ..Message::default()
//         };
//         self.senders.add_sequence(&sender);
//
//         let res = self
//             .tester
//             .executor
//             .as_mut()
//             .unwrap()
//             .execute_message(message, ApplyKind::Explicit, 100)
//             .unwrap();
//
//         match res.failure_info {
//             Some(err) => println!("Failure traces: {}", err),
//             None => {}
//         };
//
//         assert_eq!(ExitCode::from(res.msg_receipt.exit_code.value()), code);
//         ch
//     }
//
//     pub fn verify_stake(&self, st: &State, addr: Address, expect: TokenAmount) {
//         let store = self.store();
//         let bt = make_map_with_root::<_, BigIntDe>(&st.stake, store).unwrap();
//         let stake = get_stake(&bt, &addr).unwrap();
//         assert_eq!(stake, expect);
//     }
//
//     pub fn verify_check_votes(&self, st: &State, cid: &Cid, expect: usize) {
//         let store = self.store();
//         let m = make_map_with_root::<_, Votes>(&st.window_checks, store).unwrap();
//         let votes = get_votes(&m, cid).unwrap();
//         if expect == 0 {
//             assert_eq!(votes, None);
//             return;
//         }
//         assert_eq!(votes.unwrap().validators.len(), expect);
//     }
//
//     pub fn verify_checkpoint(&self, st: &State, epoch: &ChainEpoch, expect: Option<&Checkpoint>) {
//         let store = self.store();
//         let checkpoints = make_map_with_root::<_, Checkpoint>(&st.checkpoints, store).unwrap();
//         assert_eq!(get_checkpoint(&checkpoints, epoch).unwrap(), expect);
//     }
//
//     pub fn expect_send(
//         &mut self,
//         st: &State,
//         to: &Address,
//         method: MethodNum,
//         params: RawBytes,
//         value: TokenAmount,
//     ) {
//         let msg = &st.expected_msg[self.exp_msg_index];
//         assert_eq!(&msg.to, to);
//         assert_eq!(msg.method, method);
//         assert_eq!(msg.params, params);
//         assert_eq!(msg.value, value);
//         self.exp_msg_index += 1;
//     }
// }
//
// pub fn verify_empty_map<BS: Blockstore>(store: &BS, key: Cid) {
//     let map = make_map_with_root::<_, BigIntDe>(&key, store).unwrap();
//     map.for_each(|_key, _val| panic!("expected no keys"))
//         .unwrap();
// }
