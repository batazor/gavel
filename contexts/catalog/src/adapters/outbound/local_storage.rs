//! Development-only in-process object storage used by the local MVP profile.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use axum::{body::Bytes, http::HeaderMap};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use url::Url;
use uuid::Uuid;

use crate::{
    application::{
        DeleteObjectRequest, HeadObjectRequest, ObjectStorage, PortError, PresignPutRequest,
        PresignedPut,
    },
    domain::{ActualObjectMetadata, ObjectMetadata},
};

#[derive(Clone)]
pub struct LocalObjectStorage {
    upload_base_url: Url,
    state: Arc<Mutex<LocalState>>,
}

#[derive(Default)]
struct LocalState {
    uploads: HashMap<String, UploadIntent>,
    objects: HashMap<String, LocalObject>,
}

struct LocalObject {
    metadata: ActualObjectMetadata,
    body: Bytes,
}

struct UploadIntent {
    object_key: String,
    expected: ObjectMetadata,
    expires_at_millis: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalUploadError {
    NotFound,
    Expired,
    MetadataMismatch,
}

impl LocalObjectStorage {
    pub fn new(upload_base_url: Url) -> Result<Self, &'static str> {
        if !matches!(upload_base_url.scheme(), "http" | "https")
            || upload_base_url.host_str().is_none()
            || upload_base_url.query().is_some()
            || upload_base_url.fragment().is_some()
        {
            return Err("local object storage upload URL is invalid");
        }
        Ok(Self {
            upload_base_url,
            state: Arc::new(Mutex::new(LocalState::default())),
        })
    }

    pub async fn upload(
        &self,
        token: &str,
        headers: &HeaderMap,
        body: Bytes,
    ) -> Result<(), LocalUploadError> {
        let mut state = self.state.lock().await;
        let intent = state.uploads.get(token).ok_or(LocalUploadError::NotFound)?;
        if unix_millis() > intent.expires_at_millis {
            return Err(LocalUploadError::Expired);
        }

        let content_type = header(headers, "content-type")?;
        let checksum = header(headers, "x-amz-checksum-sha256")?;
        let declared_content_length = header(headers, "content-length")?
            .parse::<u64>()
            .map_err(|_| LocalUploadError::MetadataMismatch)?;
        let content_length =
            u64::try_from(body.len()).map_err(|_| LocalUploadError::MetadataMismatch)?;
        let actual_checksum = STANDARD.encode(Sha256::digest(&body));
        if content_type != intent.expected.content_type()
            || declared_content_length != intent.expected.content_length()
            || content_length != intent.expected.content_length()
            || checksum != intent.expected.checksum_sha256()
            || actual_checksum != intent.expected.checksum_sha256()
        {
            return Err(LocalUploadError::MetadataMismatch);
        }

        let object_key = intent.object_key.clone();
        state.uploads.remove(token);
        state.objects.insert(
            object_key,
            LocalObject {
                metadata: ActualObjectMetadata {
                    content_type,
                    content_length,
                    checksum_sha256: checksum,
                },
                body,
            },
        );
        Ok(())
    }

    pub async fn image(&self, image_id: &str) -> Option<(String, Bytes)> {
        if image_id.is_empty()
            || image_id.len() > 128
            || !image_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return None;
        }
        self.state
            .lock()
            .await
            .objects
            .get(&format!("catalog/{image_id}"))
            .map(|object| (object.metadata.content_type.clone(), object.body.clone()))
    }

    pub async fn ready(&self) -> Result<(), PortError> {
        Ok(())
    }
}

#[async_trait]
impl ObjectStorage for LocalObjectStorage {
    async fn presign_put(&self, request: PresignPutRequest) -> Result<PresignedPut, PortError> {
        if request.timeout_millis == 0 || request.expires_at_millis <= unix_millis() {
            return Err(PortError::Internal);
        }
        let token = Uuid::now_v7().simple().to_string();
        let mut url = self.upload_base_url.clone();
        {
            let mut segments = url.path_segments_mut().map_err(|_| PortError::Internal)?;
            segments.pop_if_empty();
            segments.push(&token);
        }
        let required_headers = vec![
            (
                "content-type".to_owned(),
                request.expected.content_type().to_owned(),
            ),
            (
                "content-length".to_owned(),
                request.expected.content_length().to_string(),
            ),
            (
                "x-amz-checksum-sha256".to_owned(),
                request.expected.checksum_sha256().to_owned(),
            ),
        ];
        self.state.lock().await.uploads.insert(
            token,
            UploadIntent {
                object_key: request.object_key,
                expected: request.expected,
                expires_at_millis: request.expires_at_millis,
            },
        );
        Ok(PresignedPut {
            url: url.to_string(),
            required_headers,
        })
    }

    async fn head_object(
        &self,
        request: HeadObjectRequest,
    ) -> Result<ActualObjectMetadata, PortError> {
        if request.timeout_millis == 0 {
            return Err(PortError::Internal);
        }
        self.state
            .lock()
            .await
            .objects
            .get(&request.object_key)
            .map(|object| object.metadata.clone())
            .ok_or(PortError::NotFound)
    }

    async fn delete_object(&self, request: DeleteObjectRequest) -> Result<(), PortError> {
        if request.timeout_millis == 0 {
            return Err(PortError::Internal);
        }
        self.state.lock().await.objects.remove(&request.object_key);
        Ok(())
    }
}

fn header(headers: &HeaderMap, name: &'static str) -> Result<String, LocalUploadError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or(LocalUploadError::MetadataMismatch)
}

fn unix_millis() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}
