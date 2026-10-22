use sha2::{Digest, Sha256};

use crate::domain::{ImageId, ImageState, LotImage, ObjectMetadata, PrincipalId};

use super::{
    ApplicationError, CleanupLease, Clock, CompleteUploadInput, DeleteObjectRequest,
    HeadObjectRequest, IdGenerator, MediaObservation, MediaOperation, MediaOutcome,
    MediaRepository, MediaTelemetry, ObjectStorage, PortError, PresignPutRequest,
    RequestUploadInput, UploadClaim, UploadClaimOutcome, UploadCompletedResult, UploadIntentResult,
};

const STORAGE_DEPENDENCY: &str = "object_storage";
const REPOSITORY_DEPENDENCY: &str = "catalog_database";

pub struct RequestLotImageUpload<'a, R, S, C, I, T> {
    repository: &'a R,
    storage: &'a S,
    clock: &'a C,
    ids: &'a I,
    telemetry: &'a T,
    upload_ttl_millis: u64,
    storage_timeout_millis: u64,
}

impl<'a, R, S, C, I, T> RequestLotImageUpload<'a, R, S, C, I, T>
where
    R: MediaRepository,
    S: ObjectStorage,
    C: Clock,
    I: IdGenerator,
    T: MediaTelemetry,
{
    pub const fn new(
        repository: &'a R,
        storage: &'a S,
        clock: &'a C,
        ids: &'a I,
        telemetry: &'a T,
        upload_ttl_millis: u64,
        storage_timeout_millis: u64,
    ) -> Self {
        Self {
            repository,
            storage,
            clock,
            ids,
            telemetry,
            upload_ttl_millis,
            storage_timeout_millis,
        }
    }

    pub async fn execute(
        &self,
        input: RequestUploadInput,
    ) -> Result<UploadIntentResult, ApplicationError> {
        let owner = authenticated_principal(&input.context)?;
        validate_idempotency_key(&input.context.idempotency_key)?;
        let expected = ObjectMetadata::new(
            input.content_type,
            input.content_length,
            input.checksum_sha256,
        )
        .map_err(|_| invalid("invalid_object_metadata", "image metadata is invalid"))?;
        let fingerprint = metadata_fingerprint(&expected);
        let now = self.clock.now_millis();
        let expires_at_millis = now
            .checked_add(self.upload_ttl_millis)
            .ok_or_else(|| invalid("invalid_upload_expiry", "upload expiry is invalid"))?;
        let image_id = self.ids.image_id().map_err(map_internal_port)?;
        let object_key = self.ids.object_key(&image_id).map_err(map_internal_port)?;
        let candidate = LotImage::pending(image_id, owner, object_key, expected, expires_at_millis)
            .map_err(|_| invalid("invalid_upload_intent", "upload intent is invalid"))?;

        let image = match self
            .repository
            .claim_upload(UploadClaim {
                caller_scope: input.context.caller_scope.clone(),
                idempotency_key: input.context.idempotency_key.clone(),
                fingerprint,
                image: candidate,
            })
            .await
            .map_err(|error| map_repository(error, self.telemetry, MediaOperation::RequestUpload))?
        {
            UploadClaimOutcome::Created(image) | UploadClaimOutcome::PendingReplay(image) => image,
            UploadClaimOutcome::CompletedReplay(result) => {
                observe(
                    self.telemetry,
                    MediaOperation::RequestUpload,
                    MediaOutcome::Replay,
                    false,
                );
                return Ok(result);
            }
            UploadClaimOutcome::Conflict => {
                observe(
                    self.telemetry,
                    MediaOperation::RequestUpload,
                    MediaOutcome::Conflict,
                    false,
                );
                return Err(ApplicationError::IdempotencyConflict);
            }
            UploadClaimOutcome::InProgress => return Err(ApplicationError::CommandInProgress),
        };

        let presigned = self
            .storage
            .presign_put(PresignPutRequest {
                object_key: image.object_key().to_owned(),
                expected: image.expected().clone(),
                expires_at_millis: image.expires_at_millis(),
                timeout_millis: self.storage_timeout_millis,
            })
            .await
            .map_err(|error| map_storage(error, self.telemetry, MediaOperation::RequestUpload))?;
        let result = UploadIntentResult {
            image_id: image.image_id().to_string(),
            upload_url: presigned.url,
            expires_at_millis: image.expires_at_millis(),
            required_headers: presigned.required_headers,
        };
        self.repository
            .complete_upload_request(
                &input.context.caller_scope,
                &input.context.idempotency_key,
                &result,
            )
            .await
            .map_err(|error| {
                map_repository(error, self.telemetry, MediaOperation::RequestUpload)
            })?;
        observe(
            self.telemetry,
            MediaOperation::RequestUpload,
            MediaOutcome::Success,
            false,
        );
        Ok(result)
    }
}

pub struct CompleteLotImageUpload<'a, R, S, T> {
    repository: &'a R,
    storage: &'a S,
    telemetry: &'a T,
    storage_timeout_millis: u64,
}

impl<'a, R, S, T> CompleteLotImageUpload<'a, R, S, T>
where
    R: MediaRepository,
    S: ObjectStorage,
    T: MediaTelemetry,
{
    pub const fn new(
        repository: &'a R,
        storage: &'a S,
        telemetry: &'a T,
        storage_timeout_millis: u64,
    ) -> Self {
        Self {
            repository,
            storage,
            telemetry,
            storage_timeout_millis,
        }
    }

    pub async fn execute(
        &self,
        input: CompleteUploadInput,
    ) -> Result<UploadCompletedResult, ApplicationError> {
        let owner = authenticated_principal(&input.context)?;
        let image_id = ImageId::new(input.image_id)
            .map_err(|_| invalid("invalid_image_id", "image id is invalid"))?;
        let mut image = self
            .repository
            .image(&image_id)
            .await
            .map_err(|error| map_repository(error, self.telemetry, MediaOperation::CompleteUpload))?
            .ok_or(ApplicationError::NotFound)?;
        if image.owner() != &owner {
            return Err(ApplicationError::NotFound);
        }
        if matches!(image.state(), ImageState::Ready { .. }) {
            observe(
                self.telemetry,
                MediaOperation::CompleteUpload,
                MediaOutcome::Replay,
                false,
            );
            return Ok(completed(image.image_id()));
        }
        if !matches!(image.state(), ImageState::Pending) {
            return Err(conflict("invalid_image_state", "image cannot be completed"));
        }
        let actual = match self
            .storage
            .head_object(HeadObjectRequest {
                object_key: image.object_key().to_owned(),
                timeout_millis: self.storage_timeout_millis,
            })
            .await
        {
            Ok(actual) => actual,
            Err(PortError::NotFound) => {
                image
                    .invalidate(&owner)
                    .map_err(|_| ApplicationError::Internal)?;
                self.repository
                    .mark_for_cleanup(&image)
                    .await
                    .map_err(|error| {
                        map_repository(error, self.telemetry, MediaOperation::CompleteUpload)
                    })?;
                observe(
                    self.telemetry,
                    MediaOperation::CompleteUpload,
                    MediaOutcome::Missing,
                    false,
                );
                return Err(conflict("object_missing", "uploaded image was not found"));
            }
            Err(error) => {
                return Err(map_storage(
                    error,
                    self.telemetry,
                    MediaOperation::CompleteUpload,
                ));
            }
        };
        if image.verify(&owner, &actual, input.now_millis).is_err() {
            self.repository
                .mark_for_cleanup(&image)
                .await
                .map_err(|error| {
                    map_repository(error, self.telemetry, MediaOperation::CompleteUpload)
                })?;
            observe(
                self.telemetry,
                MediaOperation::CompleteUpload,
                MediaOutcome::Invalid,
                false,
            );
            return Err(conflict(
                "object_metadata_mismatch",
                "uploaded image metadata does not match",
            ));
        }
        self.repository
            .save_verified(&image)
            .await
            .map_err(|error| {
                map_repository(error, self.telemetry, MediaOperation::CompleteUpload)
            })?;
        observe(
            self.telemetry,
            MediaOperation::CompleteUpload,
            MediaOutcome::Success,
            false,
        );
        Ok(completed(image.image_id()))
    }
}

pub struct CleanupWorker<'a, R, S, T> {
    repository: &'a R,
    storage: &'a S,
    telemetry: &'a T,
    storage_timeout_millis: u64,
}

impl<'a, R, S, T> CleanupWorker<'a, R, S, T>
where
    R: MediaRepository,
    S: ObjectStorage,
    T: MediaTelemetry,
{
    pub const fn new(
        repository: &'a R,
        storage: &'a S,
        telemetry: &'a T,
        storage_timeout_millis: u64,
    ) -> Self {
        Self {
            repository,
            storage,
            telemetry,
            storage_timeout_millis,
        }
    }

    pub async fn run_batch(
        &self,
        worker: &str,
        now_millis: u64,
        ready_ttl_millis: u64,
        lease_millis: u64,
        limit: usize,
    ) -> Result<usize, ApplicationError> {
        let leases = self
            .repository
            .lease_cleanup(worker, now_millis, ready_ttl_millis, lease_millis, limit)
            .await
            .map_err(|error| map_repository(error, self.telemetry, MediaOperation::Cleanup))?;
        let mut completed = 0;
        for CleanupLease { image, token } in leases {
            if !image.eligible_for_cleanup(now_millis, ready_ttl_millis) {
                self.repository
                    .release_cleanup(image.image_id(), &token)
                    .await
                    .map_err(|error| {
                        map_repository(error, self.telemetry, MediaOperation::Cleanup)
                    })?;
                continue;
            }
            match self
                .storage
                .delete_object(DeleteObjectRequest {
                    object_key: image.object_key().to_owned(),
                    timeout_millis: self.storage_timeout_millis,
                })
                .await
            {
                Ok(()) | Err(PortError::NotFound) => {
                    self.repository
                        .complete_cleanup(image.image_id(), &token)
                        .await
                        .map_err(|error| {
                            map_repository(error, self.telemetry, MediaOperation::Cleanup)
                        })?;
                    completed += 1;
                    observe(
                        self.telemetry,
                        MediaOperation::Cleanup,
                        MediaOutcome::Success,
                        false,
                    );
                }
                Err(error) => {
                    self.repository
                        .release_cleanup(image.image_id(), &token)
                        .await
                        .map_err(|repository_error| {
                            map_repository(
                                repository_error,
                                self.telemetry,
                                MediaOperation::Cleanup,
                            )
                        })?;
                    return Err(map_storage(error, self.telemetry, MediaOperation::Cleanup));
                }
            }
        }
        Ok(completed)
    }
}

fn authenticated_principal(
    context: &super::CommandContext,
) -> Result<PrincipalId, ApplicationError> {
    let principal = context
        .principal
        .as_ref()
        .ok_or(ApplicationError::Unauthenticated)?;
    PrincipalId::new(principal.principal_id.clone())
        .map_err(|_| invalid("invalid_principal", "principal is invalid"))
}

fn validate_idempotency_key(value: &str) -> Result<(), ApplicationError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_whitespace) {
        return Err(invalid(
            "invalid_idempotency_key",
            "idempotency key is invalid",
        ));
    }
    Ok(())
}

fn metadata_fingerprint(metadata: &ObjectMetadata) -> String {
    let mut digest = Sha256::new();
    digest.update(metadata.content_type().as_bytes());
    digest.update([0]);
    digest.update(metadata.content_length().to_be_bytes());
    digest.update([0]);
    digest.update(metadata.checksum_sha256().as_bytes());
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        digest.finalize(),
    )
}

fn completed(image_id: &ImageId) -> UploadCompletedResult {
    UploadCompletedResult {
        image_id: image_id.to_string(),
        status: "READY".to_owned(),
    }
}

fn invalid(reason: &'static str, message: &'static str) -> ApplicationError {
    ApplicationError::InvalidArgument {
        reason,
        safe_message: message.to_owned(),
    }
}

fn conflict(reason: &'static str, message: &'static str) -> ApplicationError {
    ApplicationError::Conflict {
        reason,
        safe_message: message.to_owned(),
    }
}

fn map_internal_port(error: PortError) -> ApplicationError {
    match error {
        PortError::Timeout | PortError::Unavailable => ApplicationError::DependencyUnavailable {
            dependency: REPOSITORY_DEPENDENCY,
        },
        PortError::NotFound | PortError::Internal => ApplicationError::Internal,
    }
}

fn map_repository<T: MediaTelemetry>(
    error: PortError,
    telemetry: &T,
    operation: MediaOperation,
) -> ApplicationError {
    observe(
        telemetry,
        operation,
        MediaOutcome::DependencyFailure,
        matches!(error, PortError::Timeout | PortError::Unavailable),
    );
    error.dependency(REPOSITORY_DEPENDENCY)
}

fn map_storage<T: MediaTelemetry>(
    error: PortError,
    telemetry: &T,
    operation: MediaOperation,
) -> ApplicationError {
    observe(
        telemetry,
        operation,
        MediaOutcome::DependencyFailure,
        matches!(error, PortError::Timeout | PortError::Unavailable),
    );
    error.dependency(STORAGE_DEPENDENCY)
}

fn observe<T: MediaTelemetry>(
    telemetry: &T,
    operation: MediaOperation,
    outcome: MediaOutcome,
    retryable: bool,
) {
    telemetry.record(MediaObservation {
        operation,
        outcome,
        retryable,
    });
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use base64::{Engine, engine::general_purpose::STANDARD};

    use crate::{
        application::{CommandContext, Principal},
        domain::{ActualObjectMetadata, AltText, LotId},
    };

    use super::*;
    use crate::application::{PresignedPut, UploadClaimOutcome};

    #[derive(Clone)]
    struct Receipt {
        fingerprint: String,
        image_id: ImageId,
        result: Option<UploadIntentResult>,
    }

    #[derive(Default)]
    struct RepositoryState {
        receipts: HashMap<(String, String), Receipt>,
        images: HashMap<ImageId, LotImage>,
        leases: HashMap<ImageId, (String, u64)>,
    }

    #[derive(Default)]
    struct FakeRepository {
        state: Mutex<RepositoryState>,
        fail: Mutex<Option<PortError>>,
    }

    impl FakeRepository {
        fn insert(&self, image: LotImage) {
            self.state
                .lock()
                .unwrap()
                .images
                .insert(image.image_id().clone(), image);
        }

        fn image_state(&self, image_id: &str) -> Option<ImageState> {
            self.state
                .lock()
                .unwrap()
                .images
                .get(&ImageId::new(image_id).unwrap())
                .map(|image| image.state().clone())
        }

        fn image_count(&self) -> usize {
            self.state.lock().unwrap().images.len()
        }

        fn set_failure(&self, error: Option<PortError>) {
            *self.fail.lock().unwrap() = error;
        }

        fn failure(&self) -> Result<(), PortError> {
            self.fail.lock().unwrap().map_or(Ok(()), Err)
        }
    }

    #[async_trait]
    impl MediaRepository for FakeRepository {
        async fn claim_upload(&self, claim: UploadClaim) -> Result<UploadClaimOutcome, PortError> {
            self.failure()?;
            let mut state = self.state.lock().unwrap();
            let key = (claim.caller_scope, claim.idempotency_key);
            if let Some(receipt) = state.receipts.get(&key) {
                if receipt.fingerprint != claim.fingerprint {
                    return Ok(UploadClaimOutcome::Conflict);
                }
                if let Some(result) = &receipt.result {
                    return Ok(UploadClaimOutcome::CompletedReplay(result.clone()));
                }
                let image = state.images.get(&receipt.image_id).unwrap().clone();
                return Ok(UploadClaimOutcome::PendingReplay(image));
            }
            let image = claim.image;
            state.receipts.insert(
                key,
                Receipt {
                    fingerprint: claim.fingerprint,
                    image_id: image.image_id().clone(),
                    result: None,
                },
            );
            state.images.insert(image.image_id().clone(), image.clone());
            Ok(UploadClaimOutcome::Created(image))
        }

        async fn complete_upload_request(
            &self,
            caller_scope: &str,
            idempotency_key: &str,
            result: &UploadIntentResult,
        ) -> Result<(), PortError> {
            self.failure()?;
            self.state
                .lock()
                .unwrap()
                .receipts
                .get_mut(&(caller_scope.to_owned(), idempotency_key.to_owned()))
                .unwrap()
                .result = Some(result.clone());
            Ok(())
        }

        async fn image(&self, image_id: &ImageId) -> Result<Option<LotImage>, PortError> {
            self.failure()?;
            Ok(self.state.lock().unwrap().images.get(image_id).cloned())
        }

        async fn save_verified(&self, image: &LotImage) -> Result<(), PortError> {
            self.failure()?;
            self.insert(image.clone());
            Ok(())
        }

        async fn mark_for_cleanup(&self, image: &LotImage) -> Result<(), PortError> {
            self.failure()?;
            self.insert(image.clone());
            Ok(())
        }

        async fn lease_cleanup(
            &self,
            worker: &str,
            now_millis: u64,
            ready_ttl_millis: u64,
            lease_millis: u64,
            limit: usize,
        ) -> Result<Vec<CleanupLease>, PortError> {
            self.failure()?;
            let mut state = self.state.lock().unwrap();
            let candidates: Vec<_> = state
                .images
                .values()
                .filter(|image| image.eligible_for_cleanup(now_millis, ready_ttl_millis))
                .filter(|image| {
                    state
                        .leases
                        .get(image.image_id())
                        .is_none_or(|(_, until)| *until <= now_millis)
                })
                .take(limit)
                .cloned()
                .collect();
            Ok(candidates
                .into_iter()
                .map(|image| {
                    let token = format!("{worker}-{}", image.image_id());
                    state.leases.insert(
                        image.image_id().clone(),
                        (token.clone(), now_millis.saturating_add(lease_millis)),
                    );
                    CleanupLease { image, token }
                })
                .collect())
        }

        async fn complete_cleanup(&self, image_id: &ImageId, token: &str) -> Result<(), PortError> {
            self.failure()?;
            let mut state = self.state.lock().unwrap();
            if state
                .leases
                .get(image_id)
                .is_some_and(|(expected, _)| expected == token)
            {
                state.images.remove(image_id);
                state.leases.remove(image_id);
            }
            Ok(())
        }

        async fn release_cleanup(&self, image_id: &ImageId, token: &str) -> Result<(), PortError> {
            self.failure()?;
            let mut state = self.state.lock().unwrap();
            if state
                .leases
                .get(image_id)
                .is_some_and(|(expected, _)| expected == token)
            {
                state.leases.remove(image_id);
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct StorageState {
        presign_error: Option<PortError>,
        head_error: Option<PortError>,
        delete_error: Option<PortError>,
        actual: Option<ActualObjectMetadata>,
        presign_requests: Vec<PresignPutRequest>,
        head_requests: Vec<HeadObjectRequest>,
        delete_requests: Vec<DeleteObjectRequest>,
    }

    #[derive(Default)]
    struct FakeStorage(Mutex<StorageState>);

    #[async_trait]
    impl ObjectStorage for FakeStorage {
        async fn presign_put(&self, request: PresignPutRequest) -> Result<PresignedPut, PortError> {
            let mut state = self.0.lock().unwrap();
            state.presign_requests.push(request);
            if let Some(error) = state.presign_error {
                return Err(error);
            }
            Ok(PresignedPut {
                url: "https://storage.invalid/signed?redacted".to_owned(),
                required_headers: vec![("x-checksum".to_owned(), "required".to_owned())],
            })
        }

        async fn head_object(
            &self,
            request: HeadObjectRequest,
        ) -> Result<ActualObjectMetadata, PortError> {
            let mut state = self.0.lock().unwrap();
            state.head_requests.push(request);
            if let Some(error) = state.head_error {
                return Err(error);
            }
            state.actual.clone().ok_or(PortError::NotFound)
        }

        async fn delete_object(&self, request: DeleteObjectRequest) -> Result<(), PortError> {
            let mut state = self.0.lock().unwrap();
            state.delete_requests.push(request);
            state.delete_error.map_or(Ok(()), Err)
        }
    }

    struct FakeClock(u64);
    impl Clock for FakeClock {
        fn now_millis(&self) -> u64 {
            self.0
        }
    }

    struct FakeIds;
    impl IdGenerator for FakeIds {
        fn image_id(&self) -> Result<ImageId, PortError> {
            ImageId::new("image-1").map_err(|_| PortError::Internal)
        }

        fn object_key(&self, image_id: &ImageId) -> Result<String, PortError> {
            Ok(format!("private/{}", image_id.as_str()))
        }

        fn event_id(&self) -> Result<String, PortError> {
            Ok("event-1".to_owned())
        }
    }

    #[derive(Default)]
    struct Telemetry(Mutex<Vec<MediaObservation>>);
    impl MediaTelemetry for Telemetry {
        fn record(&self, observation: MediaObservation) {
            self.0.lock().unwrap().push(observation);
        }
    }

    fn context(owner: &str, key: &str) -> CommandContext {
        CommandContext {
            caller_scope: owner.to_owned(),
            principal: Some(Principal {
                principal_id: owner.to_owned(),
            }),
            idempotency_key: key.to_owned(),
            correlation_id: "correlation-1".to_owned(),
            consistency_token: None,
        }
    }

    fn request(owner: &str, key: &str) -> RequestUploadInput {
        RequestUploadInput {
            context: context(owner, key),
            content_type: "image/png".to_owned(),
            content_length: 42,
            checksum_sha256: STANDARD.encode([7_u8; 32]),
        }
    }

    fn actual() -> ActualObjectMetadata {
        ActualObjectMetadata {
            content_type: "image/png".to_owned(),
            content_length: 42,
            checksum_sha256: STANDARD.encode([7_u8; 32]),
        }
    }

    fn pending(id: &str, owner: &str, expiry: u64) -> LotImage {
        LotImage::pending(
            ImageId::new(id).unwrap(),
            PrincipalId::new(owner).unwrap(),
            format!("private/{id}"),
            ObjectMetadata::new("image/png", 42, STANDARD.encode([7_u8; 32])).unwrap(),
            expiry,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn request_is_durable_before_presign_and_replays_same_safe_result() {
        let repository = FakeRepository::default();
        let storage = FakeStorage::default();
        storage.0.lock().unwrap().presign_error = Some(PortError::Timeout);
        let telemetry = Telemetry::default();
        let service = RequestLotImageUpload::new(
            &repository,
            &storage,
            &FakeClock(1_000),
            &FakeIds,
            &telemetry,
            60_000,
            250,
        );

        let error = service
            .execute(request("seller-1", "key-1"))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ApplicationError::DependencyUnavailable { .. }
        ));
        assert_eq!(repository.image_count(), 1);
        assert_eq!(
            storage.0.lock().unwrap().presign_requests[0].timeout_millis,
            250
        );

        storage.0.lock().unwrap().presign_error = None;
        let result = service.execute(request("seller-1", "key-1")).await.unwrap();
        assert_eq!(result.image_id, "image-1");
        assert!(!result.upload_url.contains("private/image-1"));
        let replay = service.execute(request("seller-1", "key-1")).await.unwrap();
        assert_eq!(replay, result);
        assert_eq!(storage.0.lock().unwrap().presign_requests.len(), 2);

        let mut changed = request("seller-1", "key-1");
        changed.content_length = 43;
        assert_eq!(
            service.execute(changed).await.unwrap_err(),
            ApplicationError::IdempotencyConflict
        );
        let count = repository.image_count();
        let mut invalid_request = request("seller-1", "key-2");
        invalid_request.content_type = "text/plain".to_owned();
        assert!(matches!(
            service.execute(invalid_request).await,
            Err(ApplicationError::InvalidArgument { .. })
        ));
        assert_eq!(repository.image_count(), count);
    }

    #[tokio::test]
    async fn completion_verifies_metadata_replays_and_never_marks_ready_on_dependency_failure() {
        let repository = FakeRepository::default();
        repository.insert(pending("image-1", "seller-1", 2_000));
        let storage = FakeStorage::default();
        storage.0.lock().unwrap().actual = Some(actual());
        let telemetry = Telemetry::default();
        let service = CompleteLotImageUpload::new(&repository, &storage, &telemetry, 300);
        let input = CompleteUploadInput {
            context: context("seller-1", "complete-1"),
            image_id: "image-1".to_owned(),
            now_millis: 1_500,
        };
        assert_eq!(
            service.execute(input.clone()).await.unwrap().status,
            "READY"
        );
        storage.0.lock().unwrap().head_error = Some(PortError::Unavailable);
        assert_eq!(service.execute(input).await.unwrap().status, "READY");
        assert_eq!(storage.0.lock().unwrap().head_requests.len(), 1);

        repository.insert(pending("image-2", "seller-1", 2_000));
        let dependency_input = CompleteUploadInput {
            context: context("seller-1", "complete-2"),
            image_id: "image-2".to_owned(),
            now_millis: 1_500,
        };
        assert!(matches!(
            service.execute(dependency_input).await,
            Err(ApplicationError::DependencyUnavailable { .. })
        ));
        assert!(matches!(
            repository.image_state("image-2"),
            Some(ImageState::Pending)
        ));
    }

    #[tokio::test]
    async fn completion_rejects_foreign_missing_mismatch_and_expired_images() {
        let repository = FakeRepository::default();
        let storage = FakeStorage::default();
        let telemetry = Telemetry::default();
        let service = CompleteLotImageUpload::new(&repository, &storage, &telemetry, 300);
        repository.insert(pending("image-1", "seller-1", 2_000));
        assert_eq!(
            service
                .execute(CompleteUploadInput {
                    context: context("seller-2", "foreign"),
                    image_id: "image-1".to_owned(),
                    now_millis: 1_000,
                })
                .await
                .unwrap_err(),
            ApplicationError::NotFound
        );
        assert_eq!(
            service
                .execute(CompleteUploadInput {
                    context: context("seller-1", "missing"),
                    image_id: "image-404".to_owned(),
                    now_millis: 1_000,
                })
                .await
                .unwrap_err(),
            ApplicationError::NotFound
        );

        storage.0.lock().unwrap().actual = Some(ActualObjectMetadata {
            content_length: 43,
            ..actual()
        });
        assert!(matches!(
            service
                .execute(CompleteUploadInput {
                    context: context("seller-1", "mismatch"),
                    image_id: "image-1".to_owned(),
                    now_millis: 1_000,
                })
                .await,
            Err(ApplicationError::Conflict { .. })
        ));
        assert!(matches!(
            repository.image_state("image-1"),
            Some(ImageState::Invalid)
        ));

        repository.insert(pending("image-2", "seller-1", 900));
        storage.0.lock().unwrap().actual = Some(actual());
        assert!(matches!(
            service
                .execute(CompleteUploadInput {
                    context: context("seller-1", "expired"),
                    image_id: "image-2".to_owned(),
                    now_millis: 1_000,
                })
                .await,
            Err(ApplicationError::Conflict { .. })
        ));
        assert!(matches!(
            repository.image_state("image-2"),
            Some(ImageState::Invalid)
        ));

        repository.insert(pending("image-3", "seller-1", 2_000));
        storage.0.lock().unwrap().actual = None;
        assert!(matches!(
            service
                .execute(CompleteUploadInput {
                    context: context("seller-1", "object-missing"),
                    image_id: "image-3".to_owned(),
                    now_millis: 1_000,
                })
                .await,
            Err(ApplicationError::Conflict { .. })
        ));
        assert!(matches!(
            repository.image_state("image-3"),
            Some(ImageState::Invalid)
        ));
    }

    #[tokio::test]
    async fn cleanup_leases_once_retries_storage_and_never_deletes_attached() {
        let repository = Arc::new(FakeRepository::default());
        repository.insert(pending("expired", "seller-1", 900));
        let mut attached = pending("attached", "seller-1", 2_000);
        attached
            .verify(&PrincipalId::new("seller-1").unwrap(), &actual(), 500)
            .unwrap();
        attached
            .attach(
                &PrincipalId::new("seller-1").unwrap(),
                LotId::new("lot-1").unwrap(),
                0,
                AltText::new("front").unwrap(),
            )
            .unwrap();
        repository.insert(attached);
        let storage = FakeStorage::default();
        storage.0.lock().unwrap().delete_error = Some(PortError::Unavailable);
        let telemetry = Telemetry::default();
        let worker = CleanupWorker::new(repository.as_ref(), &storage, &telemetry, 200);
        assert!(matches!(
            worker.run_batch("worker-a", 1_000, 100, 500, 10).await,
            Err(ApplicationError::DependencyUnavailable { .. })
        ));
        assert_eq!(repository.image_count(), 2);
        storage.0.lock().unwrap().delete_error = Some(PortError::NotFound);
        assert_eq!(
            worker
                .run_batch("worker-b", 1_000, 100, 500, 10)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            worker
                .run_batch("worker-c", 1_000, 100, 500, 10)
                .await
                .unwrap(),
            0
        );
        assert_eq!(repository.image_count(), 1);
        assert!(matches!(
            repository.image_state("attached"),
            Some(ImageState::Attached { .. })
        ));
        let requests = &storage.0.lock().unwrap().delete_requests;
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| request.timeout_millis == 200));
    }

    #[tokio::test]
    async fn concurrent_cleanup_workers_produce_one_delete_effect() {
        let repository = FakeRepository::default();
        repository.insert(pending("expired", "seller-1", 900));
        let storage = FakeStorage::default();
        let telemetry = Telemetry::default();
        let worker = CleanupWorker::new(&repository, &storage, &telemetry, 200);
        let (first, second) = tokio::join!(
            worker.run_batch("worker-a", 1_000, 100, 500, 10),
            worker.run_batch("worker-b", 1_000, 100, 500, 10)
        );
        assert_eq!(first.unwrap() + second.unwrap(), 1);
        assert_eq!(storage.0.lock().unwrap().delete_requests.len(), 1);
        assert_eq!(repository.image_count(), 0);
    }

    #[test]
    fn telemetry_contract_is_bounded_and_contains_no_sensitive_fields() {
        let observation = MediaObservation {
            operation: MediaOperation::CompleteUpload,
            outcome: MediaOutcome::DependencyFailure,
            retryable: true,
        };
        let json = serde_json::to_string(&observation).unwrap();
        assert_eq!(
            json,
            r#"{"operation":"complete_upload","outcome":"dependency_failure","retryable":true}"#
        );
        for forbidden in [
            "seller",
            "image_id",
            "object_key",
            "url",
            "checksum",
            "credential",
            "token",
        ] {
            assert!(!json.contains(forbidden));
        }
    }

    #[test]
    fn port_error_mapping_is_deterministic_and_safe() {
        let cases = [
            (
                PortError::Timeout,
                crate::application::ErrorCode::DependencyUnavailable,
                true,
            ),
            (
                PortError::Unavailable,
                crate::application::ErrorCode::DependencyUnavailable,
                true,
            ),
            (
                PortError::NotFound,
                crate::application::ErrorCode::NotFound,
                false,
            ),
            (
                PortError::Internal,
                crate::application::ErrorCode::Internal,
                false,
            ),
        ];
        for (port, code, retryable) in cases {
            let error = port.dependency("private.endpoint.example");
            assert_eq!(error.code(), code);
            assert_eq!(error.retryable(), retryable);
            assert!(!error.to_string().contains("private.endpoint.example"));
        }
    }

    #[tokio::test]
    async fn repository_timeout_is_mapped_before_storage_is_called() {
        let repository = FakeRepository::default();
        repository.set_failure(Some(PortError::Timeout));
        let storage = FakeStorage::default();
        let telemetry = Telemetry::default();
        let service = RequestLotImageUpload::new(
            &repository,
            &storage,
            &FakeClock(1_000),
            &FakeIds,
            &telemetry,
            60_000,
            250,
        );
        assert!(matches!(
            service.execute(request("seller-1", "key-1")).await,
            Err(ApplicationError::DependencyUnavailable { .. })
        ));
        assert!(storage.0.lock().unwrap().presign_requests.is_empty());
    }
}
