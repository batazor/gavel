#![allow(clippy::result_large_err)]

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

use tonic::{Request, Status, service::Interceptor};

use crate::application::WorkloadIdentity;

const SPIFFE_METADATA: &str = "x-gavel-spiffe-id";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustedProxyCidr {
    network: IpAddr,
    prefix: u8,
}

impl FromStr for TrustedProxyCidr {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (address, prefix) = value
            .split_once('/')
            .ok_or("trusted proxy must be a CIDR")?;
        let network: IpAddr = address
            .parse()
            .map_err(|_| "trusted proxy address is invalid")?;
        let prefix: u8 = prefix
            .parse()
            .map_err(|_| "trusted proxy prefix is invalid")?;
        let maximum = if network.is_ipv4() { 32 } else { 128 };
        if prefix > maximum {
            return Err("trusted proxy prefix is invalid");
        }
        Ok(Self {
            network: mask(network, prefix),
            prefix,
        })
    }
}

impl TrustedProxyCidr {
    pub fn contains(self, address: IpAddr) -> bool {
        address.is_ipv4() == self.network.is_ipv4() && mask(address, self.prefix) == self.network
    }
}

#[derive(Clone, Debug)]
enum IdentityMode {
    Production {
        spiffe_prefix: String,
        trusted_proxies: Vec<TrustedProxyCidr>,
    },
    Test,
}

#[derive(Clone, Debug)]
pub struct WorkloadIdentityInterceptor {
    mode: IdentityMode,
}

impl WorkloadIdentityInterceptor {
    pub fn production(
        spiffe_prefix: impl Into<String>,
        trusted_proxies: Vec<TrustedProxyCidr>,
    ) -> Result<Self, &'static str> {
        let spiffe_prefix = spiffe_prefix.into();
        if !spiffe_prefix.starts_with("spiffe://") || !spiffe_prefix.ends_with('/') {
            return Err("SPIFFE prefix must be an absolute prefix ending in slash");
        }
        Ok(Self {
            mode: IdentityMode::Production {
                spiffe_prefix,
                trusted_proxies,
            },
        })
    }

    pub fn test_processes(explicitly_enabled: bool) -> Result<Self, &'static str> {
        if !explicitly_enabled {
            return Err("test workload identity injector is disabled");
        }
        Ok(Self {
            mode: IdentityMode::Test,
        })
    }
}

impl Interceptor for WorkloadIdentityInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let identity = match &self.mode {
            IdentityMode::Test => WorkloadIdentity::Processes,
            IdentityMode::Production {
                spiffe_prefix,
                trusted_proxies,
            } => {
                let peer = request
                    .remote_addr()
                    .map(|address| address.ip())
                    .ok_or_else(|| Status::unauthenticated("trusted proxy is required"))?;
                if !peer.is_loopback()
                    && !trusted_proxies.iter().any(|network| network.contains(peer))
                {
                    return Err(Status::unauthenticated("trusted proxy is required"));
                }
                let spiffe_id = request
                    .metadata()
                    .get(SPIFFE_METADATA)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| Status::unauthenticated("workload identity is required"))?;
                parse_processes(spiffe_id, spiffe_prefix)?
            }
        };
        request.extensions_mut().insert(identity);
        Ok(request)
    }
}

fn parse_processes(value: &str, prefix: &str) -> Result<WorkloadIdentity, Status> {
    let workload = value
        .strip_prefix(prefix)
        .filter(|value| !value.is_empty() && !value.contains('/'))
        .ok_or_else(|| Status::unauthenticated("workload identity is invalid"))?;
    match workload {
        "processes" | "gavel-processes" => Ok(WorkloadIdentity::Processes),
        _ => Err(Status::permission_denied(
            "processes workload identity is required",
        )),
    }
}

fn mask(address: IpAddr, prefix: u8) -> IpAddr {
    match address {
        IpAddr::V4(address) => {
            let bits = u32::from(address);
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            IpAddr::V4(Ipv4Addr::from(bits & mask))
        }
        IpAddr::V6(address) => {
            let bits = u128::from(address);
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            IpAddr::V6(Ipv6Addr::from(bits & mask))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tonic::transport::server::TcpConnectInfo;

    use super::*;

    fn request(peer: Option<&str>, spiffe: Option<&str>) -> Request<()> {
        let mut request = Request::new(());
        if let Some(peer) = peer {
            request.extensions_mut().insert(TcpConnectInfo {
                local_addr: Some("127.0.0.1:50051".parse().unwrap()),
                remote_addr: Some(peer.parse::<SocketAddr>().unwrap()),
            });
        }
        if let Some(spiffe) = spiffe {
            request
                .metadata_mut()
                .insert(SPIFFE_METADATA, spiffe.parse().unwrap());
        }
        request
    }

    #[test]
    fn production_accepts_only_processes_from_trusted_proxy() {
        let cidr = "10.20.0.0/16".parse().unwrap();
        let interceptor = WorkloadIdentityInterceptor::production(
            "spiffe://cluster.local/ns/gavel/sa/",
            vec![cidr],
        )
        .unwrap();
        let mut trusted = interceptor.clone();
        let accepted = trusted
            .call(request(
                Some("10.20.1.3:443"),
                Some("spiffe://cluster.local/ns/gavel/sa/gavel-processes"),
            ))
            .unwrap();
        assert_eq!(
            accepted.extensions().get::<WorkloadIdentity>(),
            Some(&WorkloadIdentity::Processes)
        );
        for rejected in [
            request(
                Some("10.21.1.3:443"),
                Some("spiffe://cluster.local/ns/gavel/sa/gavel-processes"),
            ),
            request(
                Some("10.20.1.3:443"),
                Some("spiffe://cluster.local/ns/gavel/sa/gavel-auction"),
            ),
            request(Some("10.20.1.3:443"), None),
            request(
                None,
                Some("spiffe://cluster.local/ns/gavel/sa/gavel-processes"),
            ),
        ] {
            assert!(interceptor.clone().call(rejected).is_err());
        }
    }

    #[test]
    fn test_injector_is_explicit_and_production_has_no_insecure_default() {
        assert!(WorkloadIdentityInterceptor::test_processes(false).is_err());
        let mut test = WorkloadIdentityInterceptor::test_processes(true).unwrap();
        assert_eq!(
            test.call(Request::new(()))
                .unwrap()
                .extensions()
                .get::<WorkloadIdentity>(),
            Some(&WorkloadIdentity::Processes)
        );
        assert!(WorkloadIdentityInterceptor::production("", vec![]).is_err());
    }
}
