-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (14, NULL, 'banana', NULL), (-3, 9, 'Apple', -48.80), (11, -3, 'kiwi', 109.12), (13, 3, 'Apple', 38.48), (-3, NULL, 'banana', 127.72), (-1, -5, 'grape', 90.30), (16, 18, 'grape', 7.65), (-2, 17, NULL, -24.75), (-3, 0, 'fig', 33.09), (-5, 4, 'Apple', 93.99), (6, 1, NULL, 93.71), (20, NULL, 'date', -9.78), (10, 6, 'banana', 110.85), (19, NULL, 'kiwi', 36.83), (5, 14, 'grape', 79.72), (20, -3, 'kiwi', 132.65), (NULL, 5, 'Apple', 81.22), (13, NULL, 'apple', 75.71), (-2, 11, 'Apple', 53.77), (NULL, NULL, 'grape', 72.28), (19, 9, 'Apple', 135.94), (NULL, 12, 'cherry', 52.24), (NULL, -1, 'date', 41.29), (NULL, 8, 'Apple', 12.48), (14, 7, 'cherry', 12.93), (19, -1, 'kiwi', 127.09), (12, 12, 'date', 60.19), (NULL, 2, 'apple', -22.70), (17, 12, NULL, 140.16), (2, -4, 'apple', 48.30), (-2, 12, 'cherry', 13.68), (-5, 18, 'banana', -30.12), (3, 9, 'Apple', 54.70);
INSERT INTO t2 VALUES (17, 20, 'apple'), (14, 12, 'cherry'), (12, NULL, NULL), (7, 6, 'apple'), (11, 13, 'fig'), (1, 15, 'Apple'), (19, 5, 'Apple'), (15, 14, 'grape'), (-3, 15, 'grape'), (2, 10, 'banana'), (10, 13, 'fig'), (17, 1, 'fig'), (-5, 7, NULL), (-4, 14, ''), (5, 18, '');
-- query
SELECT max((t1.a / length(f))) AS c0 FROM t1, t2;
-- expected
[["inf"]]
