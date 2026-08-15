-- ─── Identity Schema ───────────────────────────────────────────────────────────
-- Domain: Authentication, Authorization, User Management
-- Owned by: platform-service

CREATE SCHEMA IF NOT EXISTS identity;

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

CREATE TYPE identity.user_role AS ENUM (
    'farmer', 'kios', 'supplier', 'bank_partner', 'iot_vendor', 'admin', 'super_admin'
);

CREATE TYPE identity.user_status AS ENUM (
    'active', 'inactive', 'suspended', 'pending_verification'
);

CREATE TABLE identity.users (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    phone           VARCHAR(20) UNIQUE NOT NULL,
    name            VARCHAR(255) NOT NULL,
    role            identity.user_role NOT NULL DEFAULT 'farmer',
    status          identity.user_status NOT NULL DEFAULT 'pending_verification',
    region          VARCHAR(100),
    referral_source VARCHAR(100),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    verified_at     TIMESTAMPTZ,
    last_active_at  TIMESTAMPTZ
);

CREATE INDEX idx_identity_users_phone  ON identity.users (phone);
CREATE INDEX idx_identity_users_role   ON identity.users (role);
CREATE INDEX idx_identity_users_status ON identity.users (status);
CREATE INDEX idx_identity_users_region ON identity.users (region);

CREATE TABLE identity.sessions (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id         UUID NOT NULL REFERENCES identity.users(id) ON DELETE CASCADE,
    refresh_token   TEXT NOT NULL UNIQUE,
    device_info     JSONB,
    ip_address      INET,
    expires_at      TIMESTAMPTZ NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_identity_sessions_user_id ON identity.sessions (user_id);

COMMENT ON SCHEMA identity IS 'Authentication and user management domain';
COMMENT ON TABLE identity.users IS 'All users: farmers, kios, admins, partners';
