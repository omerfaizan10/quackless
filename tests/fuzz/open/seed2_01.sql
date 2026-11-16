-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (14, NULL, 'grape', 10.24), (-2, 11, '', 29.62), (16, 18, 'fig', -17.54), (8, -4, '', 74.45), (-1, 12, 'apple', 53.93), (4, 16, 'apple', -30.78), (6, 1, 'grape', NULL), (NULL, 20, 'fig', 136.50), (10, 15, 'fig', 25.16), (0, 16, 'date', 65.78), (13, 15, '', 17.46), (NULL, 16, 'kiwi', 23.47), (NULL, 2, 'apple', 48.89), (20, 15, 'kiwi', -41.66), (14, 5, 'apple', NULL), (9, -3, 'apple', 125.55), (13, -5, 'banana', 57.42), (9, 16, 'apple', 142.30), (14, 5, 'date', 38.95), (8, NULL, 'kiwi', 6.69), (1, -4, 'cherry', 115.29), (11, -1, 'kiwi', 143.32), (9, NULL, NULL, 45.29), (4, 9, 'banana', -9.77), (19, 19, 'apple', -33.34);
INSERT INTO t2 VALUES (NULL, -2, 'kiwi'), (9, 2, 'fig'), (-4, 19, 'cherry'), (NULL, NULL, 'kiwi'), (12, -3, NULL), (18, 17, 'date'), (3, NULL, 'apple'), (NULL, -1, NULL), (NULL, 4, NULL), (1, 4, 'grape'), (11, NULL, 'cherry'), (-5, 20, 'grape'), (3, 20, 'apple');
-- query
SELECT (sum(1) + count(*)) AS c0 FROM t1 LEFT JOIN t2 ON t1.b = t2.e AND t2.a > 3 WHERE (e BETWEEN (t1.a % t1.a) AND 9);
-- expected
[[null]]
