use super::{
    ApplicationError, Authorization, DeleteLotOwnerGrant, GrantReconciliationQueue, PortError,
};

pub struct GrantReconciliationWorker<'a, Q, A> {
    queue: &'a Q,
    authorization: &'a A,
    dependency_timeout_millis: u64,
}

impl<'a, Q, A> GrantReconciliationWorker<'a, Q, A>
where
    Q: GrantReconciliationQueue,
    A: Authorization,
{
    pub const fn new(queue: &'a Q, authorization: &'a A, dependency_timeout_millis: u64) -> Self {
        Self {
            queue,
            authorization,
            dependency_timeout_millis,
        }
    }

    pub async fn run_batch(
        &self,
        worker: &str,
        now_millis: u64,
        lease_millis: u64,
        limit: usize,
    ) -> Result<usize, ApplicationError> {
        let leases = self
            .queue
            .lease_due(worker, now_millis, lease_millis, limit)
            .await
            .map_err(map_queue)?;
        let mut completed = 0;
        for lease in leases {
            if lease.not_before_millis > now_millis {
                self.queue
                    .release(&lease.operation_id, &lease.lease_token)
                    .await
                    .map_err(map_queue)?;
                continue;
            }
            if self
                .queue
                .lot_exists(&lease.lot_id)
                .await
                .map_err(map_queue)?
            {
                self.queue
                    .complete(&lease.operation_id, &lease.lease_token)
                    .await
                    .map_err(map_queue)?;
                completed += 1;
                continue;
            }
            match self
                .authorization
                .delete_lot_owner(DeleteLotOwnerGrant {
                    lot_id: lease.lot_id.to_string(),
                    seller: lease.seller,
                    idempotency_key: lease.delete_idempotency_key,
                    timeout_millis: self.dependency_timeout_millis,
                })
                .await
            {
                Ok(_) => {
                    self.queue
                        .complete(&lease.operation_id, &lease.lease_token)
                        .await
                        .map_err(map_queue)?;
                    completed += 1;
                }
                Err(error) => {
                    self.queue
                        .release(&lease.operation_id, &lease.lease_token)
                        .await
                        .map_err(map_queue)?;
                    return Err(map_authorization(error));
                }
            }
        }
        Ok(completed)
    }
}

fn map_queue(error: PortError) -> ApplicationError {
    match error {
        PortError::Timeout | PortError::Unavailable => ApplicationError::DependencyUnavailable {
            dependency: "catalog_database",
        },
        PortError::NotFound | PortError::Internal => ApplicationError::Internal,
    }
}

fn map_authorization(error: PortError) -> ApplicationError {
    match error {
        PortError::Timeout | PortError::Unavailable => ApplicationError::DependencyUnavailable {
            dependency: "authorization",
        },
        PortError::NotFound | PortError::Internal => ApplicationError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, sync::Mutex};

    use async_trait::async_trait;

    use crate::application::{
        AuthorizationRevision, LotOwnerGrant, PermissionCheck, PermissionDecision,
        ServicePermissionCheck,
    };
    use crate::domain::{LotId, PrincipalId};

    use super::*;
    use crate::application::{DeleteLotOwnerGrant, GrantReconciliationLease};

    #[derive(Clone)]
    struct Operation {
        id: String,
        lot_id: LotId,
        not_before: u64,
        leased: bool,
        completed: bool,
    }

    struct FakeQueue {
        operations: Mutex<Vec<Operation>>,
        lots: Mutex<HashSet<LotId>>,
        releases: Mutex<usize>,
    }

    #[async_trait]
    impl GrantReconciliationQueue for FakeQueue {
        async fn lease_due(
            &self,
            worker: &str,
            _now_millis: u64,
            _lease_millis: u64,
            limit: usize,
        ) -> Result<Vec<GrantReconciliationLease>, PortError> {
            let mut operations = self.operations.lock().unwrap();
            Ok(operations
                .iter_mut()
                .filter(|operation| !operation.leased && !operation.completed)
                .take(limit)
                .map(|operation| {
                    operation.leased = true;
                    GrantReconciliationLease {
                        operation_id: operation.id.clone(),
                        lot_id: operation.lot_id.clone(),
                        seller: PrincipalId::new("seller-1").unwrap(),
                        delete_idempotency_key: format!("delete-{}", operation.id),
                        not_before_millis: operation.not_before,
                        lease_token: format!("{worker}-{}", operation.id),
                    }
                })
                .collect())
        }

        async fn lot_exists(&self, lot_id: &LotId) -> Result<bool, PortError> {
            Ok(self.lots.lock().unwrap().contains(lot_id))
        }

        async fn complete(&self, operation_id: &str, _token: &str) -> Result<(), PortError> {
            let mut operations = self.operations.lock().unwrap();
            let operation = operations
                .iter_mut()
                .find(|operation| operation.id == operation_id)
                .unwrap();
            operation.completed = true;
            operation.leased = false;
            Ok(())
        }

        async fn release(&self, operation_id: &str, _token: &str) -> Result<(), PortError> {
            let mut operations = self.operations.lock().unwrap();
            operations
                .iter_mut()
                .find(|operation| operation.id == operation_id)
                .unwrap()
                .leased = false;
            *self.releases.lock().unwrap() += 1;
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeAuthorization {
        deletes: Mutex<Vec<DeleteLotOwnerGrant>>,
        failure: Mutex<Option<PortError>>,
    }

    #[async_trait]
    impl Authorization for FakeAuthorization {
        async fn check(&self, _request: PermissionCheck) -> Result<PermissionDecision, PortError> {
            unreachable!()
        }

        async fn check_service(
            &self,
            _request: ServicePermissionCheck,
        ) -> Result<PermissionDecision, PortError> {
            unreachable!()
        }

        async fn touch_lot_owner(
            &self,
            _request: LotOwnerGrant,
        ) -> Result<AuthorizationRevision, PortError> {
            unreachable!()
        }

        async fn delete_lot_owner(
            &self,
            request: DeleteLotOwnerGrant,
        ) -> Result<AuthorizationRevision, PortError> {
            self.deletes.lock().unwrap().push(request);
            if let Some(error) = *self.failure.lock().unwrap() {
                return Err(error);
            }
            Ok(AuthorizationRevision {
                revision: "delete-revision".to_owned(),
            })
        }
    }

    fn operation(id: &str, lot: &str, not_before: u64) -> Operation {
        Operation {
            id: id.to_owned(),
            lot_id: LotId::new(lot).unwrap(),
            not_before,
            leased: false,
            completed: false,
        }
    }

    #[tokio::test]
    async fn delayed_and_existing_lot_operations_never_delete_grants() {
        let queue = FakeQueue {
            operations: Mutex::new(vec![
                operation("future", "lot-future", 2_000),
                operation("exists", "lot-exists", 900),
            ]),
            lots: Mutex::new(HashSet::from([LotId::new("lot-exists").unwrap()])),
            releases: Mutex::new(0),
        };
        let auth = FakeAuthorization::default();
        let worker = GrantReconciliationWorker::new(&queue, &auth, 250);
        assert_eq!(
            worker.run_batch("worker-1", 1_000, 500, 10).await.unwrap(),
            1
        );
        assert!(auth.deletes.lock().unwrap().is_empty());
        assert_eq!(*queue.releases.lock().unwrap(), 1);
        let operations = queue.operations.lock().unwrap();
        assert!(!operations[0].completed);
        assert!(operations[1].completed);
    }

    #[tokio::test]
    async fn proven_absence_deletes_idempotently_and_retry_reuses_delete_key() {
        let queue = FakeQueue {
            operations: Mutex::new(vec![operation("orphan", "lot-orphan", 900)]),
            lots: Mutex::new(HashSet::new()),
            releases: Mutex::new(0),
        };
        let auth = FakeAuthorization::default();
        *auth.failure.lock().unwrap() = Some(PortError::Unavailable);
        let worker = GrantReconciliationWorker::new(&queue, &auth, 250);
        assert!(matches!(
            worker.run_batch("worker-1", 1_000, 500, 10).await,
            Err(ApplicationError::DependencyUnavailable { .. })
        ));
        *auth.failure.lock().unwrap() = None;
        assert_eq!(
            worker.run_batch("worker-2", 1_000, 500, 10).await.unwrap(),
            1
        );
        assert_eq!(
            worker.run_batch("worker-3", 1_000, 500, 10).await.unwrap(),
            0
        );
        let deletes = auth.deletes.lock().unwrap();
        assert_eq!(deletes.len(), 2);
        assert_eq!(deletes[0].idempotency_key, deletes[1].idempotency_key);
        assert_eq!(deletes[0].lot_id, "lot-orphan");
        assert_eq!(deletes[0].timeout_millis, 250);
    }
}
