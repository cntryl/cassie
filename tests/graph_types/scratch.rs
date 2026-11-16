use crate::fixture::started;

#[test]
fn scratch() {
    let f = started("scratch", &[]);
    let sqls = std::fs::read_to_string("/tmp/graph_scratch.sql").unwrap_or_default();
    for sql in sqls.lines().filter(|l| !l.trim().is_empty()) {
        match f.execute(sql) {
            Ok(r) => eprintln!("OK  {sql}\n    -> {:?}", r.rows()),
            Err(e) => eprintln!("ERR {sql}\n    -> {} {}", e.sql_state(), e),
        }
    }
}
