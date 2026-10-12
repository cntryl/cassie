//! Literal typed rows and command tags decoded from retained PostgreSQL 18.6 observations.
//! RETURNING order is not asserted: all snapshots are compared as bags sorted by unique id.
use super::{Case, ExpectedSnapshot, Post};

const COLUMNS_0: &[(&str, i64, i16, i32)] = &[
    ("row_id", 23, 4, -1),
    ("integer_value", 20, 8, -1),
    ("integer_control", 20, 8, -1),
    ("float_value", 701, 8, -1),
    ("marker", 25, -1, -1),
    ("eq", 16, 1, -1),
    ("ne", 16, 1, -1),
    ("lt", 16, 1, -1),
    ("le", 16, 1, -1),
    ("gt", 16, 1, -1),
    ("ge", 16, 1, -1),
    ("same", 16, 1, -1),
    ("different", 16, 1, -1),
];
const COLUMNS_1: &[(&str, i64, i16, i32)] = &[
    ("id", 23, 4, -1),
    ("k", 20, 8, -1),
    ("j", 20, 8, -1),
    ("f", 701, 8, -1),
    ("mark", 25, -1, -1),
];
const COLUMNS_2: &[(&str, i64, i16, i32)] = &[
    ("id", 23, 4, -1),
    ("k", 20, 8, -1),
    ("j", 20, 8, -1),
    ("f", 701, 8, -1),
    ("eq", 16, 1, -1),
    ("ne", 16, 1, -1),
    ("lt", 16, 1, -1),
    ("le", 16, 1, -1),
    ("gt", 16, 1, -1),
    ("ge", 16, 1, -1),
    ("same", 16, 1, -1),
    ("different", 16, 1, -1),
];
const COLUMNS_3: &[(&str, i64, i16, i32)] = &[];

const EXPECTED_0: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"selected"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"selected"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"selected"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "UPDATE 3",
};
const EXPECTED_1: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_1,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"selected"}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"selected"}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":4},"Null","Null","Null",{"String":"original"}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"original"}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"original"}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"selected"}]]"#,
    command: "SELECT 7",
};
const EXPECTED_2: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"selected"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "UPDATE 1",
};
const EXPECTED_3: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_1,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"selected"}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":4},"Null","Null","Null",{"String":"original"}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"original"}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"original"}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"original"}]]"#,
    command: "SELECT 7",
};
const EXPECTED_4: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"selected"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"selected"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":4},"Null","Null","Null",{"String":"selected"},"Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"selected"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "UPDATE 4",
};
const EXPECTED_5: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_1,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"selected"}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"selected"}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":4},"Null","Null","Null",{"String":"selected"}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"original"}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"original"}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"selected"}]]"#,
    command: "SELECT 7",
};
const EXPECTED_6: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"selected"},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"selected"},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"selected"},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}]]"#,
    command: "UPDATE 3",
};
const EXPECTED_7: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_1,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"selected"}],[{"Int64":4},"Null","Null","Null",{"String":"original"}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"selected"}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"selected"}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"original"}]]"#,
    command: "SELECT 7",
};
const EXPECTED_8: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"selected"},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"selected"},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"selected"},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}]]"#,
    command: "UPDATE 3",
};
const EXPECTED_9: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"original"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "DELETE 3",
};
const EXPECTED_10: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_1,
    rows: r#"[[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":4},"Null","Null","Null",{"String":"original"}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"original"}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"original"}]]"#,
    command: "SELECT 4",
};
const EXPECTED_11: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "DELETE 1",
};
const EXPECTED_12: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_1,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":4},"Null","Null","Null",{"String":"original"}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"original"}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"original"}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"original"}]]"#,
    command: "SELECT 6",
};
const EXPECTED_13: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":4},"Null","Null","Null",{"String":"original"},"Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"original"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "DELETE 4",
};
const EXPECTED_14: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_1,
    rows: r#"[[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"original"}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"original"}]]"#,
    command: "SELECT 3",
};
const EXPECTED_15: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"original"},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"original"},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}]]"#,
    command: "DELETE 3",
};
const EXPECTED_16: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_1,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":4},"Null","Null","Null",{"String":"original"}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"original"}]]"#,
    command: "SELECT 4",
};
const EXPECTED_17: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"original"},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"original"},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}]]"#,
    command: "DELETE 3",
};
const EXPECTED_18: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"copied"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"copied"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"copied"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "INSERT 0 3",
};
const EXPECTED_19: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_1,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"copied"}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"copied"}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"copied"}]]"#,
    command: "SELECT 3",
};
const EXPECTED_20: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_1,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"original"}],[{"Int64":4},"Null","Null","Null",{"String":"original"}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"original"}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"original"}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"original"}]]"#,
    command: "SELECT 7",
};
const EXPECTED_21: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"copied"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "INSERT 0 1",
};
const EXPECTED_22: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_1,
    rows: r#"[[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"copied"}]]"#,
    command: "SELECT 1",
};
const EXPECTED_23: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"copied"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"copied"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":4},"Null","Null","Null",{"String":"copied"},"Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"copied"},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "INSERT 0 4",
};
const EXPECTED_24: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_1,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"copied"}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"copied"}],[{"Int64":4},"Null","Null","Null",{"String":"copied"}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"String":"copied"}]]"#,
    command: "SELECT 4",
};
const EXPECTED_25: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"copied"},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"copied"},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"copied"},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}]]"#,
    command: "INSERT 0 3",
};
const EXPECTED_26: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_1,
    rows: r#"[[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"copied"}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"copied"}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"copied"}]]"#,
    command: "SELECT 3",
};
const EXPECTED_27: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_0,
    rows: r#"[[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"String":"copied"},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":5},{"Int64":7},"Null","Null",{"String":"copied"},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},{"String":"copied"},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}]]"#,
    command: "INSERT 0 3",
};
const EXPECTED_28: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "UPDATE 7",
};
const EXPECTED_29: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "SELECT 7",
};
const EXPECTED_30: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "UPDATE 7",
};
const EXPECTED_31: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "SELECT 7",
};
const EXPECTED_32: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}]]"#,
    command: "UPDATE 7",
};
const EXPECTED_33: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}]]"#,
    command: "SELECT 7",
};
const EXPECTED_34: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}]]"#,
    command: "UPDATE 7",
};
const EXPECTED_35: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}]]"#,
    command: "SELECT 7",
};
const EXPECTED_36: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "INSERT 0 7",
};
const EXPECTED_37: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"]]"#,
    command: "SELECT 7",
};
const EXPECTED_38: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "INSERT 0 7",
};
const EXPECTED_39: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}]]"#,
    command: "INSERT 0 7",
};
const EXPECTED_40: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}]]"#,
    command: "INSERT 0 7",
};
const EXPECTED_41: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "INSERT 0 3",
};
const EXPECTED_42: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "SELECT 7",
};
const EXPECTED_43: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "INSERT 0 4",
};
const EXPECTED_44: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "SELECT 7",
};
const EXPECTED_45: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}]]"#,
    command: "INSERT 0 3",
};
const EXPECTED_46: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"]]"#,
    command: "SELECT 7",
};
const EXPECTED_47: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}]]"#,
    command: "INSERT 0 3",
};
const EXPECTED_48: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false},{"Bool":true}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null",{"Bool":false},{"Bool":true}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"]]"#,
    command: "SELECT 7",
};
const EXPECTED_49: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}]]"#,
    command: "INSERT 0 2",
};
const EXPECTED_50: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":3},{"Int64":9007199254740991},{"Int64":9007199254740992},{"Float64":9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":4},"Null","Null","Null","Null","Null","Null","Null","Null","Null",{"Bool":true},{"Bool":false}],[{"Int64":5},{"Int64":7},"Null","Null","Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":6},"Null",{"Int64":7},{"Float64":7.0},"Null","Null","Null","Null","Null","Null","Null","Null"],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},"Null","Null","Null","Null","Null","Null","Null","Null"]]"#,
    command: "SELECT 7",
};
const EXPECTED_51: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_2,
    rows: r#"[[{"Int64":1},{"Int64":9007199254740993},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":2},{"Int64":9007199254740992},{"Int64":9007199254740992},{"Float64":9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}],[{"Int64":7},{"Int64":-9007199254740993},{"Int64":-9007199254740992},{"Float64":-9007199254740992.0},{"Bool":true},{"Bool":false},{"Bool":false},{"Bool":true},{"Bool":false},{"Bool":true},{"Bool":true},{"Bool":false}]]"#,
    command: "UPDATE 3",
};
const EXPECTED_52: ExpectedSnapshot = ExpectedSnapshot {
    columns: COLUMNS_3,
    rows: r#"[]"#,
    command: "ROLLBACK",
};

const SETUP_0: &[&str] = &[
    "CREATE TABLE dml_mix_source (id INT PRIMARY KEY, k BIGINT, j BIGINT, f FLOAT, mark TEXT)",
    "DELETE FROM dml_mix_source",
    "INSERT INTO dml_mix_source (id,k,j,f,mark) VALUES (1,9007199254740993,9007199254740992,9007199254740992.0,'original'), (2,9007199254740992,9007199254740992,9007199254740992.0,'original'), (3,9007199254740991,9007199254740992,9007199254740992.0,'original'), (4,NULL,NULL,NULL,'original'), (5,7,NULL,NULL,'original'), (6,NULL,7,7.0,'original'), (7,-9007199254740993,-9007199254740992,-9007199254740992.0,'original')",
];
const SETUP_1: &[&str] = &[
    "CREATE TABLE dml_mix_source (id INT PRIMARY KEY, k BIGINT, j BIGINT, f FLOAT, mark TEXT)",
    "CREATE TABLE dml_mix_target (id INT PRIMARY KEY, k BIGINT, j BIGINT, f FLOAT, mark TEXT)",
    "DELETE FROM dml_mix_source",
    "INSERT INTO dml_mix_source (id,k,j,f,mark) VALUES (1,9007199254740993,9007199254740992,9007199254740992.0,'original'), (2,9007199254740992,9007199254740992,9007199254740992.0,'original'), (3,9007199254740991,9007199254740992,9007199254740992.0,'original'), (4,NULL,NULL,NULL,'original'), (5,7,NULL,NULL,'original'), (6,NULL,7,7.0,'original'), (7,-9007199254740993,-9007199254740992,-9007199254740992.0,'original')",
    "DELETE FROM dml_mix_target",
];
const SETUP_2: &[&str] = &[
    "CREATE TABLE expr_mix_target (id INT PRIMARY KEY, k BIGINT, j BIGINT, f FLOAT, eq BOOLEAN, ne BOOLEAN, lt BOOLEAN, le BOOLEAN, gt BOOLEAN, ge BOOLEAN, same BOOLEAN, different BOOLEAN)",
    "DELETE FROM expr_mix_target",
    "INSERT INTO expr_mix_target (id,k,j,f) VALUES (1,9007199254740993,9007199254740992,9007199254740992.0), (2,9007199254740992,9007199254740992,9007199254740992.0), (3,9007199254740991,9007199254740992,9007199254740992.0), (4,NULL,NULL,NULL), (5,7,NULL,NULL), (6,NULL,7,7.0), (7,-9007199254740993,-9007199254740992,-9007199254740992.0)",
];
const SETUP_3: &[&str] = &[
    "CREATE TABLE expr_mix_target (id INT PRIMARY KEY, k BIGINT, j BIGINT, f FLOAT, eq BOOLEAN, ne BOOLEAN, lt BOOLEAN, le BOOLEAN, gt BOOLEAN, ge BOOLEAN, same BOOLEAN, different BOOLEAN)",
    "CREATE TABLE expr_mix_source (id INT PRIMARY KEY, k BIGINT, j BIGINT, f FLOAT, eq BOOLEAN, ne BOOLEAN, lt BOOLEAN, le BOOLEAN, gt BOOLEAN, ge BOOLEAN, same BOOLEAN, different BOOLEAN)",
    "DELETE FROM expr_mix_target",
    "DELETE FROM expr_mix_source",
    "INSERT INTO expr_mix_source (id,k,j,f) VALUES (1,9007199254740993,9007199254740992,9007199254740992.0), (2,9007199254740992,9007199254740992,9007199254740992.0), (3,9007199254740991,9007199254740992,9007199254740992.0), (4,NULL,NULL,NULL), (5,7,NULL,NULL), (6,NULL,7,7.0), (7,-9007199254740993,-9007199254740992,-9007199254740992.0)",
];
const SETUP_4: &[&str] = &[
    "CREATE TABLE final_mix_conflict (id INT PRIMARY KEY,k BIGINT,j BIGINT,f FLOAT,eq BOOLEAN,ne BOOLEAN,lt BOOLEAN,le BOOLEAN,gt BOOLEAN,ge BOOLEAN,same BOOLEAN,different BOOLEAN)",
    "DELETE FROM final_mix_conflict",
    "INSERT INTO final_mix_conflict (id,k,j,f) VALUES (1,9007199254740993,9007199254740992,9007199254740992.0), (2,9007199254740992,9007199254740992,9007199254740992.0), (3,9007199254740991,9007199254740992,9007199254740992.0), (4,NULL,NULL,NULL), (5,7,NULL,NULL), (6,NULL,7,7.0), (7,-9007199254740993,-9007199254740992,-9007199254740992.0)",
];

pub const UPDATE: &[Case] = &[
    Case { id: "update:mixed_columns-equal-if:simple_text", setup: SETUP_0, before: &[], sql: "UPDATE dml_mix_source SET mark = 'selected' WHERE k = f RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, k = f AS eq, k <> f AS ne, k < f AS lt, k <= f AS le, k > f AS gt, k >= f AS ge, k IS NOT DISTINCT FROM f AS same, k IS DISTINCT FROM f AS different", expected: Some(&EXPECTED_0), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_1 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "update:pure_bigint_control-equal-if:simple_text", setup: SETUP_0, before: &[], sql: "UPDATE dml_mix_source SET mark = 'selected' WHERE k = j RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, k = j AS eq, k <> j AS ne, k < j AS lt, k <= j AS le, k > j AS gt, k >= j AS ge, k IS NOT DISTINCT FROM j AS same, k IS DISTINCT FROM j AS different", expected: Some(&EXPECTED_2), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_3 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "update:mixed_columns-equal-fi:simple_text", setup: SETUP_0, before: &[], sql: "UPDATE dml_mix_source SET mark = 'selected' WHERE f = k RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, f = k AS eq, f <> k AS ne, f < k AS lt, f <= k AS le, f > k AS gt, f >= k AS ge, f IS NOT DISTINCT FROM k AS same, f IS DISTINCT FROM k AS different", expected: Some(&EXPECTED_0), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_1 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "update:pure_bigint_control-equal-fi:simple_text", setup: SETUP_0, before: &[], sql: "UPDATE dml_mix_source SET mark = 'selected' WHERE j = k RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, j = k AS eq, j <> k AS ne, j < k AS lt, j <= k AS le, j > k AS gt, j >= k AS ge, j IS NOT DISTINCT FROM k AS same, j IS DISTINCT FROM k AS different", expected: Some(&EXPECTED_2), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_3 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "update:mixed_columns-not_distinct-if:simple_text", setup: SETUP_0, before: &[], sql: "UPDATE dml_mix_source SET mark = 'selected' WHERE k IS NOT DISTINCT FROM f RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, k = f AS eq, k <> f AS ne, k < f AS lt, k <= f AS le, k > f AS gt, k >= f AS ge, k IS NOT DISTINCT FROM f AS same, k IS DISTINCT FROM f AS different", expected: Some(&EXPECTED_4), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_5 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "update:mixed_columns-not_distinct-fi:simple_text", setup: SETUP_0, before: &[], sql: "UPDATE dml_mix_source SET mark = 'selected' WHERE f IS NOT DISTINCT FROM k RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, f = k AS eq, f <> k AS ne, f < k AS lt, f <= k AS le, f > k AS gt, f >= k AS ge, f IS NOT DISTINCT FROM k AS same, f IS DISTINCT FROM k AS different", expected: Some(&EXPECTED_4), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_5 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "update:mixed_columns-distinct-if:simple_text", setup: SETUP_0, before: &[], sql: "UPDATE dml_mix_source SET mark = 'selected' WHERE k IS DISTINCT FROM f RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, k = f AS eq, k <> f AS ne, k < f AS lt, k <= f AS le, k > f AS gt, k >= f AS ge, k IS NOT DISTINCT FROM f AS same, k IS DISTINCT FROM f AS different", expected: Some(&EXPECTED_6), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_7 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "update:mixed_columns-distinct-fi:simple_text", setup: SETUP_0, before: &[], sql: "UPDATE dml_mix_source SET mark = 'selected' WHERE f IS DISTINCT FROM k RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, f = k AS eq, f <> k AS ne, f < k AS lt, f <= k AS le, f > k AS gt, f >= k AS ge, f IS NOT DISTINCT FROM k AS same, f IS DISTINCT FROM k AS different", expected: Some(&EXPECTED_8), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_7 }], recovery: "SELECT 1 AS recovery_marker" },
];

pub const DELETE: &[Case] = &[
    Case { id: "delete:mixed_columns-equal-if:simple_text", setup: SETUP_0, before: &[], sql: "DELETE FROM dml_mix_source WHERE k = f RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, k = f AS eq, k <> f AS ne, k < f AS lt, k <= f AS le, k > f AS gt, k >= f AS ge, k IS NOT DISTINCT FROM f AS same, k IS DISTINCT FROM f AS different", expected: Some(&EXPECTED_9), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_10 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "delete:pure_bigint_control-equal-if:simple_text", setup: SETUP_0, before: &[], sql: "DELETE FROM dml_mix_source WHERE k = j RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, k = j AS eq, k <> j AS ne, k < j AS lt, k <= j AS le, k > j AS gt, k >= j AS ge, k IS NOT DISTINCT FROM j AS same, k IS DISTINCT FROM j AS different", expected: Some(&EXPECTED_11), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_12 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "delete:mixed_columns-equal-fi:simple_text", setup: SETUP_0, before: &[], sql: "DELETE FROM dml_mix_source WHERE f = k RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, f = k AS eq, f <> k AS ne, f < k AS lt, f <= k AS le, f > k AS gt, f >= k AS ge, f IS NOT DISTINCT FROM k AS same, f IS DISTINCT FROM k AS different", expected: Some(&EXPECTED_9), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_10 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "delete:pure_bigint_control-equal-fi:simple_text", setup: SETUP_0, before: &[], sql: "DELETE FROM dml_mix_source WHERE j = k RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, j = k AS eq, j <> k AS ne, j < k AS lt, j <= k AS le, j > k AS gt, j >= k AS ge, j IS NOT DISTINCT FROM k AS same, j IS DISTINCT FROM k AS different", expected: Some(&EXPECTED_11), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_12 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "delete:mixed_columns-not_distinct-if:simple_text", setup: SETUP_0, before: &[], sql: "DELETE FROM dml_mix_source WHERE k IS NOT DISTINCT FROM f RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, k = f AS eq, k <> f AS ne, k < f AS lt, k <= f AS le, k > f AS gt, k >= f AS ge, k IS NOT DISTINCT FROM f AS same, k IS DISTINCT FROM f AS different", expected: Some(&EXPECTED_13), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_14 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "delete:mixed_columns-not_distinct-fi:simple_text", setup: SETUP_0, before: &[], sql: "DELETE FROM dml_mix_source WHERE f IS NOT DISTINCT FROM k RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, f = k AS eq, f <> k AS ne, f < k AS lt, f <= k AS le, f > k AS gt, f >= k AS ge, f IS NOT DISTINCT FROM k AS same, f IS DISTINCT FROM k AS different", expected: Some(&EXPECTED_13), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_14 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "delete:mixed_columns-distinct-if:simple_text", setup: SETUP_0, before: &[], sql: "DELETE FROM dml_mix_source WHERE k IS DISTINCT FROM f RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, k = f AS eq, k <> f AS ne, k < f AS lt, k <= f AS le, k > f AS gt, k >= f AS ge, k IS NOT DISTINCT FROM f AS same, k IS DISTINCT FROM f AS different", expected: Some(&EXPECTED_15), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_16 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "delete:mixed_columns-distinct-fi:simple_text", setup: SETUP_0, before: &[], sql: "DELETE FROM dml_mix_source WHERE f IS DISTINCT FROM k RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, f = k AS eq, f <> k AS ne, f < k AS lt, f <= k AS le, f > k AS gt, f >= k AS ge, f IS NOT DISTINCT FROM k AS same, f IS DISTINCT FROM k AS different", expected: Some(&EXPECTED_17), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_16 }], recovery: "SELECT 1 AS recovery_marker" },
];

pub const INSERT_SELECT: &[Case] = &[
    Case { id: "insert_select:mixed_columns-equal-if:simple_text", setup: SETUP_1, before: &[], sql: "INSERT INTO dml_mix_target (id,k,j,f,mark) SELECT id,k,j,f,'copied' FROM dml_mix_source WHERE k = f RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, k = f AS eq, k <> f AS ne, k < f AS lt, k <= f AS le, k > f AS gt, k >= f AS ge, k IS NOT DISTINCT FROM f AS same, k IS DISTINCT FROM f AS different", expected: Some(&EXPECTED_18), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_target ORDER BY id", expected: &EXPECTED_19 }, Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_20 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "insert_select:pure_bigint_control-equal-if:simple_text", setup: SETUP_1, before: &[], sql: "INSERT INTO dml_mix_target (id,k,j,f,mark) SELECT id,k,j,f,'copied' FROM dml_mix_source WHERE k = j RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, k = j AS eq, k <> j AS ne, k < j AS lt, k <= j AS le, k > j AS gt, k >= j AS ge, k IS NOT DISTINCT FROM j AS same, k IS DISTINCT FROM j AS different", expected: Some(&EXPECTED_21), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_target ORDER BY id", expected: &EXPECTED_22 }, Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_20 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "insert_select:mixed_columns-equal-fi:simple_text", setup: SETUP_1, before: &[], sql: "INSERT INTO dml_mix_target (id,k,j,f,mark) SELECT id,k,j,f,'copied' FROM dml_mix_source WHERE f = k RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, f = k AS eq, f <> k AS ne, f < k AS lt, f <= k AS le, f > k AS gt, f >= k AS ge, f IS NOT DISTINCT FROM k AS same, f IS DISTINCT FROM k AS different", expected: Some(&EXPECTED_18), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_target ORDER BY id", expected: &EXPECTED_19 }, Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_20 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "insert_select:pure_bigint_control-equal-fi:simple_text", setup: SETUP_1, before: &[], sql: "INSERT INTO dml_mix_target (id,k,j,f,mark) SELECT id,k,j,f,'copied' FROM dml_mix_source WHERE j = k RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, j = k AS eq, j <> k AS ne, j < k AS lt, j <= k AS le, j > k AS gt, j >= k AS ge, j IS NOT DISTINCT FROM k AS same, j IS DISTINCT FROM k AS different", expected: Some(&EXPECTED_21), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_target ORDER BY id", expected: &EXPECTED_22 }, Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_20 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "insert_select:mixed_columns-not_distinct-if:simple_text", setup: SETUP_1, before: &[], sql: "INSERT INTO dml_mix_target (id,k,j,f,mark) SELECT id,k,j,f,'copied' FROM dml_mix_source WHERE k IS NOT DISTINCT FROM f RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, k = f AS eq, k <> f AS ne, k < f AS lt, k <= f AS le, k > f AS gt, k >= f AS ge, k IS NOT DISTINCT FROM f AS same, k IS DISTINCT FROM f AS different", expected: Some(&EXPECTED_23), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_target ORDER BY id", expected: &EXPECTED_24 }, Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_20 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "insert_select:mixed_columns-not_distinct-fi:simple_text", setup: SETUP_1, before: &[], sql: "INSERT INTO dml_mix_target (id,k,j,f,mark) SELECT id,k,j,f,'copied' FROM dml_mix_source WHERE f IS NOT DISTINCT FROM k RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, f = k AS eq, f <> k AS ne, f < k AS lt, f <= k AS le, f > k AS gt, f >= k AS ge, f IS NOT DISTINCT FROM k AS same, f IS DISTINCT FROM k AS different", expected: Some(&EXPECTED_23), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_target ORDER BY id", expected: &EXPECTED_24 }, Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_20 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "insert_select:mixed_columns-distinct-if:simple_text", setup: SETUP_1, before: &[], sql: "INSERT INTO dml_mix_target (id,k,j,f,mark) SELECT id,k,j,f,'copied' FROM dml_mix_source WHERE k IS DISTINCT FROM f RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, k = f AS eq, k <> f AS ne, k < f AS lt, k <= f AS le, k > f AS gt, k >= f AS ge, k IS NOT DISTINCT FROM f AS same, k IS DISTINCT FROM f AS different", expected: Some(&EXPECTED_25), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_target ORDER BY id", expected: &EXPECTED_26 }, Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_20 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "insert_select:mixed_columns-distinct-fi:simple_text", setup: SETUP_1, before: &[], sql: "INSERT INTO dml_mix_target (id,k,j,f,mark) SELECT id,k,j,f,'copied' FROM dml_mix_source WHERE f IS DISTINCT FROM k RETURNING id AS row_id, k AS integer_value, j AS integer_control, f AS float_value, mark AS marker, f = k AS eq, f <> k AS ne, f < k AS lt, f <= k AS le, f > k AS gt, f >= k AS ge, f IS NOT DISTINCT FROM k AS same, f IS DISTINCT FROM k AS different", expected: Some(&EXPECTED_27), posts: &[Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_target ORDER BY id", expected: &EXPECTED_26 }, Post { sql: "SELECT id,k,j,f,mark FROM dml_mix_source ORDER BY id", expected: &EXPECTED_20 }], recovery: "SELECT 1 AS recovery_marker" },
];

pub const UPDATE_SET: &[Case] = &[
    Case { id: "update_set:mixed-if:simple_text", setup: SETUP_2, before: &[], sql: "UPDATE expr_mix_target SET eq = k = f, ne = k <> f, lt = k < f, le = k <= f, gt = k > f, ge = k >= f, same = k IS NOT DISTINCT FROM f, different = k IS DISTINCT FROM f RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_28), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM expr_mix_target ORDER BY id", expected: &EXPECTED_29 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "update_set:mixed-fi:simple_text", setup: SETUP_2, before: &[], sql: "UPDATE expr_mix_target SET eq = f = k, ne = f <> k, lt = f < k, le = f <= k, gt = f > k, ge = f >= k, same = f IS NOT DISTINCT FROM k, different = f IS DISTINCT FROM k RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_30), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM expr_mix_target ORDER BY id", expected: &EXPECTED_31 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "update_set:pure_bigint-if:simple_text", setup: SETUP_2, before: &[], sql: "UPDATE expr_mix_target SET eq = k = j, ne = k <> j, lt = k < j, le = k <= j, gt = k > j, ge = k >= j, same = k IS NOT DISTINCT FROM j, different = k IS DISTINCT FROM j RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_32), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM expr_mix_target ORDER BY id", expected: &EXPECTED_33 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "update_set:pure_bigint-fi:simple_text", setup: SETUP_2, before: &[], sql: "UPDATE expr_mix_target SET eq = j = k, ne = j <> k, lt = j < k, le = j <= k, gt = j > k, ge = j >= k, same = j IS NOT DISTINCT FROM k, different = j IS DISTINCT FROM k RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_34), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM expr_mix_target ORDER BY id", expected: &EXPECTED_35 }], recovery: "SELECT 1 AS recovery_marker" },
];

pub const INSERT_SELECT_PROJECTION: &[Case] = &[
    Case { id: "insert_select_projection:mixed-if:simple_text", setup: SETUP_3, before: &[], sql: "INSERT INTO expr_mix_target (id,k,j,f,eq,ne,lt,le,gt,ge,same,different) SELECT id,k,j,f,k = f, k <> f, k < f, k <= f, k > f, k >= f, k IS NOT DISTINCT FROM f, k IS DISTINCT FROM f FROM expr_mix_source RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_36), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM expr_mix_target ORDER BY id", expected: &EXPECTED_29 }, Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM expr_mix_source ORDER BY id", expected: &EXPECTED_37 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "insert_select_projection:mixed-fi:simple_text", setup: SETUP_3, before: &[], sql: "INSERT INTO expr_mix_target (id,k,j,f,eq,ne,lt,le,gt,ge,same,different) SELECT id,k,j,f,f = k, f <> k, f < k, f <= k, f > k, f >= k, f IS NOT DISTINCT FROM k, f IS DISTINCT FROM k FROM expr_mix_source RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_38), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM expr_mix_target ORDER BY id", expected: &EXPECTED_31 }, Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM expr_mix_source ORDER BY id", expected: &EXPECTED_37 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "insert_select_projection:pure_bigint-if:simple_text", setup: SETUP_3, before: &[], sql: "INSERT INTO expr_mix_target (id,k,j,f,eq,ne,lt,le,gt,ge,same,different) SELECT id,k,j,f,k = j, k <> j, k < j, k <= j, k > j, k >= j, k IS NOT DISTINCT FROM j, k IS DISTINCT FROM j FROM expr_mix_source RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_39), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM expr_mix_target ORDER BY id", expected: &EXPECTED_33 }, Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM expr_mix_source ORDER BY id", expected: &EXPECTED_37 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "insert_select_projection:pure_bigint-fi:simple_text", setup: SETUP_3, before: &[], sql: "INSERT INTO expr_mix_target (id,k,j,f,eq,ne,lt,le,gt,ge,same,different) SELECT id,k,j,f,j = k, j <> k, j < k, j <= k, j > k, j >= k, j IS NOT DISTINCT FROM k, j IS DISTINCT FROM k FROM expr_mix_source RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_40), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM expr_mix_target ORDER BY id", expected: &EXPECTED_35 }, Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM expr_mix_source ORDER BY id", expected: &EXPECTED_37 }], recovery: "SELECT 1 AS recovery_marker" },
];

pub const ON_CONFLICT: &[Case] = &[
    Case { id: "final_supplement:literal-upsert-mixed-=-if:simple_text", setup: SETUP_4, before: &[], sql: "INSERT INTO final_mix_conflict (id,k,j,f) VALUES (1,9007199254740993,9007199254740992,9007199254740992.0), (2,9007199254740992,9007199254740992,9007199254740992.0), (3,9007199254740991,9007199254740992,9007199254740992.0), (4,NULL,NULL,NULL), (5,7,NULL,NULL), (6,NULL,7,7.0), (7,-9007199254740993,-9007199254740992,-9007199254740992.0) ON CONFLICT (id) DO UPDATE SET eq = final_mix_conflict.k = excluded.f, ne = final_mix_conflict.k <> excluded.f, lt = final_mix_conflict.k < excluded.f, le = final_mix_conflict.k <= excluded.f, gt = final_mix_conflict.k > excluded.f, ge = final_mix_conflict.k >= excluded.f, same = final_mix_conflict.k IS NOT DISTINCT FROM excluded.f, different = final_mix_conflict.k IS DISTINCT FROM excluded.f WHERE final_mix_conflict.k = excluded.f RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_41), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM final_mix_conflict ORDER BY id", expected: &EXPECTED_42 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "final_supplement:literal-upsert-mixed-=-fi:simple_text", setup: SETUP_4, before: &[], sql: "INSERT INTO final_mix_conflict (id,k,j,f) VALUES (1,9007199254740993,9007199254740992,9007199254740992.0), (2,9007199254740992,9007199254740992,9007199254740992.0), (3,9007199254740991,9007199254740992,9007199254740992.0), (4,NULL,NULL,NULL), (5,7,NULL,NULL), (6,NULL,7,7.0), (7,-9007199254740993,-9007199254740992,-9007199254740992.0) ON CONFLICT (id) DO UPDATE SET eq = excluded.f = final_mix_conflict.k, ne = excluded.f <> final_mix_conflict.k, lt = excluded.f < final_mix_conflict.k, le = excluded.f <= final_mix_conflict.k, gt = excluded.f > final_mix_conflict.k, ge = excluded.f >= final_mix_conflict.k, same = excluded.f IS NOT DISTINCT FROM final_mix_conflict.k, different = excluded.f IS DISTINCT FROM final_mix_conflict.k WHERE excluded.f = final_mix_conflict.k RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_41), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM final_mix_conflict ORDER BY id", expected: &EXPECTED_42 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "final_supplement:literal-upsert-mixed-is_not_distinct_from-if:simple_text", setup: SETUP_4, before: &[], sql: "INSERT INTO final_mix_conflict (id,k,j,f) VALUES (1,9007199254740993,9007199254740992,9007199254740992.0), (2,9007199254740992,9007199254740992,9007199254740992.0), (3,9007199254740991,9007199254740992,9007199254740992.0), (4,NULL,NULL,NULL), (5,7,NULL,NULL), (6,NULL,7,7.0), (7,-9007199254740993,-9007199254740992,-9007199254740992.0) ON CONFLICT (id) DO UPDATE SET eq = final_mix_conflict.k = excluded.f, ne = final_mix_conflict.k <> excluded.f, lt = final_mix_conflict.k < excluded.f, le = final_mix_conflict.k <= excluded.f, gt = final_mix_conflict.k > excluded.f, ge = final_mix_conflict.k >= excluded.f, same = final_mix_conflict.k IS NOT DISTINCT FROM excluded.f, different = final_mix_conflict.k IS DISTINCT FROM excluded.f WHERE final_mix_conflict.k IS NOT DISTINCT FROM excluded.f RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_43), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM final_mix_conflict ORDER BY id", expected: &EXPECTED_44 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "final_supplement:literal-upsert-mixed-is_not_distinct_from-fi:simple_text", setup: SETUP_4, before: &[], sql: "INSERT INTO final_mix_conflict (id,k,j,f) VALUES (1,9007199254740993,9007199254740992,9007199254740992.0), (2,9007199254740992,9007199254740992,9007199254740992.0), (3,9007199254740991,9007199254740992,9007199254740992.0), (4,NULL,NULL,NULL), (5,7,NULL,NULL), (6,NULL,7,7.0), (7,-9007199254740993,-9007199254740992,-9007199254740992.0) ON CONFLICT (id) DO UPDATE SET eq = excluded.f = final_mix_conflict.k, ne = excluded.f <> final_mix_conflict.k, lt = excluded.f < final_mix_conflict.k, le = excluded.f <= final_mix_conflict.k, gt = excluded.f > final_mix_conflict.k, ge = excluded.f >= final_mix_conflict.k, same = excluded.f IS NOT DISTINCT FROM final_mix_conflict.k, different = excluded.f IS DISTINCT FROM final_mix_conflict.k WHERE excluded.f IS NOT DISTINCT FROM final_mix_conflict.k RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_43), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM final_mix_conflict ORDER BY id", expected: &EXPECTED_44 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "final_supplement:literal-upsert-mixed-is_distinct_from-if:simple_text", setup: SETUP_4, before: &[], sql: "INSERT INTO final_mix_conflict (id,k,j,f) VALUES (1,9007199254740993,9007199254740992,9007199254740992.0), (2,9007199254740992,9007199254740992,9007199254740992.0), (3,9007199254740991,9007199254740992,9007199254740992.0), (4,NULL,NULL,NULL), (5,7,NULL,NULL), (6,NULL,7,7.0), (7,-9007199254740993,-9007199254740992,-9007199254740992.0) ON CONFLICT (id) DO UPDATE SET eq = final_mix_conflict.k = excluded.f, ne = final_mix_conflict.k <> excluded.f, lt = final_mix_conflict.k < excluded.f, le = final_mix_conflict.k <= excluded.f, gt = final_mix_conflict.k > excluded.f, ge = final_mix_conflict.k >= excluded.f, same = final_mix_conflict.k IS NOT DISTINCT FROM excluded.f, different = final_mix_conflict.k IS DISTINCT FROM excluded.f WHERE final_mix_conflict.k IS DISTINCT FROM excluded.f RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_45), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM final_mix_conflict ORDER BY id", expected: &EXPECTED_46 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "final_supplement:literal-upsert-mixed-is_distinct_from-fi:simple_text", setup: SETUP_4, before: &[], sql: "INSERT INTO final_mix_conflict (id,k,j,f) VALUES (1,9007199254740993,9007199254740992,9007199254740992.0), (2,9007199254740992,9007199254740992,9007199254740992.0), (3,9007199254740991,9007199254740992,9007199254740992.0), (4,NULL,NULL,NULL), (5,7,NULL,NULL), (6,NULL,7,7.0), (7,-9007199254740993,-9007199254740992,-9007199254740992.0) ON CONFLICT (id) DO UPDATE SET eq = excluded.f = final_mix_conflict.k, ne = excluded.f <> final_mix_conflict.k, lt = excluded.f < final_mix_conflict.k, le = excluded.f <= final_mix_conflict.k, gt = excluded.f > final_mix_conflict.k, ge = excluded.f >= final_mix_conflict.k, same = excluded.f IS NOT DISTINCT FROM final_mix_conflict.k, different = excluded.f IS DISTINCT FROM final_mix_conflict.k WHERE excluded.f IS DISTINCT FROM final_mix_conflict.k RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_47), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM final_mix_conflict ORDER BY id", expected: &EXPECTED_48 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "final_supplement:literal-upsert-pure_bigint-is_not_distinct_from-if:simple_text", setup: SETUP_4, before: &[], sql: "INSERT INTO final_mix_conflict (id,k,j,f) VALUES (1,9007199254740993,9007199254740992,9007199254740992.0), (2,9007199254740992,9007199254740992,9007199254740992.0), (3,9007199254740991,9007199254740992,9007199254740992.0), (4,NULL,NULL,NULL), (5,7,NULL,NULL), (6,NULL,7,7.0), (7,-9007199254740993,-9007199254740992,-9007199254740992.0) ON CONFLICT (id) DO UPDATE SET eq = final_mix_conflict.k = excluded.j, ne = final_mix_conflict.k <> excluded.j, lt = final_mix_conflict.k < excluded.j, le = final_mix_conflict.k <= excluded.j, gt = final_mix_conflict.k > excluded.j, ge = final_mix_conflict.k >= excluded.j, same = final_mix_conflict.k IS NOT DISTINCT FROM excluded.j, different = final_mix_conflict.k IS DISTINCT FROM excluded.j WHERE final_mix_conflict.k IS NOT DISTINCT FROM excluded.j RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_49), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM final_mix_conflict ORDER BY id", expected: &EXPECTED_50 }], recovery: "SELECT 1 AS recovery_marker" },
    Case { id: "final_supplement:literal-upsert-pure_bigint-is_not_distinct_from-fi:simple_text", setup: SETUP_4, before: &[], sql: "INSERT INTO final_mix_conflict (id,k,j,f) VALUES (1,9007199254740993,9007199254740992,9007199254740992.0), (2,9007199254740992,9007199254740992,9007199254740992.0), (3,9007199254740991,9007199254740992,9007199254740992.0), (4,NULL,NULL,NULL), (5,7,NULL,NULL), (6,NULL,7,7.0), (7,-9007199254740993,-9007199254740992,-9007199254740992.0) ON CONFLICT (id) DO UPDATE SET eq = excluded.j = final_mix_conflict.k, ne = excluded.j <> final_mix_conflict.k, lt = excluded.j < final_mix_conflict.k, le = excluded.j <= final_mix_conflict.k, gt = excluded.j > final_mix_conflict.k, ge = excluded.j >= final_mix_conflict.k, same = excluded.j IS NOT DISTINCT FROM final_mix_conflict.k, different = excluded.j IS DISTINCT FROM final_mix_conflict.k WHERE excluded.j IS NOT DISTINCT FROM final_mix_conflict.k RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_49), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM final_mix_conflict ORDER BY id", expected: &EXPECTED_50 }], recovery: "SELECT 1 AS recovery_marker" },
];

pub const ROLLBACK: &[Case] = &[
    Case { id: "final_supplement:update-transaction-rollback:simple_text", setup: SETUP_4, before: &["BEGIN"], sql: "UPDATE final_mix_conflict SET eq = k = f, ne = k <> f, lt = k < f, le = k <= f, gt = k > f, ge = k >= f, same = k IS NOT DISTINCT FROM f, different = k IS DISTINCT FROM f WHERE k >= f RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: Some(&EXPECTED_51), posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM final_mix_conflict ORDER BY id", expected: &EXPECTED_42 }, Post { sql: "ROLLBACK", expected: &EXPECTED_52 }, Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM final_mix_conflict ORDER BY id", expected: &EXPECTED_37 }], recovery: "SELECT 1 AS recovery_marker" },
];

pub const ATOMICITY: &[Case] = &[
    Case { id: "final_supplement:update-assignment-error-atomicity:simple_text", setup: SETUP_4, before: &[], sql: "UPDATE final_mix_conflict SET eq = k = f, j = CAST('not-an-integer' AS BIGINT) WHERE k >= f RETURNING id,k,j,f,eq,ne,lt,le,gt,ge,same,different", expected: None, posts: &[Post { sql: "SELECT id,k,j,f,eq,ne,lt,le,gt,ge,same,different FROM final_mix_conflict ORDER BY id", expected: &EXPECTED_37 }], recovery: "SELECT 1 AS recovery_marker" },
];
