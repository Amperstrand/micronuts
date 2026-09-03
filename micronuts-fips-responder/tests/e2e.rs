//! Host e2e for the responder composition (micronuts #47 item 1): a Cashu
//! RPC client speaks through the microfips service envelope, the responder
//! delegates to the real `DemoMint`, and replies cross the segmentation /
//! reassembly codec at transport frame caps before decoding.

use cashu_core_lite::error::CashuError;
use cashu_core_lite::rpc::{RpcByteTransport, RpcMintClient};
use cashu_core_lite::transport::MintClient;
use microfips_service::{decode_response, encode_request, ServiceMethod, ServiceStatus};
use micronuts_fips_bridge::CASHU_RPC_ROUTE;
use micronuts_fips_responder::frag::Reassembler;
use micronuts_fips_responder::{
    respond_segments, FipsCashuResponder, L2CAP_FRAME_CAP, WIFI_FRAME_CAP,
};
use micronuts_mint::DemoMint;

/// Wallet-side transport stand-in: what the sidecar/wallet will do around
/// the envelope — encode request, ship segments, reassemble reply, strip
/// the envelope.
struct EnvelopeTransport {
    responder: FipsCashuResponder<DemoMint>,
    frame_cap: usize,
    msg_id: u8,
    scratch: Vec<u8>,
}

impl EnvelopeTransport {
    fn new(mint: DemoMint, frame_cap: usize) -> Self {
        Self {
            responder: FipsCashuResponder::new(mint),
            frame_cap,
            msg_id: 0,
            scratch: vec![0u8; 64 * 1024],
        }
    }
}

impl RpcByteTransport for EnvelopeTransport {
    fn exchange(&mut self, request: &[u8]) -> Result<Vec<u8>, CashuError> {
        let mut envelope = vec![
            0u8;
            microfips_service::SERVICE_REQUEST_HEADER_LEN
                + CASHU_RPC_ROUTE.len()
                + request.len()
        ];
        let len = encode_request(ServiceMethod::Post, CASHU_RPC_ROUTE, request, &mut envelope)
            .map_err(|_| CashuError::Transport("envelope encode failed".into()))?;

        self.msg_id = self.msg_id.wrapping_add(1);
        let segments = respond_segments(
            &mut self.responder,
            &envelope[..len],
            self.frame_cap,
            self.msg_id,
            &mut self.scratch,
        )
        .map_err(|_| CashuError::Transport("responder dispatch failed".into()))?;
        assert!(
            segments.iter().all(|s| s.len() <= self.frame_cap),
            "every segment must fit the frame cap"
        );

        let mut reassembler = Reassembler::<{ 64 * 1024 }>::new();
        let mut frame = None;
        for segment in &segments {
            if let Some(completed) = reassembler
                .push(segment)
                .map_err(|_| CashuError::Transport("segment stream corrupted".into()))?
            {
                assert!(frame.is_none(), "only one frame per exchange");
                frame = Some(completed.to_vec());
            }
        }
        let frame = frame.ok_or_else(|| CashuError::Transport("no reply frame".into()))?;

        let response = decode_response(&frame)
            .map_err(|_| CashuError::Transport("reply envelope undecodable".into()))?;
        if response.status != ServiceStatus::OK {
            return Err(CashuError::Protocol(
                "responder returned non-OK status".into(),
            ));
        }
        Ok(response.body.to_vec())
    }
}

#[test]
fn get_info_and_get_keys_over_segmented_envelope() {
    for frame_cap in [WIFI_FRAME_CAP, L2CAP_FRAME_CAP] {
        let transport = EnvelopeTransport::new(DemoMint::new(), frame_cap);
        let mut client = RpcMintClient::new(transport);

        let info = client.get_info().expect("get_info over envelope");
        assert_eq!(info.name, "Micronuts Demo Mint");

        let keys = client.get_keys().expect("get_keys over envelope");
        assert!(!keys.keysets.is_empty());
    }
}
