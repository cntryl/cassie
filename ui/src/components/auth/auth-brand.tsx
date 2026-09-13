import { Brand, BrandLabel } from "@askrjs/themes/components";

import { cassieLogoImageProps, cassieLogoPath } from "@/shared/cassie-brand-assets";

export function AuthBrand() {
  return (
    <Brand>
      <img
        class="cassie-brand-logo"
        src={cassieLogoPath}
        {...cassieLogoImageProps}
        width="32"
        height="32"
        alt=""
        aria-hidden="true"
      />
      <BrandLabel>Cassie Admin</BrandLabel>
    </Brand>
  );
}
