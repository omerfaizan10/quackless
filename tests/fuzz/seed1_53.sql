-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (-1, NULL, '', NULL), (3, 5, 'grape', -44.63), (10, -3, 'grape', -9.59), (4, -1, 'grape', -43.36), (6, 15, '', 132.22), (0, 10, 'grape', 46.56), (3, 1, 'fig', 120.17);
INSERT INTO t2 VALUES (16, 17, 'apple'), (7, 17, 'apple'), (0, NULL, 'fig'), (-4, 9, 'banana'), (-4, 15, 'kiwi'), (17, 1, 'Apple'), (4, 1, NULL), (17, 16, 'Apple'), (18, NULL, 'cherry'), (20, 0, 'grape'), (-2, 2, 'apple'), (NULL, 15, 'grape'), (-4, 17, NULL), (11, 16, 'fig');
-- query
SELECT max((b / e)) AS c0 FROM t1, t2;
-- expected
[["inf"]]
