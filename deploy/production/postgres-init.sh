#!/bin/sh
set -eu

: "${A3S_CLOUD_POSTGRES_MIGRATION_PASSWORD:?migration password is required}"
: "${A3S_CLOUD_POSTGRES_SERVING_PASSWORD:?serving password is required}"

psql \
  --set=ON_ERROR_STOP=1 \
  --set=bootstrap_role="$POSTGRES_USER" \
  --set=database_name="$POSTGRES_DB" \
  --set=migration_password="$A3S_CLOUD_POSTGRES_MIGRATION_PASSWORD" \
  --set=serving_password="$A3S_CLOUD_POSTGRES_SERVING_PASSWORD" \
  --username "$POSTGRES_USER" \
  --dbname "$POSTGRES_DB" <<'SQL'
SELECT format(
  'CREATE ROLE a3s_cloud_migrator LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L',
  :'migration_password'
)
WHERE NOT EXISTS (
  SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'a3s_cloud_migrator'
) \gexec

SELECT format(
  'ALTER ROLE a3s_cloud_migrator LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L',
  :'migration_password'
) \gexec

SELECT format(
  'CREATE ROLE a3s_cloud_serving LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L',
  :'serving_password'
)
WHERE NOT EXISTS (
  SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'a3s_cloud_serving'
) \gexec

SELECT format(
  'ALTER ROLE a3s_cloud_serving LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD %L',
  :'serving_password'
) \gexec

SELECT format('ALTER DATABASE %I OWNER TO a3s_cloud_migrator', :'database_name') \gexec

SELECT format('ALTER ROLE %I NOLOGIN', :'bootstrap_role') \gexec
SQL
