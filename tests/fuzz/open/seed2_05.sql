-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (1, 2, 'kiwi', 84.53), (17, NULL, 'cherry', NULL), (-4, 1, 'Apple', NULL), (19, 1, 'date', 135.36), (7, -3, 'Apple', 25.69), (4, NULL, NULL, 17.56), (-1, 1, 'grape', -41.22), (NULL, NULL, NULL, 38.94), (NULL, 6, 'kiwi', -10.39), (1, 4, 'fig', 77.10), (9, -1, 'cherry', -9.56), (10, 4, 'cherry', 107.59), (NULL, NULL, 'banana', NULL), (13, NULL, 'Apple', 14.28), (8, 10, 'kiwi', 89.17), (8, NULL, 'apple', 36.84), (13, NULL, 'date', 144.36), (16, 19, 'date', 22.32), (19, NULL, 'date', -10.32), (17, 19, 'kiwi', 144.26), (13, 10, '', 56.80), (9, 0, 'kiwi', 104.07), (20, NULL, 'Apple', 7.55), (14, NULL, '', 0.66);
INSERT INTO t2 VALUES (-5, NULL, 'date'), (15, 5, NULL), (18, 5, 'grape'), (NULL, -3, 'cherry'), (-4, 5, 'date'), (9, 10, 'date'), (17, 18, 'grape'), (14, 8, 'kiwi'), (11, 3, 'Apple'), (11, -4, 'banana'), (16, 15, 'date'), (-1, NULL, 'grape'), (4, 3, 'apple'), (-4, 7, NULL), (13, 16, 'banana'), (-3, 2, 'fig');
-- query
SELECT max(9.5) AS c0 FROM (SELECT a, b + 1 AS b2, c FROM t1 WHERE b IS NOT NULL) AS s WHERE (CAST(NULL AS INTEGER) = (b2 % b2));
-- expected
[[null]]
