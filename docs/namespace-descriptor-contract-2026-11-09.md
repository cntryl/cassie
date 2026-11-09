# Namespace catalog descriptor correction

For #817/#843, PostgreSQL 18.6 describes `pg_namespace.oid` as OID (26) and
`nspname` as NAME (19). `virtual_views_pg::namespace_schema` now exposes those
descriptors as OID and NAME. Jev selected this correction
(`target/assembly-review/namespace-descriptor-contract-*`); focused
text/binary, empty/nonempty, join and namespace identity/scope probes were
authored for the changed descriptors.

This changes the wire descriptors observed by existing clients. It does not
change stored namespace IDs, rename/restart behavior, row values, or database
scope. `nspowner` remains experimental INT and `nspacl` experimental TEXT; they
are not claimed to represent PostgreSQL owner or ACL semantics. The broader
#817 and extension lifecycle in #843 remain incomplete. The PostgreSQL oracle
was stopped, and the newly authored Cassie probes have not been executed.
