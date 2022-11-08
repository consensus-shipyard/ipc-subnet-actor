use fvm_ipld_encoding::tuple::{Deserialize_tuple, Serialize_tuple};
use fvm_ipld_encoding::RawBytes;
use fvm_shared::address::Address;
use fvm_shared::econ::TokenAmount;
use fvm_shared::MethodNum;

#[derive(Serialize_tuple, Deserialize_tuple, Clone, Debug)]
pub struct ExpectedSend {
    pub to: Address,
    pub method: MethodNum,
    pub params: RawBytes,
    pub value: TokenAmount,
}

// /// Serializes a structure as a CBOR vector of bytes, returning a serialization error on failure.
// /// `desc` is a noun phrase for the object being serialized, included in any error message.
// pub fn serialize_vec<T>(value: &T, desc: &str) -> anyhow::Result<Vec<u8>>
// where
//     T: ser::Serialize + ?Sized,
// {
//     to_vec(value).map_err(|e| anyhow!(format!("failed to serialize {}: {}", desc, e)))
// }
//
// /// Serializes a structure as CBOR bytes, returning a serialization error on failure.
// /// `desc` is a noun phrase for the object being serialized, included in any error message.
// pub fn serialize<T>(value: &T, desc: &str) -> anyhow::Result<RawBytes>
// where
//     T: ser::Serialize + ?Sized,
// {
//     Ok(RawBytes::new(serialize_vec(value, desc)?))
// }

// /// Deserialises CBOR-encoded bytes as a structure, returning a serialization error on failure.
// /// `desc` is a noun phrase for the object being deserialized, included in any error message.
// pub fn deserialize<O: de::DeserializeOwned>(v: &RawBytes, desc: &str) -> anyhow::Result<O> {
//     v.deserialize()
//         .map_err(|e| anyhow!(format!("failed to deserialize {}: {}", desc, e)))
// }

pub(crate) struct CrossActorPayload {
    pub to: Address,
    pub method: MethodNum,
    pub params: RawBytes,
    pub value: TokenAmount,
}

impl CrossActorPayload {
    pub fn new(to: Address, method: MethodNum, params: RawBytes, value: TokenAmount) -> Self {
        Self {
            to,
            method,
            params,
            value,
        }
    }
}
