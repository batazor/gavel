use sqlx::{Executor, PgPool, Row, postgres::PgPoolOptions};

async fn pool() -> Option<PgPool> {
    let url = match std::env::var("AUTHZ_TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("AUTHZ_TEST_DATABASE_URL is unset; skipping PostgreSQL integration test");
            return None;
        }
    };
    Some(
        PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .expect("test PostgreSQL must be reachable"),
    )
}

#[tokio::test]
async fn migrations_are_repeatable_constrained_and_privacy_safe() {
    let Some(pool) = pool().await else {
        return;
    };

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("fresh migration must apply");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("rerunning migrator must be a no-op");

    pool.execute(
        "INSERT INTO authz_command_receipts \
         (caller_scope, idempotency_key, request_fingerprint, operation, status, \
          lease_token, lease_expires_at) \
         VALUES ('bff', 'login-1', decode(repeat('01', 32), 'hex'), \
          'register_principal', 'PENDING', \
          '00000000-0000-7000-8000-000000000001', clock_timestamp() + interval '30 seconds')",
    )
    .await
    .expect("valid pending receipt must be accepted");

    pool.execute(
        "INSERT INTO authz_command_receipts \
         (caller_scope, idempotency_key, request_fingerprint, operation, status, \
          lease_token, lease_expires_at) \
         VALUES ('bootstrap', 'bootstrap-1', decode(repeat('02', 32), 'hex'), \
          'register_principal', 'PENDING', \
          '00000000-0000-7000-8000-000000000004', clock_timestamp() + interval '30 seconds')",
    )
    .await
    .expect("authenticated bootstrap receipt must be accepted");

    let invalid_statements = [
        "INSERT INTO authz_command_receipts \
         (caller_scope, idempotency_key, request_fingerprint, operation, status, lease_token, lease_expires_at) \
         VALUES ('unknown', 'bad-scope', decode(repeat('01', 32), 'hex'), 'register_principal', 'PENDING', \
         '00000000-0000-7000-8000-000000000002', clock_timestamp())",
        "INSERT INTO authz_command_receipts \
         (caller_scope, idempotency_key, request_fingerprint, operation, status, lease_token, lease_expires_at) \
         VALUES ('bff', 'short-fingerprint', decode('01', 'hex'), 'register_principal', 'PENDING', \
         '00000000-0000-7000-8000-000000000003', clock_timestamp())",
        "INSERT INTO authz_command_receipts \
         (caller_scope, idempotency_key, request_fingerprint, operation, status) \
         VALUES ('bff', 'pending-without-lease', decode(repeat('01', 32), 'hex'), 'register_principal', 'PENDING')",
        "INSERT INTO authz_command_receipts \
         (caller_scope, idempotency_key, request_fingerprint, operation, status) \
         VALUES ('bff', 'completed-without-result', decode(repeat('01', 32), 'hex'), 'register_principal', 'COMPLETED')",
    ];
    for statement in invalid_statements {
        assert!(
            pool.execute(statement).await.is_err(),
            "invalid row was accepted"
        );
    }

    let columns = sqlx::query(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'authz_command_receipts'",
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| row.get::<String, _>("column_name"))
    .collect::<Vec<_>>();
    for forbidden in [
        "issuer",
        "subject",
        "resource_id",
        "principal_id",
        "relationship_id",
        "credential",
        "zed_token",
    ] {
        assert!(!columns.iter().any(|column| column.contains(forbidden)));
    }
}
