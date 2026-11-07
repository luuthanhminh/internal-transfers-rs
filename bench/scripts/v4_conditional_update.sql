-- Conditional UPDATE ... WHERE bal >= amt, no lock ordering
\set a random(1, 4)
\set d random(1, 3)
\set b ((:a + :d - 1) % 4) + 1
\set amt random(1, 60)
BEGIN;
WITH u AS (UPDATE acc SET bal = bal - :amt WHERE id = :a AND bal >= :amt RETURNING 1) SELECT count(*) AS ok FROM u \gset
\if :ok = 1
UPDATE acc SET bal = bal + :amt WHERE id = :b;
INSERT INTO tx(src,dst,amount) VALUES (:a,:b,:amt);
COMMIT;
\else
ROLLBACK;
\endif
