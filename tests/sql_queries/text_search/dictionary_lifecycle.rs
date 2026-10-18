//! PostgreSQL 18.6 built-in text-search dictionary data and DDL lifecycle.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_replace_lexemes_given_the_pinned_synonym_dictionary_when_sample_words_are_lexized() {
    // PostgreSQL 18.6 tsearch_data/synonym_sample.syn SHA-256:
    // 59a46c3c25c2b5a1fe174de62bf2f53ce46021e3831eec0eb62d9301ea327e49
    // Arrange
    let fixture = sql_fixture("text-search-synonym-sample", &[]);
    fixture
        .execute(
            "CREATE TEXT SEARCH DICTIONARY synonym_sample_dictionary ( \
                 TEMPLATE = pg_catalog.synonym, SYNONYMS = 'synonym_sample' \
             )",
        )
        .expect("create a synonym dictionary from the PostgreSQL sample file");

    // Act
    let result = fixture.execute(
        "SELECT ts_lexize('synonym_sample_dictionary', 'postgres')::TEXT, \
                ts_lexize('synonym_sample_dictionary', 'gogle')::TEXT",
    );

    // Assert
    assert_eq!(
        result
            .expect("lexize the pinned synonym sample entries")
            .rows(),
        &[vec![
            Value::String("{pgsql}".to_owned()),
            Value::String("{googl}".to_owned()),
        ]]
    );
}

#[test]
fn should_expand_lexemes_given_the_pinned_ispell_dictionary_when_sample_words_are_lexized() {
    // PostgreSQL 18.6 tsearch_data/ispell_sample.affix SHA-256:
    // 3a5a3e7a54acd42c27d3c231526ccf8ccf7f29f688ef6ad74dbd04af11309d8f
    // PostgreSQL 18.6 tsearch_data/ispell_sample.dict SHA-256:
    // e913216b14f04ebfe390f8f6048c408aebab7fe7ff313d61a0d2355146d83397
    // Arrange
    let fixture = sql_fixture("text-search-ispell-sample", &[]);
    fixture
        .execute(
            "CREATE TEXT SEARCH DICTIONARY ispell_sample_dictionary ( \
                 TEMPLATE = pg_catalog.ispell, \
                 DictFile = 'ispell_sample', AffFile = 'ispell_sample' \
             )",
        )
        .expect("create an ispell dictionary from the PostgreSQL sample files");

    // Act
    let result = fixture.execute(
        "SELECT ts_lexize('ispell_sample_dictionary', 'bookings')::TEXT, \
                ts_lexize('ispell_sample_dictionary', 'footballklubber')::TEXT",
    );

    // Assert
    assert_eq!(
        result
            .expect("lexize the pinned ispell sample entries")
            .rows(),
        &[vec![
            Value::String("{booking,book}".to_owned()),
            Value::String("{footballklubber,foot,ball,klubber,football,klubber}".to_owned()),
        ]]
    );
}

#[test]
fn should_replace_phrases_given_the_pinned_thesaurus_dictionary_when_sample_text_is_vectorized() {
    // PostgreSQL 18.6 tsearch_data/thesaurus_sample.ths SHA-256:
    // 346954eaf1def3007ad848731b3c26c203ed12923911b6a0ac198604831f7dc4
    // Arrange
    let fixture = sql_fixture(
        "text-search-thesaurus-sample",
        &[
            "CREATE TEXT SEARCH DICTIONARY thesaurus_sample_dictionary ( \
                 TEMPLATE = pg_catalog.thesaurus, \
                 DictFile = 'thesaurus_sample', Dictionary = 'english_stem' \
             )",
            "CREATE TEXT SEARCH CONFIGURATION thesaurus_sample_configuration \
                 (COPY = pg_catalog.simple)",
            "ALTER TEXT SEARCH CONFIGURATION thesaurus_sample_configuration \
                 ALTER MAPPING FOR asciiword \
                 WITH thesaurus_sample_dictionary, pg_catalog.english_stem",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT \
             to_tsvector('thesaurus_sample_configuration', 'one two three')::TEXT, \
             to_tsvector('thesaurus_sample_configuration', 'booking tickets')::TEXT, \
             to_tsvector('thesaurus_sample_configuration', 'booking extra tickets')::TEXT",
    );

    // Assert
    assert_eq!(
        result
            .expect("apply sample thesaurus phrase substitutions")
            .rows(),
        &[vec![
            Value::String("'123':1".to_owned()),
            Value::String("'card':3 'invit':2 'order':1".to_owned()),
            Value::String("'book':1 'extra':2 'ticket':3".to_owned()),
        ]]
    );
}

#[test]
fn should_publish_the_new_owner_given_a_text_search_configuration_when_ownership_is_transferred() {
    // Arrange
    let fixture = sql_fixture(
        "text-search-configuration-owner",
        &[
            "CREATE ROLE search_configuration_owner",
            "CREATE SCHEMA search_configuration_schema",
            "CREATE TEXT SEARCH CONFIGURATION search_configuration_schema.owner_config \
                 (COPY = pg_catalog.simple)",
        ],
    );

    // Act
    let transfer = fixture.execute(
        "ALTER TEXT SEARCH CONFIGURATION search_configuration_schema.owner_config \
         OWNER TO search_configuration_owner",
    );
    let owner = fixture.execute(
        "SELECT pg_catalog.pg_get_userbyid(cfgowner) \
         FROM pg_catalog.pg_ts_config \
         WHERE cfgname = 'owner_config' \
           AND cfgnamespace = 'search_configuration_schema'::REGNAMESPACE",
    );

    // Assert
    transfer.expect("transfer text-search configuration ownership");
    assert_eq!(
        owner
            .expect("read the transferred configuration owner")
            .rows(),
        &[vec![Value::String("search_configuration_owner".to_owned())]]
    );
}

#[test]
fn should_publish_the_new_owner_given_a_text_search_dictionary_when_ownership_is_transferred() {
    // Arrange
    let fixture = sql_fixture(
        "text-search-dictionary-owner",
        &[
            "CREATE ROLE search_dictionary_owner",
            "CREATE SCHEMA search_dictionary_schema",
            "CREATE TEXT SEARCH DICTIONARY search_dictionary_schema.owner_dictionary \
                 (TEMPLATE = pg_catalog.simple)",
        ],
    );

    // Act
    let transfer = fixture.execute(
        "ALTER TEXT SEARCH DICTIONARY search_dictionary_schema.owner_dictionary \
         OWNER TO search_dictionary_owner",
    );
    let owner = fixture.execute(
        "SELECT pg_catalog.pg_get_userbyid(dictowner) \
         FROM pg_catalog.pg_ts_dict \
         WHERE dictname = 'owner_dictionary' \
           AND dictnamespace = 'search_dictionary_schema'::REGNAMESPACE",
    );

    // Assert
    transfer.expect("transfer text-search dictionary ownership");
    assert_eq!(
        owner.expect("read the transferred dictionary owner").rows(),
        &[vec![Value::String("search_dictionary_owner".to_owned())]]
    );
}

#[test]
fn should_resolve_the_new_schema_given_a_text_search_dictionary_when_it_is_moved_between_schemas() {
    // Arrange
    let fixture = sql_fixture(
        "text-search-dictionary-schema-move",
        &[
            "CREATE SCHEMA dictionary_source_schema",
            "CREATE SCHEMA dictionary_target_schema",
            "CREATE TEXT SEARCH DICTIONARY dictionary_source_schema.movable_dictionary \
                 (TEMPLATE = pg_catalog.simple)",
        ],
    );

    // Act
    let moved = fixture.execute(
        "ALTER TEXT SEARCH DICTIONARY dictionary_source_schema.movable_dictionary \
         SET SCHEMA dictionary_target_schema",
    );
    let result = fixture.execute(
        "SELECT dictnamespace::REGNAMESPACE::TEXT, \
                ts_lexize('dictionary_target_schema.movable_dictionary', 'The')::TEXT \
         FROM pg_catalog.pg_ts_dict \
         WHERE dictname = 'movable_dictionary' \
           AND dictnamespace = 'dictionary_target_schema'::REGNAMESPACE",
    );

    // Assert
    moved.expect("move text-search dictionary to the target schema");
    assert_eq!(
        result
            .expect("resolve the moved dictionary and inspect its schema")
            .rows(),
        &[vec![
            Value::String("dictionary_target_schema".to_owned()),
            Value::String("{the}".to_owned()),
        ]]
    );
}

#[test]
fn should_skip_missing_configurations_given_if_exists_when_text_search_configurations_are_dropped()
{
    // Arrange
    let fixture = sql_fixture("text-search-configuration-drop-if-exists", &[]);

    // Act
    let absent = fixture
        .execute("DROP TEXT SEARCH CONFIGURATION IF EXISTS missing_text_search_configuration");
    let created = fixture.execute(
        "CREATE TEXT SEARCH CONFIGURATION droppable_text_search_configuration \
         (COPY = pg_catalog.simple)",
    );
    let first_drop =
        fixture.execute("DROP TEXT SEARCH CONFIGURATION droppable_text_search_configuration");
    let repeated_drop = fixture
        .execute("DROP TEXT SEARCH CONFIGURATION IF EXISTS droppable_text_search_configuration");

    // Assert
    assert_eq!(
        absent
            .expect("ignore a missing configuration with IF EXISTS")
            .command(),
        "DROP TEXT SEARCH CONFIGURATION"
    );
    created.expect("create configuration for drop lifecycle");
    assert_eq!(
        first_drop.expect("drop the configuration once").command(),
        "DROP TEXT SEARCH CONFIGURATION"
    );
    assert_eq!(
        repeated_drop
            .expect("ignore a configuration dropped earlier")
            .command(),
        "DROP TEXT SEARCH CONFIGURATION"
    );
}

#[test]
fn should_skip_missing_dictionaries_given_if_exists_when_text_search_dictionaries_are_dropped() {
    // Arrange
    let fixture = sql_fixture("text-search-dictionary-drop-if-exists", &[]);

    // Act
    let absent =
        fixture.execute("DROP TEXT SEARCH DICTIONARY IF EXISTS missing_text_search_dictionary");
    let created = fixture.execute(
        "CREATE TEXT SEARCH DICTIONARY droppable_text_search_dictionary \
         (TEMPLATE = pg_catalog.simple)",
    );
    let first_drop =
        fixture.execute("DROP TEXT SEARCH DICTIONARY droppable_text_search_dictionary");
    let repeated_drop =
        fixture.execute("DROP TEXT SEARCH DICTIONARY IF EXISTS droppable_text_search_dictionary");

    // Assert
    assert_eq!(
        absent
            .expect("ignore a missing dictionary with IF EXISTS")
            .command(),
        "DROP TEXT SEARCH DICTIONARY"
    );
    created.expect("create dictionary for drop lifecycle");
    assert_eq!(
        first_drop.expect("drop the dictionary once").command(),
        "DROP TEXT SEARCH DICTIONARY"
    );
    assert_eq!(
        repeated_drop
            .expect("ignore a dictionary dropped earlier")
            .command(),
        "DROP TEXT SEARCH DICTIONARY"
    );
}
