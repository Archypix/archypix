> **Maintenance notice** — Do not add more details on the work you did compared to the existing documentation. The same level of precision and depth
> must be maintained in this document.

# Coding Guidelines

## Database migrations

**There is one migration file: `001_initial_schema.up.sql`. All schema changes are made directly in that file. Do not add more migration files.**

When editing the database schema, migrate the database with `cd back && cargo sqlx migrate revert && cargo sqlx migrate run && cargo sqlx prepare`

## Rust guidelines

Follow Rust best practices.
Always favor refactoring over sticking to existing legacy functions.
