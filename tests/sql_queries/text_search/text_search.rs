//! PostgreSQL 18 full-text search SQL behavior tracked by #799.

#[path = "table_predicate.rs"]
mod table_predicate;

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_normalize_text_with_the_selected_search_configuration() {
    // Arrange
    let fixture = sql_fixture("issue-799-to-tsvector", &[]);

    // Act
    let result = fixture.execute(
        "SELECT to_tsvector('english', 'The Fat Rats')::TEXT, to_tsvector('simple', 'The Fat Rats')::TEXT",
    );

    // Assert
    assert_eq!(
        result
            .expect("to_tsvector with named configurations")
            .rows(),
        &[vec![
            Value::String("'fat':2 'rat':3".to_owned()),
            Value::String("'fat':2 'rats':3 'the':1".to_owned()),
        ]]
    );
}

#[test]
fn should_follow_c_locale_case_mapping_for_non_ascii_search_lexemes() {
    // Arrange
    let fixture = sql_fixture("issue-799-c-locale-unicode-search", &[]);

    // Act
    let result = fixture.execute(
        "SELECT to_tsvector('simple', 'CAFÉ')::TEXT, \
         plainto_tsquery('simple', 'CAFÉ')::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("C-locale text-search case mapping").rows(),
        &[vec![
            Value::String("'cafÉ':1".to_owned()),
            Value::String("'cafÉ'".to_owned()),
        ]]
    );
}

#[test]
fn should_render_tsvector_literals_in_canonical_lexeme_order() {
    // Arrange
    let fixture = sql_fixture("issue-799-tsvector-literal", &[]);

    // Act
    let result = fixture.execute("SELECT 'fat:2 cat:3 rat:5A'::tsvector::TEXT");

    // Assert
    assert_eq!(
        result.expect("tsvector input and output").rows(),
        &[vec![Value::String("'cat':3 'fat':2 'rat':5A".to_owned())]]
    );
}

#[test]
fn should_render_tsquery_literals_in_canonical_operator_form() {
    // Arrange
    let fixture = sql_fixture("issue-799-tsquery-literal", &[]);

    // Act
    let result = fixture.execute("SELECT 'fat & (cat | rat)'::tsquery::TEXT");

    // Assert
    assert_eq!(
        result.expect("tsquery input and output").rows(),
        &[vec![Value::String("'fat' & ( 'cat' | 'rat' )".to_owned())]]
    );
}

#[test]
fn should_construct_plain_phrase_websearch_queries() {
    // Arrange
    let fixture = sql_fixture("issue-799-tsquery-constructors", &[]);

    // Act
    let result = fixture.execute(
        "SELECT plainto_tsquery('english', 'Fat rats')::TEXT, phraseto_tsquery('english', 'Fat rats')::TEXT, websearch_to_tsquery('english', '\"fat rats\"')::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("text-search query constructors").rows(),
        &[vec![
            Value::String("'fat' & 'rat'".to_owned()),
            Value::String("'fat' <-> 'rat'".to_owned()),
            Value::String("'fat' <-> 'rat'".to_owned()),
        ]]
    );
}

#[test]
fn should_match_text_search_operators() {
    // Arrange
    let fixture = sql_fixture("issue-799-text-search-match", &[]);

    // Act
    let result = fixture.execute(
        "SELECT to_tsvector('english', 'Fat cats ate rats') @@ to_tsquery('english', 'cat & rat'), to_tsquery('english', 'cat & rat') @@ to_tsvector('english', 'Fat cats ate rats'), 'Fat cats ate rats' @@ to_tsquery('english', 'cat & rat')",
    );

    // Assert
    assert_eq!(
        result.expect("text-search match operators").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_concatenate_tsvectors_with_adjusted_positions() {
    // Arrange
    let fixture = sql_fixture("issue-799-tsvector-concat", &[]);

    // Act
    let result = fixture.execute("SELECT ('a:1 b:2'::tsvector || 'c:1 d:2 b:3'::tsvector)::TEXT");

    // Assert
    assert_eq!(
        result.expect("tsvector concatenation").rows(),
        &[vec![Value::String("'a':1 'b':2,5 'c':3 'd':4".to_owned())]]
    );
}

#[test]
fn should_apply_tsquery_boolean_phrase_containment_operators() {
    // Arrange
    let fixture = sql_fixture("issue-799-tsquery-operators", &[]);

    // Act
    let result = fixture.execute(
        "SELECT ('fat | rat'::tsquery && 'cat'::tsquery)::TEXT, ('fat | rat'::tsquery || 'cat'::tsquery)::TEXT, (!! 'cat'::tsquery)::TEXT, (to_tsquery('fat') <-> to_tsquery('rat'))::TEXT, ('cat'::tsquery @> 'cat & rat'::tsquery), ('cat'::tsquery <@ 'cat & rat'::tsquery)",
    );

    // Assert
    assert_eq!(
        result.expect("tsquery operators").rows(),
        &[vec![
            Value::String("( 'fat' | 'rat' ) & 'cat'".to_owned()),
            Value::String("'fat' | 'rat' | 'cat'".to_owned()),
            Value::String("!'cat'".to_owned()),
            Value::String("'fat' <-> 'rat'".to_owned()),
            Value::Bool(false),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_build_phrase_queries_with_an_explicit_distance() {
    // Arrange
    let fixture = sql_fixture("issue-799-tsquery-phrase-distance", &[]);

    // Act
    let result = fixture.execute(
        "SELECT tsquery_phrase(to_tsquery('fat'), to_tsquery('cat'))::TEXT, tsquery_phrase(to_tsquery('fat'), to_tsquery('cat'), 10)::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("tsquery_phrase").rows(),
        &[vec![
            Value::String("'fat' <-> 'cat'".to_owned()),
            Value::String("'fat' <10> 'cat'".to_owned()),
        ]]
    );
}

#[test]
fn should_build_configured_tsqueries_from_operator_text() {
    // Arrange
    let fixture = sql_fixture("issue-799-to-tsquery", &[]);

    // Act
    let result = fixture.execute("SELECT to_tsquery('english', 'The & Fat & Rats')::TEXT");

    // Assert
    assert_eq!(
        result
            .expect("to_tsquery with an explicit configuration")
            .rows(),
        &[vec![Value::String("'fat' & 'rat'".to_owned())]]
    );
}

#[test]
fn should_assign_weights_to_every_tsvector_lexeme() {
    // Arrange
    let fixture = sql_fixture("issue-799-tsvector-setweight-all", &[]);

    // Act
    let result = fixture.execute("SELECT setweight('fat:2,4 cat:3 rat:5B'::tsvector, 'A')::TEXT");

    // Assert
    assert_eq!(
        result.expect("setweight over every lexeme").rows(),
        &[vec![Value::String(
            "'cat':3A 'fat':2A,4A 'rat':5A".to_owned()
        )]]
    );
}

#[test]
fn should_assign_weights_to_selected_tsvector_lexemes() {
    // Arrange
    let fixture = sql_fixture("issue-799-tsvector-setweight-selected", &[]);

    // Act
    let result = fixture.execute(
        "SELECT setweight('fat:2,4 cat:3 rat:5,6B'::tsvector, 'A', ARRAY['cat', 'rat'])::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("setweight over selected lexemes").rows(),
        &[vec![Value::String(
            "'cat':3A 'fat':2,4 'rat':5A,6A".to_owned()
        )]]
    );
}

#[test]
fn should_convert_tsvectors_to_text_arrays() {
    // Arrange
    let fixture = sql_fixture("issue-799-tsvector-arrays", &[]);

    // Act
    let result = fixture.execute(
        "SELECT array_to_tsvector(ARRAY['fat', 'cat', 'rat']::TEXT[])::TEXT, tsvector_to_array('fat:2,4 cat:3 rat:5A'::tsvector)::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("tsvector array conversion").rows(),
        &[vec![
            Value::String("'cat' 'fat' 'rat'".to_owned()),
            Value::String("{cat,fat,rat}".to_owned()),
        ]]
    );
}

#[test]
fn should_expand_tsvectors_into_lexeme_rows() {
    // Arrange
    let fixture = sql_fixture("issue-799-tsvector-unnest", &[]);

    // Act
    let result = fixture.execute(
        "SELECT lexeme, positions::TEXT, weights FROM unnest('cat:3 fat:2,4 rat:5A'::tsvector) AS terms(lexeme, positions, weights) ORDER BY lexeme",
    );

    // Assert
    assert_eq!(
        result.expect("unnest tsvector").rows(),
        &[
            vec![
                Value::String("cat".to_owned()),
                Value::String("{3}".to_owned()),
                Value::String("{D}".to_owned()),
            ],
            vec![
                Value::String("fat".to_owned()),
                Value::String("{2,4}".to_owned()),
                Value::String("{D,D}".to_owned()),
            ],
            vec![
                Value::String("rat".to_owned()),
                Value::String("{5}".to_owned()),
                Value::String("{A}".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_apply_tsvector_lexeme_utilities() {
    // Arrange
    let fixture = sql_fixture("issue-799-tsvector-utilities", &[]);

    // Act
    let result = fixture.execute(
        "SELECT length('fat:2,4 cat:3 rat:5A'::tsvector), strip('fat:2,4 cat:3 rat:5A'::tsvector)::TEXT, ts_delete('fat:2,4 cat:3 rat:5A'::tsvector, 'fat')::TEXT, ts_filter('fat:2,4 cat:3b,7c rat:5A'::tsvector, ARRAY['a', 'b']::\"char\"[])::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("tsvector utility functions").rows(),
        &[vec![
            Value::Int64(3),
            Value::String("'cat' 'fat' 'rat'".to_owned()),
            Value::String("'cat':3 'rat':5A".to_owned()),
            Value::String("'cat':3B 'rat':5A".to_owned()),
        ]]
    );
}

#[test]
fn should_report_tsquery_node_counts_with_indexable_terms() {
    // Arrange
    let fixture = sql_fixture("issue-799-tsquery-utilities", &[]);

    // Act
    let result = fixture.execute(
        "SELECT numnode('(fat & rat) | cat'::tsquery), querytree('fat & !rat'::tsquery)::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("tsquery utility functions").rows(),
        &[vec![Value::Int64(5), Value::String("'fat'".to_owned()),]]
    );
}

#[test]
fn should_rewrite_tsquery_terms_from_values_or_selects() {
    // Arrange
    let fixture = sql_fixture("issue-799-tsquery-rewrite", &[]);

    // Act
    let direct = fixture
        .execute("SELECT ts_rewrite('a & b'::tsquery, 'a'::tsquery, 'foo|bar'::tsquery)::TEXT");
    let query = fixture.execute(
        "SELECT ts_rewrite('a & b'::tsquery, $$SELECT 'a'::tsquery, 'foo|bar'::tsquery$$)::TEXT",
    );

    // Assert
    assert_eq!(
        direct.expect("direct ts_rewrite").rows(),
        &[vec![Value::String("'b' & ( 'foo' | 'bar' )".to_owned())]]
    );
    assert_eq!(
        query.expect("query-driven ts_rewrite").rows(),
        &[vec![Value::String("'b' & ( 'foo' | 'bar' )".to_owned())]]
    );
}

#[test]
fn should_return_two_tsvector_rank_modes() {
    // Arrange
    let fixture = sql_fixture("issue-799-ts-rank", &[]);

    // Act
    let result = fixture.execute(
        "SELECT ts_rank(to_tsvector('english', 'raining cats and dogs'), to_tsquery('cat')), ts_rank_cd(to_tsvector('english', 'raining cats and dogs'), to_tsquery('cat'))",
    );

    // Assert
    let result = result.expect("text-search rank functions");
    let rows = result.rows();
    let [Value::Float64(rank), Value::Float64(cover_density)] = rows[0].as_slice() else {
        panic!("rank functions must return float8 values: {rows:?}");
    };
    assert!((rank - 0.060_792_71).abs() < 0.000_001);
    assert!((cover_density - 0.1).abs() < 0.000_001);
}

#[test]
fn should_render_matching_text_search_headlines() {
    // Arrange
    let fixture = sql_fixture("issue-799-ts-headline", &[]);

    // Act
    let result = fixture.execute("SELECT ts_headline('The fat cat ate the rat.', 'cat')");

    // Assert
    assert_eq!(
        result.expect("ts_headline").rows(),
        &[vec![Value::String(
            "The fat <b>cat</b> ate the rat.".to_owned()
        )]]
    );
}

#[test]
fn should_render_configured_text_search_headlines_with_options() {
    // Arrange
    let fixture = sql_fixture("issue-799-ts-headline-options", &[]);

    // Act
    let result = fixture.execute(
        "SELECT ts_headline('english', 'The fat cat ate the rat.', to_tsquery('english', 'cat'), 'StartSel=<b>, StopSel=</b>')",
    );

    // Assert
    assert_eq!(
        result
            .expect("configured ts_headline with selection options")
            .rows(),
        &[vec![Value::String(
            "The fat <b>cat</b> ate the rat.".to_owned()
        )]]
    );
}

#[test]
fn should_normalize_selected_json_values_into_tsvectors() {
    // Arrange
    let fixture = sql_fixture("issue-799-json-tsvector", &[]);

    // Act
    let result = fixture.execute(
        "SELECT json_to_tsvector('english', '{\"a\": \"The Fat Rats\", \"b\": 123}'::JSON, '[\"string\", \"numeric\"]'::JSONB)::TEXT, jsonb_to_tsvector('english', '{\"a\": \"The Fat Rats\", \"b\": 123}'::JSONB, '\"all\"'::JSONB)::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("JSON text-search constructors").rows(),
        &[vec![
            Value::String("'123':5 'fat':2 'rat':3".to_owned()),
            Value::String("'123':8 'b':6 'fat':3 'rat':4".to_owned()),
        ]]
    );
}

#[test]
fn should_normalize_json_documents_to_tsvectors() {
    // Arrange
    let fixture = sql_fixture("issue-799-json-document-tsvector", &[]);

    // Act
    let result = fixture.execute(
        "SELECT to_tsvector('english', '{\"aa\": \"The Fat Rats\", \"b\": \"dog\"}'::JSON)::TEXT, to_tsvector('english', '{\"aa\": \"The Fat Rats\", \"b\": \"dog\"}'::JSONB)::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("JSON and JSONB to_tsvector inputs").rows(),
        &[vec![
            Value::String("'dog':5 'fat':2 'rat':3".to_owned()),
            Value::String("'dog':1 'fat':4 'rat':5".to_owned()),
        ]]
    );
}

#[test]
fn should_select_the_current_text_search_configuration_for_omitted_arguments() {
    // Arrange
    let fixture = sql_fixture("issue-799-current-ts-config", &[]);
    fixture
        .execute("SET default_text_search_config = 'simple'")
        .expect("select simple default text-search configuration");

    // Act
    let result =
        fixture.execute("SELECT get_current_ts_config()::TEXT, to_tsvector('The Cats')::TEXT");

    // Assert
    assert_eq!(
        result.expect("current text-search configuration").rows(),
        &[vec![
            Value::String("simple".to_owned()),
            Value::String("'cats':2 'the':1".to_owned()),
        ]]
    );
}

#[test]
fn should_report_postgres_text_search_type_oids() {
    // Arrange
    let fixture = sql_fixture("issue-799-ts-type-oids", &[]);

    // Act
    let vector = fixture.execute("SELECT to_tsvector('simple', 'blue sky') AS value");
    let query = fixture.execute("SELECT to_tsquery('simple', 'blue & sky') AS value");

    // Assert
    assert_eq!(vector.expect("tsvector result").columns()[0].type_oid, 3614);
    assert_eq!(query.expect("tsquery result").columns()[0].type_oid, 3615);
}

#[test]
fn should_report_postgres_text_search_array_type_oids() {
    // Arrange
    let fixture = sql_fixture("issue-799-ts-array-oids", &[]);

    // Act
    let vectors = fixture.execute("SELECT ARRAY['blue:1'::tsvector] AS values");
    let queries = fixture.execute("SELECT ARRAY['blue'::tsquery] AS values");

    // Assert
    assert_eq!(vectors.expect("tsvector array").columns()[0].type_oid, 3643);
    assert_eq!(queries.expect("tsquery array").columns()[0].type_oid, 3645);
}

#[test]
fn should_use_text_search_configuration_in_debugging_functions() {
    // Arrange
    let fixture = sql_fixture("issue-799-ts-debug", &[]);

    // Act
    let debug = fixture.execute("SELECT * FROM ts_debug('english', 'The Brightest stars')");
    let lexemes = fixture.execute("SELECT ts_lexize('english_stem', 'running')::TEXT");

    // Assert
    let debug = debug.expect("ts_debug result");
    assert_eq!(
        debug
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        [
            "alias",
            "description",
            "token",
            "dictionaries",
            "dictionary",
            "lexemes"
        ]
    );
    assert_ne!(debug.rows(), &[] as &[Vec<Value>]);
    assert_eq!(
        lexemes.expect("ts_lexize result").rows(),
        &[vec![Value::String("{run}".to_owned())]]
    );
}

#[test]
fn should_parse_text_search_tokens_by_name_or_oid() {
    // Arrange
    let fixture = sql_fixture("issue-799-ts-parser-debug", &[]);

    // Act
    let named = fixture.execute("SELECT * FROM ts_parse('default', 'hello world')");
    let token_types = fixture.execute("SELECT * FROM ts_token_type('default')");

    // Assert
    let named = named.expect("ts_parse by parser name");
    let token_types = token_types.expect("ts_token_type by parser name");
    assert_eq!(
        named
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["tokid", "token"]
    );
    assert_ne!(named.rows(), &[] as &[Vec<Value>]);
    assert_ne!(token_types.rows(), &[] as &[Vec<Value>]);
}

#[test]
fn should_parse_text_search_tokens_by_parser_oid() {
    // Arrange
    let fixture = sql_fixture("issue-799-ts-parser-oid", &[]);

    // Act
    let result = fixture.execute("SELECT * FROM ts_parse(3722, 'foo - bar')");

    // Assert
    let result = result.expect("ts_parse by parser OID");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["tokid", "token"]
    );
    assert_ne!(result.rows(), &[] as &[Vec<Value>]);
}

#[test]
fn should_list_text_search_token_types_by_parser_oid() {
    // Arrange
    let fixture = sql_fixture("issue-799-ts-token-type-oid", &[]);

    // Act
    let result = fixture.execute("SELECT * FROM ts_token_type(3722)");

    // Assert
    let result = result.expect("ts_token_type by parser OID");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["tokid", "alias", "description"]
    );
    assert_ne!(result.rows(), &[] as &[Vec<Value>]);
}

#[test]
fn should_collect_text_search_statistics_from_a_query() {
    // Arrange
    let fixture = sql_fixture(
        "issue-799-ts-stat",
        &[
            "CREATE TABLE search_documents (body TEXT)",
            "INSERT INTO search_documents VALUES ('blue sky'), ('blue ocean'), ('green field')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT word, ndoc, nentry FROM ts_stat($$SELECT to_tsvector('simple', body) FROM search_documents$$) ORDER BY word",
    );

    // Assert
    assert_eq!(
        result.expect("ts_stat query").rows(),
        &[
            vec![
                Value::String("blue".to_owned()),
                Value::Int64(2),
                Value::Int64(2)
            ],
            vec![
                Value::String("field".to_owned()),
                Value::Int64(1),
                Value::Int64(1)
            ],
            vec![
                Value::String("green".to_owned()),
                Value::Int64(1),
                Value::Int64(1)
            ],
            vec![
                Value::String("ocean".to_owned()),
                Value::Int64(1),
                Value::Int64(1)
            ],
            vec![
                Value::String("sky".to_owned()),
                Value::Int64(1),
                Value::Int64(1)
            ],
        ]
    );
}

#[test]
fn should_apply_altered_stopword_dictionary_to_its_configuration() {
    // Arrange
    let fixture = sql_fixture("issue-799-ts-dictionary-stopwords", &[]);
    fixture
        .execute(
            "CREATE TEXT SEARCH DICTIONARY issue_799_stopwords (TEMPLATE = pg_catalog.simple, STOPWORDS = english)",
        )
        .expect("create simple dictionary with English stopwords");
    fixture
        .execute(
            "CREATE TEXT SEARCH CONFIGURATION issue_799_stopword_config (COPY = pg_catalog.simple)",
        )
        .expect("copy simple configuration");
    fixture
        .execute(
            "ALTER TEXT SEARCH CONFIGURATION issue_799_stopword_config ALTER MAPPING FOR asciiword REPLACE pg_catalog.simple WITH issue_799_stopwords",
        )
        .expect("map asciiword tokens to custom dictionary");

    // Act
    let english = fixture.execute(
        "SELECT to_tsvector('issue_799_stopword_config', 'The le Cats')::TEXT, ts_lexize('issue_799_stopwords', 'the')::TEXT",
    );
    fixture
        .execute("ALTER TEXT SEARCH DICTIONARY issue_799_stopwords (STOPWORDS = french)")
        .expect("change the dictionary stopword list");
    let french = fixture.execute(
        "SELECT to_tsvector('issue_799_stopword_config', 'The le Cats')::TEXT, ts_lexize('issue_799_stopwords', 'le')::TEXT",
    );

    // Assert
    assert_eq!(
        english.expect("English stopword behavior").rows(),
        &[vec![
            Value::String("'cats':3 'le':2".to_owned()),
            Value::String("{}".to_owned()),
        ]]
    );
    assert_eq!(
        french.expect("updated French stopword behavior").rows(),
        &[vec![
            Value::String("'cats':3 'the':1".to_owned()),
            Value::String("{}".to_owned()),
        ]]
    );
}

#[test]
fn should_accept_regconfig_values_in_text_search_function_signatures() {
    // Arrange
    let fixture = sql_fixture("issue-799-regconfig-text-search-functions", &[]);

    // Act
    let result = fixture.execute(
        "SELECT to_tsvector('pg_catalog.english'::regconfig, 'The Fat Rats')::TEXT, to_tsquery('pg_catalog.simple'::regconfig, 'The & Fat')::TEXT",
    );

    // Assert
    assert_eq!(
        result
            .expect("text-search functions with regconfig inputs")
            .rows(),
        &[vec![
            Value::String("'fat':2 'rat':3".to_owned()),
            Value::String("'the' & 'fat'".to_owned()),
        ]]
    );
}

#[test]
fn should_resolve_regconfig_for_each_text_search_constructor() {
    // Arrange
    let fixture = sql_fixture("issue-799-regconfig-constructor-overloads", &[]);

    // Act
    let result = fixture.execute(
        "SELECT \
             to_tsvector('pg_catalog.english'::regconfig, 'The Fat Rats')::TEXT, \
             to_tsquery('pg_catalog.english'::regconfig, 'The & Fat & Rats')::TEXT, \
             plainto_tsquery('pg_catalog.english'::regconfig, 'The Fat Rats')::TEXT, \
             phraseto_tsquery('pg_catalog.english'::regconfig, 'The Fat Rats')::TEXT, \
             websearch_to_tsquery('pg_catalog.english'::regconfig, '\"The Fat Rats\"')::TEXT, \
             ts_headline( \
                 'pg_catalog.english'::regconfig, \
                 'The Fat Rats', \
                 to_tsquery('pg_catalog.english'::regconfig, 'rat'), \
                 'StartSel=<b>, StopSel=</b>' \
             )",
    );

    // Assert
    assert_eq!(
        result
            .expect("regconfig overloads for all text-search constructors")
            .rows(),
        &[vec![
            Value::String("'fat':2 'rat':3".to_owned()),
            Value::String("'fat' & 'rat'".to_owned()),
            Value::String("'fat' & 'rat'".to_owned()),
            Value::String("'fat' <-> 'rat'".to_owned()),
            Value::String("'fat' <-> 'rat'".to_owned()),
            Value::String("The Fat <b>Rats</b>".to_owned()),
        ]]
    );
}
