-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (NULL, 1, 'fig', NULL), (-3, 19, 'cherry', -20.49), (2, 16, 'kiwi', -19.74), (12, -3, 'fig', -34.26), (-2, 15, 'date', -21.52), (9, 14, 'cherry', 19.98), (NULL, 7, 'date', 56.22), (-4, 10, 'banana', 100.45), (NULL, -2, 'date', NULL), (2, 12, 'date', NULL), (6, 12, 'date', 98.78), (16, 2, 'kiwi', NULL), (NULL, 7, 'cherry', 116.23), (NULL, -3, NULL, -49.59), (18, 19, 'cherry', NULL), (NULL, 12, NULL, 75.93), (-5, 14, 'grape', -24.65), (14, 18, NULL, -50.73), (-2, 12, 'date', 127.25), (8, NULL, 'banana', 100.06), (14, NULL, 'date', -25.12), (-1, -4, NULL, -11.01), (6, -5, 'Apple', NULL), (8, -3, '', 78.75), (7, 3, 'banana', 143.50), (-5, 12, 'grape', -37.94), (NULL, 15, NULL, 38.85), (-3, 9, NULL, 121.93), (7, NULL, '', 147.41), (19, 17, 'apple', 12.75), (15, 1, 'cherry', -38.57), (NULL, 18, NULL, NULL);
INSERT INTO t2 VALUES (2, 1, NULL), (1, NULL, 'date'), (NULL, 4, ''), (6, -2, NULL), (19, 8, 'date'), (2, 10, NULL), (13, 12, 'apple'), (2, -4, 'apple'), (NULL, -4, 'apple'), (5, 5, 'grape'), (11, NULL, 'fig'), (6, 2, 'Apple'), (NULL, 12, 'banana'), (16, 17, 'kiwi'), (0, 1, 'cherry'), (10, 12, ''), (16, 11, 'apple'), (-5, 1, NULL), (-1, 17, 'cherry');
-- query
SELECT (11 / 0) AS c0 FROM t1;
-- expected
[["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"],["inf"]]
