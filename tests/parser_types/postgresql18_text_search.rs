//! PostgreSQL 18 text-search statement grammar tracked by #799.

use cassie::sql::parse_statement;

#[test]
fn should_parse_text_search_configuration_copy_form() {
    // Arrange
    let sql = "CREATE TEXT SEARCH CONFIGURATION issue_799_cfg (COPY = pg_catalog.english)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_configuration_parser_form() {
    // Arrange
    let sql = "CREATE TEXT SEARCH CONFIGURATION issue_799_cfg (PARSER = pg_catalog.default)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_configuration_add_mapping() {
    // Arrange
    let sql = "ALTER TEXT SEARCH CONFIGURATION issue_799_cfg ADD MAPPING FOR asciiword WITH pg_catalog.english_stem";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_configuration_replace_mapping_dictionary() {
    // Arrange
    let sql = "ALTER TEXT SEARCH CONFIGURATION issue_799_cfg ALTER MAPPING FOR asciiword REPLACE pg_catalog.simple WITH pg_catalog.english_stem";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_configuration_drop_mapping() {
    // Arrange
    let sql = "ALTER TEXT SEARCH CONFIGURATION issue_799_cfg DROP MAPPING IF EXISTS FOR asciiword";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_configuration_rename() {
    // Arrange
    let sql = "ALTER TEXT SEARCH CONFIGURATION issue_799_cfg RENAME TO issue_799_renamed";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_configuration_owner_change() {
    // Arrange
    let sql = "ALTER TEXT SEARCH CONFIGURATION issue_799_cfg OWNER TO root";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_configuration_schema_move() {
    // Arrange
    let sql = "ALTER TEXT SEARCH CONFIGURATION issue_799_cfg SET SCHEMA issue_799_schema";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_configuration_drop_modes() {
    // Arrange
    let sql = "DROP TEXT SEARCH CONFIGURATION IF EXISTS issue_799_cfg CASCADE";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_simple_text_search_dictionary_template() {
    // Arrange
    let sql = "CREATE TEXT SEARCH DICTIONARY issue_799_simple (TEMPLATE = pg_catalog.simple)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_snowball_text_search_dictionary_template() {
    // Arrange
    let sql = "CREATE TEXT SEARCH DICTIONARY issue_799_snowball (TEMPLATE = pg_catalog.snowball, language = english)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_synonym_text_search_dictionary_template() {
    // Arrange
    let sql = "CREATE TEXT SEARCH DICTIONARY issue_799_synonym (TEMPLATE = pg_catalog.synonym, synonyms = english)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_ispell_text_search_dictionary_template() {
    // Arrange
    let sql = "CREATE TEXT SEARCH DICTIONARY issue_799_ispell (TEMPLATE = pg_catalog.ispell, dictfile = english, afffile = english)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_thesaurus_text_search_dictionary_template() {
    // Arrange
    let sql = "CREATE TEXT SEARCH DICTIONARY issue_799_thesaurus (TEMPLATE = pg_catalog.thesaurus, dictfile = english, dictionary = pg_catalog.english_stem)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_dictionary_option_changes() {
    // Arrange
    let sql = "ALTER TEXT SEARCH DICTIONARY issue_799_dict (STOPWORDS = english)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_dictionary_rename() {
    // Arrange
    let sql = "ALTER TEXT SEARCH DICTIONARY issue_799_dict RENAME TO issue_799_renamed";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_dictionary_owner_change() {
    // Arrange
    let sql = "ALTER TEXT SEARCH DICTIONARY issue_799_dict OWNER TO root";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_dictionary_schema_move() {
    // Arrange
    let sql = "ALTER TEXT SEARCH DICTIONARY issue_799_dict SET SCHEMA issue_799_schema";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_dictionary_drop_statement() {
    // Arrange
    let sql = "DROP TEXT SEARCH DICTIONARY IF EXISTS issue_799_dict";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_parser_creation_before_rejecting_native_callbacks() {
    // Arrange
    let sql = "CREATE TEXT SEARCH PARSER issue_799_parser (START = issue_start, GETTOKEN = issue_next, END = issue_end, LEXTYPES = issue_lex_types)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}

#[test]
fn should_parse_text_search_template_creation_before_rejecting_native_callbacks() {
    // Arrange
    let sql =
        "CREATE TEXT SEARCH TEMPLATE issue_799_template (INIT = issue_init, LEXIZE = issue_lexize)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "{sql}: {parsed:?}");
}
