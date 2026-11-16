-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (4, NULL, 'apple', 95.09), (17, 8, 'fig', 149.13), (17, 7, 'kiwi', -41.77), (10, NULL, NULL, 56.72), (NULL, 0, 'banana', 38.54), (NULL, NULL, NULL, 35.37), (5, 7, 'cherry', 70.10), (5, NULL, 'banana', NULL), (-5, 14, 'cherry', NULL), (9, -3, 'grape', 33.21), (NULL, 11, NULL, 6.54), (8, 8, 'date', 51.61), (11, 14, NULL, 20.78), (6, 12, 'kiwi', -26.40), (14, 15, '', 89.42), (4, 10, 'kiwi', 141.68), (0, 12, '', 41.77), (17, 9, 'cherry', 81.86), (-3, 9, NULL, 41.33), (3, 4, 'Apple', -48.66), (9, 18, NULL, 33.09), (1, 16, 'cherry', -16.00), (NULL, -4, 'cherry', -21.12), (5, 16, 'cherry', 105.63), (-1, 14, 'apple', 36.99), (14, 0, 'grape', 73.30), (5, 5, 'fig', NULL);
INSERT INTO t2 VALUES (-1, NULL, NULL), (1, 11, 'grape'), (3, 14, 'Apple'), (9, 17, 'fig'), (11, 3, 'Apple'), (-1, 17, NULL), (4, 16, 'Apple'), (8, -5, ''), (2, NULL, 'Apple'), (8, 7, 'fig'), (6, 6, NULL), (-3, 12, NULL), (-1, 7, 'apple'), (2, 3, 'banana'), (15, 9, NULL), (1, NULL, 'apple'), (13, 13, ''), (2, 2, '');
-- query
SELECT (SELECT max(e) FROM t2 WHERE t2.e < (SELECT max(e) FROM t2 WHERE t2.e < t1.a)) AS c0 FROM t1 JOIN t2 ON t1.a = t2.a;
-- expected
[[-5],[2],[2],[6],[6],[7],[2],[6],[6],[null],[null],[null],[null],[null],[null]]
