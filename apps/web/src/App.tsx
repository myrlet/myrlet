/**
 * Myrlet Web UI root — the shell around the Work Atlas.
 *
 * The shell mounts `useHub`, `useAppearance`, and the notification queue once,
 * then composes either the Auth gate or the authenticated workspace. The
 * workspace is mounted only for a live Hub session, so its listeners, overlays,
 * and view state disappear on disconnect or auth loss and a new login starts
 * clean. The Task surface is strictly read-only: creation, import, cancel,
 * finish, subtask, and retry belong to the CLI, never here.
 */

import { AuthGate } from "./components/AuthGate";
import { ToastStack } from "./components/CommandPalette";
import { AuthenticatedWorkspace } from "./components/AuthenticatedWorkspace";
import { useAppearance } from "./state/useAppearance";
import { useHub } from "./state/useHub";
import { useToasts } from "./state/useToasts";

export default function App() {
  const hub = useHub();
  const appearance = useAppearance();
  const { toasts, dismissToast, notify } = useToasts();

  if (!hub.session) {
    return (
      <AuthGate
        defaultBaseUrl={hub.baseUrl}
        connecting={hub.connection === "connecting"}
        error={hub.error}
        theme={appearance.theme}
        onChangeTheme={appearance.setTheme}
        onConnect={hub.connect}
      />
    );
  }

  return (
    <div className="app-frame">
      <AuthenticatedWorkspace
        baseUrl={hub.session.baseUrl}
        hub={hub}
        theme={appearance.theme}
        onChangeTheme={appearance.setTheme}
        notify={notify}
      />
      <ToastStack toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}
