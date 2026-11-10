# `date_trunc` unit validation

PostgreSQL 18.6 validates the `date_trunc` unit before returning an infinite
value. For timestamp, timestamptz and interval, `dow`, `doy`, `isodow`,
`isoyear`, `epoch` and `julian` are unrecognized (22023); timezone units are
recognized but unsupported (0A000). Interval `week` is also unsupported. These
results hold for both signs of infinity as well as finite inputs. Supported
units preserve infinities. The oracle also returned NULL for a NULL argument,
and an invalid explicit zone took priority over an unrecognized unit. The
oracle was stopped after the probes.

The three truncation entrypoints use the trunc-specific unit table and check
supported units before their infinity shortcut. Other field extraction paths
retain their existing table. The added SQL probes are authored but unexecuted
under the consolidated build/test deferral; they do not establish completion
of #815 or executable qualification.
