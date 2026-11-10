# Float `width_bucket` span and signed zero

PostgreSQL 18.6 produces buckets 6 and 8 for 0 and ±5e307 in ascending and
descending ranges spanning −1e308 to +1e308. It rejects +0 and −0 bounds as
equal with 2201G, clamps values just inside finite and overflowing bounds to
the last bucket, and handles infinite operands outside the range. The oracle
was stopped after these checks.

As in PostgreSQL REL_18_6 `float.c::width_bucket_float8`, the float path now
divides operands and bounds by two before the ratio only when the finite bound
span overflows, and compares zero bounds by numeric equality after NaN checks.
Existing validation and other paths are unchanged. The separate preexisting
INT4 count+1 overflow and count-type/descriptor behavior across float and exact
numeric routes remain open. Authored unit and SQL probes are unexecuted under
the consolidated gate deferral; this does not complete #813.
