// M2: dockable workspace. Windows can be dragged, split and tabbed; the
// layout is saved per user (layout only - no session data).
import { useCallback, useRef } from "react";
import { DockviewReact, type DockviewApi, type DockviewReadyEvent, type IDockviewPanelProps } from "dockview-react";
import { themeAbyss } from "dockview";
import "dockview/dist/styles/dockview.css";
import App from "../App";
import SovdExplorer from "../components/SovdExplorer";
import TracePanel from "./TracePanel";
import SessionPanel from "./SessionPanel";
import { allowedModules, type ModuleId } from "./modules";
import type { SessionInfo } from "../services/studio";

const LAYOUT_VERSION = "v1";
const layoutKey = (user: string) => `velosovd.layout.${LAYOUT_VERSION}.${user}`;

function defaultLayout(api: DockviewApi, ids: ModuleId[]) {
  api.clear();
  const has = (id: ModuleId) => ids.includes(id);
  if (has("vehicle")) api.addPanel({ id: "vehicle", component: "vehicle", title: "Vehicle" });
  if (has("explorer"))
    api.addPanel({ id: "explorer", component: "explorer", title: "SOVD Explorer", position: { referencePanel: "vehicle", direction: "within" } });
  if (has("trace"))
    api.addPanel({ id: "trace", component: "trace", title: "Trace", position: { direction: "below" }, initialHeight: 220 });
  if (has("session"))
    api.addPanel({ id: "session", component: "session", title: "Session & License", position: { referencePanel: "trace", direction: "within" } });
  api.getPanel("vehicle")?.api.setActive();
}

export default function Workspace({
  session,
  onApi,
}: {
  session: SessionInfo | null;
  onApi: (reset: () => void) => void;
}) {
  const apiRef = useRef<DockviewApi | null>(null);
  const user = session?.user ?? "browser";
  const ids = allowedModules(session, () => true).map((m) => m.id);

  const components: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
    vehicle: () => <App />,
    explorer: () => <div className="h-full overflow-auto bg-[#080d18] p-4 text-slate-100"><SovdExplorer /></div>,
    trace: () => <TracePanel />,
    session: () => <SessionPanel session={session} />,
  };

  const onReady = useCallback(
    (event: DockviewReadyEvent) => {
      apiRef.current = event.api;
      const saved = localStorage.getItem(layoutKey(user));
      let restored = false;
      if (saved) {
        try {
          event.api.fromJSON(JSON.parse(saved));
          // Drop panels the user may no longer open (role changed).
          event.api.panels.filter((p) => !ids.includes(p.id as ModuleId)).forEach((p) => p.api.close());
          restored = event.api.panels.length > 0;
        } catch {
          restored = false;
        }
      }
      if (!restored) defaultLayout(event.api, ids);
      event.api.onDidLayoutChange(() => {
        localStorage.setItem(layoutKey(user), JSON.stringify(event.api.toJSON()));
      });
      onApi(() => {
        localStorage.removeItem(layoutKey(user));
        defaultLayout(event.api, ids);
      });
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [user],
  );

  return <DockviewReact className="h-full" theme={themeAbyss} components={components} onReady={onReady} />;
}
