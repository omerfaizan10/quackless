-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (4, 19, 'kiwi', 121.01), (7, 19, 'fig', 94.30), (12, 11, 'Apple', 91.89), (5, -1, 'apple', NULL), (NULL, 13, 'cherry', NULL), (0, 1, 'Apple', 124.54), (16, NULL, 'apple', 87.30), (-3, 0, NULL, NULL), (NULL, 14, 'cherry', 82.49), (NULL, -5, 'apple', 121.67), (6, 12, NULL, 129.90), (9, 9, 'fig', 103.08), (-4, -4, 'banana', 113.20), (10, 19, 'apple', 62.18), (18, 13, 'apple', 68.09), (-1, 6, 'cherry', 12.57), (-4, 3, 'banana', 3.90), (18, -1, 'apple', 71.93), (3, NULL, 'banana', 138.75), (11, 20, NULL, 15.55), (-3, 20, NULL, 118.68), (0, 14, 'apple', -38.14), (8, NULL, 'grape', 5.21);
INSERT INTO t2 VALUES (14, 16, 'kiwi'), (5, 13, 'grape'), (8, 8, 'grape'), (13, 1, 'cherry'), (18, 6, 'banana'), (-4, 7, 'banana'), (9, 8, 'banana'), (12, 2, 'kiwi'), (18, 19, 'kiwi'), (17, 0, 'kiwi'), (13, -2, 'cherry'), (4, 15, 'date'), (5, 3, 'grape'), (9, 13, 'kiwi'), (17, 10, 'kiwi');
-- query
SELECT (SELECT max(e) FROM t2 WHERE t2.e < coalesce(t2.a, (1 % b))) AS c0 FROM t1 LEFT JOIN t2 ON t1.a = t2.a;
-- expected
[[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10],[10]]
