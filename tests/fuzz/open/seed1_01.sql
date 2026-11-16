-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (9, 11, 'banana', 62.17), (9, 0, 'banana', NULL), (13, -3, 'apple', NULL), (11, NULL, 'fig', -30.69), (-4, NULL, 'Apple', 128.31);
INSERT INTO t2 VALUES (19, 6, 'Apple'), (3, 12, 'kiwi'), (-3, -2, 'fig'), (17, 9, NULL), (-4, NULL, ''), (NULL, 12, NULL), (0, NULL, 'grape'), (4, -3, NULL), (20, -3, 'Apple');
-- query
SELECT 1.5 AS c0 FROM t1 LEFT JOIN t2 ON t1.b = t2.e AND t2.a > 3 WHERE ((b % b) = (SELECT max(e) FROM t2 WHERE t2.e < e));
-- expected
[]
