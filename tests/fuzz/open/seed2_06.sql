-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (1, 17, NULL, 22.84), (-2, 9, 'Apple', NULL), (8, 4, '', NULL), (NULL, -4, 'apple', 1.93), (3, 10, NULL, 146.20), (NULL, NULL, 'grape', -30.50), (10, 7, 'fig', NULL), (9, 7, 'cherry', 28.88), (2, 19, 'banana', 42.95), (11, NULL, 'fig', NULL), (8, 5, 'fig', 13.74), (NULL, 2, 'grape', 72.66), (-1, NULL, 'kiwi', -13.16), (10, NULL, 'grape', NULL), (10, 6, '', 42.51), (-1, 7, 'grape', 67.81);
INSERT INTO t2 VALUES (1, 3, 'kiwi'), (17, 7, ''), (14, NULL, 'fig'), (7, NULL, 'cherry'), (-2, NULL, 'Apple'), (9, 17, 'cherry'), (12, 12, 'cherry'), (-2, 0, ''), (-4, NULL, 'date'), (17, NULL, NULL), (NULL, NULL, 'apple'), (5, 2, 'Apple'), (13, 14, 'kiwi'), (19, 5, 'Apple'), (-1, 19, 'grape'), (6, -2, 'kiwi'), (17, 18, 'Apple'), (8, NULL, 'grape'), (-1, 12, ''), (15, 1, 'cherry'), (NULL, 18, 'cherry'), (7, 2, NULL);
-- query
SELECT (sum((a % 0)) + count(*)) AS c0 FROM t2 HAVING max((- 9)) >= 0;
-- expected
[]
