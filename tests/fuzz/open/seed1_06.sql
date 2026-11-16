-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (20, NULL, 'fig', -8.73), (6, 14, 'banana', 80.96), (11, 17, 'banana', 25.60), (13, 5, 'banana', 8.02), (20, 12, 'apple', 42.49), (19, 11, 'banana', -32.66), (7, 13, 'Apple', -37.15), (15, 19, 'apple', 101.59), (18, 2, 'banana', NULL), (-3, 2, NULL, NULL), (0, 14, 'apple', NULL), (14, 3, 'fig', 94.24), (15, 16, '', -18.52), (17, NULL, '', 7.70), (6, -5, 'grape', 2.91), (NULL, 0, NULL, NULL), (NULL, -4, NULL, 112.76), (11, 18, 'fig', 47.83), (7, 20, 'cherry', 55.74), (5, 12, '', 103.99), (10, 8, 'banana', 88.74), (9, NULL, 'Apple', 138.73), (20, 17, NULL, 122.29);
INSERT INTO t2 VALUES (-2, 2, 'date'), (15, NULL, 'Apple'), (18, 9, 'Apple');
-- query
SELECT min(0.5) AS c0 FROM t1 LEFT JOIN t2 ON t1.b = t2.e AND t2.a > 3 WHERE (e <> (-2 % b));
-- expected
[[null]]
