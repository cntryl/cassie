import { MoonIcon, SunIcon } from "@askrjs/lucide";
import { Block } from "@askrjs/themes/components";
import { ThemeScope, ThemeToggle } from "@askrjs/themes/theme";

export function AuthPage({ children }: { children?: unknown }) {
  return (
    <ThemeScope defaultTheme="system" storageKey="cassie-admin-theme">
      <Block as="main" id="main-content" class="cassie-auth-page" background="canvas" tabIndex={-1}>
        <div class="cassie-auth-theme-toggle">
          <ThemeToggle
            aria-label="Toggle color theme"
            variant="ghost"
            size="icon"
            lightIcon={<SunIcon size={16} />}
            darkIcon={<MoonIcon size={16} />}
          />
        </div>

        <Block class="cassie-auth-panel" width="full">
          {children as never}
        </Block>
      </Block>
    </ThemeScope>
  );
}
