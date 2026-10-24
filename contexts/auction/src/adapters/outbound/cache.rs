use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;

use crate::application::{AuctionCache, AuctionProjectionReader, CacheError, PriceSnapshot};
use crate::telemetry::{CacheOutcome, CacheTier, record_cache};

const DEFAULT_FRESHNESS: Duration = Duration::from_millis(500);

pub struct L1PriceCache {
    capacity: usize,
    freshness: Duration,
    state: Mutex<CacheState>,
    refresh_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

#[derive(Default)]
struct CacheState {
    entries: HashMap<String, CacheEntry>,
    access_sequence: u64,
}

struct CacheEntry {
    snapshot: PriceSnapshot,
    stored_at: Instant,
    last_access: u64,
}

impl L1PriceCache {
    pub fn new(capacity: usize) -> Self {
        Self::with_freshness(capacity, DEFAULT_FRESHNESS)
    }

    pub fn with_freshness(capacity: usize, freshness: Duration) -> Self {
        Self {
            capacity: capacity.max(1),
            freshness,
            state: Mutex::new(CacheState::default()),
            refresh_locks: Mutex::new(HashMap::new()),
        }
    }

    pub async fn get_or_refresh<F, Fut>(
        &self,
        auction_id: &str,
        refresh: F,
    ) -> Result<Option<PriceSnapshot>, CacheError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Option<PriceSnapshot>, CacheError>>,
    {
        if let Some(snapshot) = self.fresh(auction_id)? {
            return Ok(Some(snapshot));
        }
        let lock = {
            let mut locks = self
                .refresh_locks
                .lock()
                .map_err(|_| CacheError::Unavailable)?;
            Arc::clone(
                locks
                    .entry(auction_id.to_owned())
                    .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
            )
        };
        let guard = lock.lock().await;
        if let Some(snapshot) = self.fresh(auction_id)? {
            drop(guard);
            self.release_lock(auction_id, &lock)?;
            return Ok(Some(snapshot));
        }
        match refresh().await {
            Ok(mut refreshed) => {
                if let Some(snapshot) = &refreshed {
                    if let Some(existing) = self.peek(auction_id)?
                        && existing.version >= snapshot.version
                    {
                        self.touch(auction_id)?;
                        refreshed = Some(existing);
                    } else {
                        self.put_if_newer(snapshot.clone()).await?;
                    }
                }
                drop(guard);
                self.release_lock(auction_id, &lock)?;
                Ok(refreshed)
            }
            Err(error) => {
                drop(guard);
                self.release_lock(auction_id, &lock)?;
                Err(error)
            }
        }
    }

    fn fresh(&self, auction_id: &str) -> Result<Option<PriceSnapshot>, CacheError> {
        let mut state = self.state.lock().map_err(|_| CacheError::Unavailable)?;
        let is_fresh = state
            .entries
            .get(auction_id)
            .is_some_and(|entry| entry.stored_at.elapsed() <= self.freshness);
        if !is_fresh {
            record_cache(CacheTier::L1, CacheOutcome::Miss);
            return Ok(None);
        }
        state.access_sequence = state.access_sequence.saturating_add(1);
        let access = state.access_sequence;
        let entry = state
            .entries
            .get_mut(auction_id)
            .expect("fresh entry was just checked");
        entry.last_access = access;
        record_cache(CacheTier::L1, CacheOutcome::Hit);
        Ok(Some(entry.snapshot.clone()))
    }

    fn peek(&self, auction_id: &str) -> Result<Option<PriceSnapshot>, CacheError> {
        self.state
            .lock()
            .map_err(|_| CacheError::Unavailable)
            .map(|state| {
                state
                    .entries
                    .get(auction_id)
                    .map(|entry| entry.snapshot.clone())
            })
    }

    fn touch(&self, auction_id: &str) -> Result<(), CacheError> {
        let mut state = self.state.lock().map_err(|_| CacheError::Unavailable)?;
        state.access_sequence = state.access_sequence.saturating_add(1);
        let access = state.access_sequence;
        if let Some(entry) = state.entries.get_mut(auction_id) {
            entry.stored_at = Instant::now();
            entry.last_access = access;
        }
        Ok(())
    }

    fn release_lock(
        &self,
        auction_id: &str,
        lock: &Arc<tokio::sync::Mutex<()>>,
    ) -> Result<(), CacheError> {
        let mut locks = self
            .refresh_locks
            .lock()
            .map_err(|_| CacheError::Unavailable)?;
        if Arc::strong_count(lock) == 2
            && locks
                .get(auction_id)
                .is_some_and(|stored| Arc::ptr_eq(stored, lock))
        {
            locks.remove(auction_id);
        }
        Ok(())
    }
}

pub struct CurrentPriceReader<L2, D> {
    l1: Arc<L1PriceCache>,
    l2: Arc<L2>,
    database: Arc<D>,
}

impl<L2, D> CurrentPriceReader<L2, D>
where
    L2: AuctionCache,
    D: AuctionProjectionReader,
{
    pub fn new(l1: Arc<L1PriceCache>, l2: Arc<L2>, database: Arc<D>) -> Self {
        Self { l1, l2, database }
    }

    pub async fn read(&self, auction_id: &str) -> Result<Option<PriceSnapshot>, CacheError> {
        self.l1
            .get_or_refresh(auction_id, || async {
                if let Ok(Some(snapshot)) = self.l2.get(auction_id).await {
                    return Ok(Some(snapshot));
                }
                let projection = match self.database.auction(auction_id).await {
                    Ok(projection) => projection,
                    Err(_) => {
                        record_cache(CacheTier::Postgres, CacheOutcome::Failure);
                        return Err(CacheError::Unavailable);
                    }
                };
                let Some(projection) = projection else {
                    record_cache(CacheTier::Postgres, CacheOutcome::Miss);
                    return Ok(None);
                };
                record_cache(CacheTier::Postgres, CacheOutcome::Hit);
                let snapshot = PriceSnapshot {
                    auction_id: projection.view.auction_id,
                    amount_minor: projection.view.current_price.minor_units,
                    currency_code: projection.view.current_price.currency_code,
                    version: projection.view.version,
                    updated_at_millis: projection.updated_at.as_millis(),
                };
                let _ = self.l2.put_if_newer(snapshot.clone()).await;
                Ok(Some(snapshot))
            })
            .await
    }
}

#[async_trait]
impl AuctionCache for L1PriceCache {
    async fn get(&self, auction_id: &str) -> Result<Option<PriceSnapshot>, CacheError> {
        self.fresh(auction_id)
    }

    async fn put_if_newer(&self, snapshot: PriceSnapshot) -> Result<bool, CacheError> {
        let mut state = self.state.lock().map_err(|_| CacheError::Unavailable)?;
        if state
            .entries
            .get(&snapshot.auction_id)
            .is_some_and(|stored| stored.snapshot.version >= snapshot.version)
        {
            return Ok(false);
        }
        if !state.entries.contains_key(&snapshot.auction_id)
            && state.entries.len() >= self.capacity
            && let Some(oldest) = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_access)
                .map(|(key, _)| key.clone())
        {
            state.entries.remove(&oldest);
        }
        state.access_sequence = state.access_sequence.saturating_add(1);
        let access = state.access_sequence;
        state.entries.insert(
            snapshot.auction_id.clone(),
            CacheEntry {
                snapshot,
                stored_at: Instant::now(),
                last_access: access,
            },
        );
        record_cache(CacheTier::L1, CacheOutcome::Write);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;
    use crate::application::{
        AuctionProjection, AuctionView, MoneyView, PortError, PublicAuctionStatus,
    };
    use crate::domain::Timestamp;

    fn snapshot(auction_id: &str, version: u64) -> PriceSnapshot {
        PriceSnapshot {
            auction_id: auction_id.to_owned(),
            amount_minor: version * 100,
            currency_code: "USD".to_owned(),
            version,
            updated_at_millis: 1_700_000_000_000,
        }
    }

    #[tokio::test]
    async fn fresh_entries_expire_and_versions_only_move_forward() {
        let cache = L1PriceCache::with_freshness(2, Duration::from_millis(20));
        assert!(cache.put_if_newer(snapshot("a", 2)).await.unwrap());
        assert!(!cache.put_if_newer(snapshot("a", 1)).await.unwrap());
        assert_eq!(cache.get("a").await.unwrap().unwrap().version, 2);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(cache.get("a").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn capacity_evicts_the_least_recently_used_entry() {
        let cache = L1PriceCache::new(2);
        cache.put_if_newer(snapshot("a", 1)).await.unwrap();
        cache.put_if_newer(snapshot("b", 1)).await.unwrap();
        cache.get("a").await.unwrap();
        cache.put_if_newer(snapshot("c", 1)).await.unwrap();
        assert!(cache.get("a").await.unwrap().is_some());
        assert!(cache.get("b").await.unwrap().is_none());
        assert!(cache.get("c").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn concurrent_misses_join_one_refresh_per_auction() {
        let cache = Arc::new(L1PriceCache::new(10));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..20 {
            let cache = Arc::clone(&cache);
            let calls = Arc::clone(&calls);
            tasks.push(tokio::spawn(async move {
                cache
                    .get_or_refresh("a", || async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        Ok(Some(snapshot("a", 3)))
                    })
                    .await
                    .unwrap()
                    .unwrap()
            }));
        }
        for task in tasks {
            assert_eq!(task.await.unwrap().version, 3);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[derive(Default)]
    struct StubL2 {
        values: Mutex<HashMap<String, PriceSnapshot>>,
        gets: AtomicUsize,
        fail_get: AtomicBool,
        fail_put: AtomicBool,
    }

    #[async_trait]
    impl AuctionCache for StubL2 {
        async fn get(&self, auction_id: &str) -> Result<Option<PriceSnapshot>, CacheError> {
            self.gets.fetch_add(1, Ordering::SeqCst);
            if self.fail_get.load(Ordering::SeqCst) {
                return Err(CacheError::Unavailable);
            }
            Ok(self.values.lock().unwrap().get(auction_id).cloned())
        }

        async fn put_if_newer(&self, snapshot: PriceSnapshot) -> Result<bool, CacheError> {
            if self.fail_put.load(Ordering::SeqCst) {
                return Err(CacheError::Unavailable);
            }
            let mut values = self.values.lock().unwrap();
            if values
                .get(&snapshot.auction_id)
                .is_some_and(|stored| stored.version >= snapshot.version)
            {
                return Ok(false);
            }
            values.insert(snapshot.auction_id.clone(), snapshot);
            Ok(true)
        }
    }

    #[derive(Default)]
    struct StubDatabase {
        values: Mutex<HashMap<String, AuctionProjection>>,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl AuctionProjectionReader for StubDatabase {
        async fn auction(&self, auction_id: &str) -> Result<Option<AuctionProjection>, PortError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.values.lock().unwrap().get(auction_id).cloned())
        }

        async fn auction_by_lot_id(
            &self,
            lot_id: &str,
        ) -> Result<Option<AuctionProjection>, PortError> {
            Ok(self
                .values
                .lock()
                .unwrap()
                .values()
                .find(|projection| projection.view.lot_id == lot_id)
                .cloned())
        }
    }

    fn projection(auction_id: &str, version: u64) -> AuctionProjection {
        AuctionProjection {
            view: AuctionView {
                auction_id: auction_id.to_owned(),
                lot_id: format!("lot-{auction_id}"),
                status: PublicAuctionStatus::Open,
                current_price: MoneyView {
                    minor_units: version * 100,
                    currency_code: "USD".to_owned(),
                },
                ends_at_millis: 1_700_000_100_000,
                version,
            },
            updated_at: Timestamp::from_millis(1_700_000_000_000),
        }
    }

    #[tokio::test]
    async fn read_chain_uses_tiers_and_best_effort_fills() {
        let l1 = Arc::new(L1PriceCache::new(10));
        let l2 = Arc::new(StubL2::default());
        let database = Arc::new(StubDatabase::default());
        l2.values
            .lock()
            .unwrap()
            .insert("l2".to_owned(), snapshot("l2", 2));
        database
            .values
            .lock()
            .unwrap()
            .insert("db".to_owned(), projection("db", 3));
        let reader = CurrentPriceReader::new(l1, Arc::clone(&l2), Arc::clone(&database));

        assert_eq!(reader.read("l2").await.unwrap().unwrap().version, 2);
        assert_eq!(reader.read("l2").await.unwrap().unwrap().version, 2);
        assert_eq!(l2.gets.load(Ordering::SeqCst), 1, "second read must hit L1");
        assert_eq!(database.calls.load(Ordering::SeqCst), 0);

        assert_eq!(reader.read("db").await.unwrap().unwrap().version, 3);
        assert_eq!(database.calls.load(Ordering::SeqCst), 1);
        assert_eq!(l2.values.lock().unwrap().get("db").unwrap().version, 3);
        assert!(reader.read("missing").await.unwrap().is_none());

        l2.fail_get.store(true, Ordering::SeqCst);
        l2.fail_put.store(true, Ordering::SeqCst);
        database
            .values
            .lock()
            .unwrap()
            .insert("fallback".to_owned(), projection("fallback", 4));
        assert_eq!(reader.read("fallback").await.unwrap().unwrap().version, 4);
    }

    #[tokio::test]
    async fn older_l2_value_cannot_replace_a_newer_stale_l1_value() {
        let l1 = Arc::new(L1PriceCache::with_freshness(10, Duration::from_millis(10)));
        l1.put_if_newer(snapshot("a", 10)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let l2 = Arc::new(StubL2::default());
        l2.values
            .lock()
            .unwrap()
            .insert("a".to_owned(), snapshot("a", 9));
        let reader =
            CurrentPriceReader::new(Arc::clone(&l1), l2, Arc::new(StubDatabase::default()));
        assert_eq!(reader.read("a").await.unwrap().unwrap().version, 10);
        assert_eq!(l1.get("a").await.unwrap().unwrap().version, 10);
    }
}
