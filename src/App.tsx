import { AppShell } from "./app/AppShell";
import { useApplyAccent, useApplyTheme } from "./hooks/useTheme";

export default function App() {
  useApplyTheme();
  useApplyAccent();
  return <AppShell />;
}
