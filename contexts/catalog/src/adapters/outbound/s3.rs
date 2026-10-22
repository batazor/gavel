use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use reqwest::{Client, Method, StatusCode, Url, header::HeaderMap};
use sha2::{Digest, Sha256};

use crate::{
    application::{
        DeleteObjectRequest, HeadObjectRequest, ObjectStorage, PortError, PresignPutRequest,
        PresignedPut,
    },
    domain::ActualObjectMetadata,
};

type HmacSha256 = Hmac<Sha256>;
const ALGORITHM: &str = "AWS4-HMAC-SHA256";
const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";
const MAX_PRESIGN_SECONDS: u64 = 7 * 24 * 60 * 60;

#[derive(Clone)]
pub struct S3Config {
    pub endpoint: Url,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    pub session_token: Option<String>,
    pub path_style: bool,
}

#[derive(Clone)]
pub struct S3ObjectStorage {
    client: Client,
    config: S3Config,
}

impl S3ObjectStorage {
    pub fn new(config: S3Config) -> Result<Self, &'static str> {
        validate_config(&config)?;
        let client = Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .map_err(|_| "object storage HTTP client is invalid")?;
        Ok(Self { client, config })
    }

    pub fn with_client(config: S3Config, client: Client) -> Result<Self, &'static str> {
        validate_config(&config)?;
        Ok(Self { client, config })
    }

    pub async fn ensure_bucket(&self, timeout: Duration) -> Result<(), PortError> {
        if timeout.is_zero() {
            return Err(PortError::Internal);
        }
        let response = self
            .signed_url_request(Method::PUT, self.bucket_url()?, timeout)
            .await?;
        if response.status().is_success() || response.status() == StatusCode::CONFLICT {
            Ok(())
        } else {
            Err(map_status(response.status()))
        }
    }

    pub async fn ready(&self, timeout: Duration) -> Result<(), PortError> {
        if timeout.is_zero() {
            return Err(PortError::Internal);
        }
        let response = self
            .signed_url_request(Method::HEAD, self.bucket_url()?, timeout)
            .await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(map_status(response.status()))
        }
    }

    fn object_url(&self, object_key: &str) -> Result<Url, PortError> {
        if object_key.is_empty()
            || object_key.len() > 1_024
            || object_key.starts_with('/')
            || object_key.split('/').any(|segment| segment == "..")
        {
            return Err(PortError::Internal);
        }
        let mut url = self.config.endpoint.clone();
        if self.config.path_style {
            let mut segments = url.path_segments_mut().map_err(|_| PortError::Internal)?;
            segments.pop_if_empty();
            segments.push(&self.config.bucket);
            for segment in object_key.split('/') {
                segments.push(segment);
            }
        } else {
            let host = url.host_str().ok_or(PortError::Internal)?;
            url.set_host(Some(&format!("{}.{}", self.config.bucket, host)))
                .map_err(|_| PortError::Internal)?;
            let mut segments = url.path_segments_mut().map_err(|_| PortError::Internal)?;
            segments.pop_if_empty();
            for segment in object_key.split('/') {
                segments.push(segment);
            }
        }
        Ok(url)
    }

    fn bucket_url(&self) -> Result<Url, PortError> {
        let mut url = self.config.endpoint.clone();
        if self.config.path_style {
            let mut segments = url.path_segments_mut().map_err(|_| PortError::Internal)?;
            segments.pop_if_empty();
            segments.push(&self.config.bucket);
        } else {
            let host = url.host_str().ok_or(PortError::Internal)?;
            url.set_host(Some(&format!("{}.{}", self.config.bucket, host)))
                .map_err(|_| PortError::Internal)?;
        }
        Ok(url)
    }

    fn presign_put_url(
        &self,
        object_key: &str,
        content_type: &str,
        checksum: &str,
        expires_at_millis: u64,
    ) -> Result<Url, PortError> {
        let now = SystemTime::now();
        let now_millis = now
            .duration_since(UNIX_EPOCH)
            .map_err(|_| PortError::Internal)?
            .as_millis();
        let now_millis = u64::try_from(now_millis).map_err(|_| PortError::Internal)?;
        let remaining_millis = expires_at_millis
            .checked_sub(now_millis)
            .ok_or(PortError::Internal)?;
        let expires = remaining_millis.div_ceil(1_000);
        if !(1..=MAX_PRESIGN_SECONDS).contains(&expires) {
            return Err(PortError::Internal);
        }
        let date: DateTime<Utc> = now.into();
        let timestamp = date.format("%Y%m%dT%H%M%SZ").to_string();
        let date_scope = date.format("%Y%m%d").to_string();
        let scope = format!("{date_scope}/{}/s3/aws4_request", self.config.region);
        let mut url = self.object_url(object_key)?;
        let host = authority(&url)?;
        let signed_headers = "content-type;host;x-amz-checksum-sha256";
        let mut query = vec![
            ("X-Amz-Algorithm", ALGORITHM.to_owned()),
            (
                "X-Amz-Credential",
                format!("{}/{}", self.config.access_key, scope),
            ),
            ("X-Amz-Date", timestamp.clone()),
            ("X-Amz-Expires", expires.to_string()),
            ("X-Amz-SignedHeaders", signed_headers.to_owned()),
        ];
        if let Some(token) = &self.config.session_token {
            query.push(("X-Amz-Security-Token", token.clone()));
        }
        let canonical_query_string = canonical_query(&query);
        let canonical_headers = format!(
            "content-type:{}\nhost:{}\nx-amz-checksum-sha256:{}\n",
            normalize_header(content_type),
            host,
            normalize_header(checksum)
        );
        let canonical_request = format!(
            "PUT\n{}\n{}\n{}\n{}\n{}",
            canonical_uri(&url),
            canonical_query_string,
            canonical_headers,
            signed_headers,
            UNSIGNED_PAYLOAD
        );
        let string_to_sign = format!(
            "{ALGORITHM}\n{timestamp}\n{scope}\n{}",
            hex_sha256(canonical_request.as_bytes())
        );
        let signature = signature(
            &self.config.secret_key,
            &date_scope,
            &self.config.region,
            &string_to_sign,
        )?;
        query.push(("X-Amz-Signature", signature));
        url.set_query(Some(&canonical_query(&query)));
        Ok(url)
    }

    async fn signed_request(
        &self,
        method: Method,
        object_key: &str,
        timeout: Duration,
    ) -> Result<reqwest::Response, PortError> {
        let url = self.object_url(object_key)?;
        self.signed_url_request(method, url, timeout).await
    }

    async fn signed_url_request(
        &self,
        method: Method,
        url: Url,
        timeout: Duration,
    ) -> Result<reqwest::Response, PortError> {
        let host = authority(&url)?;
        let now: DateTime<Utc> = SystemTime::now().into();
        let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date_scope = now.format("%Y%m%d").to_string();
        let scope = format!("{date_scope}/{}/s3/aws4_request", self.config.region);
        let mut headers = vec![
            ("host", host),
            ("x-amz-content-sha256", UNSIGNED_PAYLOAD.to_owned()),
            ("x-amz-date", timestamp.clone()),
        ];
        if method == Method::HEAD {
            headers.push(("x-amz-checksum-mode", "ENABLED".to_owned()));
        }
        if let Some(token) = &self.config.session_token {
            headers.push(("x-amz-security-token", token.clone()));
        }
        headers.sort_by_key(|(name, _)| *name);
        let signed_headers = headers
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(";");
        let canonical_headers = headers
            .iter()
            .map(|(name, value)| format!("{name}:{}\n", normalize_header(value)))
            .collect::<String>();
        let canonical_request = format!(
            "{}\n{}\n\n{}\n{}\n{}",
            method.as_str(),
            canonical_uri(&url),
            canonical_headers,
            signed_headers,
            UNSIGNED_PAYLOAD
        );
        let string_to_sign = format!(
            "{ALGORITHM}\n{timestamp}\n{scope}\n{}",
            hex_sha256(canonical_request.as_bytes())
        );
        let signature = signature(
            &self.config.secret_key,
            &date_scope,
            &self.config.region,
            &string_to_sign,
        )?;
        let authorization = format!(
            "{ALGORITHM} Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.config.access_key
        );
        let mut builder = self.client.request(method, url).timeout(timeout);
        for (name, value) in headers {
            builder = builder.header(name, value);
        }
        builder
            .header("authorization", authorization)
            .send()
            .await
            .map_err(map_reqwest)
    }
}

#[async_trait]
impl ObjectStorage for S3ObjectStorage {
    async fn presign_put(&self, request: PresignPutRequest) -> Result<PresignedPut, PortError> {
        if request.timeout_millis == 0 {
            return Err(PortError::Internal);
        }
        let content_type = request.expected.content_type().to_owned();
        let checksum = request.expected.checksum_sha256().to_owned();
        let content_length = request.expected.content_length().to_string();
        let url = self.presign_put_url(
            &request.object_key,
            &content_type,
            &checksum,
            request.expires_at_millis,
        )?;
        Ok(PresignedPut {
            url: url.to_string(),
            required_headers: vec![
                ("content-type".to_owned(), content_type),
                ("content-length".to_owned(), content_length),
                ("x-amz-checksum-sha256".to_owned(), checksum),
            ],
        })
    }

    async fn head_object(
        &self,
        request: HeadObjectRequest,
    ) -> Result<ActualObjectMetadata, PortError> {
        let response = self
            .signed_request(
                Method::HEAD,
                &request.object_key,
                request_timeout(request.timeout_millis)?,
            )
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(PortError::NotFound);
        }
        if !response.status().is_success() {
            return Err(map_status(response.status()));
        }
        metadata(response.headers())
    }

    async fn delete_object(&self, request: DeleteObjectRequest) -> Result<(), PortError> {
        let response = self
            .signed_request(
                Method::DELETE,
                &request.object_key,
                request_timeout(request.timeout_millis)?,
            )
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(PortError::NotFound);
        }
        if response.status().is_success() {
            Ok(())
        } else {
            Err(map_status(response.status()))
        }
    }
}

fn validate_config(config: &S3Config) -> Result<(), &'static str> {
    if !matches!(config.endpoint.scheme(), "http" | "https")
        || config.endpoint.host_str().is_none()
        || config.endpoint.query().is_some()
        || config.endpoint.fragment().is_some()
        || !config.endpoint.username().is_empty()
        || config.endpoint.password().is_some()
        || config.region.trim().is_empty()
        || config.bucket.trim().is_empty()
        || config.access_key.trim().is_empty()
        || config.secret_key.is_empty()
        || config
            .session_token
            .as_ref()
            .is_some_and(|value| value.is_empty())
    {
        return Err("object storage configuration is invalid");
    }
    Ok(())
}

fn request_timeout(millis: u64) -> Result<Duration, PortError> {
    if millis == 0 {
        Err(PortError::Internal)
    } else {
        Ok(Duration::from_millis(millis))
    }
}

fn metadata(headers: &HeaderMap) -> Result<ActualObjectMetadata, PortError> {
    let content_type = header(headers, "content-type")?;
    let content_length = header(headers, "content-length")?
        .parse::<u64>()
        .map_err(|_| PortError::Internal)?;
    let checksum_sha256 = header(headers, "x-amz-checksum-sha256")?;
    Ok(ActualObjectMetadata {
        content_type,
        content_length,
        checksum_sha256,
    })
}

fn header(headers: &HeaderMap, name: &'static str) -> Result<String, PortError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or(PortError::Internal)
}

fn authority(url: &Url) -> Result<String, PortError> {
    let host = url.host_str().ok_or(PortError::Internal)?;
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

fn canonical_uri(url: &Url) -> String {
    if url.path().is_empty() {
        "/".to_owned()
    } else {
        url.path().to_owned()
    }
}

fn canonical_query(values: &[(&str, String)]) -> String {
    let mut encoded = values
        .iter()
        .map(|(name, value)| (aws_encode(name.as_bytes()), aws_encode(value.as_bytes())))
        .collect::<Vec<_>>();
    encoded.sort();
    encoded
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn aws_encode(value: &[u8]) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(*byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn normalize_header(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn hex_sha256(value: &[u8]) -> String {
    hex(&Sha256::digest(value))
}

fn signature(
    secret: &str,
    date: &str,
    region: &str,
    string_to_sign: &str,
) -> Result<String, PortError> {
    let date_key = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes())?;
    let region_key = hmac(&date_key, region.as_bytes())?;
    let service_key = hmac(&region_key, b"s3")?;
    let signing_key = hmac(&service_key, b"aws4_request")?;
    Ok(hex(&hmac(&signing_key, string_to_sign.as_bytes())?))
}

fn hmac(key: &[u8], value: &[u8]) -> Result<Vec<u8>, PortError> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| PortError::Internal)?;
    mac.update(value);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn hex(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn map_reqwest(error: reqwest::Error) -> PortError {
    if error.is_timeout() {
        PortError::Timeout
    } else if error.is_connect() || error.is_request() {
        PortError::Unavailable
    } else {
        PortError::Internal
    }
}

fn map_status(status: StatusCode) -> PortError {
    if status == StatusCode::NOT_FOUND {
        PortError::NotFound
    } else if status.is_server_error()
        || status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::REQUEST_TIMEOUT
    {
        PortError::Unavailable
    } else {
        PortError::Internal
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine, engine::general_purpose::STANDARD};

    use crate::domain::{ImageId, LotImage, ObjectMetadata, PrincipalId};

    use super::*;

    fn config() -> S3Config {
        S3Config {
            endpoint: "http://127.0.0.1:9000".parse().unwrap(),
            region: "us-east-1".to_owned(),
            bucket: "catalog-images".to_owned(),
            access_key: "catalog-access".to_owned(),
            secret_key: "catalog-secret".to_owned(),
            session_token: None,
            path_style: true,
        }
    }

    #[tokio::test]
    async fn presigned_put_contains_only_public_credential_id_and_required_checksum_headers() {
        let adapter = S3ObjectStorage::new(config()).unwrap();
        let expected =
            ObjectMetadata::new("image/png", 3, STANDARD.encode(Sha256::digest(b"abc"))).unwrap();
        let expires_at_millis = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
            + 60_000;
        let result = adapter
            .presign_put(PresignPutRequest {
                object_key: "seller/image.png".to_owned(),
                expected,
                expires_at_millis,
                timeout_millis: 1_000,
            })
            .await
            .unwrap();
        assert!(
            result
                .url
                .starts_with("http://127.0.0.1:9000/catalog-images/seller/image.png?")
        );
        assert!(result.url.contains("X-Amz-Signature="));
        assert!(result.url.contains("X-Amz-Credential=catalog-access%2F"));
        assert!(!result.url.contains("catalog-secret"));
        assert_eq!(
            result
                .required_headers
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["content-type", "content-length", "x-amz-checksum-sha256"]
        );
    }

    #[test]
    fn config_and_object_keys_fail_closed_without_exposing_credentials() {
        let mut invalid = config();
        invalid.endpoint = "ftp://catalog-secret@localhost/bucket".parse().unwrap();
        let error = S3ObjectStorage::new(invalid).err().unwrap();
        assert_eq!(error, "object storage configuration is invalid");
        assert!(!error.contains("catalog-secret"));

        let adapter = S3ObjectStorage::new(config()).unwrap();
        assert_eq!(adapter.object_url("../escape"), Err(PortError::Internal));
    }

    #[test]
    fn aws_encoding_and_signature_match_the_published_sigv4_primitives() {
        assert_eq!(aws_encode(b"a b/c"), "a%20b%2Fc");
        assert_eq!(
            hex_sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[tokio::test]
    async fn minio_round_trip_verifies_metadata_mismatch_expiry_and_delete() {
        let Ok(endpoint) = std::env::var("CATALOG_TEST_S3_ENDPOINT") else {
            return;
        };
        let config = S3Config {
            endpoint: endpoint.parse().unwrap(),
            region: "us-east-1".to_owned(),
            bucket: "catalog-images".to_owned(),
            access_key: std::env::var("CATALOG_TEST_S3_ACCESS_KEY").unwrap(),
            secret_key: std::env::var("CATALOG_TEST_S3_SECRET_KEY").unwrap(),
            session_token: None,
            path_style: true,
        };
        let adapter = S3ObjectStorage::new(config).unwrap();
        adapter.ensure_bucket(Duration::from_secs(2)).await.unwrap();

        let body = b"real png bytes";
        let checksum = STANDARD.encode(Sha256::digest(body));
        let expected =
            ObjectMetadata::new("image/png", body.len() as u64, checksum.clone()).unwrap();
        let expires_at_millis = unix_millis() + 60_000;
        let presigned = adapter
            .presign_put(PresignPutRequest {
                object_key: "integration/round-trip.png".to_owned(),
                expected: expected.clone(),
                expires_at_millis,
                timeout_millis: 2_000,
            })
            .await
            .unwrap();
        let upload = adapter
            .client
            .put(&presigned.url)
            .header("content-type", "image/png")
            .header("content-length", body.len())
            .header("x-amz-checksum-sha256", &checksum)
            .body(body.to_vec())
            .send()
            .await
            .unwrap();
        assert!(
            upload.status().is_success(),
            "upload failed: {}",
            upload.status()
        );

        let actual = adapter
            .head_object(HeadObjectRequest {
                object_key: "integration/round-trip.png".to_owned(),
                timeout_millis: 2_000,
            })
            .await
            .unwrap();
        assert_eq!(actual.content_type, "image/png");
        assert_eq!(actual.content_length, body.len() as u64);
        assert_eq!(actual.checksum_sha256, checksum);

        let owner = PrincipalId::new("seller-1".to_owned()).unwrap();
        let mut image = LotImage::pending(
            ImageId::new("image-1".to_owned()).unwrap(),
            owner.clone(),
            "integration/round-trip.png".to_owned(),
            ObjectMetadata::new("image/png", body.len() as u64, STANDARD.encode([9_u8; 32]))
                .unwrap(),
            expires_at_millis,
        )
        .unwrap();
        assert!(image.verify(&owner, &actual, unix_millis()).is_err());

        let short_lived = adapter
            .presign_put(PresignPutRequest {
                object_key: "integration/expired.png".to_owned(),
                expected,
                expires_at_millis: unix_millis() + 1_000,
                timeout_millis: 2_000,
            })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
        let expired = adapter
            .client
            .put(short_lived.url)
            .header("content-type", "image/png")
            .header("content-length", body.len())
            .header(
                "x-amz-checksum-sha256",
                STANDARD.encode(Sha256::digest(body)),
            )
            .body(body.to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(expired.status(), StatusCode::FORBIDDEN);

        adapter
            .delete_object(DeleteObjectRequest {
                object_key: "integration/round-trip.png".to_owned(),
                timeout_millis: 2_000,
            })
            .await
            .unwrap();
        assert_eq!(
            adapter
                .head_object(HeadObjectRequest {
                    object_key: "integration/round-trip.png".to_owned(),
                    timeout_millis: 2_000,
                })
                .await,
            Err(PortError::NotFound)
        );
    }

    fn unix_millis() -> u64 {
        u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
    }
}
