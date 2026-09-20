const TOKEN_KEY = "panel_token";

export function setToken(t: string) {
  localStorage.setItem(TOKEN_KEY, t);
}
export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}
export function clearToken() {
  localStorage.removeItem(TOKEN_KEY);
}

export async function apiFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const token = getToken();
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(init.headers as Record<string, string>),
  };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await fetch(path, { ...init, headers });
  if (res.status === 401) {
    clearToken();
    throw new Error("unauthorized");
  }
  return res;
}

export interface Plugin {
  name: string;
  version: string;
  enabled: boolean;
}

export async function login(password: string): Promise<string> {
  const res = await fetch("/api/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ password }),
  });
  if (!res.ok) throw new Error("密码错误");
  const data = await res.json();
  return data.token as string;
}

export async function listPlugins(): Promise<Plugin[]> {
  const res = await apiFetch("/api/plugins");
  if (!res.ok) throw new Error("加载插件列表失败");
  return res.json();
}

export async function setPluginEnabled(name: string, enabled: boolean): Promise<void> {
  const action = enabled ? "enable" : "disable";
  const res = await apiFetch(`/api/plugins/${encodeURIComponent(name)}/${action}`, { method: "POST" });
  if (!res.ok) throw new Error(await res.text());
}

export async function restartPlugin(name: string): Promise<void> {
  const res = await apiFetch(`/api/plugins/${encodeURIComponent(name)}/restart`, { method: "POST" });
  if (!res.ok) throw new Error(await res.text());
}

export interface Status {
  online: boolean;
  plugin_count: number;
}
export async function getStatus(): Promise<Status> {
  const res = await apiFetch("/api/status");
  if (!res.ok) throw new Error("加载状态失败");
  return res.json();
}

export interface FsEntry {
  name: string;
  is_dir: boolean;
  size: number;
}

async function fsError(res: Response, fallback: string): Promise<Error> {
  try {
    const data = await res.json();
    return new Error(data.error ?? fallback);
  } catch {
    return new Error(fallback);
  }
}

export async function fsList(path: string): Promise<FsEntry[]> {
  const res = await apiFetch(`/api/fs/list?path=${encodeURIComponent(path)}`);
  if (!res.ok) throw await fsError(res, "读取目录失败");
  return res.json();
}

export async function fsRead(path: string): Promise<{ content: string; plugin: string | null }> {
  const res = await apiFetch(`/api/fs/read?path=${encodeURIComponent(path)}`);
  if (!res.ok) throw await fsError(res, "读取文件失败");
  return res.json();
}

export async function fsWrite(path: string, content: string): Promise<void> {
  const res = await apiFetch(`/api/fs/write?path=${encodeURIComponent(path)}`, {
    method: "PUT",
    headers: { "Content-Type": "text/plain" },
    body: content,
  });
  if (!res.ok) throw await fsError(res, "保存失败");
}
