-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (7, -4, NULL, 23.41), (NULL, -4, 'banana', NULL), (0, 7, 'date', 78.96), (NULL, 16, 'Apple', -8.39), (5, -2, '', 38.89), (10, 14, 'date', 110.51), (NULL, 2, NULL, 42.94), (NULL, 3, 'kiwi', NULL), (NULL, 12, 'cherry', 96.42), (1, 0, NULL, 98.52), (-3, NULL, NULL, 129.93), (9, NULL, 'kiwi', -4.65), (20, 0, NULL, 108.58), (13, 18, 'cherry', -19.20), (11, 2, 'banana', 58.74), (-5, 1, 'fig', NULL), (15, 18, 'date', 138.26), (NULL, -2, 'kiwi', 106.24), (3, NULL, 'apple', 43.76), (-5, 3, 'Apple', -26.94), (-5, 6, 'fig', 91.94), (1, 6, 'apple', 148.36), (NULL, 19, 'kiwi', NULL), (20, 11, 'grape', -45.70), (11, 9, 'grape', 85.69), (-5, NULL, 'grape', 94.45), (14, 19, NULL, 36.72), (5, -3, 'apple', NULL), (4, 6, 'cherry', 113.18), (NULL, 18, 'apple', 9.53);
INSERT INTO t2 VALUES (NULL, 5, 'kiwi'), (3, 11, 'cherry'), (18, 4, 'banana'), (5, 5, NULL), (NULL, NULL, 'date'), (18, 5, 'Apple'), (NULL, 17, 'banana'), (13, 8, 'Apple'), (-1, 11, 'kiwi'), (7, 2, 'banana'), (-5, 20, NULL), (19, 14, 'cherry'), (16, 15, 'cherry');
-- query
SELECT (CAST(NULL AS INTEGER) * ((b % a) + (SELECT max(e) FROM t2 WHERE t2.e < 14))) AS c0 FROM t1;
-- expected
[[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null],[null]]
