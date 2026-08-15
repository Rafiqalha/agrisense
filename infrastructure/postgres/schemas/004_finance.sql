-- ─── Finance Schema ───────────────────────────────────────────────────────────
-- Domain: Transactions, cashflow, credit scoring
-- Owned by: finance-service
-- Future: Agrifinance product

CREATE SCHEMA IF NOT EXISTS finance;

CREATE TYPE finance.transaction_type AS ENUM ('expense', 'revenue', 'transfer');
CREATE TYPE finance.loan_type AS ENUM ('kur', 'commercial', 'micro_loan');
CREATE TYPE finance.loan_status AS ENUM ('pending', 'approved', 'rejected', 'disbursed', 'repaid', 'defaulted');

CREATE TABLE finance.transactions (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    farmer_id       UUID NOT NULL,
    farm_id         UUID,
    amount_idr      BIGINT NOT NULL,    -- always in IDR (Rupiah)
    type            finance.transaction_type NOT NULL,
    category        VARCHAR(100) NOT NULL,
    description     TEXT,
    reference_id    UUID,               -- links to order, harvest, etc.
    transaction_date DATE NOT NULL DEFAULT CURRENT_DATE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_finance_transactions_farmer_id        ON finance.transactions (farmer_id);
CREATE INDEX idx_finance_transactions_transaction_date ON finance.transactions (transaction_date);
CREATE INDEX idx_finance_transactions_type             ON finance.transactions (type);

CREATE TABLE finance.credit_scores (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    farmer_id       UUID NOT NULL UNIQUE,
    score           DECIMAL(5,2) NOT NULL,   -- 0–1000
    model_version   VARCHAR(50) NOT NULL,
    factors         JSONB,
    calculated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    valid_until     TIMESTAMPTZ
);

CREATE INDEX idx_finance_credit_scores_farmer_id ON finance.credit_scores (farmer_id);

CREATE TABLE finance.loan_applications (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    farmer_id       UUID NOT NULL,
    loan_type       finance.loan_type NOT NULL,
    amount_requested BIGINT NOT NULL,
    purpose         TEXT NOT NULL,
    credit_score    DECIMAL(5,2),
    status          finance.loan_status NOT NULL DEFAULT 'pending',
    approved_amount BIGINT,
    interest_rate   DECIMAL(5,4),
    tenure_months   SMALLINT,
    partner_bank    VARCHAR(255),
    applied_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON SCHEMA finance IS 'Finance domain - path to Agrifinance product';
COMMENT ON TABLE finance.credit_scores IS 'AI-powered credit scores for KUR applications';
