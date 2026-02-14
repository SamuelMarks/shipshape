//! Database connection pool utilities.

#[cfg(test)]
use diesel::RunQueryDsl;
use diesel::pg::PgConnection;
use diesel::r2d2::{self, ConnectionManager};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

/// Pooled PostgreSQL connections for the ShipShape server.
pub type DbPool = r2d2::Pool<ConnectionManager<PgConnection>>;

/// Embedded Diesel migrations.
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Initialize the database pool using `DATABASE_URL`.
pub fn init_pool() -> DbPool {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set to a PostgreSQL connection string");
    let manager = ConnectionManager::<PgConnection>::new(database_url);
    let pool = r2d2::Pool::builder()
        .build(manager)
        .expect("failed to create database pool");
    run_migrations(&pool);
    pool
}

/// Run pending Diesel migrations.
pub fn run_migrations(pool: &DbPool) {
    let mut conn = pool.get().expect("failed to fetch database connection");
    conn.run_pending_migrations(MIGRATIONS)
        .expect("run migrations");
}

#[cfg(test)]
fn split_database_url(database_url: &str) -> (String, String) {
    let (url_base, query) = database_url.split_once('?').unwrap_or((database_url, ""));
    let (base, _db_name) = url_base
        .rsplit_once('/')
        .expect("DATABASE_URL must include a database name");
    let query_suffix = if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    };
    (base.to_string(), query_suffix)
}

#[cfg(test)]
/// A temporary PostgreSQL database for tests.
pub(crate) struct TestDatabase {
    database_url: String,
    admin_url: String,
    db_name: String,
    pool: Option<DbPool>,
}

#[cfg(test)]
impl TestDatabase {
    /// Create a new isolated test database using `TEST_DATABASE_URL` or `DATABASE_URL`.
    pub(crate) fn new() -> Self {
        use diesel::Connection;

        let base_url = std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .expect("set TEST_DATABASE_URL or DATABASE_URL for PostgreSQL tests");
        let (base, query_suffix) = split_database_url(&base_url);
        let db_name = format!("shipshape_test_{}", uuid::Uuid::new_v4().simple());
        let admin_url = format!("{}/postgres{}", base, query_suffix);
        let database_url = format!("{}/{}{}", base, db_name, query_suffix);

        let mut admin_conn = PgConnection::establish(&admin_url).expect("connect admin database");
        diesel::sql_query(format!("CREATE DATABASE \"{db_name}\""))
            .execute(&mut admin_conn)
            .expect("create test database");

        Self {
            database_url,
            admin_url,
            db_name,
            pool: None,
        }
    }

    /// Return the test database URL.
    pub(crate) fn database_url(&self) -> &str {
        &self.database_url
    }

    /// Get a pooled connection for the test database (runs migrations once).
    pub(crate) fn pool(&mut self) -> DbPool {
        if self.pool.is_none() {
            let manager = ConnectionManager::<PgConnection>::new(self.database_url.clone());
            let pool = r2d2::Pool::builder()
                .max_size(1)
                .build(manager)
                .expect("pool");
            run_migrations(&pool);
            self.pool = Some(pool);
        }
        self.pool.as_ref().expect("pool").clone()
    }
}

#[cfg(test)]
impl Drop for TestDatabase {
    fn drop(&mut self) {
        use diesel::Connection;

        let _ = self.pool.take();
        if let Ok(mut conn) = PgConnection::establish(&self.admin_url) {
            let escaped = self.db_name.replace('\'', "''");
            let _ = diesel::sql_query(format!(
                "SELECT pg_terminate_backend(pid) \
                 FROM pg_stat_activity \
                 WHERE datname = '{escaped}' AND pid <> pg_backend_pid()"
            ))
            .execute(&mut conn);
            let _ = diesel::sql_query(format!("DROP DATABASE IF EXISTS \"{}\"", self.db_name))
                .execute(&mut conn);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DbPool, TestDatabase, init_pool};
    use diesel::prelude::*;
    use diesel::sql_types::Text;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn env_lock() -> MutexGuard<'static, ()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock")
    }

    #[derive(QueryableByName)]
    struct TableName {
        #[diesel(sql_type = Text)]
        name: String,
    }

    #[test]
    fn init_pool_runs_migrations() {
        let _guard = env_lock();
        let test_db = TestDatabase::new();
        let previous = std::env::var("DATABASE_URL").ok();
        unsafe {
            std::env::set_var("DATABASE_URL", test_db.database_url());
        }
        let pool: DbPool = init_pool();

        let mut conn = pool.get().expect("conn");
        let tables: Vec<TableName> = diesel::sql_query(
            "SELECT tablename AS name FROM pg_tables WHERE schemaname = 'public' AND tablename = 'workflows'",
        )
        .load(&mut conn)
        .expect("query tables");

        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "workflows");

        match previous {
            Some(value) => unsafe {
                std::env::set_var("DATABASE_URL", value);
            },
            None => unsafe {
                std::env::remove_var("DATABASE_URL");
            },
        }
    }
}
