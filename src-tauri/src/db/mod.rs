/*!
 * SOURCE OF TRUTH KEYWORDS: db, Database, apply_migrations, SCHEMA_VERSION
 * WHAT:  Barrel for database access and schema migrations.
 * WHERE: Constructed in lib.rs setup; consumed only by services/.
 */

pub mod connection;
pub mod migrations;

pub use connection::Database;
pub use migrations::SCHEMA_VERSION;
