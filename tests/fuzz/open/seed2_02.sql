-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (-5, 13, NULL, 50.76), (16, 3, 'fig', 134.99), (NULL, NULL, 'grape', 142.20), (NULL, 12, '', 70.87), (12, -2, 'date', -49.26), (2, -4, 'date', -16.96), (14, 4, '', 124.65), (15, -4, 'kiwi', 32.86), (16, NULL, 'Apple', 72.14), (8, 12, 'grape', 70.70), (7, 15, NULL, 58.88), (4, -4, 'cherry', 119.29), (17, NULL, 'cherry', 75.11), (13, -1, 'cherry', -43.32), (14, 3, 'date', -14.04), (0, 15, 'apple', 39.40), (3, 11, 'grape', NULL), (15, 19, NULL, -6.77), (8, NULL, 'kiwi', -32.31), (9, 2, 'banana', 21.61), (10, 12, 'kiwi', NULL), (8, 12, 'cherry', NULL), (11, NULL, 'cherry', 121.62), (9, NULL, NULL, -27.37), (-1, 1, NULL, 48.13), (-3, 3, 'date', 134.78), (0, 7, 'banana', 143.42), (16, -1, '', 24.19), (1, 15, 'date', 14.42), (10, 2, 'banana', 99.67), (17, NULL, 'banana', 92.92), (NULL, 5, 'kiwi', 104.46), (NULL, 19, 'apple', NULL);
INSERT INTO t2 VALUES (11, 6, 'Apple'), (16, 3, ''), (-3, 18, 'date'), (9, NULL, 'apple'), (NULL, 4, 'fig'), (10, 19, 'date'), (4, 15, 'grape'), (8, 19, ''), (12, -5, ''), (NULL, -5, 'apple'), (8, 0, 'Apple'), (2, 1, 'grape'), (15, 11, 'Apple'), (9, -3, 'apple'), (14, 13, 'apple'), (NULL, 3, 'grape'), (-4, 10, 'cherry'), (11, 2, 'grape'), (7, 6, 'banana');
-- query
SELECT 10.5 AS c0 FROM t1 LEFT JOIN t2 ON t1.b = t2.e AND t2.a > 3 WHERE (b BETWEEN (b * b) AND (b % t1.a));
-- expected
[]
