//! `FipsCashuResponder` — the host-side FIPS responder for Cashu RPC
//! (`docs/FIPS-INTEGRATION-ADR.md` gate-3 composition, micronuts #47 item 1).
//!
//! ```text
//! FSP datagram -> microfips_service::decode_request
//!              -> FipsCashuResponder (microfips ServiceHandler)
//!              -> delegates to micronuts_fips_bridge::CashuRpcServiceAdapter<M>
//!              -> encode_response -> (segmented) FSP datagrams back
//! ```
//!
//! Because the FMP frame cap (2048 B WiFi / 768 B D0WD L2CAP) is smaller
//! than real mint replies, [`respond_segments`] splits the encoded response
//! envelope across datagrams using the [`frag`] codec (micronuts #47 item 4
//! sizing verdict: segmentation is required; RPC-level paging is only an
//! optimization).
//!
//! Authorization note (ADR gate 2, micronuts #57): this responder answers
//! whatever FSP session delivers datagrams to it. Since the microfips
//! PeerContext lift (#198) the session peer is OBSERVABLE — `on_peer`
//! records it on the adapter before each request — but per-peer policy
//! still lives with the wiring (daemon-side `peers.allow` per ADR v1).

pub mod frag;

use cashu_core_lite::rpc::MintService;
use microfips_service::{
    ContentType, ServiceError, ServiceMethod, ServiceReply, ServiceRequest, ServiceStatus,
};
use micronuts_fips_bridge::{
    CashuRpcServiceAdapter, ContentType as BridgeContentType, ServiceError as BridgeServiceError,
    ServiceHandler as _, ServiceMethod as BridgeServiceMethod,
    ServiceStatus as BridgeServiceStatus,
};

/// FMP application frame cap on WiFi/UDP transports (`FRAME_CAP` in the
/// microfips WiFi/L2CAP builds).
pub const WIFI_FRAME_CAP: usize = 2048;

/// FMP application frame cap on the ESP32-D0WD L2CAP build.
pub const L2CAP_FRAME_CAP: usize = 768;

/// A microfips [`microfips_service::ServiceHandler`] that terminates the
/// service envelope and proxies Cashu RPC to a mint service.
pub struct FipsCashuResponder<M> {
    adapter: CashuRpcServiceAdapter<M>,
}

impl<M: MintService> FipsCashuResponder<M> {
    pub fn new(mint: M) -> Self {
        Self {
            adapter: CashuRpcServiceAdapter::new(mint),
        }
    }

    /// Access the wrapped Cashu RPC adapter.
    pub fn adapter(&self) -> &CashuRpcServiceAdapter<M> {
        &self.adapter
    }

    /// Mutably access the wrapped Cashu RPC adapter.
    pub fn adapter_mut(&mut self) -> &mut CashuRpcServiceAdapter<M> {
        &mut self.adapter
    }
}

fn method_to_bridge(method: ServiceMethod) -> BridgeServiceMethod {
    match method {
        ServiceMethod::Get => BridgeServiceMethod::Get,
        ServiceMethod::Post => BridgeServiceMethod::Post,
        ServiceMethod::Put => BridgeServiceMethod::Put,
        ServiceMethod::Delete => BridgeServiceMethod::Delete,
    }
}

fn content_type_from_bridge(content_type: BridgeContentType) -> ContentType {
    match content_type {
        BridgeContentType::Binary => ContentType::Binary,
        BridgeContentType::Json => ContentType::Json,
        BridgeContentType::Text => ContentType::Text,
    }
}

fn status_from_bridge(status: BridgeServiceStatus) -> ServiceStatus {
    match status {
        BridgeServiceStatus::Ok => ServiceStatus::OK,
        BridgeServiceStatus::Created => ServiceStatus::CREATED,
        BridgeServiceStatus::BadRequest => ServiceStatus::BAD_REQUEST,
        BridgeServiceStatus::NotFound => ServiceStatus::NOT_FOUND,
        BridgeServiceStatus::MethodNotAllowed => ServiceStatus::METHOD_NOT_ALLOWED,
        BridgeServiceStatus::PayloadTooLarge => ServiceStatus::PAYLOAD_TOO_LARGE,
        BridgeServiceStatus::InternalError => ServiceStatus::INTERNAL_ERROR,
    }
}

/// Writes a bridge handler error as an OK-shaped text reply so the mapped
/// HTTP status (incl. 500, which the messageless microfips `ServiceError`
/// cannot express) survives to the wallet.
fn write_error_reply(
    err: BridgeServiceError,
    response: &mut [u8],
) -> Result<ServiceReply, ServiceError> {
    let message = err.message.as_bytes();
    if response.len() < message.len() {
        return Err(ServiceError::BufferTooSmall);
    }
    response[..message.len()].copy_from_slice(message);
    Ok(ServiceReply {
        status: status_from_bridge(err.status),
        content_type: ContentType::Text,
        body_len: message.len(),
    })
}

impl<M: MintService> microfips_service::ServiceHandler for FipsCashuResponder<M> {
    fn on_peer(&mut self, peer: &microfips_service::PeerContext) {
        self.adapter.observe_peer(
            &peer.link_pubkey,
            peer.src_addr.as_ref().map(|a| a.as_bytes()),
        );
    }

    fn handle(
        &mut self,
        request: ServiceRequest<'_>,
        response: &mut [u8],
    ) -> Result<ServiceReply, ServiceError> {
        let bridge_request = micronuts_fips_bridge::ServiceRequest {
            method: method_to_bridge(request.method),
            route: request.route,
            payload: request.payload,
        };
        match self.adapter.handle(bridge_request, response) {
            Ok(reply) => Ok(ServiceReply {
                status: status_from_bridge(reply.status),
                content_type: content_type_from_bridge(reply.content_type),
                body_len: reply.body_len,
            }),
            Err(err) => write_error_reply(err, response),
        }
    }
}

/// Handles one inbound service-envelope datagram and returns the encoded
/// response envelope split into wire segments of at most `frame_cap` bytes
/// each (usually one — segmentation engages only above the cap).
///
/// `msg_id` groups the segments of one response; rotate it per response on
/// the same session so a reassembler can tell interleaves apart. `scratch`
/// backs the encoded response envelope and must outlive the returned data.
pub fn respond_segments<H: microfips_service::ServiceHandler>(
    handler: &mut H,
    request_bytes: &[u8],
    frame_cap: usize,
    msg_id: u8,
    scratch: &mut [u8],
) -> Result<Vec<Vec<u8>>, ServiceError> {
    let len = microfips_service::dispatch_request(handler, request_bytes, scratch)?;
    let mut segments = Vec::with_capacity(frag::segment_count(len, frame_cap));
    frag::for_each_segment(&scratch[..len], frame_cap, msg_id, |header, payload| {
        let mut segment = Vec::with_capacity(frag::SEGMENT_HEADER_LEN + payload.len());
        segment.extend_from_slice(header);
        segment.extend_from_slice(payload);
        segments.push(segment);
    });
    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use microfips_service::{decode_response, encode_request};
    use micronuts_mint::DemoMint;

    fn envelope(method: ServiceMethod, route: &str, payload: &[u8]) -> Vec<u8> {
        let mut buf =
            vec![0u8; microfips_service::SERVICE_REQUEST_HEADER_LEN + route.len() + payload.len()];
        let len = encode_request(method, route, payload, &mut buf).expect("encode request");
        buf.truncate(len);
        buf
    }

    fn dispatch(responder: &mut FipsCashuResponder<DemoMint>, env: &[u8]) -> Vec<u8> {
        let mut scratch = vec![0u8; 16 * 1024];
        let len =
            microfips_service::dispatch_request(responder, env, &mut scratch).expect("dispatch");
        scratch.truncate(len);
        scratch
    }

    #[test]
    fn fips_responder_records_peer_context() {
        let mut responder = FipsCashuResponder::new(DemoMint::new());
        let peer = microfips_service::PeerContext {
            link_pubkey: [0x33; 32],
            src_addr: None,
        };
        microfips_service::ServiceHandler::on_peer(&mut responder, &peer);
        let recorded = responder.adapter().last_peer().expect("recorded");
        assert_eq!(recorded.link_pubkey, [0x33; 32]);
        assert!(recorded.src_addr.is_none());
    }

    #[test]
    fn unknown_route_maps_to_404_envelope() {
        let mut responder = FipsCashuResponder::new(DemoMint::new());
        let reply = dispatch(&mut responder, &envelope(ServiceMethod::Post, "/nope", b""));
        let decoded = decode_response(&reply).expect("decode");
        assert_eq!(decoded.status, ServiceStatus::NOT_FOUND);
        assert_eq!(decoded.body, b"unknown route");
    }

    #[test]
    fn wrong_method_maps_to_405_envelope() {
        let mut responder = FipsCashuResponder::new(DemoMint::new());
        let reply = dispatch(
            &mut responder,
            &envelope(
                ServiceMethod::Get,
                micronuts_fips_bridge::CASHU_RPC_ROUTE,
                b"",
            ),
        );
        let decoded = decode_response(&reply).expect("decode");
        assert_eq!(decoded.status, ServiceStatus::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn malformed_rpc_payload_maps_to_400_envelope() {
        let mut responder = FipsCashuResponder::new(DemoMint::new());
        let reply = dispatch(
            &mut responder,
            &envelope(
                ServiceMethod::Post,
                micronuts_fips_bridge::CASHU_RPC_ROUTE,
                &[0xff, 0x00, 0x7f],
            ),
        );
        let decoded = decode_response(&reply).expect("decode");
        assert_eq!(decoded.status, ServiceStatus::BAD_REQUEST);
    }

    #[test]
    fn respond_segments_single_frame_when_under_cap() {
        let mut responder = FipsCashuResponder::new(DemoMint::new());
        let env = envelope(
            ServiceMethod::Post,
            micronuts_fips_bridge::CASHU_RPC_ROUTE,
            b"\x01", // minimal GetInfo request
        );
        let mut scratch = vec![0u8; 16 * 1024];
        let segments =
            respond_segments(&mut responder, &env, WIFI_FRAME_CAP, 1, &mut scratch).expect("ok");
        assert_eq!(segments.len(), 1);
        assert!(segments[0].len() <= WIFI_FRAME_CAP);
    }

    #[test]
    fn respond_segments_splits_oversized_bodies_and_reassembles() {
        // Handler with a body larger than any transport cap exercises the
        // segmentation path with a real encoded envelope.
        struct BigBody;
        impl microfips_service::ServiceHandler for BigBody {
            fn handle(
                &mut self,
                _request: ServiceRequest<'_>,
                response: &mut [u8],
            ) -> Result<ServiceReply, ServiceError> {
                let body: Vec<u8> = (0..5462u32).map(|i| (i % 251) as u8).collect();
                response[..body.len()].copy_from_slice(&body);
                Ok(ServiceReply {
                    status: ServiceStatus::OK,
                    content_type: ContentType::Binary,
                    body_len: body.len(),
                })
            }
        }

        for cap in [WIFI_FRAME_CAP, L2CAP_FRAME_CAP, 64] {
            let mut handler = BigBody;
            let env = envelope(ServiceMethod::Post, "/x", b"");
            let mut scratch = vec![0u8; 8 + 5462];
            let segments = respond_segments(&mut handler, &env, cap, 9, &mut scratch).expect("ok");
            assert!(
                segments.len() > 1,
                "oversized body must segment at cap {cap}"
            );
            assert!(segments.iter().all(|s| s.len() <= cap));

            let mut reassembler = frag::Reassembler::<8192>::new();
            let mut frame = None;
            for seg in &segments {
                if let Some(completed) = reassembler.push(seg).expect("in order") {
                    frame = Some(completed.to_vec());
                }
            }
            let frame = frame.expect("completed");
            let decoded = decode_response(&frame).expect("decode");
            assert_eq!(decoded.status, ServiceStatus::OK);
            assert_eq!(decoded.body.len(), 5462);
            assert_eq!(decoded.body[0], 0);
            assert_eq!(decoded.body[5461], (5461 % 251) as u8);
        }
    }
}
