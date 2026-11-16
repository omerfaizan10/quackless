-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (20, 12, 'date', 93.02), (8, 11, NULL, NULL), (0, -3, 'fig', 27.37), (2, 4, 'apple', -14.98), (-1, -2, NULL, 20.50), (18, NULL, NULL, 3.17), (-4, -2, 'kiwi', 15.41), (9, -5, '', NULL), (13, NULL, 'date', NULL), (8, NULL, 'apple', -16.04), (3, 3, 'kiwi', -10.87);
INSERT INTO t2 VALUES (4, NULL, 'Apple'), (0, 8, 'kiwi'), (NULL, 12, 'date'), (12, 20, 'banana'), (4, -5, NULL), (13, 9, 'banana'), (-4, 16, 'kiwi'), (12, 12, 'Apple'), (15, 13, 'banana'), (19, NULL, 'date'), (3, 11, 'date'), (1, 5, 'grape'), (5, -4, 'grape');
-- query
SELECT avg(coalesce(a, (a % a))) AS c0 FROM t2;
-- expected
[[7.0]]
