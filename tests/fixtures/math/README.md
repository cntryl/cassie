These exact NUMERIC result fixtures were captured together from PostgreSQL
18.6 (Debian 18.6-1.pgdg13+2). They cover every integer digit of `exp(5000)`
and the result scales of `power(1 + 10^-400, ±10^403)`.

The corresponding tests use static expected values and require no database
server. Range decisions follow the pinned upstream
[numeric implementation](https://github.com/postgres/postgres/blob/REL_18_6/src/backend/utils/adt/numeric.c).

Capture queries:

```sql
SELECT exp(5000::numeric)::text;
SELECT power(('1.' || repeat('0', 399) || '1')::numeric, 1e403::numeric)::text;
SELECT power(('1.' || repeat('0', 399) || '1')::numeric, -1e403::numeric)::text;
```
