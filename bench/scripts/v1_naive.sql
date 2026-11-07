-- Read both balances, compute in the app, write absolute values (no locks)
\set a random(1, 4)
\set d random(1, 3)
\set b ((:a + :d - 1) % 4) + 1
\set amt random(1, 60)
BEGIN;
SELECT bal AS sbal FROM acc WHERE id = :a \gset
SELECT bal AS dbal FROM acc WHERE id = :b \gset
\if :sbal >= :amt
UPDATE acc SET bal = :sbal - :amt WHERE id = :a;
UPDATE acc SET bal = :dbal + :amt WHERE id = :b;
INSERT INTO tx(src,dst,amount) VALUES (:a,:b,:amt);
COMMIT;
\else
ROLLBACK;
\endif
