-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (NULL, -4, 'fig', 77.18), (6, 8, 'banana', -4.73), (17, 14, 'fig', -36.49), (3, 17, NULL, NULL), (4, 14, 'fig', 29.31), (4, 16, 'kiwi', -38.36), (-4, -2, '', 121.34), (2, NULL, 'kiwi', 55.41), (NULL, 16, '', 114.29), (11, 4, 'date', 18.62), (-4, 17, 'grape', 74.48), (NULL, 11, 'apple', 78.22), (6, 12, NULL, 0.17), (14, 13, NULL, 17.32), (1, -5, 'fig', 113.01), (17, 2, 'apple', 72.66), (-5, -1, 'cherry', -43.17), (14, 3, 'cherry', NULL), (17, NULL, 'grape', 149.55), (14, 16, 'date', 28.51), (2, 16, 'banana', 99.05), (NULL, 19, 'Apple', NULL), (6, -4, NULL, NULL), (2, 7, 'date', 43.13), (12, 18, 'grape', 45.30), (14, 19, NULL, NULL), (10, -4, 'cherry', 84.09), (-4, 13, 'apple', 30.10), (-2, NULL, 'cherry', 92.01), (7, 7, 'cherry', 94.59);
INSERT INTO t2 VALUES (NULL, 8, 'apple'), (13, 10, 'banana'), (3, 9, 'fig');
-- query
SELECT count(DISTINCT b2) AS c0 FROM (SELECT a, b + 1 AS b2, c FROM t1 WHERE b IS NOT NULL) AS s WHERE (b2 IN (a, b2, (a % b2)));
-- expected
[[17]]
