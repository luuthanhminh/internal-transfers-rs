-- Every balance stays below 10^20, the same bound as a single amount, so a
-- balance always fits the 20 integer digits clients are told to expect. The
-- application checks this under the row lock; this constraint is the backstop.
-- Bounded lock wait: docs/adr/0006-forward-only-migrations.md.
SET LOCAL lock_timeout = '5s';

-- NOT VALID: enforced for every new write, but existing rows are not scanned,
-- so the lock on `accounts` is brief and a database that already holds a
-- larger balance can still be upgraded. Validating existing rows is a separate
-- migration (`VALIDATE CONSTRAINT` does not block reads or writes) once any
-- such balances have been dealt with.
ALTER TABLE accounts
    ADD CONSTRAINT accounts_balance_below_limit CHECK (balance < 1e20) NOT VALID;
