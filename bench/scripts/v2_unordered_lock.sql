-- FOR UPDATE in request order: source, then destination
\set a random(1, 4)
\set d random(1, 3)
\set b ((:a + :d - 1) % 4) + 1
\set amt random(1, 60)
BEGIN;
SELECT bal AS sbal FROM acc WHERE id = :a FOR UPDATE \gset
SELECT bal AS dbal FROM acc WHERE id = :b FOR UPDATE \gset
\if :sbal >= :amt
UPDATE acc SET bal = bal - :amt WHERE id = :a;
UPDATE acc SET bal = bal + :amt WHERE id = :b;
INSERT INTO tx(src,dst,amount) VALUES (:a,:b,:amt);
COMMIT;
\else
ROLLBACK;
\endif
