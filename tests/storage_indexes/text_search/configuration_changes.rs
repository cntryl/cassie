//! Indexed text-search configuration and dictionary reindex behavior (#799).

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_rebuild_indexed_lexemes_given_a_changed_text_search_mapping_when_the_gin_index_is_reindexed(
) {
    // Arrange
    let fixture = sql_fixture("issue-799-mapping-reindex", &[]);
    for sql in [
        "CREATE SCHEMA issue_799_mapping",
        "CREATE TEXT SEARCH CONFIGURATION issue_799_mapping.cfg (COPY = pg_catalog.simple)",
        "CREATE TABLE issue_799_mapping.documents (body TEXT)",
        "INSERT INTO issue_799_mapping.documents VALUES ('rats')",
        "INSERT INTO issue_799_mapping.documents \
         SELECT 'cats' FROM generate_series(1, 2000)",
        "CREATE INDEX documents_tsv_idx ON issue_799_mapping.documents \
         USING GIN (to_tsvector('issue_799_mapping.cfg', body))",
    ] {
        fixture
            .execute(sql)
            .unwrap_or_else(|error| panic!("PostgreSQL-valid index setup failed: {sql}: {error}"));
    }

    // Act
    let before = fixture.execute(
        "SELECT count(*) FROM issue_799_mapping.documents \
         WHERE to_tsvector('issue_799_mapping.cfg', body) \
               @@ to_tsquery('simple', 'rats')",
    );
    fixture
        .execute(
            "ALTER TEXT SEARCH CONFIGURATION issue_799_mapping.cfg \
             ALTER MAPPING FOR asciiword REPLACE pg_catalog.simple WITH pg_catalog.english_stem",
        )
        .expect("replace the configuration's dictionary mapping");
    let stale_index = fixture.execute(
        "SELECT count(*) FROM issue_799_mapping.documents \
         WHERE to_tsvector('issue_799_mapping.cfg', body) \
               @@ to_tsquery('simple', 'rat')",
    );
    fixture
        .execute("REINDEX INDEX issue_799_mapping.documents_tsv_idx")
        .expect("rebuild lexemes after the mapping changed");
    let rebuilt_index = fixture.execute(
        "SELECT count(*) FROM issue_799_mapping.documents \
         WHERE to_tsvector('issue_799_mapping.cfg', body) \
               @@ to_tsquery('simple', 'rat')",
    );

    // Assert
    assert_eq!(
        before.expect("pre-change lexeme query").rows(),
        &[vec![Value::Int64(1)]]
    );
    assert_eq!(
        stale_index.expect("stale-index query").rows(),
        &[vec![Value::Int64(0)]]
    );
    assert_eq!(
        rebuilt_index.expect("post-reindex query").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_rebuild_indexed_lexemes_given_changed_dictionary_options_when_the_gin_index_is_reindexed()
{
    // Arrange
    let fixture = sql_fixture("issue-799-dictionary-reindex", &[]);
    for sql in [
        "CREATE SCHEMA issue_799_dictionary",
        "CREATE TEXT SEARCH DICTIONARY issue_799_dictionary.stop_words \
         (TEMPLATE = pg_catalog.simple)",
        "CREATE TEXT SEARCH CONFIGURATION issue_799_dictionary.cfg \
         (COPY = pg_catalog.simple)",
        "ALTER TEXT SEARCH CONFIGURATION issue_799_dictionary.cfg \
         ALTER MAPPING FOR asciiword WITH issue_799_dictionary.stop_words",
        "CREATE TABLE issue_799_dictionary.documents (body TEXT)",
        "INSERT INTO issue_799_dictionary.documents VALUES ('the rat')",
        "INSERT INTO issue_799_dictionary.documents \
         SELECT 'a rat' FROM generate_series(1, 2000)",
        "CREATE INDEX documents_tsv_idx ON issue_799_dictionary.documents \
         USING GIN (to_tsvector('issue_799_dictionary.cfg', body))",
    ] {
        fixture
            .execute(sql)
            .unwrap_or_else(|error| panic!("PostgreSQL-valid index setup failed: {sql}: {error}"));
    }

    // Act
    let before = fixture.execute(
        "SELECT count(*) FROM issue_799_dictionary.documents \
         WHERE to_tsvector('issue_799_dictionary.cfg', body) \
               @@ to_tsquery('simple', 'the')",
    );
    fixture
        .execute(
            "ALTER TEXT SEARCH DICTIONARY issue_799_dictionary.stop_words (STOPWORDS = english)",
        )
        .expect("change the dictionary stopword option");
    let stale_index = fixture.execute(
        "SELECT count(*) FROM issue_799_dictionary.documents \
         WHERE to_tsvector('issue_799_dictionary.cfg', body) \
               @@ to_tsquery('simple', 'the')",
    );
    fixture
        .execute("REINDEX INDEX issue_799_dictionary.documents_tsv_idx")
        .expect("rebuild lexemes after the dictionary option changed");
    let rebuilt_index = fixture.execute(
        "SELECT count(*) FROM issue_799_dictionary.documents \
         WHERE to_tsvector('issue_799_dictionary.cfg', body) \
               @@ to_tsquery('simple', 'the')",
    );

    // Assert
    assert_eq!(
        before.expect("pre-change dictionary query").rows(),
        &[vec![Value::Int64(1)]]
    );
    assert_eq!(
        stale_index.expect("stale-index dictionary query").rows(),
        &[vec![Value::Int64(1)]]
    );
    assert_eq!(
        rebuilt_index.expect("post-reindex dictionary query").rows(),
        &[vec![Value::Int64(0)]]
    );
}
