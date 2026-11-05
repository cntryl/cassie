// pgwire_catalogs: subsystem-owned wire contracts; shared fixtures stay in support/.

#[path = "pgwire_extended/catalogs/empty_registries_a.rs"]
mod catalog_empty_registries_a;

#[path = "pgwire_extended/catalogs/empty_registries_b.rs"]
mod catalog_empty_registries_b;

#[path = "pgwire_extended/catalogs/unavailable_virtual_view.rs"]
mod catalog_unavailable_virtual_view;

#[path = "pgwire_extended/catalogs/scalar_0.rs"]
mod catalog_scalar_0;

#[path = "pgwire_extended/catalogs/scalar_1.rs"]
mod catalog_scalar_1;

#[path = "pgwire_extended/catalogs/scalar_2.rs"]
mod catalog_scalar_2;

#[path = "pgwire_extended/catalogs/scalar_3.rs"]
mod catalog_scalar_3;

#[path = "pgwire_extended/catalogs/scalar_4.rs"]
mod catalog_scalar_4;

#[path = "pgwire_extended/catalogs/scalar_errors.rs"]
mod catalog_scalar_errors;

#[path = "pgwire_extended/catalogs/scalar_parameters.rs"]
mod catalog_scalar_parameters;

#[path = "pgwire_extended/catalogs/scalar_signatures.rs"]
mod catalog_scalar_signatures;

#[path = "pgwire_extended/catalogs/scalar_precedence.rs"]
mod catalog_scalar_precedence;

#[path = "pgwire_extended/catalogs/scalar_pending.rs"]
mod catalog_scalar_pending;

#[path = "pgwire_extended/catalogs/runtime_scalars_0.rs"]
mod catalog_runtime_scalars_0;

#[path = "pgwire_extended/catalogs/runtime_scalars_1.rs"]
mod catalog_runtime_scalars_1;

#[path = "pgwire_extended/catalogs/runtime_modules.rs"]
mod catalog_runtime_modules;

#[path = "pgwire_extended/catalogs/runtime_parameters.rs"]
mod catalog_runtime_parameters;

#[path = "pgwire_extended/catalogs/runtime_signatures.rs"]
mod catalog_runtime_signatures;

#[path = "pgwire_extended/catalogs/options_0.rs"]
mod catalog_options_0;

#[path = "pgwire_extended/catalogs/options_1.rs"]
mod catalog_options_1;

#[path = "pgwire_extended/catalogs/options_parameters.rs"]
mod catalog_options_parameters;

#[path = "pgwire_extended/catalogs/options_signatures.rs"]
mod catalog_options_signatures;

#[path = "pgwire_extended/catalogs/options_errors.rs"]
mod catalog_options_errors;

#[path = "pgwire_extended/catalogs/options_escaping.rs"]
mod catalog_options_escaping;

#[path = "pgwire_extended/catalogs/current_query.rs"]
mod catalog_current_query;

#[path = "pgwire_extended/catalogs/current_query_simple.rs"]
mod catalog_current_query_simple;

#[path = "pgwire_extended/catalogs/current_query_lifecycle.rs"]
mod catalog_current_query_lifecycle;

#[path = "pgwire_extended/catalogs/current_query_parameters.rs"]
mod catalog_current_query_parameters;

#[path = "pgwire_extended/catalogs/current_query_arity.rs"]
mod catalog_current_query_arity;

#[path = "pgwire_extended/catalogs/datum_sizes_0.rs"]
mod catalog_datum_sizes_0;

#[path = "pgwire_extended/catalogs/datum_sizes_1.rs"]
mod catalog_datum_sizes_1;

#[path = "pgwire_extended/catalogs/datum_sizes_2.rs"]
mod catalog_datum_sizes_2;

#[path = "pgwire_extended/catalogs/datum_sizes_3.rs"]
mod catalog_datum_sizes_3;

#[path = "pgwire_extended/catalogs/datum_parameters.rs"]
mod catalog_datum_parameters;

#[path = "pgwire_extended/catalogs/datum_errors.rs"]
mod catalog_datum_errors;

#[path = "pgwire_extended/catalogs/datum_nested.rs"]
mod catalog_datum_nested;

use cassie_test_support::support_bind_planning;

use cassie_test_support::support_bytea_input_errors;

#[path = "support/catalog_read_model_wire.rs"]
mod support_catalog_read_model_wire;

#[path = "support/options_wire.rs"]
mod support_options_wire;

use cassie_test_support::support_parameter_occurrence_wire;

use cassie_test_support::support_pgwire;

#[path = "support/runtime_information_wire.rs"]
mod support_runtime_information_wire;

use cassie_test_support::support_type_metadata_contract;

#[path = "support/values_casts.rs"]
mod support_values_casts;
