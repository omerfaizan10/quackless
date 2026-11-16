-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (20, 3, NULL, -15.92), (18, NULL, 'banana', 100.73), (8, 6, NULL, 118.02), (8, 3, 'date', -34.21), (8, 9, 'cherry', 17.29), (12, NULL, 'banana', 86.54), (17, NULL, 'banana', 1.79), (-1, 11, 'banana', 21.15), (8, 14, NULL, 29.68), (0, 2, 'grape', 93.41), (20, -4, 'grape', 111.20), (18, 6, NULL, 17.71), (NULL, 20, 'date', NULL), (2, 19, 'fig', 55.95), (15, 9, 'Apple', 14.68), (8, NULL, 'grape', -10.10), (19, 3, 'apple', -22.20), (NULL, NULL, 'grape', -45.49), (14, -1, 'Apple', -40.50), (-2, 6, 'grape', -44.24), (-4, 15, 'cherry', 61.34), (4, NULL, 'Apple', 80.63), (12, NULL, '', 117.51), (2, NULL, 'date', 71.35), (NULL, 1, 'cherry', -47.03);
INSERT INTO t2 VALUES (-2, 13, 'Apple'), (4, 13, 'kiwi'), (12, 8, 'banana'), (NULL, 11, NULL), (11, 19, 'banana'), (16, -4, 'cherry'), (13, 4, 'cherry'), (4, NULL, 'grape'), (17, 1, NULL), (NULL, -3, 'date'), (3, 1, 'apple'), (19, NULL, 'Apple'), (1, 13, 'fig');
-- query
SELECT (e >= (SELECT max(e) FROM t2 WHERE t2.e < (SELECT max(e) FROM t2 WHERE t2.e < b))) AS c0 FROM t1 JOIN t2 ON t1.a = t2.a;
-- expected
[[null],[true],[null],[null],[null],[null],[null]]
