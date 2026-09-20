import { useEffect, useState } from "react";
import { fsList, fsRead, fsWrite, restartPlugin, type FsEntry } from "@/api";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import { toast } from "sonner";

const SELF = "kovi-plugin-panel";

function fmtSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

interface Editing {
  path: string;
  content: string;
  original: string;
  plugin: string | null;
}

export default function Files({
  initialPath, pluginNames, onBack,
}: { initialPath: string; pluginNames: string[]; onBack: () => void }) {
  const [path, setPath] = useState(initialPath);
  const [entries, setEntries] = useState<FsEntry[]>([]);
  const [editing, setEditing] = useState<Editing | null>(null);

  const dirty = editing !== null && editing.content !== editing.original;

  async function load(p: string) {
    try {
      setEntries(await fsList(p));
      setPath(p);
    } catch (e) {
      toast.error((e as Error).message);
    }
  }

  useEffect(() => {
    load(initialPath);
  }, [initialPath]);

  async function openFile(p: string) {
    try {
      const { content, plugin } = await fsRead(p);
      setEditing({ path: p, content, original: content, plugin });
    } catch (e) {
      toast.error((e as Error).message);
    }
  }

  function confirmDiscard(): boolean {
    return !dirty || window.confirm("有未保存的修改,确定放弃?");
  }

  async function save(restart: boolean) {
    if (!editing) return;
    try {
      const savedContent = editing.content;
      await fsWrite(editing.path, savedContent);
      setEditing((prev) => (prev ? { ...prev, original: savedContent } : prev));
      toast.success("已保存");
      if (restart && editing.plugin) {
        await restartPlugin(editing.plugin);
        toast.success(`${editing.plugin} 已重启`);
      }
    } catch (e) {
      toast.error((e as Error).message);
    }
  }

  const crumbs = path === "" ? [] : path.split("/");
  const canRestart =
    editing?.plugin && editing.plugin !== SELF && pluginNames.includes(editing.plugin);

  if (editing) {
    return (
      <div className="min-h-screen bg-muted p-4 md:p-8">
        <div className="max-w-4xl mx-auto space-y-4">
          <div className="flex items-center justify-between gap-2 flex-wrap">
            <div className="text-sm text-muted-foreground break-all">
              data / {editing.path.split("/").join(" / ")}
              {dirty && <span className="ml-2 text-orange-500">●未保存</span>}
            </div>
            <div className="flex gap-2">
              <Button variant="outline" onClick={() => { if (confirmDiscard()) setEditing(null); }}>
                返回
              </Button>
              <Button onClick={() => save(false)} disabled={!dirty}>保存</Button>
              {canRestart && (
                <Button variant="secondary" onClick={() => save(true)}>
                  保存并重启 {editing.plugin}
                </Button>
              )}
            </div>
          </div>
          <textarea
            className="w-full h-[70vh] font-mono text-sm p-3 rounded-md border bg-background resize-none"
            value={editing.content}
            onChange={(e) => setEditing({ ...editing, content: e.target.value })}
            spellCheck={false}
          />
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-muted p-4 md:p-8">
      <div className="max-w-3xl mx-auto space-y-4">
        <div className="flex items-center justify-between">
          <div className="text-sm break-all">
            <button className="underline-offset-2 hover:underline" onClick={() => load("")}>data</button>
            {crumbs.map((seg, i) => (
              <span key={i}>
                {" / "}
                <button
                  className="underline-offset-2 hover:underline"
                  onClick={() => load(crumbs.slice(0, i + 1).join("/"))}
                >
                  {seg}
                </button>
              </span>
            ))}
          </div>
          <Button variant="outline" onClick={onBack}>返回插件列表</Button>
        </div>
        <Card className="p-0 overflow-hidden">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>名称</TableHead>
                <TableHead className="text-right">大小</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {entries.map((e) => {
                const child = path === "" ? e.name : `${path}/${e.name}`;
                return (
                  <TableRow
                    key={e.name}
                    className="cursor-pointer"
                    onClick={() => (e.is_dir ? load(child) : openFile(child))}
                  >
                    <TableCell className="font-medium">
                      {e.is_dir ? "📁" : "📄"} {e.name}
                    </TableCell>
                    <TableCell className="text-right text-muted-foreground">
                      {e.is_dir ? "—" : fmtSize(e.size)}
                    </TableCell>
                  </TableRow>
                );
              })}
              {entries.length === 0 && (
                <TableRow>
                  <TableCell colSpan={2} className="text-center text-muted-foreground">
                    空目录
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </Card>
      </div>
    </div>
  );
}
