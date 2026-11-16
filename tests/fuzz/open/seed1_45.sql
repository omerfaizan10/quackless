-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (-2, -2, 'banana', 28.32), (17, 2, 'grape', 13.71), (-5, 11, '', 114.92), (NULL, -1, 'banana', 60.07), (9, 1, 'cherry', 75.30), (16, 16, 'cherry', -44.37);
INSERT INTO t2 VALUES (NULL, 4, 'cherry'), (9, 3, NULL), (NULL, 1, 'grape'), (18, 4, ''), (NULL, -5, 'fig');
-- query
SELECT max(1.5) AS c0 FROM t1 LEFT JOIN t2 ON t1.a = t2.a WHERE EXISTS (SELECT 1 FROM t2 WHERE t2.e = (SELECT max(e) FROM t2 WHERE t2.e < t1.a));
-- expected
[[1.5]]
