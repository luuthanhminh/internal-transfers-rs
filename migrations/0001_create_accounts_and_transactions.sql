-- Accounts hold the current balance plus the immutable opening balance, so that
-- every balance can be reconciled as: initial_balance + credits - debits.
CREATE TABLE accounts (
    account_id      BIGINT      PRIMARY KEY CHECK (account_id > 0),
    initial_balance NUMERIC     NOT NULL CHECK (initial_balance >= 0),
    -- Last line of defence: the database refuses a negative balance even if the
    -- application check is ever bypassed.
    balance         NUMERIC     NOT NULL CHECK (balance >= 0),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Append-only ledger of completed transfers.
-- NUMERIC is deliberately unconstrained: NUMERIC(p, s) would silently round
-- values with more than `s` decimal places. Precision is validated by the app.
CREATE TABLE transactions (
    transaction_id         BIGSERIAL   PRIMARY KEY,
    source_account_id      BIGINT      NOT NULL REFERENCES accounts (account_id) ON DELETE RESTRICT,
    destination_account_id BIGINT      NOT NULL REFERENCES accounts (account_id) ON DELETE RESTRICT,
    amount                 NUMERIC     NOT NULL CHECK (amount > 0),
    -- Optional client-supplied key; NULLs never conflict with each other.
    idempotency_key        TEXT        UNIQUE,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (source_account_id <> destination_account_id)
);

CREATE INDEX transactions_source_account_id_idx ON transactions (source_account_id);
CREATE INDEX transactions_destination_account_id_idx ON transactions (destination_account_id);

-- Enforce immutability in the database rather than by convention.
CREATE FUNCTION reject_mutation() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION '% on % is not allowed: %', TG_OP, TG_TABLE_NAME, TG_ARGV[0];
END;
$$;

CREATE TRIGGER transactions_are_append_only
    BEFORE UPDATE OR DELETE ON transactions
    FOR EACH ROW EXECUTE FUNCTION reject_mutation('transaction history is immutable');

CREATE TRIGGER transactions_cannot_be_truncated
    BEFORE TRUNCATE ON transactions
    FOR EACH STATEMENT EXECUTE FUNCTION reject_mutation('transaction history is immutable');

CREATE TRIGGER accounts_initial_balance_is_immutable
    BEFORE UPDATE OF initial_balance ON accounts
    FOR EACH ROW
    WHEN (OLD.initial_balance IS DISTINCT FROM NEW.initial_balance)
    EXECUTE FUNCTION reject_mutation('initial_balance is immutable');
