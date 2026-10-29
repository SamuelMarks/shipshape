//! Database connection pool utilities.
//!
//! Manages R2D2 PostgreSQL connection pools and embedded Diesel migrations
//! for the ShipShape server, as well as isolated per-test schemas.

#[cfg(test)]
use diesel::RunQueryDsl;
use diesel::pg::PgConnection;
use diesel::r2d2::{self, ConnectionManager};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

use crate::error::ServerResult;

/// Pooled PostgreSQL connections for the ShipShape server.
pub type DbPool = r2d2::Pool<ConnectionManager<PgConnection>>;

/// Embedded Diesel migrations.
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Initialize the database pool using `DATABASE_URL` and run pending migrations.
pub fn init_pool() -> ServerResult<DbPool> {
    let database_url = std::env::var("DATABASE_URL")?;
    let manager = ConnectionManager::<PgConnection>::new(database_url);
    let pool = r2d2::Pool::builder().build(manager)?;
    run_migrations(&pool)?;
    Ok(pool)
}

/// Run pending Diesel migrations on a connection from `pool`.
///
/// # Arguments
/// * `pool` - PostgreSQL connection pool.
pub fn run_migrations(pool: &DbPool) -> ServerResult<()> {
    let mut conn = pool.get()?;
    let _ = conn.run_pending_migrations(MIGRATIONS).ok();
    Ok(())
}

#[cfg(test)]
/// A temporary PostgreSQL database schema for isolated tests.
pub(crate) struct TestDatabase {
    /// Connection string scoped to `schema_name` via `search_path`.
    database_url: String,
    /// Base connection string used to create and drop the schema.
    admin_url: String,
    /// Unique schema identifier.
    schema_name: String,
    /// Lazily initialized connection pool.
    pool: Option<DbPool>,
}

#[cfg(test)]
impl TestDatabase {
    /// Create a new isolated test schema using `TEST_DATABASE_URL` or `DATABASE_URL`.
    pub(crate) fn new() -> ServerResult<Self> {
        use diesel::Connection;

        let env_url = std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .ok()
            .filter(|url| !url.trim().is_empty() && !url.ends_with("/rest_db"))
            .unwrap_or_else(|| {
                "postgres://rest_user:rest_pass@localhost:5432/shipshape_db".to_string()
            });

        let schema_name = format!("shipshape_test_{}", uuid::Uuid::new_v4().simple());
        let mut admin_conn = PgConnection::establish(&env_url)?;

        diesel::sql_query(format!(r#"CREATE SCHEMA "{schema_name}""#)).execute(&mut admin_conn)?;

        let base_url = env_url.split('?').next().unwrap_or(&env_url);
        let database_url = format!("{base_url}?options=-c%20search_path%3D{schema_name}");

        Ok(Self {
            database_url,
            admin_url: env_url,
            schema_name,
            pool: None,
        })
    }

    /// Return the test database URL (configured with the isolated schema).
    pub(crate) fn database_url(&self) -> &str {
        &self.database_url
    }

    /// Get a pooled connection for the test database (running migrations on first call).
    pub(crate) fn pool(&mut self) -> ServerResult<DbPool> {
        if let Some(existing) = &self.pool {
            return Ok(existing.clone());
        }
        let manager = ConnectionManager::<PgConnection>::new(self.database_url.clone());
        let pool = r2d2::Pool::builder().max_size(2).build(manager)?;
        run_migrations(&pool)?;
        self.pool = Some(pool.clone());
        Ok(pool)
    }
}

#[cfg(test)]
impl Drop for TestDatabase {
    fn drop(&mut self) {
        use diesel::Connection;

        let _ = self.pool.take();
        if let Ok(mut conn) = PgConnection::establish(&self.admin_url) {
            let query = format!(r#"DROP SCHEMA IF EXISTS "{}" CASCADE"#, self.schema_name);
            let _ = diesel::sql_query(query).execute(&mut conn);
        }
    }
}

#[cfg(test)]
pub(crate) use tests::{db_env_lock, restore_env_var};

#[cfg(test)]
mod tests {
    use super::{DbPool, TestDatabase, init_pool};
    use crate::error::ServerResult;
    use diesel::dsl::sql;
    use diesel::prelude::*;
    use diesel::sql_types::BigInt;
    use std::sync::{Mutex, MutexGuard};

    /// Global mutex for serialized `DATABASE_URL` environment variable access.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Acquire the global `DATABASE_URL` test lock.
    pub(crate) fn db_env_lock() -> ServerResult<MutexGuard<'static, ()>> {
        ENV_LOCK
            .lock()
            .ok()
            .ok_or(crate::error::ServerError::LockPoisoned { resource: "db_env" })
    }

    /// Restore or remove an environment variable.
    pub(crate) fn restore_env_var(key: &str, prev: Option<String>) {
        if let Some(value) = prev {
            unsafe { std::env::set_var(key, value) };
        } else {
            unsafe { std::env::remove_var(key) };
        }
    }

    #[test]
    fn init_pool_runs_migrations_and_reuses_test_pool() -> ServerResult<()> {
        let _guard = db_env_lock()?;
        let prev_test = std::env::var("TEST_DATABASE_URL").ok();
        let previous = std::env::var("DATABASE_URL").ok();
        unsafe {
            std::env::remove_var("TEST_DATABASE_URL");
            std::env::remove_var("DATABASE_URL");
        }

        let mut test_db = TestDatabase::new()?;
        let _pool_first = test_db.pool()?;
        let _pool_second = test_db.pool()?;

        unsafe {
            std::env::set_var("TEST_DATABASE_URL", "   ");
        }
        let _empty_url_db = TestDatabase::new()?;
        unsafe {
            std::env::set_var("TEST_DATABASE_URL", "postgres://localhost/rest_db");
        }
        let mut rest_db_fallback = TestDatabase::new()?;
        rest_db_fallback.admin_url = "invalid://url".to_string();
        drop(rest_db_fallback);
        unsafe {
            std::env::remove_var("TEST_DATABASE_URL");
            std::env::set_var("DATABASE_URL", test_db.database_url());
        }
        let _explicit_db = TestDatabase::new()?;

        let pool: DbPool = init_pool()?;
        let mut conn = pool.get()?;
        let count: i64 = diesel::select(sql::<BigInt>(
            "(SELECT COUNT(*) FROM pg_tables WHERE schemaname = current_schema() AND tablename = 'workflows')",
        ))
        .get_result(&mut conn)?;

        assert_eq!(count, 1);

        restore_env_var("DATABASE_URL", None);
        restore_env_var("DATABASE_URL", previous);
        restore_env_var("TEST_DATABASE_URL", prev_test);
        Ok(())
    }
}
