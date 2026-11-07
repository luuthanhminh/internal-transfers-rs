-- Chosen strategy: lock both rows in ascending id order, check funds after locking
\set a random(1, 4)
\set d random(1, 3)
\set b ((:a + :d - 1) % 4) + 1
\set amt random(1, 60)
BEGIN;
SELECT count(*) AS locked FROM (SELECT id FROM acc WHERE id IN (:a,:b) ORDER BY id FOR UPDATE) l \gset
SELECT (bal >= :amt)::int AS ok FROM acc WHERE id = :a \gset
\if :ok = 1
UPDATE acc SET bal = bal - :amt WHERE id = :a;
UPDATE acc SET bal = bal + :amt WHERE id = :b;
INSERT INTO tx(src,dst,amount) VALUES (:a,:b,:amt);
COMMIT;
\else
ROLLBACK;
\endif
