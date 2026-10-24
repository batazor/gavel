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
    Test(WorkloadIdentity),
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

    pub fn test(
        identity: WorkloadIdentity,
        explicitly_enabled: bool,
    ) -> Result<Self, &'static str> {
        if !explicitly_enabled {
            return Err("test workload identity injector is disabled");
        }
        Ok(Self {
            mode: IdentityMode::Test(identity),
        })
    }
}

impl Interceptor for WorkloadIdentityInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let identity = match &self.mode {
            IdentityMode::Test(identity) => *identity,
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
                parse_workload(spiffe_id, spiffe_prefix)?
            }
        };
        request.extensions_mut().insert(identity);
        Ok(request)
    }
}

fn parse_workload(value: &str, prefix: &str) -> Result<WorkloadIdentity, Status> {
    let workload = value
        .strip_prefix(prefix)
        .filter(|value| !value.is_empty() && !value.contains('/'))
        .ok_or_else(|| Status::unauthenticated("workload identity is invalid"))?;
    match workload {
        "authz-bootstrap" | "gavel-authz-bootstrap" => Ok(WorkloadIdentity::Bootstrap),
        "bff" | "gavel-bff" => Ok(WorkloadIdentity::Bff),
        "auction" | "gavel-auction" => Ok(WorkloadIdentity::Auction),
        "catalog" | "gavel-catalog" => Ok(WorkloadIdentity::Catalog),
        "payments" | "gavel-payments" => Ok(WorkloadIdentity::Payments),
        "notify" | "gavel-notify" => Ok(WorkloadIdentity::Notify),
        "processes" | "gavel-processes" => Ok(WorkloadIdentity::Processes),
        _ => Err(Status::unauthenticated("workload identity is unknown")),
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

    fn request(peer: &str, spiffe: Option<&str>) -> Request<()> {
        let mut request = Request::new(());
        request.extensions_mut().insert(TcpConnectInfo {
            local_addr: Some("127.0.0.1:50051".parse().unwrap()),
            remote_addr: Some(peer.parse::<SocketAddr>().unwrap()),
        });
        if let Some(spiffe) = spiffe {
            request
                .metadata_mut()
                .insert(SPIFFE_METADATA, spiffe.parse().unwrap());
        }
        request
    }

    #[test]
    fn production_identity_requires_trusted_peer_and_closed_spiffe_workload() {
        let mut interceptor = WorkloadIdentityInterceptor::production(
            "spiffe://cluster.local/ns/gavel/sa/",
            vec!["10.20.0.0/16".parse().unwrap()],
        )
        .unwrap();
        let accepted = interceptor
            .call(request(
                "10.20.4.8:41234",
                Some("spiffe://cluster.local/ns/gavel/sa/auction"),
            ))
            .unwrap();
        assert_eq!(
            accepted.extensions().get::<WorkloadIdentity>(),
            Some(&WorkloadIdentity::Auction)
        );

        let bootstrap = interceptor
            .call(request(
                "10.20.4.8:41234",
                Some("spiffe://cluster.local/ns/gavel/sa/gavel-authz-bootstrap"),
            ))
            .unwrap();
        assert_eq!(
            bootstrap.extensions().get::<WorkloadIdentity>(),
            Some(&WorkloadIdentity::Bootstrap)
        );

        let deployed_service_account = interceptor
            .call(request(
                "10.20.4.8:41234",
                Some("spiffe://cluster.local/ns/gavel/sa/gavel-auction"),
            ))
            .unwrap();
        assert_eq!(
            deployed_service_account
                .extensions()
                .get::<WorkloadIdentity>(),
            Some(&WorkloadIdentity::Auction)
        );

        for mut rejected in [
            request(
                "10.21.4.8:41234",
                Some("spiffe://cluster.local/ns/gavel/sa/auction"),
            ),
            request("10.20.4.8:41234", None),
            request(
                "10.20.4.8:41234",
                Some("spiffe://cluster.local/ns/gavel/sa/unknown"),
            ),
        ] {
            rejected
                .metadata_mut()
                .insert("x-workload", "auction".parse().unwrap());
            rejected
                .metadata_mut()
                .insert("x-principal-id", "admin".parse().unwrap());
            assert_eq!(
                interceptor.call(rejected).unwrap_err().code(),
                tonic::Code::Unauthenticated
            );
        }
    }

    #[test]
    fn loopback_proxy_is_allowed_but_raw_identity_headers_are_ignored() {
        let mut interceptor =
            WorkloadIdentityInterceptor::production("spiffe://cluster.local/ns/gavel/sa/", vec![])
                .unwrap();
        let mut raw_only = request("127.0.0.1:1234", None);
        raw_only
            .metadata_mut()
            .insert("x-workload", "bff".parse().unwrap());
        assert!(interceptor.call(raw_only).is_err());
    }

    #[test]
    fn test_injector_is_fail_closed_unless_explicitly_enabled() {
        assert!(WorkloadIdentityInterceptor::test(WorkloadIdentity::Bff, false).is_err());
        let mut enabled = WorkloadIdentityInterceptor::test(WorkloadIdentity::Bff, true).unwrap();
        let injected = enabled.call(Request::new(())).unwrap();
        assert_eq!(
            injected.extensions().get::<WorkloadIdentity>(),
            Some(&WorkloadIdentity::Bff)
        );
    }

    #[test]
    fn cidr_matching_handles_ipv4_and_ipv6_boundaries() {
        let v4: TrustedProxyCidr = "10.20.0.0/16".parse().unwrap();
        assert!(v4.contains("10.20.255.1".parse().unwrap()));
        assert!(!v4.contains("10.21.0.1".parse().unwrap()));
        let v6: TrustedProxyCidr = "fd00:1234::/32".parse().unwrap();
        assert!(v6.contains("fd00:1234::42".parse().unwrap()));
        assert!(!v6.contains("fd00:5678::42".parse().unwrap()));
    }
}
