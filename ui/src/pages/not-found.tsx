import { Link } from "@askrjs/askr/router";
import { ArrowLeftIcon, DatabaseIcon } from "@askrjs/lucide";
import {
  Block,
  Brand,
  BrandLabel,
  BrandMark,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  Page,
} from "@askrjs/themes/components";

export default function NotFoundPage() {
  return (
    <Page background="muted" center>
      <Block as="main" align="center" justify="center" grow id="main-content" tabindex={-1}>
        <Block width="full" maxWidth="sm">
          <Card variant="raised">
            <CardHeader>
              <Brand>
                <BrandMark aria-hidden="true">
                  <DatabaseIcon size={16} />
                </BrandMark>
                <BrandLabel>Cassie Admin</BrandLabel>
              </Brand>
              <CardTitle>Page not found</CardTitle>
              <CardDescription>The requested admin page does not exist.</CardDescription>
            </CardHeader>
            <CardContent>
              <Button asChild variant="primary" width="full">
                <Link href="/">
                  <ArrowLeftIcon size={16} aria-hidden="true" />
                  Return to query workspace
                </Link>
              </Button>
            </CardContent>
          </Card>
        </Block>
      </Block>
    </Page>
  );
}
