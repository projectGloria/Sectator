import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Titlebar } from "./components/Titlebar";

export default function App() {
  const [version, setVersion] = useState<string>();
  useEffect(() => {
    invoke<{ version: string }>("app_info").then((i) => setVersion(i.version));
  }, []);
  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <Titlebar version={version} />
      <main className="flex flex-1 items-center justify-center text-dim">Sectator</main>
    </div>
  );
}
