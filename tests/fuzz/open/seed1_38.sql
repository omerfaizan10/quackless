-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (3, 17, 'grape', 133.03), (-1, 8, 'cherry', 64.04), (11, 20, 'apple', NULL), (NULL, NULL, 'apple', -19.45), (-1, 19, 'date', NULL), (7, 9, 'banana', 104.81), (18, NULL, 'banana', 81.19), (NULL, 9, 'cherry', 116.21), (13, 20, '', 13.71), (NULL, 16, 'fig', 58.06), (11, 20, 'grape', 94.62), (-4, -5, NULL, NULL), (18, 4, 'banana', 20.65), (17, 3, NULL, 139.82), (12, -4, NULL, -32.00), (20, 7, '', NULL), (5, 10, 'banana', -44.15), (6, 20, NULL, -30.24), (19, 8, 'Apple', 37.92), (NULL, 13, 'grape', -27.70), (5, -1, 'kiwi', NULL), (7, -2, 'fig', NULL), (6, NULL, NULL, NULL), (17, 12, 'apple', -4.31), (13, NULL, 'apple', 122.33), (19, 6, 'grape', -33.42);
INSERT INTO t2 VALUES (8, 13, 'cherry'), (19, -4, 'apple'), (13, 0, 'banana'), (0, 20, 'apple'), (20, 10, ''), (NULL, 1, 'date'), (6, NULL, ''), (17, NULL, 'kiwi'), (-4, -4, 'cherry');
-- query
SELECT avg(e) AS c0 FROM t1 JOIN t2 ON t1.a = t2.a WHERE ((SELECT max(e) FROM t2 WHERE t2.e < (SELECT max(e) FROM t2 WHERE t2.e < b)) IS NULL);
-- expected
[[-2.0]]
