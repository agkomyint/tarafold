import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { loadInitial, type Loaded } from "./report/load";
import "./style.css";

function Root() {
  const [state, setState] = useState<{ loaded?: Loaded; error?: string }>({});
  useEffect(() => {
    loadInitial()
      .then((loaded) => setState({ loaded }))
      .catch((e: unknown) => setState({ error: e instanceof Error ? e.message : String(e) }));
  }, []);
  if (state.error)
    return (
      <main className="view">
        <h1>Tarafold</h1>
        <p role="alert">{state.error}</p>
      </main>
    );
  if (!state.loaded) return <p className="view">Loading report…</p>;
  return <App loaded={state.loaded} />;
}

const el = document.getElementById("root");
if (el)
  createRoot(el).render(
    <StrictMode>
      <Root />
    </StrictMode>,
  );
