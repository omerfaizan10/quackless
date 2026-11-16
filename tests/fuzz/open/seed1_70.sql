-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (NULL, NULL, NULL, 128.16), (1, NULL, 'date', 79.59), (NULL, 0, 'apple', 48.89), (2, 15, 'cherry', 89.73), (17, 6, 'Apple', -37.30), (6, NULL, 'cherry', -50.28), (0, 15, NULL, -26.35), (15, 6, 'banana', NULL), (NULL, NULL, 'cherry', 57.41), (8, 6, 'banana', 111.25), (4, NULL, NULL, -30.48), (18, 18, 'banana', NULL), (11, -2, 'fig', 10.52), (NULL, 4, 'Apple', 16.88), (-5, 6, NULL, NULL), (1, 2, 'fig', -31.05), (0, 7, 'apple', NULL), (4, NULL, 'kiwi', 105.70), (13, 18, NULL, -39.51), (16, -5, NULL, -19.71), (-5, 14, NULL, -3.32), (17, 19, '', 63.68), (16, 18, 'cherry', -38.97), (10, 13, 'grape', -18.18), (7, 19, 'cherry', 28.69), (NULL, 6, 'kiwi', NULL), (11, NULL, 'Apple', -6.80);
INSERT INTO t2 VALUES (0, 5, NULL), (5, 18, ''), (5, 16, ''), (13, NULL, 'fig'), (7, -3, 'fig'), (10, 11, 'kiwi'), (NULL, -5, NULL), (18, 1, NULL), (16, 12, NULL), (7, -5, ''), (6, 20, ''), (1, 10, ''), (NULL, 14, 'fig'), (12, 7, NULL), (19, -1, 'fig'), (14, 15, 'banana'), (19, 19, 'banana'), (17, -5, NULL);
-- query
SELECT count(DISTINCT 1) AS c0 FROM t1 LEFT JOIN t2 ON t1.b = t2.e AND t2.a > 3 WHERE EXISTS (SELECT 1 FROM t2 WHERE t2.e = (SELECT max(e) FROM t2 WHERE t2.e < b));
-- expected
[[1]]
