#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

pub mod crypto;
pub mod error;
pub mod keypair;
pub mod nuts;
pub mod rpc;
pub mod token;
pub mod transport;
pub mod wallet;

pub use crypto::{
    blind_message, hash_to_curve, sign_message, unblind_signature, verify_signature,
    BlindedMessage, HashToCurveError,
};
pub use error::CashuError;
pub use keypair::{PublicKey, SecretKey};
pub use rpc::{
    decode_rpc_request, decode_rpc_response, encode_rpc_request, encode_rpc_response, MeltQuoteLookupRequest,
    MintQuoteLookupRequest, MintRpcHandler, MintRpcMethod, MintRpcPayload, MintRpcRequest,
    MintRpcResponse, MintRpcResult, MintService, RpcByteTransport, RpcMintClient,
};
pub use token::{decode_token, encode_token, Proof, TokenV4, TokenV4Token};
pub use transport::MintClient;
pub use wallet::Wallet;
