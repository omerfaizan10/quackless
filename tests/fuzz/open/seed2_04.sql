-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (-1, 17, 'Apple', 81.79), (0, NULL, 'kiwi', -34.20), (2, -1, 'date', 97.03), (12, 15, 'banana', 48.21), (NULL, NULL, 'grape', 36.57), (10, 19, 'banana', 118.94), (NULL, 9, 'date', 37.04), (16, -2, 'apple', 147.36), (NULL, 20, 'kiwi', 137.69), (17, 2, 'kiwi', 68.45), (19, NULL, 'fig', -19.55), (-4, 9, 'kiwi', 5.32), (-3, 12, '', -15.52), (7, NULL, 'cherry', NULL), (9, -1, 'kiwi', 60.08), (16, 0, 'grape', 141.57), (12, 12, '', 8.29), (7, 2, 'Apple', 78.81), (2, 11, 'fig', 24.68), (0, -1, 'apple', 110.29), (20, NULL, 'banana', NULL), (17, 13, '', NULL), (20, 19, 'kiwi', 43.20), (NULL, -4, 'apple', 95.60);
INSERT INTO t2 VALUES (-1, NULL, 'banana'), (0, 8, 'grape'), (0, -3, NULL), (-2, 13, 'grape'), (12, 12, 'grape'), (13, 5, 'cherry'), (6, NULL, NULL), (NULL, 3, 'apple'), (-2, 4, 'date'), (2, 11, 'Apple'), (NULL, -1, 'kiwi'), (-4, 4, 'banana'), (NULL, -2, 'grape'), (-4, 18, ''), (19, 14, 'date'), (NULL, 5, 'fig');
-- query
SELECT 1.5 AS c0 FROM (SELECT a, b + 1 AS b2, c FROM t1 WHERE b IS NOT NULL) AS s WHERE (a BETWEEN b2 AND (11 % nullif(a, b2)));
-- expected
[]
