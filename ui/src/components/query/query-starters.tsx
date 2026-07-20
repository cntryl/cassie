import { Button, Text } from "@askrjs/themes/components";

interface QueryStarter {
  label: string;
  sql: string;
}

const starters: QueryStarter[] = [
  {
    label: "Create table",
    sql: "CREATE TABLE ui_demo (\n  demo_id INT PRIMARY KEY,\n  name TEXT NOT NULL\n);",
  },
  {
    label: "Insert rows",
    sql: "INSERT INTO ui_demo (demo_id, name)\nVALUES (1, 'Ada'), (2, 'Grace');",
  },
  {
    label: "Query rows",
    sql: "SELECT demo_id, name\nFROM ui_demo\nORDER BY demo_id;",
  },
];

export function QueryStarters({ onSelect }: { onSelect: (sql: string) => void }) {
  return (
    <div class="cassie-query-starters" data-testid="query-starters">
      <Text as="span" size="sm" tone="muted">
        Start with
      </Text>
      {starters.map((starter) => (
        <Button
          key={starter.label}
          type="button"
          size="xs"
          variant="ghost"
          onPress={() => onSelect(starter.sql)}
        >
          {starter.label}
        </Button>
      ))}
    </div>
  );
}
