-- Idempotency key stored with a plain INSERT at the end (unique violation aborts)
\set k random(1, 60)
\set a (:k % 4) + 1
\set b ((:k + 1) % 4) + 1
\set amt (:k % 7) + 1
BEGIN;
SELECT count(*) AS locked FROM (SELECT id FROM acc WHERE id IN (:a,:b) ORDER BY id FOR UPDATE) l \gset
SELECT (bal >= :amt)::int AS ok FROM acc WHERE id = :a \gset
\if :ok = 1
UPDATE acc SET bal = bal - :amt WHERE id = :a;
UPDATE acc SET bal = bal + :amt WHERE id = :b;
INSERT INTO tx(src,dst,amount,idem_key) VALUES (:a,:b,:amt,'k' || :k);
COMMIT;
\else
ROLLBACK;
\endif
