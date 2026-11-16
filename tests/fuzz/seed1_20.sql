-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (2, NULL, 'fig', 77.37), (10, 5, NULL, 95.26), (16, 8, NULL, NULL), (NULL, NULL, 'date', -21.37), (0, 7, 'fig', 143.83);
INSERT INTO t2 VALUES (5, -1, 'grape'), (NULL, 8, NULL), (4, NULL, '');
-- query
SELECT min((e / (b % e))) AS c0 FROM t1, t2;
-- expected
[["-inf"]]
