-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (2, 16, 'Apple', -31.81), (-3, 1, 'grape', NULL), (7, 7, 'apple', 145.91), (12, 16, NULL, 59.51), (4, 20, NULL, 126.21), (9, 4, 'Apple', 75.75), (13, 10, 'banana', NULL), (-2, NULL, 'fig', 62.69), (-4, 18, 'date', 25.42), (-2, NULL, 'Apple', 18.42), (-3, 12, 'kiwi', -39.24), (18, NULL, 'grape', 53.18), (13, NULL, '', 14.59), (9, NULL, 'Apple', 32.88), (20, 3, '', 67.47), (-3, NULL, '', 66.10), (0, -3, 'Apple', -2.20), (14, 0, 'date', 12.82), (13, 9, 'Apple', 128.62), (NULL, 19, NULL, 98.75), (-2, -5, 'grape', NULL), (NULL, -2, 'cherry', NULL), (11, -5, NULL, 70.56);
INSERT INTO t2 VALUES (NULL, 5, ''), (12, 20, 'apple'), (12, -1, 'kiwi'), (-3, 0, 'kiwi'), (3, 9, NULL), (3, 0, 'cherry'), (6, 7, 'kiwi'), (8, 15, 'fig'), (-1, 14, 'date'), (9, 13, 'fig'), (9, 13, 'fig'), (14, 17, ''), (7, 17, 'banana');
-- query
SELECT count(DISTINCT CASE WHEN TRUE THEN 1 ELSE (a % a) END) AS c0 FROM t1;
-- expected
[[1]]
