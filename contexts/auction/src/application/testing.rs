//! Deterministic in-memory implementations of application ports.
//!
//! They are deliberately available to integration tests and local development;
//! production composition uses outbound adapters.

use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;

use super::{
    AuctionCache, AuctionProjection, AuctionProjectionReader, AuctionUnitOfWork, Authorization,
    AuthorizationDecision, CacheError, CommandIdentity, CommandLookup, CommandResult, CommitError,
    CommitOutcome, CommitRequest, Inbox, InboxError, InboxOutcome, LoadedAuction, PaymentReadiness,
    PaymentReadinessStore, PaymentReadinessUpdate, PaymentReadinessUpdateOutcome, PermissionCheck,
    PortError, PriceSnapshot, ReconciliationOperation, ReconciliationQueue, RelationshipWrite,
};
use crate::domain::{AuctionEvent, PublicEvent};

#[derive(Default)]
struct StoreState {
    streams: HashMap<String, Vec<AuctionEvent>>,
    command_results: HashMap<(String, String), (String, CommandResult)>,
    projections: HashMap<String, AuctionProjection>,
    outbox: Vec<PublicEvent>,
    reconciliation: HashMap<String, ReconciliationOperation>,
    fail_next_commit: bool,
    conflict_commits_remaining: usize,
}

#[derive(Default)]
pub struct InMemoryAuctionStore {
    state: Mutex<StoreState>,
}

impl InMemoryAuctionStore {
    pub fn fail_next_commit(&self) {
        self.state
            .lock()
            .expect("store lock poisoned")
            .fail_next_commit = true;
    }

    pub fn conflict_next_commits(&self, count: usize) {
        self.state
            .lock()
            .expect("store lock poisoned")
            .conflict_commits_remaining = count;
    }

    pub fn stream(&self, auction_id: &str) -> Vec<AuctionEvent> {
        self.state
            .lock()
            .expect("store lock poisoned")
            .streams
            .get(auction_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn outbox(&self) -> Vec<PublicEvent> {
        self.state
            .lock()
            .expect("store lock poisoned")
            .outbox
            .clone()
    }

    pub fn command_result(&self, identity: &CommandIdentity) -> Option<CommandResult> {
        self.state
            .lock()
            .expect("store lock poisoned")
            .command_results
            .get(&(
                identity.principal_scope.clone(),
                identity.idempotency_key.clone(),
            ))
            .map(|(_, result)| result.clone())
    }
}

#[async_trait]
impl AuctionUnitOfWork for InMemoryAuctionStore {
    async fn load(&self, auction_id: &str) -> Result<Option<LoadedAuction>, CommitError> {
        let state = self.state.lock().map_err(|_| CommitError::Internal)?;
        Ok(state.streams.get(auction_id).map(|events| LoadedAuction {
            snapshot: None,
            events: events.clone(),
            version: events.len() as u64,
        }))
    }

    async fn lookup_command(
        &self,
        identity: &CommandIdentity,
    ) -> Result<CommandLookup, CommitError> {
        let state = self.state.lock().map_err(|_| CommitError::Internal)?;
        Ok(
            match state.command_results.get(&(
                identity.principal_scope.clone(),
                identity.idempotency_key.clone(),
            )) {
                Some((fingerprint, result)) if fingerprint == &identity.request_fingerprint => {
                    CommandLookup::Replay(result.clone())
                }
                Some(_) => CommandLookup::Conflict,
                None => CommandLookup::Missing,
            },
        )
    }

    async fn commit(&self, request: CommitRequest) -> Result<CommitOutcome, CommitError> {
        let mut state = self.state.lock().map_err(|_| CommitError::Internal)?;
        if state.fail_next_commit {
            state.fail_next_commit = false;
            return Err(CommitError::Unavailable);
        }
        if state.conflict_commits_remaining > 0 {
            state.conflict_commits_remaining -= 1;
            return Err(CommitError::VersionConflict);
        }

        if let Some((identity, _)) = &request.command
            && let Some((fingerprint, result)) = state.command_results.get(&(
                identity.principal_scope.clone(),
                identity.idempotency_key.clone(),
            ))
        {
            return if fingerprint == &identity.request_fingerprint {
                Ok(CommitOutcome::Replayed {
                    result: result.clone(),
                })
            } else {
                Err(CommitError::IdempotencyConflict)
            };
        }

        let current_version = state
            .streams
            .get(&request.auction_id)
            .map_or(0, |events| events.len() as u64);
        if current_version != request.expected_version {
            return Err(CommitError::VersionConflict);
        }
        let resulting_version = current_version
            .checked_add(request.private_events.len() as u64)
            .ok_or(CommitError::InvalidRequest)?;
        if request
            .projection
            .as_ref()
            .is_some_and(|projection| projection.view.version != resulting_version)
        {
            return Err(CommitError::InvalidRequest);
        }

        state
            .streams
            .entry(request.auction_id.clone())
            .or_default()
            .extend(request.private_events);
        state.outbox.extend(request.public_events);
        if let Some(projection) = request.projection {
            state
                .projections
                .insert(request.auction_id.clone(), projection);
        }
        if let Some((identity, result)) = request.command {
            state.command_results.insert(
                (identity.principal_scope, identity.idempotency_key),
                (identity.request_fingerprint, result),
            );
        }
        for operation in request.reconciliation {
            state
                .reconciliation
                .insert(operation.operation_id.clone(), operation);
        }

        Ok(CommitOutcome::Committed {
            version: resulting_version,
        })
    }
}

#[async_trait]
impl AuctionProjectionReader for InMemoryAuctionStore {
    async fn auction(&self, auction_id: &str) -> Result<Option<AuctionProjection>, PortError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| PortError::Internal)?
            .projections
            .get(auction_id)
            .cloned())
    }
}

#[async_trait]
impl ReconciliationQueue for InMemoryAuctionStore {
    async fn pending(&self, limit: usize) -> Result<Vec<ReconciliationOperation>, PortError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| PortError::Internal)?
            .reconciliation
            .values()
            .take(limit)
            .cloned()
            .collect())
    }

    async fn complete(&self, operation_id: &str) -> Result<(), PortError> {
        self.state
            .lock()
            .map_err(|_| PortError::Internal)?
            .reconciliation
            .remove(operation_id);
        Ok(())
    }
}

pub struct StubAuthorization {
    decision: Mutex<Result<AuthorizationDecision, PortError>>,
    writes: Mutex<Vec<RelationshipWrite>>,
    write_failure: Mutex<Option<PortError>>,
}

impl StubAuthorization {
    pub fn allowed() -> Self {
        Self {
            decision: Mutex::new(Ok(AuthorizationDecision::Allowed)),
            writes: Mutex::new(Vec::new()),
            write_failure: Mutex::new(None),
        }
    }

    pub fn with_result(result: Result<AuthorizationDecision, PortError>) -> Self {
        Self {
            decision: Mutex::new(result),
            writes: Mutex::new(Vec::new()),
            write_failure: Mutex::new(None),
        }
    }

    pub fn writes(&self) -> Vec<RelationshipWrite> {
        self.writes
            .lock()
            .expect("authorization lock poisoned")
            .clone()
    }

    pub fn fail_writes_with(&self, error: PortError) {
        *self
            .write_failure
            .lock()
            .expect("authorization lock poisoned") = Some(error);
    }

    pub fn allow_writes(&self) {
        *self
            .write_failure
            .lock()
            .expect("authorization lock poisoned") = None;
    }
}

#[async_trait]
impl Authorization for StubAuthorization {
    async fn check(&self, _request: PermissionCheck) -> Result<AuthorizationDecision, PortError> {
        self.decision
            .lock()
            .map_err(|_| PortError::Internal)?
            .clone()
    }

    async fn write_relationship(&self, request: RelationshipWrite) -> Result<(), PortError> {
        if let Some(error) = self
            .write_failure
            .lock()
            .map_err(|_| PortError::Internal)?
            .clone()
        {
            return Err(error);
        }
        self.writes
            .lock()
            .map_err(|_| PortError::Internal)?
            .push(request);
        Ok(())
    }
}

#[derive(Default)]
pub struct InMemoryPaymentReadiness {
    readiness: Mutex<HashMap<String, bool>>,
    source_versions: Mutex<HashMap<String, (u64, String)>>,
    failure: Mutex<Option<PortError>>,
}

impl InMemoryPaymentReadiness {
    pub fn set(&self, user_id: impl Into<String>, ready: bool) {
        self.readiness
            .lock()
            .expect("readiness lock poisoned")
            .insert(user_id.into(), ready);
    }

    pub fn fail_with(&self, error: PortError) {
        *self.failure.lock().expect("readiness lock poisoned") = Some(error);
    }
}

#[async_trait]
impl PaymentReadiness for InMemoryPaymentReadiness {
    async fn is_ready(&self, user_id: &str) -> Result<bool, PortError> {
        if let Some(error) = self
            .failure
            .lock()
            .map_err(|_| PortError::Internal)?
            .clone()
        {
            return Err(error);
        }
        Ok(self
            .readiness
            .lock()
            .map_err(|_| PortError::Internal)?
            .get(user_id)
            .copied()
            .unwrap_or(false))
    }
}

#[async_trait]
impl PaymentReadinessStore for InMemoryPaymentReadiness {
    async fn apply_update(
        &self,
        update: PaymentReadinessUpdate,
    ) -> Result<PaymentReadinessUpdateOutcome, PortError> {
        let mut versions = self
            .source_versions
            .lock()
            .map_err(|_| PortError::Internal)?;
        let outcome = match versions.get(&update.user_id) {
            Some((version, event_id))
                if *version == update.source_version && event_id == &update.source_event_id =>
            {
                PaymentReadinessUpdateOutcome::Duplicate
            }
            Some((version, _)) if *version >= update.source_version => {
                PaymentReadinessUpdateOutcome::Stale
            }
            _ => {
                self.readiness
                    .lock()
                    .map_err(|_| PortError::Internal)?
                    .insert(update.user_id.clone(), update.ready);
                versions.insert(
                    update.user_id,
                    (update.source_version, update.source_event_id),
                );
                PaymentReadinessUpdateOutcome::Applied
            }
        };
        Ok(outcome)
    }
}

#[derive(Default)]
pub struct InMemoryPriceCache {
    values: Mutex<HashMap<String, PriceSnapshot>>,
    failure: Mutex<Option<CacheError>>,
}

impl InMemoryPriceCache {
    pub fn fail_with(&self, error: CacheError) {
        *self.failure.lock().expect("cache lock poisoned") = Some(error);
    }
}

#[async_trait]
impl AuctionCache for InMemoryPriceCache {
    async fn get(&self, auction_id: &str) -> Result<Option<PriceSnapshot>, CacheError> {
        if let Some(error) = self
            .failure
            .lock()
            .map_err(|_| CacheError::Unavailable)?
            .clone()
        {
            return Err(error);
        }
        Ok(self
            .values
            .lock()
            .map_err(|_| CacheError::Unavailable)?
            .get(auction_id)
            .cloned())
    }

    async fn put_if_newer(&self, snapshot: PriceSnapshot) -> Result<bool, CacheError> {
        if let Some(error) = self
            .failure
            .lock()
            .map_err(|_| CacheError::Unavailable)?
            .clone()
        {
            return Err(error);
        }
        let mut values = self.values.lock().map_err(|_| CacheError::Unavailable)?;
        let should_write = values
            .get(&snapshot.auction_id)
            .is_none_or(|stored| snapshot.version >= stored.version);
        if should_write {
            values.insert(snapshot.auction_id.clone(), snapshot);
        }
        Ok(should_write)
    }
}

#[derive(Default)]
pub struct InMemoryInbox {
    entries: Mutex<HashMap<(String, String), String>>,
}

#[async_trait]
impl Inbox for InMemoryInbox {
    async fn accept(
        &self,
        consumer: &str,
        event_id: &str,
        payload_fingerprint: &str,
    ) -> Result<InboxOutcome, InboxError> {
        let mut entries = self.entries.lock().map_err(|_| InboxError::Internal)?;
        let key = (consumer.to_owned(), event_id.to_owned());
        match entries.get(&key) {
            Some(existing) if existing == payload_fingerprint => Ok(InboxOutcome::Duplicate),
            Some(_) => Ok(InboxOutcome::PayloadConflict),
            None => {
                entries.insert(key, payload_fingerprint.to_owned());
                Ok(InboxOutcome::Accepted)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{AuctionView, CommandResult, MoneyView, PublicAuctionStatus},
        domain::{AuctionId, PublicEvent, Timestamp},
    };

    fn projection(version: u64) -> AuctionProjection {
        AuctionProjection {
            view: AuctionView {
                auction_id: "auction-1".to_owned(),
                lot_id: "lot-1".to_owned(),
                status: PublicAuctionStatus::Open,
                current_price: MoneyView {
                    minor_units: 100,
                    currency_code: "USD".to_owned(),
                },
                ends_at_millis: 500,
                version,
            },
            updated_at: Timestamp::from_millis(10),
        }
    }

    fn request() -> CommitRequest {
        CommitRequest {
            auction_id: "auction-1".to_owned(),
            expected_version: 0,
            private_events: Vec::new(),
            public_events: vec![PublicEvent::AuctionCancelled {
                auction_id: AuctionId::from("auction-1"),
                version: 0,
            }],
            command: Some((
                CommandIdentity {
                    principal_scope: "user:user-1".to_owned(),
                    idempotency_key: "key-1".to_owned(),
                    request_fingerprint: "fingerprint-1".to_owned(),
                    operation: "test".to_owned(),
                    correlation_id: "correlation-1".to_owned(),
                    causation_id: "request-1".to_owned(),
                },
                CommandResult::Auction(projection(0).view),
            )),
            projection: Some(projection(0)),
            reconciliation: vec![ReconciliationOperation {
                operation_id: "relationship-1".to_owned(),
                resource_type: "auction".to_owned(),
                resource_id: "auction-1".to_owned(),
                relation: "participant".to_owned(),
                subject_type: "user".to_owned(),
                subject_id: "user-1".to_owned(),
            }],
        }
    }

    #[tokio::test]
    async fn failed_commit_is_atomic() {
        let store = InMemoryAuctionStore::default();
        let request = request();
        let identity = request.command.as_ref().unwrap().0.clone();
        store.fail_next_commit();

        assert_eq!(store.commit(request).await, Err(CommitError::Unavailable));
        assert!(store.stream("auction-1").is_empty());
        assert!(store.outbox().is_empty());
        assert!(store.command_result(&identity).is_none());
        assert!(store.auction("auction-1").await.unwrap().is_none());
        assert!(store.pending(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn duplicate_command_replays_and_changed_payload_conflicts() {
        let store = InMemoryAuctionStore::default();
        let request = request();
        let result = request.command.as_ref().unwrap().1.clone();
        assert_eq!(
            store.commit(request.clone()).await,
            Ok(CommitOutcome::Committed { version: 0 })
        );
        assert_eq!(
            store.commit(request.clone()).await,
            Ok(CommitOutcome::Replayed { result })
        );

        let mut changed = request;
        changed.command.as_mut().unwrap().0.request_fingerprint = "different".to_owned();
        assert_eq!(
            store.commit(changed).await,
            Err(CommitError::IdempotencyConflict)
        );
    }

    #[tokio::test]
    async fn dependency_failures_are_observable() {
        let authz = StubAuthorization::with_result(Err(PortError::Unavailable));
        let check = PermissionCheck {
            principal: super::super::Principal {
                kind: super::super::PrincipalKind::User,
                id: "user-1".to_owned(),
            },
            resource_type: "auction".to_owned(),
            resource_id: "auction-1".to_owned(),
            permission: "place_bid".to_owned(),
        };
        assert_eq!(authz.check(check).await, Err(PortError::Unavailable));

        let readiness = InMemoryPaymentReadiness::default();
        readiness.fail_with(PortError::Unavailable);
        assert_eq!(
            readiness.is_ready("user-1").await,
            Err(PortError::Unavailable)
        );
    }

    #[tokio::test]
    async fn inbox_and_cache_are_monotonic_and_idempotent() {
        let inbox = InMemoryInbox::default();
        assert_eq!(
            inbox.accept("payments", "event-1", "a").await.unwrap(),
            InboxOutcome::Accepted
        );
        assert_eq!(
            inbox.accept("payments", "event-1", "a").await.unwrap(),
            InboxOutcome::Duplicate
        );
        assert_eq!(
            inbox.accept("payments", "event-1", "b").await.unwrap(),
            InboxOutcome::PayloadConflict
        );

        let cache = InMemoryPriceCache::default();
        let snapshot = |version| PriceSnapshot {
            auction_id: "auction-1".to_owned(),
            amount_minor: version * 100,
            currency_code: "USD".to_owned(),
            version,
            updated_at_millis: version,
        };
        assert!(cache.put_if_newer(snapshot(2)).await.unwrap());
        assert!(!cache.put_if_newer(snapshot(1)).await.unwrap());
        assert_eq!(cache.get("auction-1").await.unwrap().unwrap().version, 2);
    }
}
