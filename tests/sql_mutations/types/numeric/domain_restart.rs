//! Numeric domain typmod and value identity survive a storage restart (#812).

use cassie::app::Cassie;
use cassie::types::Value;

struct NumericDomainDirectory(String);

impl Drop for NumericDomainDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn should_preserve_numeric_domain_typmods_given_persisted_domain_values_when_storage_restarts() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("numeric-domain-typmod-restart");
    let _directory = NumericDomainDirectory(path.clone());
    let exact_value = "123456789012345678901234567890.12345678901234567890";
    let adjacent_value = "123456789012345678901234567890.12345678901234567889";
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create numeric domain fixture");
        cassie.startup().expect("start numeric domain fixture");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE DOMAIN precise_amount AS NUMERIC(50, 20) CHECK (VALUE > 0)",
                vec![],
            )
            .expect("create a typmod-constrained numeric domain");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE domain_amounts (amount precise_amount NOT NULL)",
                vec![],
            )
            .expect("create a table using the numeric domain");
        cassie
            .execute_sql(
                &session,
                &format!("INSERT INTO domain_amounts VALUES ('{exact_value}')"),
                vec![],
            )
            .expect("persist the exact domain value");
        drop(session);
        cassie.shutdown();
    }

    let restarted = Cassie::new_with_data_dir(&path).expect("reopen numeric domain fixture");
    restarted.startup().expect("restart numeric domain fixture");
    let session = restarted.create_session("tester", None);

    // Act
    let result = restarted.execute_sql(
        &session,
        &format!(
            "SELECT amount::TEXT, pg_typeof(amount)::TEXT, \
             amount = CAST('{exact_value}' AS NUMERIC), \
             amount <> CAST('{adjacent_value}' AS NUMERIC) \
             FROM domain_amounts"
        ),
        vec![],
    );

    // Assert
    let result = result.expect("domain value, type identity and typmod survive restart");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [25, 25, 16, 16]
    );
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String(exact_value.to_owned()),
            Value::String("precise_amount".to_owned()),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
    drop(session);
    restarted.shutdown();
}
