-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (4, 13, 'date', 61.82), (NULL, 5, 'kiwi', 113.87), (NULL, 8, NULL, -15.68), (1, 20, NULL, 144.93), (-2, 3, NULL, -21.39), (11, NULL, 'kiwi', 100.52), (8, 10, 'grape', NULL), (10, -4, 'kiwi', -11.17), (7, -5, NULL, NULL), (3, 11, 'date', 119.08), (NULL, 11, 'banana', 88.41), (-5, -4, NULL, 1.68), (5, NULL, 'fig', 5.14), (2, 10, 'date', 103.67), (20, NULL, 'cherry', 77.77), (16, 5, 'cherry', 148.36), (17, 14, 'banana', 104.14), (-5, 18, 'fig', 105.22), (18, NULL, 'Apple', 64.24), (NULL, 10, 'apple', 133.94), (19, NULL, 'cherry', -42.54), (5, NULL, 'grape', 117.48), (16, 15, 'Apple', 137.35), (9, NULL, '', -3.76), (1, NULL, 'kiwi', 21.17), (14, -5, 'fig', 12.03), (19, 6, NULL, NULL);
INSERT INTO t2 VALUES (11, 8, NULL), (-5, -5, 'date'), (-4, NULL, 'apple'), (18, 15, NULL), (NULL, NULL, NULL), (-3, 13, 'Apple'), (15, -2, 'kiwi'), (12, 12, ''), (17, NULL, 'apple'), (18, -3, 'grape'), (1, 7, NULL), (-5, 2, NULL), (9, 20, 'banana'), (14, 9, 'Apple'), (15, 1, 'date'), (15, NULL, ''), (2, 0, ''), (6, 2, 'date'), (18, -2, 'date');
-- query
SELECT e AS c0 FROM t2 WHERE ((a = (a % e)) AND (a IN (e, e, e)));
-- expected
[]
