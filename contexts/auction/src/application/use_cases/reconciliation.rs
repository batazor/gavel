use std::sync::Arc;

use super::super::{Authorization, PortError, ReconciliationQueue, RelationshipWrite};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReconciliationReport {
    pub completed: usize,
    pub pending: usize,
}

pub struct ReconciliationWorker<Q, A> {
    queue: Arc<Q>,
    authorization: Arc<A>,
}

impl<Q, A> ReconciliationWorker<Q, A>
where
    Q: ReconciliationQueue,
    A: Authorization,
{
    pub fn new(queue: Arc<Q>, authorization: Arc<A>) -> Self {
        Self {
            queue,
            authorization,
        }
    }

    pub async fn run_batch(&self, limit: usize) -> Result<ReconciliationReport, PortError> {
        let operations = self.queue.pending(limit).await?;
        let mut report = ReconciliationReport::default();

        for operation in operations {
            let request = RelationshipWrite {
                idempotency_key: operation.operation_id.clone(),
                operation: operation.clone(),
            };
            match self.authorization.write_relationship(request).await {
                Ok(()) => {
                    self.queue.complete(&operation.operation_id).await?;
                    report.completed += 1;
                }
                Err(PortError::Unavailable) => report.pending += 1,
                Err(error) => return Err(error),
            }
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{
        AuctionUnitOfWork, CommitRequest, ReconciliationOperation,
        testing::{InMemoryAuctionStore, StubAuthorization},
    };

    #[tokio::test]
    async fn unavailable_authz_leaves_operation_for_idempotent_retry() {
        let store = Arc::new(InMemoryAuctionStore::default());
        let authorization = Arc::new(StubAuthorization::allowed());
        let operation = ReconciliationOperation {
            operation_id: "relationship-1".to_owned(),
            resource_type: "auction".to_owned(),
            resource_id: "auction-1".to_owned(),
            relation: "participant".to_owned(),
            subject_type: "user".to_owned(),
            subject_id: "user-1".to_owned(),
        };
        store
            .commit(CommitRequest {
                auction_id: "auction-1".to_owned(),
                expected_version: 0,
                private_events: Vec::new(),
                public_events: Vec::new(),
                command: None,
                projection: None,
                reconciliation: vec![operation],
            })
            .await
            .unwrap();
        authorization.fail_writes_with(PortError::Unavailable);
        let worker = ReconciliationWorker::new(Arc::clone(&store), Arc::clone(&authorization));

        assert_eq!(
            worker.run_batch(10).await.unwrap(),
            ReconciliationReport {
                completed: 0,
                pending: 1
            }
        );
        assert_eq!(store.pending(10).await.unwrap().len(), 1);

        authorization.allow_writes();
        assert_eq!(
            worker.run_batch(10).await.unwrap(),
            ReconciliationReport {
                completed: 1,
                pending: 0
            }
        );
        assert!(store.pending(10).await.unwrap().is_empty());
        assert_eq!(authorization.writes().len(), 1);
    }
}
