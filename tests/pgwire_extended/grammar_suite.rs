// Registrations retain the existing pgwire_extended module namespace.

#[path = "grammar.rs"]
mod postgresql18_grammar;

#[path = "grammar/chained_cast_binary.rs"]
mod grammar_chained_cast_binary;

#[path = "grammar/dollar_quotes.rs"]
mod grammar_dollar_quotes;

#[path = "grammar/lexical_literals.rs"]
mod grammar_lexical_literals;

#[path = "grammar/join_using_errors.rs"]
mod grammar_join_using_errors;

#[path = "grammar/values_errors.rs"]
mod grammar_values_errors;

#[path = "grammar/integer_inputs.rs"]
mod postgresql18_integer_inputs;

#[path = "grammar/row_predicates.rs"]
mod postgresql18_row_predicates;

#[path = "grammar/values_constants.rs"]
mod postgresql18_values_constants;

#[path = "grammar/unary_numeric.rs"]
mod unary_numeric;
