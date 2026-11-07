-- SERIALIZABLE isolation, no explicit locks
\set a random(1, 4)
\set d random(1, 3)
\set b ((:a + :d - 1) % 4) + 1
\set amt random(1, 60)
BEGIN ISOLATION LEVEL SERIALIZABLE;
SELECT (bal >= :amt)::int AS ok FROM acc WHERE id = :a \gset
\if :ok = 1
UPDATE acc SET bal = bal - :amt WHERE id = :a;
UPDATE acc SET bal = bal + :amt WHERE id = :b;
INSERT INTO tx(src,dst,amount) VALUES (:a,:b,:amt);
COMMIT;
\else
ROLLBACK;
\endif
