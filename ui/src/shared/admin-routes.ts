import type { IconProps } from "@askrjs/lucide";
import {
  CircleGaugeIcon,
  DatabaseIcon,
  HardDriveIcon,
  SearchIcon,
  SettingsIcon,
  SquareTerminalIcon,
} from "@askrjs/lucide";

type AdminRouteIcon = (props: IconProps) => JSX.Element;

export type AdminRoute = {
  description: string;
  icon: AdminRouteIcon;
  label: string;
  path: string;
};

export const adminRoutes: AdminRoute[] = [
  {
    description: "Reserved for database status and operator entry points.",
    icon: DatabaseIcon,
    label: "Overview",
    path: "/admin",
  },
  {
    description: "Reserved for schemas, collections, indexes, and catalog metadata.",
    icon: SearchIcon,
    label: "Catalog",
    path: "/admin/catalog",
  },
  {
    description: "Reserved for SQL and document query workflows.",
    icon: SquareTerminalIcon,
    label: "Query",
    path: "/admin/query",
  },
  {
    description: "Reserved for runtime health and operational measurements.",
    icon: CircleGaugeIcon,
    label: "Metrics",
    path: "/admin/metrics",
  },
  {
    description: "Reserved for Midge storage posture and data layout visibility.",
    icon: HardDriveIcon,
    label: "Storage",
    path: "/admin/storage",
  },
  {
    description: "Reserved for administrative preferences and runtime controls.",
    icon: SettingsIcon,
    label: "Settings",
    path: "/admin/settings",
  },
];

export function adminRouteForPath(path: string) {
  return adminRoutes.find((route) => route.path === path) ?? adminRoutes[0];
}
