// M2: module manifest. Each workspace window declares what it needs; the
// shell hides modules the user's role or the license does not allow.
// Licensing (F16) then becomes configuration, not a rewrite.
import { ROLE_RANK, type Feature, type SessionInfo } from "../services/studio";

export type ModuleId = "vehicle" | "explorer" | "faults" | "trace" | "session";

export type ModuleManifest = {
  id: ModuleId;
  title: string;
  /** Minimum role to open the module. */
  minRole: SessionInfo["role"];
  /** License feature that unlocks the module. */
  feature: Feature;
};

export const MODULES: ModuleManifest[] = [
  { id: "vehicle", title: "Vehicle", minRole: "viewer", feature: "diagnostics" },
  { id: "explorer", title: "SOVD Explorer", minRole: "viewer", feature: "diagnostics" },
  { id: "faults", title: "Faults", minRole: "viewer", feature: "diagnostics" },
  { id: "trace", title: "Trace", minRole: "viewer", feature: "diagnostics" },
  { id: "session", title: "Session & License", minRole: "viewer", feature: "diagnostics" },
];

/** Modules this user may open (role) and this installation may run (license). */
export function allowedModules(session: SessionInfo | null, licensed: (f: Feature) => boolean) {
  return MODULES.filter(
    (m) => licensed(m.feature) && (!session || ROLE_RANK[session.role] >= ROLE_RANK[m.minRole]),
  );
}
