use cashu_core_lite::error::CashuError;
use cashu_core_lite::rpc::{MintRpcHandler, MintService, RpcByteTransport};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceMethod {
    Get,
    Post,
    Put,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Binary,
    Json,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Ok,
    Created,
    BadRequest,
    NotFound,
    MethodNotAllowed,
    PayloadTooLarge,
    InternalError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceError {
    pub status: ServiceStatus,
    pub message: &'static str,
}

impl ServiceError {
    pub const fn bad_request(message: &'static str) -> Self {
        Self {
            status: ServiceStatus::BadRequest,
            message,
        }
    }

    pub const fn internal_error(message: &'static str) -> Self {
        Self {
            status: ServiceStatus::InternalError,
            message,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceRequest<'a> {
    pub method: ServiceMethod,
    pub route: &'a str,
    pub payload: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceReply {
    pub status: ServiceStatus,
    pub content_type: ContentType,
    pub body_len: usize,
}

pub trait ServiceHandler {
    fn handle(
        &mut self,
        request: ServiceRequest<'_>,
        response: &mut [u8],
    ) -> Result<ServiceReply, ServiceError>;
}

pub const CASHU_RPC_ROUTE: &str = "/rpc/mint";

pub struct CashuRpcServiceAdapter<S> {
    handler: MintRpcHandler<S>,
}

impl<S> CashuRpcServiceAdapter<S> {
    pub fn new(service: S) -> Self {
        Self {
            handler: MintRpcHandler::new(service),
        }
    }

    pub fn handler(&self) -> &MintRpcHandler<S> {
        &self.handler
    }

    pub fn handler_mut(&mut self) -> &mut MintRpcHandler<S> {
        &mut self.handler
    }
}

impl<S: MintService> ServiceHandler for CashuRpcServiceAdapter<S> {
    fn handle(
        &mut self,
        request: ServiceRequest<'_>,
        response: &mut [u8],
    ) -> Result<ServiceReply, ServiceError> {
        if request.method != ServiceMethod::Post {
            return Err(ServiceError {
                status: ServiceStatus::MethodNotAllowed,
                message: "cashu rpc requires POST",
            });
        }

        if request.route != CASHU_RPC_ROUTE {
            return Err(ServiceError {
                status: ServiceStatus::NotFound,
                message: "unknown route",
            });
        }

        let rpc_response = self
            .handler
            .handle_bytes(request.payload)
            .map_err(cashu_error_to_service_error)?;

        if rpc_response.len() > response.len() {
            return Err(ServiceError {
                status: ServiceStatus::PayloadTooLarge,
                message: "response buffer too small",
            });
        }

        response[..rpc_response.len()].copy_from_slice(&rpc_response);
        Ok(ServiceReply {
            status: ServiceStatus::Ok,
            content_type: ContentType::Binary,
            body_len: rpc_response.len(),
        })
    }
}

pub struct ServiceHandlerTransport<H> {
    handler: H,
    response_buffer: Vec<u8>,
}

impl<H> ServiceHandlerTransport<H> {
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            response_buffer: vec![0u8; 16 * 1024],
        }
    }

    pub fn with_response_capacity(handler: H, capacity: usize) -> Self {
        Self {
            handler,
            response_buffer: vec![0u8; capacity],
        }
    }

    pub fn handler(&self) -> &H {
        &self.handler
    }

    pub fn handler_mut(&mut self) -> &mut H {
        &mut self.handler
    }
}

impl<H: ServiceHandler> RpcByteTransport for ServiceHandlerTransport<H> {
    fn exchange(&mut self, request: &[u8]) -> Result<Vec<u8>, CashuError> {
        let reply = self
            .handler
            .handle(
                ServiceRequest {
                    method: ServiceMethod::Post,
                    route: CASHU_RPC_ROUTE,
                    payload: request,
                },
                &mut self.response_buffer,
            )
            .map_err(service_error_to_cashu_error)?;

        if reply.status != ServiceStatus::Ok {
            return Err(CashuError::Protocol(
                "service handler returned non-OK status".into(),
            ));
        }

        if reply.body_len > self.response_buffer.len() {
            return Err(CashuError::Protocol(
                "service reply body length exceeds buffer".into(),
            ));
        }

        Ok(self.response_buffer[..reply.body_len].to_vec())
    }
}

fn cashu_error_to_service_error(error: CashuError) -> ServiceError {
    match error {
        CashuError::Protocol(_) => ServiceError::bad_request("rpc protocol error"),
        CashuError::Transport(_) => ServiceError::internal_error("rpc transport error"),
        _ => ServiceError::internal_error("mint handler error"),
    }
}

fn service_error_to_cashu_error(error: ServiceError) -> CashuError {
    match error.status {
        ServiceStatus::BadRequest => CashuError::Protocol(error.message.into()),
        ServiceStatus::NotFound => CashuError::Protocol(error.message.into()),
        ServiceStatus::MethodNotAllowed => CashuError::Protocol(error.message.into()),
        ServiceStatus::PayloadTooLarge => CashuError::Transport(error.message.into()),
        ServiceStatus::InternalError => CashuError::Transport(error.message.into()),
        ServiceStatus::Ok | ServiceStatus::Created => CashuError::Unknown(error.message.into()),
    }
}

#[cfg(test)]
mod tests {
    use cashu_core_lite::error::CashuError;
    use cashu_core_lite::rpc::RpcMintClient;
    use cashu_core_lite::transport::MintClient;
    use micronuts_mint::DemoMint;

    use super::{CashuRpcServiceAdapter, ServiceHandlerTransport};

    #[test]
    fn rpc_roundtrip_works_over_service_handler_transport() {
        let service = CashuRpcServiceAdapter::new(DemoMint::new());
        let transport = ServiceHandlerTransport::new(service);
        let mut client = RpcMintClient::new(transport);

        let info = client.get_info().expect("get_info should succeed");
        assert_eq!(info.name, "Micronuts Demo Mint");

        let keys = client.get_keys().expect("get_keys should succeed");
        assert!(!keys.keysets.is_empty());
    }

    #[test]
    fn payload_too_large_maps_to_transport_error() {
        let service = CashuRpcServiceAdapter::new(DemoMint::new());
        let transport = ServiceHandlerTransport::with_response_capacity(service, 8);
        let mut client = RpcMintClient::new(transport);

        let err = client.get_info().expect_err("small buffer should fail");
        assert!(matches!(err, CashuError::Transport(_)));
    }
}
