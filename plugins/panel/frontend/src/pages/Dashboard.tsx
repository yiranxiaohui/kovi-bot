import { useEffect, useState } from "react";
import {
  listPlugins, setPluginEnabled, restartPlugin, getStatus,
  clearToken, type Plugin, type Status,
} from "@/api";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Card } from "@/components/ui/card";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import { toast } from "sonner";
import Files from "@/pages/Files";

export default function Dashboard({ onLogout }: { onLogout: () => void }) {
  const [plugins, setPlugins] = useState<Plugin[]>([]);
  const [status, setStatus] = useState<Status | null>(null);
  const [filesPath, setFilesPath] = useState<string | null>(null);

  async function refresh() {
    try {
      setPlugins(await listPlugins());
      setStatus(await getStatus());
    } catch (e) {
      if ((e as Error).message === "unauthorized") onLogout();
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function toggle(p: Plugin) {
    try {
      await setPluginEnabled(p.name, !p.enabled);
      toast.success(`${p.name} 已${p.enabled ? "禁用" : "启用"}`);
      refresh();
    } catch (e) {
      toast.error((e as Error).message);
    }
  }

  async function doRestart(p: Plugin) {
    try {
      await restartPlugin(p.name);
      toast.success(`${p.name} 已重启`);
      refresh();
    } catch (e) {
      toast.error((e as Error).message);
    }
  }

  function logout() {
    clearToken();
    onLogout();
  }

  if (filesPath !== null) {
    return (
      <Files
        initialPath={filesPath}
        pluginNames={plugins.map((p) => p.name)}
        onBack={() => setFilesPath(null)}
      />
    );
  }

  return (
    <div className="min-h-screen bg-muted p-4 md:p-8">
      <div className="max-w-3xl mx-auto space-y-4">
        <div className="flex items-center justify-between">
          <h1 className="text-2xl font-semibold">kovi 插件管理</h1>
          <div className="flex gap-2">
            <Button variant="outline" onClick={() => setFilesPath("")}>文件</Button>
            <Button variant="outline" onClick={logout}>登出</Button>
          </div>
        </div>
        <Card className="p-4 flex gap-6 text-sm">
          <span>状态:{status?.online ? "🟢 在线" : "⚪ 未知"}</span>
          <span>插件数:{status?.plugin_count ?? "—"}</span>
        </Card>
        <Card className="p-0 overflow-hidden">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>插件</TableHead>
                <TableHead>版本</TableHead>
                <TableHead className="text-center">启用</TableHead>
                <TableHead className="text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {plugins.map((p) => (
                <TableRow key={p.name}>
                  <TableCell className="font-medium">{p.name}</TableCell>
                  <TableCell className="text-muted-foreground">{p.version}</TableCell>
                  <TableCell className="text-center">
                    <Switch checked={p.enabled} onCheckedChange={() => toggle(p)} />
                  </TableCell>
                  <TableCell className="text-right">
                    <Button size="sm" variant="outline" onClick={() => doRestart(p)}>
                      重启
                    </Button>
                    <Button size="sm" variant="ghost" onClick={() => setFilesPath(p.name)}>
                      文件
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
      </div>
    </div>
  );
}
