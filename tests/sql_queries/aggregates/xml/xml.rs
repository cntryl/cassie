//! PostgreSQL 18 XML, `XPath` and XMLTABLE behavior tracked by #906.

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, sql_fixture_with_config};

#[path = "ordered_aggregation.rs"]
mod ordered_aggregation;

struct XmlDirectory(String);

impl Drop for XmlDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn should_aggregate_xml_values_while_skipping_null_inputs() {
    // Arrange
    let fixture = sql_fixture("issue-906-xmlagg-null-input", &[]);
    let sql = "SELECT XMLSERIALIZE(CONTENT XMLAGG(value) AS TEXT) FROM (VALUES (XMLELEMENT(NAME item, 'one')), (NULL::xml)) AS input(value)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XMLAGG skips NULL inputs").rows(),
        &[vec![Value::String("<item>one</item>".to_owned())]]
    );
}

include!("../../types/xml/xml.rs");
