-- OxiGDAL PostgreSQL/PostGIS bootstrap
-- Runs automatically on first container start via the postgres image's
-- /docker-entrypoint-initdb.d mechanism (see docker/docker-compose.yml and
-- docker/docker-compose.dev.yml, both of which mount this directory read-only).
--
-- postgis/postgis:16-3.4 already installs the extensions below; this script makes them
-- explicit and idempotent so a plain postgres:16 image (or a manually provisioned database)
-- also ends up with a working PostGIS schema for crates/oxigdal-postgis.

CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS postgis_topology;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
