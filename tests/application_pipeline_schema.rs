use cassie::app::Cassie;
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-application-schema-{label}-{}",
        Uuid::new_v4()
    ));
    path.to_string_lossy().to_string()
}

const PIPELINE_APPLICATION_STATEMENTS: &[&str] = &[
    r#"CREATE TABLE "pipeline_statuses" (
  "id" UUID NOT NULL DEFAULT gen_random_uuid(),
  "entity_kind" TEXT NOT NULL,
  "entity_id" TEXT NOT NULL,
  "entity_type" TEXT NOT NULL,
  "current_state" TEXT NOT NULL,
  "previous_state" TEXT,
  "status_updated_at" TIMESTAMP(3),
  "project" JSONB,
  "selected_partner" JSONB,
  "partner_eligibility" JSONB,
  "company_name" TEXT,
  "created_at" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "updated_at" TIMESTAMP(3) NOT NULL,
  CONSTRAINT "pipeline_statuses_pkey" PRIMARY KEY ("id")
)"#,
    r#"CREATE TABLE "pipeline_status_history" (
  "id" UUID NOT NULL DEFAULT gen_random_uuid(),
  "entity_kind" TEXT NOT NULL,
  "entity_id" TEXT NOT NULL,
  "event_id" TEXT,
  "state" TEXT NOT NULL,
  "previous_state" TEXT,
  "action" TEXT,
  "source" TEXT,
  "entered_at" TIMESTAMP(3),
  "sequence" INTEGER,
  "created_at" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "status_id" UUID,
  CONSTRAINT "pipeline_status_history_pkey" PRIMARY KEY ("id")
)"#,
    r#"CREATE TABLE "pipeline_state_progression" (
  "id" UUID NOT NULL DEFAULT gen_random_uuid(),
  "group" TEXT NOT NULL,
  "state" TEXT NOT NULL,
  "label" TEXT,
  "sort_order" INTEGER NOT NULL,
  "created_at" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "updated_at" TIMESTAMP(3) NOT NULL,
  CONSTRAINT "pipeline_state_progression_pkey" PRIMARY KEY ("id")
)"#,
    r#"CREATE TABLE "microf_brands" (
  "id" TEXT NOT NULL,
  "name" TEXT NOT NULL,
  "active" BOOLEAN NOT NULL DEFAULT true,
  "sort_order" INTEGER NOT NULL DEFAULT 0,
  "created_at" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "updated_at" TIMESTAMP(3) NOT NULL,
  CONSTRAINT "microf_brands_pkey" PRIMARY KEY ("id")
)"#,
    r#"CREATE TABLE "scheduled_charges" (
  "id" UUID NOT NULL DEFAULT gen_random_uuid(),
  "scheduled_charge_id" TEXT NOT NULL,
  "application_id" TEXT NOT NULL,
  "status" TEXT NOT NULL,
  "due_at" TIMESTAMP(3),
  "properties" JSONB,
  "created_at" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "updated_at" TIMESTAMP(3) NOT NULL,
  CONSTRAINT "scheduled_charges_pkey" PRIMARY KEY ("id")
)"#,
    r#"CREATE TABLE "partner_offers" (
  "id" UUID NOT NULL DEFAULT gen_random_uuid(),
  "application_id" TEXT NOT NULL,
  "partner" TEXT NOT NULL,
  "partner_type" TEXT,
  "offer_id" TEXT,
  "status" TEXT NOT NULL,
  "details" JSONB,
  "created_at" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "updated_at" TIMESTAMP(3) NOT NULL,
  CONSTRAINT "partner_offers_pkey" PRIMARY KEY ("id")
)"#,
    r#"CREATE UNIQUE INDEX "pipeline_statuses_entity_key" ON "pipeline_statuses"("entity_kind", "entity_id")"#,
    r#"CREATE UNIQUE INDEX "pipeline_status_history_event_id_key" ON "pipeline_status_history"("event_id")"#,
    r#"CREATE INDEX "pipeline_status_history_entity_sequence_idx" ON "pipeline_status_history"("entity_kind", "entity_id", "sequence")"#,
    r#"CREATE UNIQUE INDEX "pipeline_state_progression_group_state_key" ON "pipeline_state_progression"("group", "state")"#,
    r#"CREATE UNIQUE INDEX "scheduled_charges_scheduled_charge_id_key" ON "scheduled_charges"("scheduled_charge_id")"#,
    r#"CREATE INDEX "scheduled_charges_status_due_at_idx" ON "scheduled_charges"("status", "due_at")"#,
    r#"CREATE INDEX "scheduled_charges_application_id_idx" ON "scheduled_charges"("application_id")"#,
    r#"CREATE INDEX "partner_offers_application_partner_type_idx" ON "partner_offers"("application_id", "partner_type")"#,
    r#"ALTER TABLE "pipeline_status_history"
  ADD CONSTRAINT "pipeline_status_history_status_id_fkey"
  FOREIGN KEY ("status_id") REFERENCES "pipeline_statuses"("id")
  ON DELETE CASCADE ON UPDATE CASCADE"#,
];

#[test]
fn should_apply_pipeline_application_schema() {
    // Arrange
    with_fallback();
    let path = data_dir("pipeline");
    let path_for_cleanup = path.clone();
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("schema-app", None);

    // Act
    for statement in PIPELINE_APPLICATION_STATEMENTS {
        cassie
            .execute_sql(&session, statement, vec![])
            .unwrap_or_else(|error| panic!("failed to apply statement:\n{statement}\n{error}"));
    }

    // Assert
    assert!(cassie.catalog.exists("pipeline_statuses"));
    assert!(cassie.catalog.exists("pipeline_status_history"));
    assert!(cassie
        .catalog
        .get_index("scheduled_charges", "scheduled_charges_status_due_at_idx")
        .is_some());
    assert!(cassie
        .catalog
        .get_constraints("pipeline_status_history")
        .iter()
        .any(
            |constraint| constraint.references_table.as_deref() == Some("pipeline_statuses")
                && constraint.references_field.as_deref() == Some("id")
        ));

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}
