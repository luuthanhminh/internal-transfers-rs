SELECT sum(bal) AS total, min(bal) AS min_bal,
 (SELECT count(*) FROM tx) AS tx_rows,
 (SELECT count(*) FROM acc a WHERE a.bal <> a.initial
    + coalesce((SELECT sum(amount) FROM tx WHERE dst=a.id),0)
    - coalesce((SELECT sum(amount) FROM tx WHERE src=a.id),0)) AS unreconciled
FROM acc;
