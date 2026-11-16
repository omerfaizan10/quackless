-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (13, 3, 'apple', 148.18), (3, 10, '', 122.57), (11, NULL, 'banana', 14.71), (NULL, 18, NULL, -49.82), (5, 18, NULL, -9.23), (-4, 8, NULL, 120.72), (-3, -5, 'banana', 95.22), (15, 0, 'banana', 17.73), (NULL, NULL, '', NULL), (20, 17, 'fig', -29.12);
INSERT INTO t2 VALUES (15, 0, 'Apple'), (-2, 12, 'Apple'), (NULL, -4, 'kiwi'), (7, 5, 'kiwi'), (2, 19, 'apple'), (16, NULL, 'Apple'), (4, 7, 'cherry'), (11, NULL, 'grape'), (6, 15, 'date'), (20, 13, 'date'), (NULL, 3, NULL), (13, 11, 'date'), (2, 17, 'grape'), (13, 16, 'cherry');
-- query
SELECT count(DISTINCT (e % e)) AS c0 FROM t1, t2 HAVING sum(12) < 3;
-- expected
[]
