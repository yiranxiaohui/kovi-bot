import { useState } from "react";
import { getToken } from "@/api";
import Login from "@/pages/Login";
import Dashboard from "@/pages/Dashboard";
import { Toaster } from "sonner";

export default function App() {
  const [authed, setAuthed] = useState(() => !!getToken());
  return (
    <>
      {authed ? (
        <Dashboard onLogout={() => setAuthed(false)} />
      ) : (
        <Login onLogin={() => setAuthed(true)} />
      )}
      <Toaster richColors position="top-center" />
    </>
  );
}
