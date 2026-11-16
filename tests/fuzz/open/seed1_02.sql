-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (NULL, NULL, 'kiwi', 19.58), (16, 12, NULL, 118.63), (2, NULL, NULL, 25.83), (14, 14, 'cherry', 39.24), (8, 17, 'apple', 46.99), (1, 4, 'cherry', 13.82), (17, -3, 'banana', 57.35), (NULL, 20, 'cherry', -7.46), (NULL, 13, 'banana', NULL), (7, 10, 'kiwi', 122.79), (17, 0, 'grape', 25.07), (20, NULL, 'kiwi', -5.56), (-4, 10, 'grape', 81.93), (20, NULL, 'banana', NULL), (15, 9, 'banana', 39.45), (4, 20, 'Apple', -29.71), (9, 15, '', -21.57), (-5, 13, 'apple', 108.14), (-2, -5, 'grape', 129.88);
INSERT INTO t2 VALUES (9, 17, ''), (12, -3, 'cherry'), (15, -1, 'banana'), (-3, 16, NULL), (1, 18, ''), (NULL, NULL, 'Apple'), (9, 6, 'kiwi'), (20, 7, ''), (12, -5, 'fig'), (4, 20, 'Apple'), (13, -3, 'date'), (2, NULL, ''), (-4, 10, 'cherry'), (-1, 10, 'banana');
-- query
SELECT 1.5 AS c0 FROM t1 LEFT JOIN t2 ON t1.b = t2.e AND t2.a > 3 WHERE (e BETWEEN 14 AND (-4 % (-2 + t1.a)));
-- expected
[]
